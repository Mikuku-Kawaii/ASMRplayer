package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.data.DownloadTask
import com.asmrplayer.android.data.RemoteAsset
import com.asmrplayer.android.data.WorkAsset
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.online.OnlineFile
import kotlinx.coroutines.CancellationException

internal data class DetailAttachment(
    val id: String, val path: String, val title: String, val format: String, val bytes: Long?,
    val local: WorkAsset?, val remote: OnlineFile?, val reference: RemoteAsset?,
) {
    val available get() = local?.available == true
    val picture get() = !LibraryIndex.isVideoFormat(format) &&
        (format.lowercase() in LibraryIndex.imageExtensions || remote?.kind == "image" || reference?.kind == "image")
    val uri get() = local?.takeIf { it.available }?.uri ?: remote?.let { it.downloadUrl ?: it.streamUrl }
    val downloadable get() = remote != null || reference != null
}

internal fun detailAttachments(workId: String, local: List<WorkAsset>, remote: List<OnlineFile>, references: List<RemoteAsset>): List<DetailAttachment> {
    val locals = local.groupBy { it.relativePath }.mapValues { (_, files) -> files.firstOrNull { it.available } ?: files.first() }
    val refs = references.filter { it.kind != "cover" }.associateBy { it.relativePath }
    val files = remote.filter { it.kind != "audio" }.associateBy { it.path }
    return (files.keys + refs.keys + locals.keys).map { path ->
        val file = files[path]
        val ref = refs[path]
        val saved = locals[path]
        DetailAttachment(ref?.id ?: file?.let { LibraryIndex.id(workId, "asset", it.id, it.path) } ?: saved!!.id,
            path, file?.title ?: path.substringAfterLast('/'),
            file?.format ?: ref?.format ?: saved!!.format, file?.bytes ?: ref?.byteSize, saved, file, ref)
    }
}

@Composable
internal fun AttachmentImageCard(
    entry: DetailAttachment, task: DownloadTask?, busy: Boolean, modifier: Modifier = Modifier, titleLines: Int = 1,
    resolve: suspend (RemoteAsset) -> String, preview: (String) -> Unit, download: () -> Unit, info: () -> Unit,
) {
    val active = LocalRouteActive.current
    var resolved by remember(entry.id, entry.uri, entry.reference) { mutableStateOf(entry.uri) }
    var resolving by remember(entry.id) { mutableStateOf(false) }
    var attempt by remember(entry.id) { mutableIntStateOf(0) }
    LaunchedEffect(entry.id, entry.uri, entry.reference, active, attempt) {
        resolved = entry.uri
        if (resolved == null && entry.reference != null && active) {
            resolving = true
            try { resolved = resolve(entry.reference) }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { resolved = null }
            finally { resolving = false }
        }
    }
    val artwork = rememberArtwork(resolved, thumbnailEdge = 384)
    val colors = MaterialTheme.colorScheme
    CardActions(entry.title, modifier = modifier, menu = { close ->
        CardAction("文件信息", close) { info() }
        CardAction("查看图片", close, resolved != null) { resolved?.let(preview) }
    }) {
        Surface(modifier = Modifier.fillMaxSize(), shape = RoundedCornerShape(14.dp), color = colors.surfaceContainerLow,
            border = BorderStroke(1.dp, colors.outlineVariant.copy(alpha = .55f))) {
            Column {
                Box(Modifier.fillMaxWidth().aspectRatio(4f / 3f).background(colors.surfaceContainerHigh)
                    .clickable(enabled = resolved != null || entry.reference != null,
                        onClickLabel = "预览图片 ${entry.title}") { resolved?.let(preview) ?: run { attempt++ } },
                    contentAlignment = Alignment.Center) {
                    artwork.image?.let { Image(it, entry.title, Modifier.fillMaxSize(), contentScale = ContentScale.Fit) }
                        ?: Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            if (resolving || artwork.loading) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                            else {
                                AppGlyph(Glyph.FileImage, tint = colors.onSurfaceVariant)
                                Text(if (resolved != null) "点击查看原图" else if (entry.reference != null) "点击重试" else "图片不可用",
                                    style = MaterialTheme.typography.labelSmall, color = colors.onSurfaceVariant)
                            }
                        }
                }
                Column(Modifier.padding(horizontal = 10.dp, vertical = 8.dp)) {
                    Text(entry.title, style = MaterialTheme.typography.titleSmall, minLines = titleLines, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(attachmentMetadata(entry), Modifier.weight(1f), style = MaterialTheme.typography.bodySmall,
                            color = colors.onSurfaceVariant)
                        AttachmentDownloadButton(entry, task, busy, download)
                    }
                }
            }
        }
    }
}

internal fun attachmentMetadata(entry: DetailAttachment): String = buildString {
    append(entry.format.uppercase())
    entry.bytes?.let { append(" · "); append(byteSize(it)) }
}

@Composable
internal fun AttachmentDownloadButton(entry: DetailAttachment, task: DownloadTask?, busy: Boolean, download: () -> Unit) {
    val running = task?.state in setOf("QUEUED", "DOWNLOADING", "VERIFYING")
    val label = when {
        entry.available -> "已保存"
        running -> when (task?.state) { "QUEUED" -> "等待中"; "VERIFYING" -> "校验中"; else -> "下载中" }
        task?.state == "FAILED" -> "重试"
        task?.state == "PAUSED" -> "继续"
        else -> "下载"
    }
    if (entry.available || !entry.downloadable) {
        Text(if (entry.available) "已保存" else "不可用", style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.heightIn(min = 48.dp).wrapContentHeight())
    } else {
        val source = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
        FilledTonalButton(onClick = download, enabled = !busy && !running,
            modifier = Modifier.heightIn(min = 48.dp).pressMotion(source).semantics { contentDescription = "$label ${entry.title}" },
            interactionSource = source, contentPadding = PaddingValues(horizontal = 10.dp, vertical = 6.dp)) {
            Text(label, maxLines = 1)
        }
    }
}

@Composable
internal fun AttachmentFileRow(entry: DetailAttachment, task: DownloadTask?, busy: Boolean, download: () -> Unit,
    previewSubtitle: () -> Unit, menu: CardMenu) {
    CardActions(entry.title, menu = menu) {
        Surface(shape = RoundedCornerShape(14.dp), color = MaterialTheme.colorScheme.surfaceContainerLow,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = .55f))) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                AppGlyph(Glyph.Details, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(entry.title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    MaintenanceHint(attachmentMetadata(entry))
                    if (LibraryIndex.isVideoFormat(entry.format)) MaintenanceHint("暂不支持预览")
                    else if (!entry.available && !entry.downloadable) MaintenanceHint("文件不可用")
                }
                if (entry.available && entry.format.lowercase() in LibraryIndex.subtitleExtensions) {
                    MotionOutlinedButton(previewSubtitle, enabled = !busy) { Text("预览") }
                } else AttachmentDownloadButton(entry, task, busy, download)
                var more by remember { mutableStateOf(false) }
                GlyphButton(Glyph.More, "附件操作 ${entry.title}", { more = true })
                if (more) PlayerSheet("附件操作", { more = false }) { Column { menu { more = false } } }
            }
        }
    }
}
