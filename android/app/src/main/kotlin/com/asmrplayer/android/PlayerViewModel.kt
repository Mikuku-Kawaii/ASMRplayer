package com.asmrplayer.android

import android.app.Application
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import androidx.core.content.ContextCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.media3.common.*
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.*
import com.asmrplayer.android.playback.*
import com.asmrplayer.core.DspConfig
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import com.asmrplayer.android.data.CatalogMetadata
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.ui.LibraryAudio
import java.util.UUID
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

data class UiFeedback(val message: String, val actionLabel: String? = null, val action: (() -> Unit)? = null)

data class QueueEntry(val id: String, val title: String)
data class PlaybackToolsState(
    val ready: Boolean = false, val versionId: String? = null, val abVersionId: String? = null,
    val aMs: Long? = null, val bMs: Long? = null, val deadlineMs: Long? = null,
    val endOfTrack: Boolean = false, val fadeMs: Long = 30_000, val sleepGain: Float = 1f, val unavailable: Set<String> = emptySet(),
    val warning: String? = null, val busy: Boolean = false,
)
data class PlayerScreenState(
    val connected: Boolean = false, val playing: Boolean = false, val playRequested: Boolean = false, val buffering: Boolean = false,
    val ended: Boolean = false, val speed: Float = 1f,
    val queue: List<QueueEntry> = emptyList(), val index: Int = -1,
    val position: Long = 0, val duration: Long = 0, val seekable: Boolean = false, val canPrepareSeek: Boolean = false, val repeatMode: Int = 0,
    val shuffle: Boolean = false, val canPrevious:Boolean = false, val canNext:Boolean = false,
    val dsp: DspConfig = DspConfig(), val privateMetadata: Boolean = false,
    val loudnessStatus: String = "", val loudnessBusy: Boolean = false, val loudnessProgress: Int = -1,
    val normalizationGainDb: Double = 0.0,
    val error: String? = null, val opening: Boolean = false,
    val lookup: com.asmrplayer.android.ui.LibraryLookup = com.asmrplayer.android.ui.LibraryLookup(), val libraryReady: Boolean = false,
    val showNsfw: Boolean = false, val includeUnrated: Boolean = true,
    val theme: String = "dark", val colorPreset: String = "berry", val lowEffects: Boolean = false,
    val metadata: Map<String, CatalogMetadata> = emptyMap(),
    val session: PlaybackToolsState = PlaybackToolsState(),
    val headphones: HeadphoneState = HeadphoneState(), val headphonesReady: Boolean = false,
    val headphonesBusy: Boolean = false,
) {
    val library get() = lookup.works
    val currentWork get() = lookup.forVersion(queue.getOrNull(index)?.id)
}

@UnstableApi
@OptIn(ExperimentalCoroutinesApi::class)
class PlayerViewModel(application: Application) : AndroidViewModel(application) {
    private val mutable = MutableStateFlow(PlayerScreenState())
    val screenState = mutable.asStateFlow()
    private val position = MutableStateFlow(0L)
    val positionMs = position.asStateFlow()
    // Legacy callers can read the result of an edit synchronously, even on the main thread.
    // The UI subscribes to screenState and positionMs separately.
    private fun snapshot() = mutable.value.copy(position = position.value)
    val state: StateFlow<PlayerScreenState> by lazy {
        val updates = combine(screenState, position) { screen, ms -> screen.copy(position = ms) }
            .stateIn(viewModelScope, SharingStarted.Eagerly, snapshot())
        object : StateFlow<PlayerScreenState> by updates {
            override val value get() = snapshot()
            override val replayCache get() = listOf(value)
        }
    }
    private val syncStatusMutable = MutableStateFlow<com.asmrplayer.android.sync.SyncStatus?>(null)
    val syncStatus = syncStatusMutable.asStateFlow()
    private var announcedResume: String? = null
    private val foreground = MutableStateFlow(true)
    fun setForeground(visible: Boolean) { foreground.value = visible }
    private var soundDirty = true
    private var sessionDirty = true
    private val feedbackChannel = kotlinx.coroutines.channels.Channel<UiFeedback>(kotlinx.coroutines.channels.Channel.UNLIMITED)
    val feedback = feedbackChannel.receiveAsFlow()
    fun feedback(message: String, actionLabel: String? = null, action: (() -> Unit)? = null) {
        feedbackChannel.trySend(UiFeedback(message, actionLabel, action))
    }
    val tools = LibraryTools(application, viewModelScope)
    val incoming = com.asmrplayer.android.incoming.IncomingTools(application, viewModelScope, tools::message)
    private val playlistSelection = MutableStateFlow<List<String>?>(null)
    val playlistTarget = playlistSelection.asStateFlow()
    fun choosePlaylist(ids: List<String>) {
        val selected = ids.distinct()
        if (selected.isEmpty()) { feedback("没有可加入的音轨"); return }
        if (selected.size > 500) { feedback("每个歌单最多 500 首音轨"); return }
        playlistSelection.value = selected
    }
    fun closePlaylistPicker() { playlistSelection.value = null }
    val queueGroups = QueueGroupTools(application, viewModelScope, ::feedback)
    val details = DetailTools(application, viewModelScope)
    val downloadPlan = DownloadPlanTools(application, viewModelScope) { feedback(it) }
    val collections = CollectionTools(application, viewModelScope, ::feedback)
    val maintenance = MaintenanceTools(application, viewModelScope) {
        check(send(LibraryPlayback.SAVE_SESSION, Bundle.EMPTY).resultCode == SessionResult.RESULT_SUCCESS)
    }
    val online = OnlineTools(application, viewModelScope, enqueue=::enqueue, feedback=::feedback, playlist=::choosePlaylist, play=::playGroup)
    private val sessionMutex = Mutex()
    private val mainExecutor = ContextCompat.getMainExecutor(application)
    private val connection = MediaController.Builder(application, SessionToken(application, ComponentName(application, PlaybackService::class.java)))
        .setListener(object : MediaController.Listener {
            override fun onExtrasChanged(controller: MediaController, extras: Bundle) {
                if (extras.getString("changed") == "sound") soundDirty = true else sessionDirty = true
            }
        }).buildAsync()
    private var controller: MediaController? = null
    private var openGeneration = 0L
    private var settingsGeneration = 0L
    private val soundMutex = Mutex()
    private var pendingSoundEdits = 0
    private var titles = emptyMap<String, String>()
    private val libraryStore = AndroidBackend.get(application).library
    private val libraryMutex = Mutex()
    private val appearance = application.getSharedPreferences("appearance", Application.MODE_PRIVATE)
    private val listener = object : Player.Listener {
        override fun onEvents(player: Player, events: Player.Events) {
            val queueChanged = events.containsAny(Player.EVENT_TIMELINE_CHANGED, Player.EVENT_MEDIA_METADATA_CHANGED)
            refresh(queueChanged)
            if (events.containsAny(Player.EVENT_MEDIA_ITEM_TRANSITION, Player.EVENT_PLAYBACK_STATE_CHANGED)) {
                sessionDirty = true
                soundDirty = true
            }
            if (events.contains(Player.EVENT_TIMELINE_CHANGED)) viewModelScope.launch {
                try { readTitles(send(DspSettings.GET_DSP, Bundle.EMPTY)); refresh(queueChanged = true) }
                catch (e: CancellationException) { throw e }
                catch (_: Exception) { /* Playback state remains available through the controller. */ }
            }
        }
        override fun onPlayerError(error: PlaybackException) {
            // Do not surface signed URLs or provider credentials from nested decoder/network errors.
            mutable.update { it.copy(error = "播放失败（${error.errorCodeName}）。请检查文件授权、网络或音频格式。") }
        }
    }

    init {
        viewModelScope.launch {
            foreground.collectLatest { visible ->
                if(visible) while(isActive) {
                    try {
                        val status=withContext(Dispatchers.IO) { AndroidBackend.get(getApplication()).sync.status() }
                        syncStatusMutable.value=status
                        status.resume?.let { resume ->
                            if(announcedResume!=resume.optString("id")) {
                                announcedResume=resume.optString("id")
                                feedback("另一台设备有更新的收听位置", "继续收听") {resumeSynced(resume.getString("id"),status.provider)}
                            }
                        }
                    } catch(e:CancellationException) { throw e } catch(_:Exception) { /* Sync page exposes binding errors. */ }
                    delay(4000)
                }
            }
        }

        viewModelScope.launch {
            AndroidBackend.get(application).database.metadata().observe()
                .map { rows -> rows.associateBy { it.workId } }.flowOn(Dispatchers.Default)
                .collect { metadata -> mutable.update { it.copy(metadata = metadata) } }
        }
        mutable.update { it.copy(theme = appearance.getString("theme", "dark") ?: "dark",
            colorPreset = appearance.getString("colorPreset", "berry")?.takeIf { key -> key in setOf("ink_blue", "wisteria", "berry", "amber") } ?: "berry",
            lowEffects = appearance.getBoolean("lowEffects", false)) }
        viewModelScope.launch {
            screenState.map { current ->
                val id = current.queue.getOrNull(current.index)?.id
                val work = current.lookup.forVersion(id)
                (id?.takeIf { work != null }) to work?.id
            }.distinctUntilChanged().collect { (version, work) -> tools.selectVersion(version, work) }
        }
        viewModelScope.launch {
            try {
                combine(libraryStore.works, libraryStore.versionsByWork) { works, versions ->
                    com.asmrplayer.android.ui.LibraryLookup(works.map { work ->
                        LibraryAudio(work.id, work.title, work.favorite, work.rating, versions[work.id].orEmpty(),
                            work.coverUri, work.code, work.inLibrary)
                    })
                }.flowOn(Dispatchers.Default).collect { lookup -> mutable.update { it.copy(lookup = lookup, libraryReady = true) } }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { showError("音频库无法读取，请重新打开应用重试。") }
        }
        viewModelScope.launch {
            libraryStore.preferences.catch { showError("内容显示设置无法读取。") }.collect { prefs ->
                mutable.update { it.copy(showNsfw = prefs.includeNsfw, includeUnrated = prefs.includeUnrated) }
            }
        }
        viewModelScope.launch {
            try {
                controller = connection.awaitResult()
                controller!!.addListener(listener)
                foreground.collectLatest { visible ->
                    if (!visible) return@collectLatest
                    var needSettings = true
                    soundDirty = true
                    sessionDirty = true
                    while (currentCoroutineContext().isActive) {
                        if (needSettings) {
                            try {
                                soundMutex.withLock { readSettings() }
                                needSettings = false
                            } catch (e: CancellationException) { throw e }
                            catch (_: Exception) { /* Retry while the activity is visible. */ }
                        }
                        samplePosition()
                        val session = mutable.value.session
                        if (sessionDirty || !session.ready || session.deadlineMs != null) {
                            sessionDirty = false
                            try { sessionMutex.withLock { readSession(send(LibraryPlayback.GET_STATE, Bundle.EMPTY)) } }
                            catch (e: CancellationException) { throw e }
                            catch (_: Exception) { mutable.update { it.copy(session = it.session.copy(ready = false, warning = "播放工具状态暂不可用，正在重新连接。")) } }
                        }
                        if (soundDirty || !mutable.value.headphonesReady || mutable.value.loudnessBusy) {
                            try { soundMutex.withLock {
                                if (pendingSoundEdits == 0) {
                                    soundDirty = false
                                    val generation = settingsGeneration
                                    val result = send(HeadphoneCommands.GET, Bundle.EMPTY)
                                    if (generation == settingsGeneration && pendingSoundEdits == 0) readHeadphones(result)
                                }
                            } } catch (e: CancellationException) { throw e }
                            catch (_: Exception) { mutable.update { it.copy(headphonesReady = false) } }
                        }
                        delay(500)
                    }
                }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { mutable.update { it.copy(error = "无法连接播放服务，请重新打开应用。", connected = false) } }
        }
    }

    fun refreshQueue() {
        refresh(queueChanged = true)
        sessionDirty = true
        queueGroups.refresh()
    }

    private fun samplePosition() { controller?.let { position.value = it.currentPosition.coerceAtLeast(0) } }

    private fun refresh(queueChanged: Boolean = false) {
        val p = controller ?: return
        samplePosition()
        mutable.update { it.copy(
            connected = p.isConnected, playing = p.isPlaying,
            playRequested = p.playWhenReady && p.playbackState in listOf(Player.STATE_BUFFERING, Player.STATE_READY),
            buffering = p.playbackState == Player.STATE_BUFFERING, ended = p.playbackState == Player.STATE_ENDED,
            queue = if (!queueChanged) it.queue else List(p.mediaItemCount) { i -> p.getMediaItemAt(i).let { item -> QueueEntry(item.mediaId, titles[item.mediaId] ?: item.mediaMetadata.title?.toString() ?: "音频") } },
            index = if (p.mediaItemCount == 0) -1 else p.currentMediaItemIndex,
            duration = p.duration.takeIf { d -> d > 0 } ?: 0,
            seekable = p.isCurrentMediaItemSeekable,
            canPrepareSeek = p.currentMediaItem != null && p.playbackState == Player.STATE_IDLE && !p.isCurrentMediaItemLive,
            repeatMode = p.repeatMode, speed = p.playbackParameters.speed,
            shuffle=p.shuffleModeEnabled,canPrevious=p.hasPreviousMediaItem(),canNext=p.hasNextMediaItem(),
        ) }
    }

    fun openDocuments(uris: List<Uri>) {
        if (uris.isEmpty()) return
        openSources(uris, true)
    }

    fun openStream(address: String): Boolean {
        val uri = Uri.parse(address.trim())
        if (uri.scheme != "https" || uri.host.isNullOrBlank() || uri.userInfo != null) {
            showError("请输入不含用户名和密码的 HTTPS 音频地址。")
            return false
        }
        openSources(listOf(uri), false)
        return true
    }

    private fun openSources(uris: List<Uri>, documents: Boolean) {
        val generation = ++openGeneration
        mutable.update { it.copy(opening = true, error = null) }
        viewModelScope.launch {
            try {
                require(uris.size <= 500) { "一次最多打开 500 个音频文件。" }
                val app = getApplication<Application>()
                val sources = withContext(Dispatchers.IO) {
                    uris.distinct().map { uri ->
                        val title = if (documents) {
                            app.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
                            app.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                                if (cursor.moveToFirst()) cursor.getString(0) else null
                            } ?: "本地音频"
                        } else "在线音频"
                        if (documents) libraryStore.importDocument(uri, title).versionId else uri.toString()
                    }
                }
                val p = controller ?: connection.awaitResult()
                if (generation != openGeneration) return@launch
                if (documents) {
                    check(LibraryPlayback.play(p, sources).awaitResult().resultCode == SessionResult.RESULT_SUCCESS)
                } else {
                    p.setMediaItems(sources.map { MediaItem.Builder().setMediaId(UUID.randomUUID().toString()).setUri(it)
                        .setMediaMetadata(MediaMetadata.Builder().setTitle("在线音频").build()).build() })
                    p.prepare()
                    p.play()
                }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) {
                if (generation == openGeneration) showError("无法打开音频，请重新选择文件或检查地址。一次最多打开 500 个文件。")
            } finally {
                if (generation == openGeneration) mutable.update { it.copy(opening = false) }
            }
        }
    }

    fun togglePlayback() {
        controller?.let { p ->
            if (p.playWhenReady && p.playbackState in listOf(Player.STATE_BUFFERING, Player.STATE_READY)) p.pause()
            else {
                if (p.playbackState == Player.STATE_IDLE) p.prepare()
                if (p.playbackState == Player.STATE_ENDED) p.seekToDefaultPosition()
                p.play()
            }
        }
    }
    fun stop() { ++openGeneration; controller?.stop(); mutable.update { it.copy(opening = false) } }
    fun previous() { controller?.seekToPreviousMediaItem() }
    fun next() { controller?.seekToNextMediaItem() }
    fun seek(position: Long) {
        val version = controller?.currentMediaItem?.mediaId ?: return
        viewModelScope.launch {
            try {
                sessionMutex.withLock {
                    readSession(send(LibraryPlayback.SEEK_POSITION, Bundle().apply {
                        putString("versionId", version); putLong("positionMs", position.coerceAtLeast(0))
                    }))
                }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { tools.fail("跳转未完成，请检查当前音轨和文件授权。") }
        }
    }
    fun select(index: Int) { controller?.let { it.seekToDefaultPosition(index); it.prepare(); it.play() } }
    fun playLibrary(id: String, groupPath: String? = null, format: String? = null) {
        val work = mutable.value.library.firstOrNull { it.id == id } ?: return
        val first = work.versions.firstOrNull { it.available }
        val group = groupPath ?: first?.groupPath
        val selectedFormat = format ?: first?.format
        val versions = work.versions.filter { it.available && it.groupPath == group && it.format == selectedFormat }.map { it.id }
        playVersions(versions)
    }
    fun resumeSynced(id: String, provider:com.asmrplayer.android.sync.Storage) {
        if (screenState.value.opening) return
        val generation=++openGeneration
        mutable.update { it.copy(opening=true,error=null) }
        viewModelScope.launch {
            try {
                val repo=AndroidBackend.get(getApplication()).sync
                val resume=withContext(Dispatchers.IO) { repo.resumeCandidate(provider,id) }
                    ?: error("此位置已失效或已在本机继续收听")
                if(generation!=openGeneration) return@launch
                val p=controller ?: connection.awaitResult()
                if(generation!=openGeneration) return@launch
                check(LibraryPlayback.play(p,listOf(resume.getString("versionId")),startPositionMs=resume.getLong("positionMs")).awaitResult().resultCode==SessionResult.RESULT_SUCCESS)
                withContext(Dispatchers.IO) { repo.dismissResume(provider,id) }
                syncStatusMutable.value=withContext(Dispatchers.IO) { repo.status() }
            } catch(e:CancellationException) { throw e }
            catch(e:Exception) { if(generation==openGeneration) showError(e.message ?: "无法继续收听，请检查音源") }
            finally { if(generation==openGeneration) mutable.update { it.copy(opening=false) } }
        }
    }
    fun dismissSynced(id:String,provider:com.asmrplayer.android.sync.Storage) {
        viewModelScope.launch {
            try { val repo=AndroidBackend.get(getApplication()).sync
                syncStatusMutable.value=withContext(Dispatchers.IO) {repo.dismissResume(provider,id);repo.status()} }
            catch(e:CancellationException){throw e}catch(e:Exception){showError(e.message ?: "操作未完成")}
        }
    }
    fun playVersion(id: String) = playVersions(listOf(id))
    fun repeat(mode: Int) { if (mode in 0..2) playbackOrder(PlaybackOrder.from(mode,false)) }
    fun playbackOrder(order:PlaybackOrder) { controller?.let { it.repeatMode=order.repeatMode;it.shuffleModeEnabled=order.shuffle } }
    fun playGroup(ids: List<String>, fromStart: Boolean = false, startVersion:String?=null) = playVersionsAt(ids, if(fromStart) 0 else null,ids.indexOf(startVersion).coerceAtLeast(0))
    fun enqueue(ids: List<String>, next: Boolean = false) {
        if(ids.isEmpty()) return
        toolCommand(LibraryPlayback.ENQUEUE, Bundle().apply {
            putStringArrayList("versionIds", ArrayList(ids)); putBoolean("next", next)
        }, if(next) "已设为下一首播放。" else "已加入队列。")
    }
    private fun playVersions(ids: List<String>) = playVersionsAt(ids, null)
    fun playBookmark(versionId: String, positionMs: Long) = playVersionsAt(listOf(versionId), positionMs)
    private fun playVersionsAt(ids: List<String>, positionMs: Long?, index:Int=0) {
        if (ids.isEmpty()) { showError("没有可用音轨，请重新授权或扫描作品目录。"); return }
        val generation = ++openGeneration
        mutable.update { it.copy(opening = true, error = null) }
        viewModelScope.launch {
            try {
                val p = controller ?: connection.awaitResult()
                if (generation != openGeneration) return@launch
                check(LibraryPlayback.play(p, ids, index=index, startPositionMs = positionMs).awaitResult().resultCode == SessionResult.RESULT_SUCCESS)
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { if (generation == openGeneration) showError("无法播放作品，请检查文件授权。") }
            finally { if (generation == openGeneration) mutable.update { it.copy(opening = false) } }
        }
    }
    fun toggleFavorite(id: String) = editLibrary {
        val old = requireNotNull(libraryStore.work(id)).favorite
        libraryStore.setFavorite(id, !old)
        feedback(if(old) "已取消喜欢" else "已加入喜欢", "撤销") { editLibrary { libraryStore.setFavorite(id, old) } }
    }
    fun addToLibrary(id: String) = editLibrary {
        val work = requireNotNull(libraryStore.work(id))
        if (work.inLibrary) { feedback("已在音频库中"); return@editLibrary }
        libraryStore.setMembership(id, true)
        feedback("已加入音频库", "撤销") { editLibrary { libraryStore.setMembership(id, false) } }
    }
    fun setLibraryMembership(id:String,member:Boolean) = editLibrary { libraryStore.setMembership(id,member) }
    fun downloadCurrent() {
        val work = mutable.value.library.firstOrNull { it.containsVersion(mutable.value.queue.getOrNull(mutable.value.index)?.id) }
        val id = mutable.value.queue.getOrNull(mutable.value.index)?.id ?: return
        viewModelScope.launch {
            try {
                val backend=AndroidBackend.get(getApplication())
                if(work==null||backend.database.library().remote(id)==null) {feedback("当前音轨已是本地文件，或没有可下载来源");return@launch}
                downloadPlan.open(workId=work.id, initialIds=setOf(id))
            }
            catch(e: CancellationException) { throw e } catch(_: Exception) { showError("无法添加下载，请在下载管理中检查来源。") }
        }
    }
    fun speed(value: Float) { if(value in .5f..2f) controller?.setPlaybackSpeed(value) }
    fun setNsfw(id: String, enabled: Boolean) = editLibrary { libraryStore.setRating(id, if (enabled) "NSFW" else "SAFE") }
    private fun editLibrary(change: suspend () -> Unit) {
        viewModelScope.launch {
            try { libraryMutex.withLock { change() } }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { showError("作品设置未保存，请重试。") }
        }
    }
    fun setShowNsfw(value: Boolean) = editLibrary { libraryStore.setContentFilter(value, mutable.value.includeUnrated) }
    fun setIncludeUnrated(value: Boolean) = editLibrary { libraryStore.setContentFilter(mutable.value.showNsfw, value) }
    fun moveQueue(from: Int, to: Int, expected: List<String>) = editQueue(expected) { p ->
        require(from in expected.indices && to in expected.indices); p.moveMediaItem(from, to)
    }
    fun removeQueue(index: Int, expected: List<String>) {
        if (controllerForQueue(expected) == null) return
        ++openGeneration
        mutable.update { it.copy(opening = false) }
        toolCommand(LibraryPlayback.REMOVE_QUEUE_ITEM, Bundle().apply {
            putInt("index", index); putStringArrayList("expected", ArrayList(expected))
        }, "已移出队列") { result ->
            val token = result.extras.getString("queueUndoToken")
            feedback("已移出队列", "撤销") {
                toolCommand(LibraryPlayback.UNDO_QUEUE_REMOVE, Bundle().apply { putString("undoToken", token) }, "已恢复到队列")
            }
        }
    }
    private fun controllerForQueue(expected: List<String>): MediaController? {
        val p = controller ?: return null
        if (p.mediaItemCount == expected.size && expected.indices.all { p.getMediaItemAt(it).mediaId == expected[it] }) return p
        showError("队列已变化，请重试。")
        return null
    }
    private fun editQueue(expected: List<String>, change: (MediaController) -> Unit) {
        val p = controllerForQueue(expected) ?: return
        ++openGeneration
        mutable.update { it.copy(opening = false) }
        change(p); refresh(queueChanged = true)
    }
    suspend fun prepareUpdate() = AndroidBackend.get(getApplication()).updates.prepareInstall(controller ?: connection.awaitResult())
    fun setTheme(value: String) { appearance.edit().putString("theme", value).apply(); mutable.update { it.copy(theme = value) } }
    fun setLowEffects(value: Boolean) { appearance.edit().putBoolean("lowEffects", value).apply(); mutable.update { it.copy(lowEffects = value) } }
    fun setColorPreset(value: String) {
        if(value !in setOf("ink_blue", "wisteria", "berry", "amber")) return
        appearance.edit().putString("colorPreset", value).apply()
        mutable.update { it.copy(colorPreset=value) }
    }
    fun showError(message: String?) { mutable.update { it.copy(error = message) } }

    private fun readSession(result: SessionResult) {
        check(result.resultCode == SessionResult.RESULT_SUCCESS)
        val extra = result.extras
        mutable.update { current -> current.copy(session = PlaybackToolsState(
            ready = extra.getBoolean("ready"), versionId = extra.getString("versionId"), abVersionId = extra.getString("abVersionId"),
            aMs = if (extra.containsKey("aMs")) extra.getLong("aMs") else null,
            bMs = if (extra.containsKey("bMs")) extra.getLong("bMs") else null,
            deadlineMs = if (extra.containsKey("deadlineMs")) extra.getLong("deadlineMs") else null,
            fadeMs = extra.getLong("fadeMs", 30_000), sleepGain = extra.getFloat("sleepGain", 1f),
            endOfTrack = extra.getBoolean("endOfTrack"), unavailable = extra.getStringArrayList("unavailableVersionIds").orEmpty().toSet(),
            warning = extra.getString("warning"), busy = current.session.busy,
        )) }
    }

    private fun toolCommand(action: String, args: Bundle, success: String, done: ((SessionResult) -> Unit)? = null) {
        if (mutable.value.session.busy) return
        mutable.update { it.copy(session = it.session.copy(busy = true)) }
        viewModelScope.launch {
            try {
                val result = sessionMutex.withLock { send(action, args).also { readSession(it) } }
                if(done != null) done(result) else feedback(success)
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { tools.fail("操作未完成，请检查当前音轨、文件授权和播放状态后重试。") }
            finally { mutable.update { it.copy(session = it.session.copy(busy = false)) } }
        }
    }

    fun addBookmark(version: String, note: String, saved: ((String) -> Unit)? = null) = toolCommand(LibraryPlayback.ADD_BOOKMARK,
        Bundle().apply { putString("versionId", version); putString("note", note) }, "标记已保存。") { result ->
            val id = result.extras.getString("bookmarkId")
            id?.let { saved?.invoke(it) }
            feedback("标记已保存", "撤销") { id?.let { collections.removeBookmark(it) } }
        }
    fun setAb(version: String, a: Long?, b: Long?) = toolCommand(LibraryPlayback.SET_AB, Bundle().apply {
        putString("versionId", version)
        if (a != null && b != null) { putLong("aMs", a); putLong("bMs", b) }
    }, if (a == null) "A–B 循环已关闭。" else "A–B 循环已开启。")
    fun setSleep(minutes: Int? = null, endOfTrack: Boolean = false, fadeMs: Long = mutable.value.session.fadeMs) = toolCommand(LibraryPlayback.SET_SLEEP, Bundle().apply {
        if (minutes != null) { require(minutes in 1..1440); putLong("deadlineMs", System.currentTimeMillis() + minutes * 60_000L) }
        putBoolean("endOfTrack", endOfTrack); putLong("fadeMs", fadeMs)
    }, if (minutes == null && !endOfTrack) "睡眠定时已取消。" else "睡眠定时已设置。")
    fun setSleepFade(fadeMs: Long) = toolCommand(LibraryPlayback.SET_SLEEP, Bundle().apply {
        mutable.value.session.deadlineMs?.let { putLong("deadlineMs",it) }
        putBoolean("endOfTrack",mutable.value.session.endOfTrack);putLong("fadeMs",fadeMs)
    }, "渐弱时长已更新。")
    fun extendSleep(minutes: Int) = toolCommand(LibraryPlayback.SET_SLEEP, Bundle().apply { putInt("extendMinutes", minutes) }, "睡眠定时已延长。")

    fun seekBookmark(version: String, position: Long) {
        val p = controller ?: return
        if (p.currentMediaItem?.mediaId == version && p.isCurrentMediaItemSeekable) p.seekTo(position)
        else tools.fail("音轨已切换或尚未加载，请先播放对应版本。")
    }

    fun updateDsp(next: DspConfig) {
        val generation = ++settingsGeneration
        ++pendingSoundEdits
        val previous = mutable.value.dsp
        mutable.update { it.copy(dsp = next) }
        viewModelScope.launch {
            try {
                val result = soundMutex.withLock { send(DspSettings.SET_DSP, Bundle().apply { putString(DspSettings.JSON, DspSettings.encode(next)) }) }
                check(result.resultCode == SessionResult.RESULT_SUCCESS)
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) {
                if (generation == settingsGeneration) {
                    mutable.update { it.copy(dsp = previous) }
                    try { readSettings() } catch (e: CancellationException) { throw e } catch (_: Exception) { /* Keep the last displayed value when disconnected. */ }
                    showError("音效设置未保存，请重试。")
                }
            } finally { --pendingSoundEdits }
        }
    }

    private fun readHeadphones(result: SessionResult) {
        check(result.resultCode == SessionResult.RESULT_SUCCESS)
        val headphones = HeadphoneCommands.decode(requireNotNull(result.extras.getString("headphones")))
        val dsp = DspSettings.decode(requireNotNull(result.extras.getString(DspSettings.JSON)))
        mutable.update { it.copy(headphones = headphones, headphonesReady = true, dsp = dsp,
            loudnessStatus = result.extras.getString("loudnessStatus").orEmpty(),
            loudnessBusy = result.extras.getBoolean("loudnessBusy"), loudnessProgress = result.extras.getInt("loudnessProgress", -1),
            normalizationGainDb = result.extras.getDouble("normalizationGainDb")) }
    }

    fun analyzeLoudness(cancel: Boolean = false) {
        viewModelScope.launch {
            try { soundMutex.withLock { readHeadphones(send(if (cancel) DspSettings.CANCEL_ANALYSIS else DspSettings.ANALYZE, Bundle.EMPTY)) } }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { showError("响度分析操作未完成，请重试。") }
        }
    }

    fun headphoneAction(operation: String, id: String? = null, name: String? = null, route: String? = null, enabled: Boolean = false, onSuccess: (() -> Unit)? = null) {
        if (mutable.value.headphonesBusy) return
        ++settingsGeneration
        ++pendingSoundEdits
        mutable.update { it.copy(headphonesBusy = true) }
        viewModelScope.launch {
            try {
                soundMutex.withLock { readHeadphones(send(HeadphoneCommands.EDIT, Bundle().apply {
                    putString("operation", operation); putString("id", id); putString("name", name)
                    putString("route", route); putBoolean("enabled", enabled)
                })) }
                feedback(when (operation) {
                    "save", "common" -> "音效方案已保存。"
                    "select" -> "音效方案已应用。"
                    "bind" -> "输出设备关联已保存。"
                    "remove" -> "方案及其设备关联已删除。"
                    else -> "耳机偏好已更新。"
                })
                onSuccess?.invoke()
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { feedback("未完成：请检查方案名称或输出设备是否已变化后重试。") }
            finally { --pendingSoundEdits; mutable.update { it.copy(headphonesBusy = false) } }
        }
    }

    fun setPrivacy(enabled: Boolean) {
        viewModelScope.launch {
            try {
                val result = send(DspSettings.SET_PRIVACY, Bundle().apply { putBoolean("private", enabled) })
                check(result.resultCode == SessionResult.RESULT_SUCCESS)
                mutable.update { it.copy(privateMetadata = enabled) }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { showError("系统媒体信息设置未保存，请重试。") }
        }
    }

    private suspend fun readSettings() {
        val generation = settingsGeneration
        val result = send(DspSettings.GET_DSP, Bundle.EMPTY)
        check(result.resultCode == SessionResult.RESULT_SUCCESS)
        val dsp = DspSettings.decode(requireNotNull(result.extras.getString(DspSettings.JSON)))
        readTitles(result)
        mutable.update { it.copy(dsp = if (settingsGeneration == generation && pendingSoundEdits == 0) dsp else it.dsp, privateMetadata = result.extras.getBoolean("private", false), error = result.extras.getString("warning") ?: it.error) }
        refresh(queueChanged = true)
    }

    private fun readTitles(result: SessionResult) {
        titles = (result.extras.getStringArrayList("titleIds") ?: arrayListOf()).zip(result.extras.getStringArrayList("titles") ?: arrayListOf()).toMap()
    }

    private suspend fun send(action: String, args: Bundle): SessionResult =
        (controller ?: connection.awaitResult()).sendCustomCommand(SessionCommand(action, Bundle.EMPTY), args).awaitResult()

    private suspend fun <T> ListenableFuture<T>.awaitResult(): T = suspendCancellableCoroutine { c ->
        addListener({ if (c.isActive) try { c.resume(get()) } catch (e: Exception) { c.resumeWithException(e) } }, mainExecutor)
    }

    override fun onCleared() {
        controller?.removeListener(listener)
        MediaController.releaseFuture(connection)
        super.onCleared()
    }
}
