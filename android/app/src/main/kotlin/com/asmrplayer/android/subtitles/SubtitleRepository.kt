package com.asmrplayer.android.subtitles

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.room.withTransaction
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.online.AsmrOneProvider
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.ensureActive
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.util.UUID

private const val DISPLAY_TIMEOUT_MS = 20_000L

data class BilingualSubtitles(val preference: SubtitlePreference, val original: SubtitleTimeline?, val translation: SubtitleTimeline?,
    val originalError: String? = null, val translationError: String? = null) {
    fun active(positionMs: Long): Pair<List<SubtitleCue>, List<SubtitleCue>> =
        original?.active(positionMs, preference.offsetMs).orEmpty() to
            translation?.active(positionMs, preference.offsetMs + preference.translationOffsetMs).orEmpty()
}

class SubtitleRepository(private val context: Context, private val db: LibraryDatabase, private val provider: AsmrOneProvider? = null) {
    private val dao = db.library()
    fun sources(versionId: String) = dao.observeSubtitles(versionId)
    fun preference(versionId: String) = dao.observeSubtitlePreference(versionId)

    suspend fun select(value: SubtitlePreference) {
        requireNotNull(dao.version(value.versionId))
        require(value.offsetMs in -600_000L..600_000L && value.translationOffsetMs in -600_000L..600_000L)
        listOfNotNull(value.originalId, value.translationId).forEach { id ->
            require(dao.subtitle(id)?.versionId == value.versionId) { "字幕属于另一音轨版本" }
        }
        dao.putSubtitlePreference(value)
    }

    /** Call with a scanned asset URI (persistGrant=false) or an OpenDocument result (true). */
    suspend fun attach(versionId: String, uri: Uri, name: String, persistGrant: Boolean = true): SubtitleSource {
        requireNotNull(dao.version(versionId)); require(uri.scheme == "content")
        val format = name.substringAfterLast('.', "").lowercase()
        require(format in LibraryIndex.subtitleExtensions)
        if (persistGrant) withContext(Dispatchers.IO) {
            context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        val asset = dao.assetByUri(uri.toString())
        val source = SubtitleSource(UUID.randomUUID().toString(), versionId, asset?.rootId,
            asset?.relativePath ?: name, uri.toString(), name, format, manual = true)
        read(source) // Reject invalid input before persisting a usable association.
        dao.putSubtitle(source)
        return source
    }

    suspend fun load(versionId: String, allowPartial: Boolean = false): BilingualSubtitles {
        discover(versionId)
        val saved = dao.subtitlePreference(versionId)
        // A stored null is the user's explicit "不显示" choice, not an invitation to auto-select.
        if (saved == null) {
            val version = requireNotNull(dao.version(versionId))
            val sources = dao.observeSubtitles(versionId).first().filter { it.available }
            val total = dao.observeVersions(version.workId).first().distinctBy { SubtitleMatcher.stem(it.title) }.size
            var failure: Exception? = null
            val ranked = sources.map { source -> source to (if (source.manual) 2000 else
                SubtitleMatcher.score(source.relativePath, version.title, version.groupPath, total, sources.size) +
                    if (source.uri.startsWith("asmr-subtitle:")) 0 else 25) }.sortedByDescending { it.second }
            suspend fun automatic(): BilingualSubtitles {
                for ((source, _) in ranked) {
                    try {
                        val timeline = read(source)
                        // Keep automatic defaults ephemeral so a later better/local attachment can win.
                        return BilingualSubtitles(SubtitlePreference(versionId, originalId = source.id), timeline, null)
                    } catch (e: CancellationException) { throw e }
                    catch (e: Exception) { failure = e }
                }
                failure?.let {
                    if (!allowPartial) throw it
                    return BilingualSubtitles(SubtitlePreference(versionId), null, null, "匹配到的原文字幕无法读取。请重试或重新选择文件。")
                }
                return BilingualSubtitles(SubtitlePreference(versionId), null, null)
            }
            return if (!allowPartial) automatic() else withTimeoutOrNull(DISPLAY_TIMEOUT_MS) { automatic() }
                ?: BilingualSubtitles(SubtitlePreference(versionId), null, null, "原文字幕读取超时。请检查来源连接后重试。")
        }
        val pref = saved
        suspend fun timeline(id: String?): Pair<SubtitleTimeline?, String?> = try {
            if (id == null) null to null
            else if (!allowPartial) read(requireNotNull(dao.subtitle(id))) to null
            else withTimeoutOrNull(DISPLAY_TIMEOUT_MS) { read(requireNotNull(dao.subtitle(id))) to null }
                ?: (null to "字幕读取超时。请检查来源连接后重试。")
        } catch (e: CancellationException) { throw e }
        catch (e: Exception) {
            if (!allowPartial) throw e
            null to "文件无法读取。请检查来源连接或文件授权，也可以重新选择字幕。"
        }
        val (original, originalError) = timeline(pref.originalId)
        val (translation, translationError) = timeline(pref.translationId)
        return BilingualSubtitles(pref, original, translation, originalError, translationError)
    }

    suspend fun retryRole(previous: BilingualSubtitles, translation: Boolean): BilingualSubtitles {
        val pref = previous.preference
        val id = if (translation) pref.translationId else pref.originalId
        // An automatic original has no saved selection: rank candidates again.
        if (id == null) return load(pref.versionId, allowPartial = true)
        val timeline = try { withTimeoutOrNull(DISPLAY_TIMEOUT_MS) { read(requireNotNull(dao.subtitle(id))) } ?: return previous }
        catch (e: CancellationException) { throw e }
        catch (_: Exception) { return previous }
        return if (translation) previous.copy(translation = timeline, translationError = null)
            else previous.copy(original = timeline, originalError = null)
    }

    /** Match already indexed work attachments on playback, including libraries imported before this fix. */
    private suspend fun discover(versionId: String) {
        val version = dao.version(versionId) ?: return
        val work = requireNotNull(dao.work(version.workId))
        val local = dao.observeAssets(work.id).first().filter { it.available && it.format in LibraryIndex.subtitleExtensions }
        val remote = dao.observeRemoteAssets(work.id).first().filter { it.available && it.kind == "subtitle" }
        val existing = dao.observeSubtitles(versionId).first()
        val tracks = dao.observeVersions(work.id).first().distinctBy { SubtitleMatcher.stem(it.title) }.size
        val count = (local.map { it.relativePath } + remote.map { it.relativePath }).distinct().size
        fun matches(path: String): Boolean = SubtitleMatcher.score(path.removePrefix(work.relativePath.takeIf { it.isNotEmpty() }?.plus('/') ?: ""),
            version.title, version.groupPath, tracks, count) > 0
        val candidates = local.filter { matches(it.relativePath) }.map { asset ->
            SubtitleSource(LibraryIndex.id(versionId, "subtitle", asset.relativePath), versionId,
                asset.rootId.takeUnless { it.startsWith("online:") }, asset.relativePath, asset.uri,
                asset.relativePath.substringAfterLast('/'), asset.format)
        } + remote.filter { matches(it.relativePath) && local.none { a -> a.id == it.id } }.map { asset ->
            SubtitleSource(LibraryIndex.id("remote-subtitle", versionId, asset.id), versionId, null,
                asset.relativePath, "asmr-subtitle://${asset.id}", asset.relativePath.substringAfterLast('/'), asset.format)
        }
        db.withTransaction {
            for (source in candidates) {
                if (existing.none { it.manual && it.uri == source.uri } && dao.subtitle(source.id) != source) dao.putSubtitle(source)
            }
            // Don't continue selecting an automatically linked online file removed by a metadata refresh.
            for (old in existing.filter { !it.manual && it.uri.startsWith("asmr-subtitle:") }) {
                val available = remote.any { old.uri == "asmr-subtitle://${it.id}" }
                if (old.available != available) dao.putSubtitle(old.copy(available = available))
            }
        }
    }
    /** Rebind a standalone imported subtitle while retaining selection and offsets. */
    suspend fun reauthorize(sourceId: String, uri: Uri): SubtitleSource = withContext(Dispatchers.IO) {
        val old = requireNotNull(dao.subtitle(sourceId))
        require(old.rootId == null && uri.scheme == "content" && !old.uri.startsWith("unbound://asset/") && !old.uri.startsWith("file:"))
        context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        val source = old.copy(uri = uri.toString(), available = true)
        read(source)
        dao.putSubtitle(source)
        source
    }

    /** Explicitly bind a completed subtitle attachment from the same work; no name-based guessing. */
    suspend fun attachDownloaded(versionId: String, assetId: String): SubtitleSource {
        val remote = requireNotNull(dao.remoteAsset(assetId))
        require(remote.kind == "subtitle" && dao.version(versionId)?.workId == remote.workId) { "附件不属于当前作品" }
        val asset = requireNotNull(dao.asset(assetId)); check(asset.available) { "附件尚未下载或已移除" }
        val source = SubtitleSource(LibraryIndex.id("downloaded-subtitle", versionId, assetId), versionId, null,
            remote.relativePath, asset.uri, remote.relativePath.substringAfterLast('/'), remote.format, manual = true)
        read(source)
        return db.withTransaction {
            check(dao.asset(assetId)?.available == true) { "附件已移除" }
            dao.putSubtitle(source)
            source
        }
    }

    suspend fun read(source: SubtitleSource): SubtitleTimeline = withContext(Dispatchers.IO) {
        if (source.uri.startsWith("asmr-subtitle://")) {
            check(source.available) { "字幕附件已移除，请刷新作品" }
            val asset = requireNotNull(dao.remoteAsset(source.uri.substringAfter("asmr-subtitle://")))
            check(asset.available && asset.kind == "subtitle" && dao.version(source.versionId)?.workId == asset.workId)
            val local = dao.asset(asset.id)?.takeIf { it.available }
            if (local != null) return@withContext read(source.copy(uri = local.uri))
            val online = requireNotNull(provider) { "在线字幕来源不可用" }
            val address = online.resolve(asset)
            val bytes = online.http.response(address.url) { response ->
                check(response.responseCode in 200..299) { "在线字幕读取失败，请重试" }
                response.inputStream.use { readBytes(it) }
            }
            return@withContext withContext(Dispatchers.Default) { SubtitleParser.decode(bytes, source.format) }
        }
        check(source.available) { "字幕不可用，请重新扫描或授权" }
        val bytes = context.contentResolver.openInputStream(Uri.parse(source.uri))?.use { input ->
            readBytes(input)
        } ?: throw IOException("无法读取字幕")
        withContext(Dispatchers.Default) { SubtitleParser.decode(bytes, source.format) }
    }

    private suspend fun readBytes(input: java.io.InputStream): ByteArray {
        val output = ByteArrayOutputStream(); val block = ByteArray(8192)
        while (true) {
            kotlinx.coroutines.currentCoroutineContext().ensureActive()
            val count = input.read(block); if (count < 0) break
            require(output.size() + count <= 8 * 1024 * 1024) { "字幕文件超过 8 MiB" }
            output.write(block, 0, count)
        }
        return output.toByteArray()
    }
}
