package com.asmrplayer.android.playback

import android.net.Uri
import android.os.Bundle
import androidx.media3.common.*
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.SessionError
import androidx.media3.session.SessionResult
import com.asmrplayer.android.data.*
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import kotlinx.coroutines.*
import java.util.concurrent.Executor

/** Main-looper playback state; Room work is suspend/IO. Lives exactly as long as PlaybackService. */
@UnstableApi
internal class LibrarySessionCoordinator(
    private val player: ExoPlayer,
    private val backend: AndroidBackend,
    private val titles: MutableMap<String, String>,
    private val metadata: (MediaItem) -> MediaItem,
    private val sleepPreferences: android.content.SharedPreferences,
    private val notifyChange: () -> Unit = {},
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val storage = backend.sessions
    private val dao = backend.database.library()
    private val stableIds = hashSetOf<String>()
    private val unavailable = linkedSetOf<String>()
    private var generation = 0L
    private var addGeneration = 0L
    private data class RemovedItem(val token: String, val item: MediaItem, val index: Int, val remaining: List<String>)
    private var removedItem: RemovedItem? = null
    private var ready = false
    private var warning: String? = null
    private var abVersion: String? = null
    private var a: Long? = null
    private var b: Long? = null
    private var deadline: Long? = null
    private var endOfTrack = false
    private var fadeMs = sleepPreferences.getLong("fadeMs", 30_000L).coerceIn(0, 300_000)
    private var baseVolume = player.volume
    private var appliedGain = 1f
    private var changingVolume = false
    private fun applyGain(gain: Float) {
        if (gain == appliedGain) return
        changingVolume = true
        player.volume = (baseVolume * gain).coerceIn(0f, 1f)
        changingVolume = false
        appliedGain = gain
    }
    private var completed = false
    private val listener = object : Player.Listener {
        override fun onVolumeChanged(volume: Float) { if (!changingVolume) { baseVolume = volume; appliedGain = 1f } }
        override fun onEvents(player: Player, events: Player.Events) { if (ready && (events.size() != 1 || !events.contains(Player.EVENT_VOLUME_CHANGED))) enqueue() }
        override fun onMediaItemTransition(item: MediaItem?, reason: Int) {
            completed = false
            if (item?.mediaId != abVersion) clearAb()
        }
        override fun onPlaybackStateChanged(playbackState: Int) {
            if (playbackState == Player.STATE_ENDED) completed = true
        }
        override fun onPositionDiscontinuity(oldPosition: Player.PositionInfo, newPosition: Player.PositionInfo, reason: Int) {
            if (reason == Player.DISCONTINUITY_REASON_SEEK || reason == Player.DISCONTINUITY_REASON_AUTO_TRANSITION) completed = false
            val id = oldPosition.mediaItem?.mediaId ?: return
            if (ready && id in stableIds && (id != newPosition.mediaItem?.mediaId || oldPosition.mediaItemIndex != newPosition.mediaItemIndex)) {
                val record = snapshot()
                storage.enqueue(record, PlaybackPosition(id, oldPosition.positionMs.coerceAtLeast(0), 0,
                    completed = reason == Player.DISCONTINUITY_REASON_AUTO_TRANSITION))
            }
        }
        override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
            if (!playWhenReady && reason == Player.PLAY_WHEN_READY_CHANGE_REASON_END_OF_MEDIA_ITEM) {
                if (endOfTrack) { applyGain(1f); completed = true; endOfTrack = false; deadline = null; clearAb(); updatePauseAtEnd(); enqueue() }
                else if (abVersion == player.currentMediaItem?.mediaId && a != null) { player.seekTo(a!!); player.play() }
            }
        }
    }
    private val restore: Job

    init {
        player.addListener(listener)
        restore = scope.launch {
            val token = generation
            try {
                val saved = storage.read()
                if (saved != null) {
                    val ids = SessionPersistence.decode(saved.queueJson)
                    val items = ids.mapNotNull { id ->
                        val version = dao.version(id) ?: return@mapNotNull null
                        val source = dao.sources(id).firstOrNull() ?: return@mapNotNull null
                        val uri = try { backend.library.playbackReference(id).uri }
                            catch (e: CancellationException) { throw e }
                            catch (_: Exception) { unavailable.add(id); source.uri }
                        Triple(id, version.title, uri)
                    }
                    if (generation == token && player.mediaItemCount == 0) {
                        items.forEach { (id, title, _) -> stableIds.add(id); titles[id] = title }
                        val wanted = ids.getOrNull(saved.index)
                        val index = if (items.size == ids.size) saved.index.coerceIn(0, maxOf(0, items.lastIndex))
                            else items.indexOfFirst { it.first == wanted }.coerceAtLeast(0)
                        val position = if (items.getOrNull(index)?.first == wanted) saved.positionMs.coerceAtLeast(0) else 0
                        if (items.isNotEmpty()) player.setMediaItems(items.map { (id, _, uri) ->
                            metadata(MediaItem.Builder().setMediaId(id).setUri(uri).build())
                        }, index, position)
                        // Deliberately do not prepare/play during process recreation.
                        if (saved.abVersionId == items.getOrNull(index)?.first && saved.aMs != null && saved.bMs != null && saved.aMs >= 0 && saved.bMs > saved.aMs) {
                            abVersion = saved.abVersionId; a = saved.aMs; b = saved.bMs
                        }
                        deadline = saved.sleepDeadlineMs?.takeIf { it > System.currentTimeMillis() }
                        endOfTrack = saved.sleepEndOfTrack
                        updatePauseAtEnd()
                        if (unavailable.isNotEmpty()) warning = "部分音频不可用，请重新授权或扫描目录；已保留队列和进度"
                    }
                }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { warning = "无法恢复上次会话，请重新选择音频" }
            finally { ready = true }
        }
        scope.launch {
            var ticks = 0
            while (isActive) {
                delay(100)
                if (!ready) continue
                val now = System.currentTimeMillis()
                if (deadline?.let { now >= it } == true) { deadline = null; endOfTrack = false; player.pause(); applyGain(1f); updatePauseAtEnd(); enqueue() }
                val remaining = deadline?.minus(now) ?: if (endOfTrack && player.duration > 0) player.duration - player.currentPosition else null
                applyGain(if (player.playWhenReady) SleepEnvelope.gain(remaining, fadeMs) else 1f)
                if (abVersion != null && player.duration > 0 && b!! > player.duration) clearAb()
                if (player.isPlaying && abVersion == player.currentMediaItem?.mediaId && b != null && player.currentPosition >= b!!) player.seekTo(a!!)
                if (++ticks >= 20) { ticks = 0; if (player.isPlaying) enqueue() }
            }
        }
    }

    fun userIntent() { ++generation; ++addGeneration }
    // A pause cancels pending library resolution, but must not cancel Media3's queued set/prepare/play sequence.
    fun playbackIntent() { ++generation }

    /** Resolves library IDs; transient legacy/HTTPS items remain playable but aren't durable queue identities. */
    fun add(items: List<MediaItem>, persistable: (Uri) -> Boolean): ListenableFuture<List<MediaItem>> {
        ++generation
        val token = ++addGeneration
        return future {
            restore.join()
            require(items.size <= 500)
            val result = items.map { item ->
                val uri = requireNotNull(item.localConfiguration?.uri)
                val version = dao.version(item.mediaId)
                require(uri.scheme != "asmr" || version != null)
                val resolved = when {
                    version != null -> backend.library.playbackReference(version.id)
                    uri.scheme == "content" && persistable(uri) -> backend.library.importDocument(uri, item.mediaMetadata.title?.toString() ?: "本地音频")
                    else -> null
                }
                val result = if (resolved != null) {
                    stableIds.add(resolved.versionId); unavailable.remove(resolved.versionId)
                    titles[resolved.versionId] = resolved.title
                    item.buildUpon().setMediaId(resolved.versionId).setUri(resolved.uri).build()
                } else { titles[item.mediaId] = item.mediaMetadata.title?.toString() ?: "音频"; item }
                metadata(result)
            }
            if (addGeneration != token) throw CancellationException("Superseded media request")
            result
        }
    }

    fun command(action: String, args: Bundle): ListenableFuture<SessionResult> {
        val token = if (action == LibraryPlayback.PLAY_VERSIONS) { ++addGeneration; ++generation } else generation
        return future {
            restore.join()
            try {
                var bookmarkId: String? = null
                var widgetMessage: String? = null
                when (action) {
                    LibraryPlayback.SEEK_POSITION -> {
                        val id = args.getString("versionId")
                        require(id != null && id == player.currentMediaItem?.mediaId)
                        require(!player.isCurrentMediaItemLive && (player.isCurrentMediaItemSeekable || player.playbackState == Player.STATE_IDLE))
                        val target = args.getLong("positionMs").coerceAtLeast(0).let {
                            if (player.duration > 0) it.coerceAtMost(player.duration) else it
                        }
                        // An explicit seek may load a restored session, but never calls play().
                        player.seekTo(player.currentMediaItemIndex, target)
                        if (player.playbackState == Player.STATE_IDLE) player.prepare()
                    }
                    LibraryPlayback.WIDGET_ACTION -> {
                        val id = args.getString("versionId")
                        if (id.isNullOrBlank() || id != player.currentMediaItem?.mediaId)
                            return@future SessionResult(SessionError.ERROR_BAD_VALUE, Bundle().apply { putString("widgetMessage", "音轨已变化，请重新点击") })
                        when (args.getString("operation")) {
                            "play" -> {
                                playbackIntent()
                                if (player.playbackState == Player.STATE_IDLE) player.prepare()
                                if (player.playbackState == Player.STATE_ENDED) player.seekToDefaultPosition()
                                player.play()
                            }
                            "pause" -> { playbackIntent(); player.pause() }
                            "previous" -> { userIntent(); player.seekToPreviousMediaItem() }
                            "next" -> { userIntent(); player.seekToNextMediaItem() }
                            "favorite", "library" -> {
                                val version = requireNotNull(dao.version(id))
                                // Database reads may suspend while another controller changes tracks.
                                if (id != player.currentMediaItem?.mediaId) return@future SessionResult(SessionError.ERROR_BAD_VALUE)
                                if (args.getString("operation") == "favorite") {
                                    require(args.containsKey("favorite"))
                                    val desired = args.getBoolean("favorite")
                                    backend.library.collections.setVersionFavorite(id, desired)
                                    widgetMessage = if (desired) "已添加书签" else "已取消书签"
                                } else {
                                    backend.library.setMembership(version.workId, true)
                                    widgetMessage = "已加入音频库"
                                }
                            }
                            else -> return@future SessionResult(SessionError.ERROR_BAD_VALUE)
                        }
                    }
                    LibraryPlayback.PLAY_VERSIONS -> {
                        val ids = requireNotNull(args.getStringArrayList("versionIds"))
                        val index = args.getInt("index")
                        require(ids.isNotEmpty() && ids.size <= 500 && index in ids.indices)
                        val resolved = ids.map { backend.library.playbackReference(it) }
                        // Commit the outgoing track before looking up the target, including replay of the current item.
                        flush()
                        val saved = if (args.getBoolean("resume", true)) dao.position(ids[index]) else null
                        val position = if (args.containsKey("positionMs")) args.getLong("positionMs").also { require(it >= 0) }
                            else saved?.takeUnless { it.completed }?.positionMs ?: 0
                        if (generation != token) return@future SessionResult(SessionError.INFO_CANCELLED)
                        clearAb(); unavailable.clear(); warning = null
                        val items = resolved.map { v ->
                            stableIds.add(v.versionId); titles[v.versionId] = v.title
                            metadata(MediaItem.Builder().setMediaId(v.versionId).setUri(v.uri).build())
                        }
                        player.setMediaItems(items, index, position); player.prepare(); player.play()
                    }
                    LibraryPlayback.REMOVE_QUEUE_ITEM -> {
                        val ids = (0 until player.mediaItemCount).map { player.getMediaItemAt(it).mediaId }
                        require(ids == args.getStringArrayList("expected"))
                        val index = args.getInt("index", -1)
                        require(index in ids.indices)
                        val item = player.getMediaItemAt(index)
                        userIntent()
                        player.removeMediaItem(index)
                        removedItem = RemovedItem(java.util.UUID.randomUUID().toString(), item, index,
                            (0 until player.mediaItemCount).map { player.getMediaItemAt(it).mediaId })
                    }
                    LibraryPlayback.UNDO_QUEUE_REMOVE -> {
                        val removed = requireNotNull(removedItem)
                        require(removed.token == args.getString("undoToken"))
                        require(removed.remaining == (0 until player.mediaItemCount).map { player.getMediaItemAt(it).mediaId })
                        userIntent()
                        player.addMediaItem(removed.index.coerceAtMost(player.mediaItemCount), removed.item)
                        removedItem = null
                    }
                    LibraryPlayback.ENQUEUE -> {
                        val ids = requireNotNull(args.getStringArrayList("versionIds"))
                        require(ids.isNotEmpty() && ids.size + player.mediaItemCount <= 500)
                        val resolved = ids.map { backend.library.playbackReference(it) }
                        if (generation != token) return@future SessionResult(SessionError.INFO_CANCELLED)
                        require(resolved.size + player.mediaItemCount <= 500)
                        val items = resolved.map { v ->
                            stableIds.add(v.versionId); titles[v.versionId] = v.title
                            metadata(MediaItem.Builder().setMediaId(v.versionId).setUri(v.uri).build())
                        }
                        val at = if(args.getBoolean("next") && player.mediaItemCount > 0) player.currentMediaItemIndex + 1 else player.mediaItemCount
                        player.addMediaItems(at, items)
                    }
                    LibraryPlayback.SET_AB -> {
                        val id = args.getString("versionId")
                        require(id != null && id == player.currentMediaItem?.mediaId && id in stableIds)
                        if (!args.containsKey("aMs") && !args.containsKey("bMs")) clearAb()
                        else {
                            require(args.containsKey("aMs") && args.containsKey("bMs"))
                            val start = args.getLong("aMs"); val end = args.getLong("bMs")
                            require(player.isCurrentMediaItemSeekable && start >= 0 && end > start && end <= player.duration)
                            abVersion = id; a = start; b = end; updatePauseAtEnd()
                        }
                    }
                    LibraryPlayback.SET_SLEEP -> {
                        val next = if (args.containsKey("extendMinutes")) SleepEnvelope.extend(deadline, System.currentTimeMillis(), args.getInt("extendMinutes"))
                            else args.takeIf { it.containsKey("deadlineMs") }?.getLong("deadlineMs")
                        val atEnd = args.getBoolean("endOfTrack")
                        require(next == null || next > System.currentTimeMillis())
                        require(next == null || !atEnd)
                        require(!atEnd || player.mediaItemCount > 0)
                        if (args.containsKey("fadeMs")) {
                            fadeMs = args.getLong("fadeMs").also { require(it in 0..300_000L) }
                            sleepPreferences.edit().putLong("fadeMs", fadeMs).apply()
                        }
                        applyGain(1f); deadline = next; endOfTrack = atEnd; updatePauseAtEnd()
                    }
                    LibraryPlayback.ADD_BOOKMARK -> {
                        val id = requireNotNull(player.currentMediaItem?.mediaId)
                        args.getString("versionId")?.let { require(it == id) { "音轨版本已切换" } }
                        require(id in stableIds)
                        bookmarkId = backend.library.addBookmark(id, player.currentPosition.coerceAtLeast(0), args.getString("note").orEmpty()).id
                    }
                    LibraryPlayback.GET_STATE -> Unit
                    LibraryPlayback.SAVE_SESSION -> Unit
                    LibraryPlayback.PREPARE_UPDATE -> { userIntent(); player.pause() }
                    else -> return@future SessionResult(SessionError.ERROR_NOT_SUPPORTED)
                }
                if (action != LibraryPlayback.GET_STATE) { flush(); notifyChange() }
                if (action == LibraryPlayback.PREPARE_UPDATE) withContext(Dispatchers.IO) {
                    backend.database.openHelper.writableDatabase.query("PRAGMA wal_checkpoint(FULL)").use {
                        check(it.moveToFirst() && it.getInt(0) == 0) { "数据库正在写入，请稍后重试安装" }
                    }
                }
                SessionResult(SessionResult.RESULT_SUCCESS, state().apply {
                    bookmarkId?.let { putString("bookmarkId", it) }
                    widgetMessage?.let { putString("widgetMessage", it) }
                })
            } catch (e: CancellationException) { throw e }
            catch (_: IllegalArgumentException) { SessionResult(SessionError.ERROR_BAD_VALUE) }
            catch (_: Exception) { SessionResult(SessionError.ERROR_IO, Bundle().apply { putString("warning", "操作失败，请检查文件授权或存储空间") }) }
        }
    }

    private fun state() = Bundle().apply {
        removedItem?.let { putString("queueUndoToken", it.token) }
        putLong("fadeMs", fadeMs); putFloat("sleepGain", appliedGain); putBoolean("ready", ready); putString("warning", storage.warning.value ?: warning)
        putString("versionId", player.currentMediaItem?.mediaId?.takeIf { it in stableIds })
        putString("abVersionId", abVersion); a?.let { putLong("aMs", it) }; b?.let { putLong("bMs", it) }
        deadline?.let { putLong("deadlineMs", it) }; putBoolean("endOfTrack", endOfTrack)
        putStringArrayList("unavailableVersionIds", ArrayList(unavailable))
    }
    private fun clearAb() { abVersion = null; a = null; b = null; updatePauseAtEnd() }
    private fun updatePauseAtEnd() { player.pauseAtEndOfMediaItems = endOfTrack || abVersion != null }
    private fun snapshot(): SessionRecord {
        val queue = (0 until player.mediaItemCount).map { player.getMediaItemAt(it).mediaId }
        val persistent = queue.filter { it in stableIds }
        val current = queue.getOrNull(player.currentMediaItemIndex)
        val index = if (current in stableIds) queue.take(player.currentMediaItemIndex).count { it in stableIds } else 0
        return SessionRecord(queueJson = SessionPersistence.encode(persistent), index = index,
            positionMs = if (current in stableIds) player.currentPosition.coerceAtLeast(0) else 0,
            abVersionId = abVersion, aMs = a, bMs = b, sleepDeadlineMs = deadline, sleepEndOfTrack = endOfTrack)
    }
    private fun currentPosition(): PlaybackPosition? {
        val id = player.currentMediaItem?.mediaId?.takeIf { it in stableIds } ?: return null
        // A restored, unprepared timeline has no duration; do not erase its previous completion marker.
        if (player.playbackState == Player.STATE_IDLE && player.duration <= 0) return null
        return PlaybackPosition(id, player.currentPosition.coerceAtLeast(0), player.duration.coerceAtLeast(0),
            completed = completed || player.playbackState == Player.STATE_ENDED)
    }
    private fun enqueue() { if (ready) storage.enqueue(snapshot(), currentPosition()) }
    private suspend fun flush() {
        val ack = CompletableDeferred<Unit>(); storage.enqueue(snapshot(), currentPosition(), ack); ack.await()
    }
    private fun <T> future(block: suspend () -> T): ListenableFuture<T> {
        val result = SettableFuture.create<T>()
        val job = scope.launch { try { result.set(block()) } catch (e: CancellationException) { result.cancel(false) } catch (e: Exception) { result.setException(e) } }
        result.addListener({ if (result.isCancelled) job.cancel() }, Executor { it.run() })
        return result
    }
    fun close() {
        enqueue(); applyGain(1f); player.removeListener(listener); scope.cancel()
    }
}
