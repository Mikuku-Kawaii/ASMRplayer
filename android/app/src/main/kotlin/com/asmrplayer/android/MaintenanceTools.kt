package com.asmrplayer.android

import android.content.Context
import android.net.Uri
import com.asmrplayer.android.backup.*
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.download.DownloadStorageUsage
import com.asmrplayer.android.library.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

data class MaintenanceState(val busy: Boolean = false, val preview: ImportPreview? = null,
    val storage: DownloadStorageUsage? = null, val relinks: List<ReauthorizationItem> = emptyList(),
    val relinksReady: Boolean = false, val rootPreview: RootRelinkPreview? = null,
    val error: String? = null, val notice: String? = null)

class MaintenanceTools(context: Context, private val scope: CoroutineScope,
    private val saveSession: suspend () -> Unit) {
    private val app = context.applicationContext
    private val backend = AndroidBackend.get(app)
    private val mutable = MutableStateFlow(MaintenanceState())
    val state = mutable.asStateFlow()
    val update = backend.updates.state
    private var prepared: PreparedImport? = null
    private var rootUri: Uri? = null
    private var operation: Job? = null
    fun dismiss() { mutable.update { it.copy(error = null, notice = null) } }
    fun fail(message: String) { mutable.update { it.copy(error = message, notice = null) } }
    fun notice(message: String) { mutable.update { it.copy(notice = message, error = null) } }
    private fun run(failure: String, block: suspend () -> Unit) {
        if (mutable.value.busy) return
        mutable.update { it.copy(busy = true, error = null, notice = null) }
        operation = scope.launch {
            try { block() }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { fail(failure) }
            finally { mutable.update { it.copy(busy = false) } }
        }
    }
    fun cancel() { operation?.cancel(); notice("操作已取消；已完成的写入仍会保留。") }
    fun export(uri: Uri) = run("导出未完成，请检查目标文件权限或空间后重试。") {
        saveSession()
        backend.backups.exportTo(uri)
        notice("资料已导出，不包含音频、账号凭据或文件授权。")
    }
    fun prepare(uri: Uri) = run("无法读取资料包，请选择受支持的 Android 资料 JSON 文件。") {
        prepared = null; mutable.update { it.copy(preview = null) }
        prepared = backend.backups.prepare(uri)
        mutable.update { it.copy(preview = prepared!!.preview) }
    }
    fun cancelImport() { if (!mutable.value.busy) { prepared = null; mutable.update { it.copy(preview = null) } } }
    fun applyImport(policy: ImportPolicy) = run("资料没有导入成功，请检查包内容或冲突后重试。") {
        val result = backend.backups.apply(requireNotNull(prepared), policy)
        prepared = null; mutable.update { it.copy(preview = null) }
        notice("导入完成：新增 ${result.inserted}，更新 ${result.updated}，保留 ${result.kept}。当前播放队列保持不变。")
        readRelinks()
    }
    private suspend fun readRelinks() { val items = backend.library.reauthorizationItems(); mutable.update { it.copy(relinks = items, relinksReady = true) } }
    fun refreshRelinks() = run("无法检查文件授权，请重试。") { readRelinks() }
    fun previewRoot(id: String, uri: Uri) = run("所选目录无法匹配原记录，请选择原目录的新位置后重试。") {
        rootUri = null; mutable.update { it.copy(rootPreview = null) }
        val preview = backend.library.previewReauthorization(id, uri)
        rootUri = uri; mutable.update { it.copy(rootPreview = preview) }
    }
    fun cancelRoot() { if (!mutable.value.busy) { rootUri = null; mutable.update { it.copy(rootPreview = null) } } }
    fun confirmRoot(allowMissing: Boolean) = run("重新授权失败，原绑定已保留。请检查目录内容与读取权限。") {
        val preview = requireNotNull(mutable.value.rootPreview)
        backend.library.reauthorizeRoot(preview.rootId, requireNotNull(rootUri), allowMissing)
        rootUri = null; mutable.update { it.copy(rootPreview = null) }; readRelinks(); notice("目录已重新授权，收藏与进度已保留。")
    }
    fun relink(item: ReauthorizationItem, uri: Uri) = run("文件无法重新关联，请选择正确文件并允许读取权限。") {
        when (item.kind) {
            "audio" -> backend.library.reauthorizeAudioSource(item.id, uri)
            "subtitle" -> backend.subtitles.reauthorize(item.id, uri)
            else -> error("Use directory preview")
        }
        readRelinks(); notice("文件已重新关联。")
    }
    fun refreshStorage() = run("存储统计读取失败，请重试。") { val usage = backend.downloads.storageUsage(); mutable.update { it.copy(storage = usage) } }
    fun clearCovers() = run("封面清理未完成，请刷新统计后重试。") {
        val result = backend.downloads.clearCompletedCoverCache()
        val (removed, failed) = withContext(Dispatchers.IO) { com.asmrplayer.android.artwork.CoverDiskCache.clearStored(app) }
        val usage = backend.downloads.storageUsage(); mutable.update { it.copy(storage = usage) }
        val failures = result.failedIds.size + failed
        notice("已清理 ${result.removedIds.size + removed} 个封面文件" + if (failures == 0) "。" else "，$failures 个未成功，可重试。")
    }
    fun checkUpdate() = run("检查更新失败，请重试。") { backend.updates.check() }
    fun autoCheck(value: Boolean) { backend.updates.setAutoCheck(value) }
    fun skipUpdate(value: String?) { backend.updates.skipVersion(value) }
    fun downloadUpdate() { try { backend.updates.startDownload() } catch (_: Exception) { fail("无法安排下载，请重试。") } }
    fun cancelUpdate() { backend.updates.cancelDownload() }
}
