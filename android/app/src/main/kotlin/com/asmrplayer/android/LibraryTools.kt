package com.asmrplayer.android

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.*
import com.asmrplayer.android.subtitles.BilingualSubtitles
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

data class LibraryToolsState(
    val roots: List<LibraryRoot> = emptyList(),
    val rootsReady: Boolean = false,
    val scans: Map<String, ScanState> = emptyMap(),
    val draftUri: String? = null,
    val replacingRoot: String? = null,
    val mode: RootMode = RootMode.AUTO,
    val preview: ScanPreview? = null,
    val previewBusy: Boolean = false,
    val importBusy: Boolean = false,
    val previewError: String? = null,
    val versionId: String? = null,
    val bookmarks: List<Bookmark> = emptyList(),
    val sources: List<SubtitleSource> = emptyList(),
    val assets: List<WorkAsset> = emptyList(),
    val preference: SubtitlePreference? = null,
    val subtitles: BilingualSubtitles? = null,
    val subtitleBusy: Boolean = false,
    val subtitleSaving: Boolean = false,
    val subtitleError: String? = null,
    val error: String? = null,
    val notice: String? = null,
)

/** ViewModel-owned IO and drafts. Media time and playback execution remain in the service. */
class LibraryTools(context: Context, private val scope: CoroutineScope) {
    private val app = context.applicationContext
    private val backend = AndroidBackend.get(app)
    private val mutable = MutableStateFlow(LibraryToolsState())
    val state = mutable.asStateFlow()
    private var previewJob: Job? = null
    private var previewGeneration = 0L
    private var versionJob: Job? = null
    private val reload = MutableStateFlow(0)
    private val subtitleWrites = Mutex()

    init {
        scope.launch {
            backend.library.roots.catch { fail("无法读取目录，请重新打开应用。") }.collect { roots ->
                mutable.update { it.copy(roots = roots, rootsReady = true) }
            }
        }
        scope.launch { backend.library.scanStates.collect { scans -> mutable.update { it.copy(scans = scans) } } }
    }

    fun message(text: String) { mutable.update { it.copy(notice = text, error = null) } }
    fun fail(text: String) { mutable.update { it.copy(error = text, notice = null) } }
    fun dismissMessage() { mutable.update { it.copy(error = null, notice = null) } }

    fun prepareRoot(uri: Uri, replacing: String?) {
        if (mutable.value.importBusy) return
        val mode = mutable.value.roots.firstOrNull { it.id == replacing }?.mode
            ?.let { runCatching { RootMode.valueOf(it) }.getOrNull() } ?: RootMode.AUTO
        mutable.update { it.copy(draftUri = uri.toString(), replacingRoot = replacing, mode = mode, preview = null) }
        preview(mode)
    }

    fun preview(mode: RootMode = mutable.value.mode) {
        val uri = mutable.value.draftUri ?: return
        if (mutable.value.importBusy) return
        val generation = ++previewGeneration
        previewJob?.cancel()
        mutable.update { it.copy(mode = mode, preview = null, previewBusy = true, previewError = null) }
        previewJob = scope.launch {
            try {
                val result = backend.library.preview(Uri.parse(uri), mode)
                if (generation == previewGeneration) mutable.update { it.copy(preview = result) }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) {
                if (generation == previewGeneration) mutable.update { it.copy(previewError = "无法预览目录，请检查授权或重新选择。") }
            } finally {
                if (generation == previewGeneration) mutable.update { it.copy(previewBusy = false) }
            }
        }
    }

    fun cancelPreview() {
        if (mutable.value.importBusy) return
        ++previewGeneration; previewJob?.cancel()
        mutable.update { it.copy(draftUri = null, replacingRoot = null, preview = null, previewBusy = false, previewError = null) }
    }

    fun confirmImport() {
        val draft = mutable.value
        val uri = draft.draftUri ?: return
        if (draft.preview == null || draft.previewBusy || draft.importBusy) return
        mutable.update { it.copy(importBusy = true) }
        scope.launch {
            try {
                val root = backend.library.authorizeRoot(Uri.parse(uri), draft.mode, draft.replacingRoot)
                mutable.update { it.copy(importBusy = false) }
                cancelPreview()
                scan(root.id)
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { mutable.update { it.copy(previewError = "无法保存目录授权，请重新选择目录。") } }
            finally { mutable.update { it.copy(importBusy = false) } }
        }
    }

    fun scan(rootId: String) {
        if (mutable.value.scans[rootId]?.running == true) return
        scope.launch {
            try {
                val result = backend.library.scan(rootId)
                message("扫描完成：${result.works.size} 部作品，${result.documentCount} 个文件。")
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { /* The repository keeps the error alongside this root and preserves its records. */ }
        }
    }
    fun cancelScan(rootId: String) = backend.library.cancelScan(rootId)
    fun removeRoot(rootId: String) {
        scope.launch {
            try { backend.library.removeRoot(rootId); message("已移除作品目录") }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { fail("未能移除目录，请重试。") }
        }
    }

    fun selectVersion(versionId: String?, workId: String?) {
        if (mutable.value.versionId == versionId) return
        versionJob?.cancel()
        mutable.update { it.copy(versionId = versionId, bookmarks = emptyList(), sources = emptyList(), assets = emptyList(),
            preference = null, subtitles = null, subtitleBusy = versionId != null, subtitleError = null) }
        if (versionId == null) return
        versionJob = scope.launch {
            launch {
                backend.library.bookmarks(versionId).catch { fail("标记无法读取，请重新打开应用。") }
                    .collect { list -> mutable.update { it.copy(bookmarks = list) } }
            }
            if (workId != null) launch {
                backend.library.assets(workId).catch { fail("目录附件无法读取，请重新扫描目录。") }.collect { list ->
                    mutable.update { it.copy(assets = list.filter { a -> a.format in LibraryIndex.subtitleExtensions }) }
                }
            }
            val attachments = if (workId == null) flowOf(Unit) else combine(backend.library.assets(workId).distinctUntilChanged(), backend.online.assets(workId).distinctUntilChanged()) { _, _ -> Unit }
            reload.collectLatest {
                combine(backend.subtitles.sources(versionId).distinctUntilChanged(), backend.subtitles.preference(versionId).distinctUntilChanged(), attachments) { sources, pref, _ ->
                    sources to (pref ?: SubtitlePreference(versionId))
                }.catch { mutable.update { it.copy(subtitleError = "字幕列表无法读取，请重试。", subtitleBusy = false) } }
                    .collectLatest { (sources, pref) ->
                        mutable.update { it.copy(sources = sources, preference = pref, subtitleBusy = true, subtitleError = null, subtitles = null) }
                        try {
                            val loaded = backend.subtitles.load(versionId, allowPartial = true)
                            ensureActive()
                            if (mutable.value.versionId == versionId) mutable.update { it.copy(subtitles = loaded, preference = loaded.preference) }
                        } catch (e: CancellationException) { throw e }
                        catch (_: Exception) {
                            if (mutable.value.versionId == versionId) mutable.update { it.copy(subtitleError = "字幕无法读取，请检查授权、文件编码和格式，或重新选择字幕。") }
                        } finally {
                            if (mutable.value.versionId == versionId) mutable.update { it.copy(subtitleBusy = false) }
                        }
                    }
            }
        }
    }

    fun retrySubtitles() { reload.value++ }
    fun retrySubtitle(translation: Boolean) {
        val current = mutable.value
        val previous = current.subtitles ?: return retrySubtitles()
        val owner = versionJob ?: return
        if (current.subtitleBusy) return
        mutable.update { it.copy(subtitleBusy = true) }
        scope.launch(owner) {
            try {
                val next = backend.subtitles.retryRole(previous, translation)
                mutable.update { if (it.subtitles === previous) it.copy(subtitles = next, preference = next.preference, subtitleBusy = false) else it }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) {
                mutable.update { if (it.subtitles === previous) it.copy(subtitleBusy = false) else it }
            }
        }
    }
    fun chooseSubtitle(version: String, translation: Boolean, id: String?) = writeSubtitle(version) { pref ->
        backend.subtitles.select(if (translation) pref.copy(translationId = id) else pref.copy(originalId = id))
    }
    fun saveOffsets(version: String, offset: Long, translatedOffset: Long) = writeSubtitle(version) { pref ->
        backend.subtitles.select(pref.copy(offsetMs = offset, translationOffsetMs = translatedOffset))
    }
    fun attachSubtitle(version: String, translation: Boolean, uri: Uri, asset: WorkAsset? = null) = writeSubtitle(version) { pref ->
        val name = asset?.relativePath?.substringAfterLast('/') ?: withContext(Dispatchers.IO) {
            app.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) cursor.getString(0) else null
            }
        } ?: throw IllegalArgumentException("没有文件名")
        val source = if (asset?.rootId?.startsWith("online:") == true) backend.subtitles.attachDownloaded(version, asset.id)
            else backend.subtitles.attach(version, uri, name, persistGrant = asset == null)
        backend.subtitles.select(if (translation) pref.copy(translationId = source.id) else pref.copy(originalId = source.id))
    }
    private fun writeSubtitle(version: String, action: suspend (SubtitlePreference) -> Unit) {
        if (mutable.value.subtitleSaving) return
        if (mutable.value.versionId != version) { fail("音轨已切换，请为当前版本重新选择字幕。"); return }
        mutable.update { it.copy(subtitleSaving = true) }
        scope.launch {
            try {
                subtitleWrites.withLock {
                    val pref = backend.subtitles.preference(version).first() ?: mutable.value.preference?.takeIf { it.versionId == version } ?: SubtitlePreference(version)
                    action(pref)
                }
                message("字幕设置已保存。")
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { fail("字幕未保存。请检查文件授权、格式（SRT/VTT/LRC/ASS/SSA）或偏移范围。") }
            finally { mutable.update { it.copy(subtitleSaving = false) } }
        }
    }
    fun removeBookmark(id: String) {
        scope.launch {
            try { backend.library.removeBookmark(id); message("标记已删除。") }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { fail("标记未删除，请重试。") }
        }
    }
}
