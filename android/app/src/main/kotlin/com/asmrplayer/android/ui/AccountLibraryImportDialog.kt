package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.selection.triStateToggleable
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.OnlineTools
import com.asmrplayer.android.SiteLibraryDialog
import com.asmrplayer.android.SiteSelectionGroup

@Composable
internal fun AccountLibraryImportDialog(dialog: SiteLibraryDialog, tools: OnlineTools) {
    val preview = dialog.preview
    val selected = dialog.selection
    val workCount = selected.workIds.size
    val listCount = selected.playlistIds.size
    val total = (preview?.works?.size ?: 0) + (preview?.lists?.size ?: 0)
    val enabled = !dialog.saving
    PlayerAlertDialog(
        onDismissRequest = tools::rejectSiteLibrary,
        title = { Text("同步收藏与播放列表") },
        text = {
            LazyColumn(Modifier.fillMaxWidth().heightIn(max = 400.dp)) {
                dialog.error?.let { message -> item { MaintenanceHint(message, error = true) } }
                if (preview == null) {
                    if (dialog.error == null) item { LoadingRing("正在读取 asmr.one 的收藏与播放列表…") }
                } else {
                    item {
                        Text("收藏 ${preview.favorites} 部，${preview.playlists} 个播放列表共含 ${preview.playlistWorks} 部。合并重复作品后，已有 ${preview.alreadyInLibrary} 部在音频库。")
                    }
                    if (preview.lists.isNotEmpty()) item {
                        Text("勾选的歌单将保存为作品集，收录已在库和本次勾选的作品。", Modifier.padding(vertical = 8.dp),
                            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    if (preview.works.isNotEmpty() && preview.lists.isNotEmpty()) item {
                        ImportGroupToggle("全选", "全选", workCount + listCount, total, enabled, "$workCount 部作品 · $listCount 个歌单") {
                            tools.selectSiteItems(SiteSelectionGroup.All, it)
                        }
                    }
                    if (preview.lists.isNotEmpty()) item {
                        ImportGroupToggle("保留官网歌单", "全选歌单", listCount, preview.lists.size, enabled) {
                            tools.selectSiteItems(SiteSelectionGroup.Playlists, it)
                        }
                    }
                    items(preview.lists, key = { "playlist-${it.id}" }) { list ->
                        ImportChoice(list.name, "${list.sources.size} 部", list.id in selected.playlistIds, enabled) {
                            tools.selectSiteItems(SiteSelectionGroup.Playlists, it, list.id)
                        }
                    }
                    if (preview.works.isNotEmpty()) item {
                        ImportGroupToggle("加入音频库的作品", "全选作品", workCount, preview.works.size, enabled) {
                            tools.selectSiteItems(SiteSelectionGroup.Works, it)
                        }
                    } else item { Text("没有待加入的作品。", Modifier.padding(top = 8.dp)) }
                    items(preview.works, key = { "work-${it.sourceId}" }) { work ->
                        ImportChoice(work.title, work.code, work.sourceId in selected.workIds, enabled) {
                            tools.selectSiteItems(SiteSelectionGroup.Works, it, work.sourceId)
                        }
                    }
                }
            }
        },
        confirmButton = {
            if (total > 0) MotionButton(tools::acceptSiteLibrary, enabled = enabled && workCount + listCount > 0) {
                Text(when {
                    dialog.saving -> "正在保存…"
                    workCount > 0 -> "确认加入 ${workCount} 部"
                    listCount > 0 -> "保存 ${listCount} 个作品集"
                    else -> "确认加入"
                })
            }
        },
        dismissButton = {
            MotionTextButton(tools::rejectSiteLibrary, enabled = enabled) {
                Text(if (total == 0 && (preview != null || dialog.error != null)) "关闭" else "拒绝")
            }
        },
    )
}

@Composable
private fun ImportGroupToggle(
    title: String, name: String, count: Int, total: Int, enabled: Boolean,
    summary: String = "已选 $count / $total", change: (Boolean) -> Unit,
) {
    val state = when (count) {
        0 -> ToggleableState.Off
        total -> ToggleableState.On
        else -> ToggleableState.Indeterminate
    }
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp)
        .triStateToggleable(state, enabled = enabled, role = Role.Checkbox, onClick = { change(count != total) })
        .semantics { contentDescription = name }, verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        TriStateCheckbox(state, onClick = null, enabled = enabled)
        Text(title, Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
        Text(summary, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun ImportChoice(title: String, detail: String, checked: Boolean, enabled: Boolean, change: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp)
        .toggleable(checked, enabled = enabled, role = Role.Checkbox, onValueChange = change)
        .padding(start = 12.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Checkbox(checked, onCheckedChange = null, enabled = enabled)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyMedium)
            Text(detail, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
