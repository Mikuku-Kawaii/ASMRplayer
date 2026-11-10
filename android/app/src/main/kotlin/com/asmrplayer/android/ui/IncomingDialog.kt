package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.PlayerScreenState
import com.asmrplayer.android.incoming.*

@UnstableApi
@Composable
internal fun IncomingDialog(request: IncomingState, tools: IncomingTools, state: PlayerScreenState, openWork: (String) -> Unit) {
    if (!request.visible) return
    var target by rememberSaveable(request.files) { mutableStateOf(state.session.versionId) }
    var translation by rememberSaveable { mutableStateOf(false) }
    var choosing by remember { mutableStateOf(false) }
    val versions = state.library.filter { visibleRating(it.rating, state) }.flatMap { work -> work.versions.map { it.id to "${work.title} · ${it.title}" } }
    val needsTarget = request.files.any { it.subtitle }
    PlayerAlertDialog(onDismissRequest = tools::dismiss, title = { Text(if (request.workId != null) "打开分享作品" else "导入分享文件") },
        text = { Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            request.workId?.let { Text("RJ$it · 使用当前在线来源查看详情") }
            if (request.files.isNotEmpty()) Text("音频复制到应用资料库；字幕绑定到你选择的音轨。导入不会开始播放。")
            request.files.forEach { Text(it.name, style = MaterialTheme.typography.bodySmall) }
            if (needsTarget) {
                MotionOutlinedButton({ choosing = !choosing }, enabled = !request.busy) { Text(versions.firstOrNull { it.first == target }?.second ?: "选择字幕对应音轨", maxLines = 2) }
                if (choosing) versions.forEach { (id, title) -> MotionTextButton({ target = id; choosing = false }) { Text(title, maxLines = 2) } }
                ToggleRow("作为译文字幕", "关闭时导入为原文字幕，不进行识别或翻译。", translation, !request.busy) { translation = it }
                if (request.files.count { it.subtitle } > 1) Text("多份字幕都会保留，最后一份设为当前字幕。",style=MaterialTheme.typography.bodySmall)
                if (versions.isEmpty()) Text("请先导入音频，再分享字幕。")
            }
            if (request.busy) { LoadingRing(); Text(request.progress) }
            request.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        } },
        confirmButton = { MotionTextButton({
            request.workId?.let { tools.dismiss(); openWork(it) } ?: tools.importFiles(target, translation)
        }, enabled = !request.busy && (request.workId != null || request.files.isNotEmpty() && (!needsTarget || versions.any { it.first == target }))) { Text(if (request.workId != null) "查看作品" else "确认导入") } },
        dismissButton = { MotionTextButton(tools::dismiss) { Text("取消") } })
}
