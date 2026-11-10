package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.LibraryTools
import com.asmrplayer.android.LibraryToolsState
import com.asmrplayer.android.library.RootMode

private fun RootMode.label() = when (this) {
    RootMode.AUTO -> "自动判断"; RootMode.WORK -> "单部作品"; RootMode.COLLECTION -> "作品集合"
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun DirectoryPage(state: LibraryToolsState, tools: LibraryTools, choose: (String?) -> Unit) {
    var removing by rememberSaveable { mutableStateOf<String?>(null) }
    state.roots.firstOrNull { it.id == removing }?.let { root ->
        PlayerAlertDialog(onDismissRequest = { removing = null }, title = { Text("移除作品目录？") },
            text = { Text("「${root.title}」将停止扫描，仅来自此目录的作品会移出音频库。手机里的原文件、喜欢和收听记录会保留。") },
            confirmButton = { TextButton(onClick = { removing = null; tools.removeRoot(root.id) }) {
                Text("移除目录", color = MaterialTheme.colorScheme.error)
            } }, dismissButton = { TextButton(onClick = { removing = null }) { Text("取消") } })
    }
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            MaintenanceRow("已授权目录", if (state.rootsReady) "${state.roots.size} 个目录" else "正在读取…") {
                MotionButton({ choose(null) }, enabled = !state.importBusy && state.draftUri == null) { Text("添加目录") }
            }
        }

        if (state.draftUri != null) {
            item {
                MaintenanceCard(if (state.replacingRoot == null) "导入预览" else "重新授权预览") {
                    if (state.replacingRoot != null) MaintenanceHint("选择原目录的新位置，保留原分组方式以延续喜欢和进度。")
                    Text("如何分组", style = MaterialTheme.typography.titleSmall)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        RootMode.entries.forEach { mode -> ChoiceChip(state.mode == mode, { tools.preview(mode) }, { Text(mode.label()) }, enabled = !state.importBusy) }
                    }
                    MaintenanceHint(when (state.mode) {
                        RootMode.AUTO -> "根据音频所在层级自动判断；下方会显示实际分组结果。"
                        RootMode.WORK -> "整个目录及子目录归为一部作品。"
                        RootMode.COLLECTION -> "首层子目录各自归为一部作品。"
                    })
                    if (state.previewBusy || state.importBusy) {
                        LoadingRing()
                        MaintenanceHint(if (state.importBusy) "正在保存授权并准备扫描…" else "正在读取目录结构…")
                    }
                    state.previewError?.let {
                        MaintenanceStatus(it, error = true)
                        MotionTextButton({ tools.preview() }, enabled = !state.importBusy) { Text("重试预览") }
                    }
                    state.preview?.let { preview ->
                        MaintenanceValue("${preview.mode.label()} · 作品", "${preview.works.size} 部")
                        MaintenanceValue("音频版本", "${preview.works.sumOf { it.audio.size }} 个")
                        MaintenanceValue("已关联字幕", "${preview.works.sumOf { it.subtitles.values.sumOf { files -> files.size } }} 个")
                        if (preview.works.isEmpty()) MaintenanceHint("未找到支持的音频。可更换分组方式，或取消后选择其他目录。")
                    }
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        MotionButton(tools::confirmImport, enabled = state.preview != null && !state.previewBusy && !state.importBusy) { Text("确认导入并扫描") }
                        MotionTextButton(tools::cancelPreview, enabled = !state.importBusy) { Text("取消预览") }
                    }
                }
            }
            // Keep large directory previews lazy; no giant nested scrolling card.
            items(state.preview?.works.orEmpty(), key = { "preview:${it.path}" }) { work ->
                CardActions(work.title, menu = { _ ->
                    MaintenanceHint("来源目录")
                    androidx.compose.foundation.text.selection.SelectionContainer {
                        Text(work.path.ifBlank { work.title }, style = MaterialTheme.typography.bodyMedium)
                    }
                }) {
                    Column(Modifier.workListCard().padding(horizontal = 12.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(work.title, style = MaterialTheme.typography.titleSmall)
                        MaintenanceHint("${work.audio.size} 个音频版本 · ${work.assets.size} 个附件")
                    }
                }
            }
        }
        if (!state.rootsReady) item {
            LoadingRing(); MaintenanceHint("正在读取目录…")
        } else if (state.roots.isEmpty()) item {
            EmptyContent("还没有添加目录", "选择作品目录后可预览音轨、字幕与分组。", Glyph.Folder)
        }
        items(state.roots, key = { it.id }) { root ->
            val scan = state.scans[root.id]
            val running = scan?.running == true
            CardActions(root.title, menu = { close ->
                if (running) CardAction("取消扫描", close) { tools.cancelScan(root.id) }
                else {
                    CardAction("重新扫描", close) { tools.scan(root.id) }
                    CardAction("重新授权", close, !state.importBusy && state.draftUri == null) { choose(root.id) }
                }
                CardAction("移除目录", close, !state.importBusy && state.draftUri == null) { removing = root.id }
            }) {
                MaintenanceCard {
                    val status = when {
                        running -> "正在扫描"
                        !root.available -> "需要重新授权"
                        scan?.error != null -> "扫描未完成"
                        root.lastScanMs > 0 -> "可用"
                        else -> "等待扫描"
                    }
                    val path = readableDirectory(root.treeUri)
                    MaintenanceRow(root.title, path.split(" / ").takeLast(2).joinToString(" / ")) {
                        WorkBadge(status, accent = running)
                    }
                    if (root.lastScanMs > 0) MaintenanceHint("上次扫描 · " + java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.SHORT, java.text.DateFormat.SHORT).format(java.util.Date(root.lastScanMs)))
                    if (running) {
                        LoadingRing()
                        MaintenanceHint("已读取 ${scan?.documents ?: 0} 个文件")
                    } else if (scan?.error != null) MaintenanceHint(scan.error, error = true)
                    else if (!root.available) MaintenanceHint("目录暂不可读。恢复原位置后重新扫描，或重新选择目录授权。", error = true)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (running) MotionOutlinedButton({ tools.cancelScan(root.id) }) { Text("取消扫描") }
                        else {
                            MotionOutlinedButton({ tools.scan(root.id) }) { Text(if (scan?.error != null) "重试扫描" else "重新扫描") }
                            if (!root.available) MotionButton({ choose(root.id) }, enabled = !state.importBusy && state.draftUri == null) { Text("重新授权") }

                        }
                    }
                    DetailDisclosure("目录信息", runCatching { RootMode.valueOf(root.mode).label() }.getOrDefault(root.mode)) {
                        Text(path, style = MaterialTheme.typography.bodyMedium)
                        if (root.available && !running) MotionTextButton({ choose(root.id) }, enabled = !state.importBusy && state.draftUri == null) { Text("重新授权") }
                        MaintenanceHint("系统授权地址")
                        androidx.compose.foundation.text.selection.SelectionContainer { Text(root.treeUri, style = MaterialTheme.typography.bodySmall) }
                    }
                }
            }
        }
    }
}

private fun readableDirectory(uri: String): String = runCatching {
    android.provider.DocumentsContract.getTreeDocumentId(android.net.Uri.parse(uri))
        .replace("primary:", "内部存储/").replace(':', '/').split('/').filter { it.isNotBlank() }.joinToString(" / ")
}.getOrDefault("通过系统文件选择器授权")
