package com.asmrplayer.android.incoming

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.room.withTransaction
import androidx.core.content.FileProvider
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.data.SubtitlePreference
import com.asmrplayer.android.library.LibraryIndex
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import java.io.File
import java.util.UUID

data class IncomingFile(val uri: Uri, val name: String, val subtitle: Boolean)
data class IncomingState(val files: List<IncomingFile> = emptyList(), val workId: String? = null,
    val visible: Boolean = false, val busy: Boolean = false, val progress: String = "", val error: String? = null)

class IncomingTools(private val context: Context, private val scope: CoroutineScope, private val notice: (String) -> Unit) {
    private val mutable = MutableStateFlow(IncomingState())
    val state = mutable.asStateFlow()
    private var job: Job? = null
    fun dismiss() { job?.cancel(); mutable.value = IncomingState() }
    @Suppress("DEPRECATION")
    fun receive(intent: Intent) {
        if (intent.action !in listOf(Intent.ACTION_SEND, Intent.ACTION_SEND_MULTIPLE, Intent.ACTION_VIEW)) return
        if (mutable.value.busy) { notice("请先完成当前导入，再重新分享。"); return }
        dismiss()
        val link = if (intent.action == Intent.ACTION_VIEW && intent.data?.scheme == "https") intent.dataString
            else intent.getStringExtra(Intent.EXTRA_TEXT)
        val id = link?.let(WorkLink::id)
        if (id != null) { mutable.value = IncomingState(workId = id, visible = true); return }
        val uris = buildList<Uri> {
            if (intent.action == Intent.ACTION_VIEW) intent.data?.let(::add)
            if(intent.action == Intent.ACTION_SEND_MULTIPLE) intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.let(::addAll)
            else (intent.getParcelableExtra<android.os.Parcelable>(Intent.EXTRA_STREAM) as? Uri)?.let(::add)
            intent.clipData?.let { clip -> for (i in 0 until clip.itemCount) clip.getItemAt(i).uri?.let(::add) }
        }.distinct()
        mutable.value = IncomingState(visible = true, busy = true, progress = "读取分享内容…")
        job = scope.launch {
            try {
                require(uris.isNotEmpty() && uris.size <= 50) { "支持音频、字幕文件及 ASMR/DLsite 作品链接，一次最多50个文件。" }
                val files = withContext(Dispatchers.IO) { uris.map { uri ->
                    require(uri.scheme == "content" && !uri.authority.orEmpty().startsWith(context.packageName)) { "请选择其他应用提供的文件。" }
                    val name = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                        if (it.moveToFirst()) it.getString(0) else null
                    }?.substringAfterLast('/')?.substringAfterLast('\\')?.take(160) ?: "共享文件"
                    val extension = name.substringAfterLast('.', "").lowercase()
                    val subtitle = extension in LibraryIndex.subtitleExtensions
                    require(subtitle || extension in LibraryIndex.audioExtensions || context.contentResolver.getType(uri).orEmpty().startsWith("audio/")) { "不支持的文件：$name" }
                    IncomingFile(uri, name, subtitle)
                } }
                mutable.value = IncomingState(files = files, visible = true)
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { mutable.value = IncomingState(visible = true, error = e.message ?: "无法读取分享内容，请重新分享。") }
        }
    }
    fun importFiles(target: String?, translation: Boolean) {
        val request = mutable.value
        if (request.busy || request.files.isEmpty()) return
        if (request.files.any { it.subtitle } && target == null) return
        mutable.value = request.copy(busy = true, error = null)
        job = scope.launch {
            val backend = AndroidBackend.get(context)
            var imported = 0
            val failed = mutableListOf<IncomingFile>()
            try {
                for ((index, item) in request.files.withIndex()) {
                    mutable.update { it.copy(progress = "导入 ${index + 1}/${request.files.size} · ${item.name}") }
                    var directory: File? = null
                    try {
                        val uri = withContext(Dispatchers.IO) {
                            val folder = File(context.filesDir, "shared-imports/${UUID.randomUUID()}").also { check(it.mkdirs()) }
                            directory = folder
                            val file = File(folder, item.name.replace(Regex("[\\p{Cntrl}:*?\"<>|]"), "_").ifBlank { "shared" })
                            context.contentResolver.openInputStream(item.uri)?.use { input -> file.outputStream().use { output ->
                                val buffer = ByteArray(64 * 1024); var size = 0L
                                while (true) {
                                    currentCoroutineContext().ensureActive()
                                    val count = input.read(buffer); if (count < 0) break
                                    size += count; require(size <= if (item.subtitle) 8_000_000L else 2_147_483_648L) { "文件过大" }
                                    output.write(buffer, 0, count)
                                }
                                require(size > 0) { "文件为空" }
                            } } ?: error("文件无法读取")
                            FileProvider.getUriForFile(context, "${context.packageName}.imports", file)
                        }
                        withContext(NonCancellable) {
                        if (item.subtitle) {
                            backend.database.withTransaction {
                            val version = requireNotNull(target)
                            val source = backend.subtitles.attach(version, uri, item.name, persistGrant = false)
                            val pref = backend.subtitles.preference(version).first() ?: SubtitlePreference(version)
                            backend.subtitles.select(if (translation) pref.copy(translationId = source.id) else pref.copy(originalId = source.id))
                            }
                        } else backend.library.importDocument(uri, item.name)
                        directory = null // Committed source owns this managed copy.
                        }
                        imported++
                    } catch (e: CancellationException) { throw e }
                    catch (_: Exception) { failed.add(item) }
                    finally { directory?.deleteRecursively() }
                }
                notice("已导入 $imported 个文件" + if (failed.isEmpty()) "。" else "，${failed.size} 个失败，可重试。")
                mutable.value = if (failed.isEmpty()) IncomingState() else IncomingState(files = failed, visible = true, error = "部分文件未导入，请检查授权或存储空间后重试。")
            } finally { mutable.update { it.copy(busy = false) } }
        }
    }
}
