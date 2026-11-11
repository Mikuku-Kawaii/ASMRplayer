package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.foundation.selection.triStateToggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.*
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.asmrplayer.android.DownloadPlanState
import com.asmrplayer.android.download.*

@Composable
internal fun DownloadSelectionDialog(state: DownloadPlanState, toggle: (List<String>) -> Unit,
    close: () -> Unit, retry: () -> Unit, confirm: () -> Unit) {
    if (!state.visible) return
    val plan = state.plan
    var expanded by rememberSaveable(plan?.workId) { mutableStateOf(emptyList<String>()) }
    val roots = remember(plan?.items) { downloadFolders(plan?.items.orEmpty()) }
    // Size from the collapsed tree so opening a directory never moves the footer.
    val initialRows = roots.sumOf { if (it.path.isEmpty()) it.files.size else 1 }
    val dialogHeight = (216 + (initialRows * 48).coerceIn(144, 320)).dp
        .coerceAtMost((LocalConfiguration.current.screenHeightDp * .76f).dp)
    val eligible = plan?.items.orEmpty().filter { it.selectable }
    val chosen = eligible.filter { it.id in state.selected }
    Dialog(onDismissRequest = { if (!state.busy) close() }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        DialogMotion()
        Surface(Modifier.widthIn(max = 560.dp).fillMaxWidth(.94f), shape = RoundedCornerShape(20.dp),
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
            color = MaterialTheme.colorScheme.surfaceContainerLow) {
            Column(Modifier.height(dialogHeight).padding(horizontal = 12.dp, vertical = 8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("创建下载任务", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                    GlyphButton(Glyph.Close, "关闭下载选择", { if (!state.busy) close() })
                }
                plan?.let {
                  Surface(shape = MaterialTheme.shapes.medium,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                    color = MaterialTheme.colorScheme.surface) {
                   Column {
                    LazyRow(Modifier.fillMaxWidth().padding(horizontal = 8.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        items(it.items.groupBy { file -> file.format }.entries.toList(), key = { entry -> entry.key }) { entry ->
                            val ids = entry.value.filter { file -> file.selectable }.map { file -> file.id }
                            FilterChip(ids.any { id -> id in state.selected }, { toggle(ids) },
                                { Text(entry.key.lowercase(), style = MaterialTheme.typography.labelMedium) },
                                shape = RoundedCornerShape(50), enabled = !state.busy && ids.isNotEmpty(),
                                colors = FilterChipDefaults.filterChipColors(
                                    selectedContainerColor = MaterialTheme.colorScheme.primary,
                                    selectedLabelColor = MaterialTheme.colorScheme.onPrimary))
                        }
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    Row(Modifier.padding(start = 10.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                      Column(Modifier.weight(1f)) {
                        Text("${chosen.size} 已选 · ${byteSize(chosen.sumOf { file -> file.bytes ?: 0 })}" +
                            if (chosen.any { file -> file.bytes == null }) " + 未知" else "",
                            style = MaterialTheme.typography.labelMedium)
                      }
                        MotionTextButton({ toggle(eligible.map { file -> file.id }) }, enabled = !state.busy && eligible.isNotEmpty()) {
                            Text(if (chosen.size == eligible.size) "取消全选" else "全选", style = MaterialTheme.typography.labelSmall)
                        }
                    }
                   }
                  }
                  Spacer(Modifier.height(8.dp))
                }
                state.error?.let {
                    Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
                    if (plan == null) MotionTextButton(retry, enabled = !state.loading) { Text("重新加载") }
                }
                if (state.loading) Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { LoadingRing() }
                if (plan != null) {
                  Surface(Modifier.fillMaxWidth().weight(1f), shape = MaterialTheme.shapes.medium,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                    color = MaterialTheme.colorScheme.surface) {
                   Column {
                    Text(plan.title, Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 8.dp),
                        maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    LazyColumn(Modifier.fillMaxWidth().weight(1f).semantics { contentDescription = "下载文件列表" },
                        contentPadding = PaddingValues(horizontal = 4.dp, vertical = 4.dp)) {
                        if (roots.isEmpty()) item { Text("当前作品暂无可下载文件", Modifier.padding(vertical = 24.dp)) }
                        roots.forEach { folder -> downloadFolderRows(folder, 0, state, expanded,
                            { path -> expanded = if (path in expanded) expanded - path else expanded + path }, toggle) }
                    }
                   }
                  }
                } else Spacer(Modifier.weight(1f))
                Spacer(Modifier.height(6.dp))
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
                    if (state.busy) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                    MotionTextButton(close, enabled = !state.busy) { Text("取消") }
                    MotionButton(confirm, enabled = !state.busy && chosen.isNotEmpty()) { Text("下载") }
                }
                NoticeOverlay(modal = true)
            }
        }
    }
}

private fun LazyListScope.downloadFolderRows(folder: DownloadFolder, depth: Int, state: DownloadPlanState,
    expanded: List<String>, expand: (String) -> Unit, toggle: (List<String>) -> Unit) {
    val root = folder.path.isEmpty()
    if (!root) item(key = "folder:${folder.path}") {
        val ids = folder.allFiles.filter { it.selectable }.map { it.id }
        val count = ids.count { it in state.selected }
        val checked = when { count == 0 -> ToggleableState.Off; count == ids.size -> ToggleableState.On; else -> ToggleableState.Indeterminate }
        Row(Modifier.fillMaxWidth().padding(start = (depth.coerceAtMost(3) * 10).dp)
            .heightIn(min = 48.dp).clickable { expand(folder.path) }
            .semantics { contentDescription = "${if (folder.path in expanded) "收起" else "展开"}目录 ${folder.path}" },
            verticalAlignment = Alignment.CenterVertically) {
            AppGlyph(if (folder.path in expanded) Glyph.ChevronDown else Glyph.ChevronRight,
                Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            TriStateCheckbox(checked, { toggle(ids) }, enabled = !state.busy && ids.isNotEmpty(),
                modifier = Modifier.semantics { contentDescription = "选择目录 ${folder.path}" })
            AppGlyph(Glyph.Folder, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.tertiary)
            Text(folder.path.substringAfterLast('/'), Modifier.weight(1f).padding(start = 6.dp),
                maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.labelMedium)
            DownloadFileSize(byteSize(folder.allFiles.sumOf { it.bytes ?: 0 }) +
                if (folder.allFiles.any { it.bytes == null }) "+" else "")
        }
    }
    if (root || folder.path in expanded) {
        folder.files.forEach { file -> item(key = "file:${file.id}") {
            val checked = if (file.id in state.selected && file.selectable) ToggleableState.On else ToggleableState.Off
            Row(Modifier.fillMaxWidth().padding(start = (16 + (if (root) 0 else depth + 1).coerceAtMost(3) * 10).dp)
                .triStateToggleable(checked, enabled = file.selectable && !state.busy, role = Role.Checkbox, onClick = { toggle(listOf(file.id)) })
                .heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
                    TriStateCheckbox(checked, onClick = null, enabled = file.selectable && !state.busy)
                }
                AppGlyph(when {
                    !file.attachment -> Glyph.FileAudio
                    file.format.lowercase() in setOf("jpg", "jpeg", "png", "webp", "gif", "封面") -> Glyph.FileImage
                    else -> Glyph.Details
                }, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
                Column(Modifier.weight(1f).padding(start = 6.dp)) {
                    Text(file.path.substringAfterLast('/'), maxLines = 1, overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodySmall)
                    if (file.downloaded || file.queued) Text(if (file.downloaded) "已下载" else "队列中",
                        style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                DownloadFileSize(if (file.bytes == null) "未知" else byteSize(file.bytes))
            }
        } }
        folder.children.forEach { downloadFolderRows(it, depth + 1, state, expanded, expand, toggle) }
    }
}

@Composable
private fun DownloadFileSize(value: String) {
    val style = MaterialTheme.typography.labelSmall.copy(fontFeatureSettings = "tnum")
    val measured = rememberTextMeasurer().measure(value, style, softWrap = false)
    val width = with(LocalDensity.current) { measured.size.width.toDp() + 10.dp }.coerceAtLeast(66.dp)
    Text(value, Modifier.width(width).padding(start = 4.dp, end = 4.dp),
        maxLines = 1, softWrap = false, textAlign = TextAlign.End, style = style,
        color = MaterialTheme.colorScheme.onSurfaceVariant)
}
