package com.asmrplayer.android.ui

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import com.asmrplayer.android.update.UpdatePhase
import com.asmrplayer.android.update.UpdateSnapshot

@Composable
internal fun UpdateReminder(update: UpdateSnapshot, canShow: Boolean, openUpdates: () -> Unit) {
    var dismissedVersion by rememberSaveable { mutableStateOf<String?>(null) }
    if (!canShow || !update.autoCheck) return
    val ready = when (update.phase) {
        UpdatePhase.AVAILABLE -> false
        UpdatePhase.READY -> true
        else -> return
    }
    val release = update.release ?: return
    if (release.version == update.skippedVersion || release.version == dismissedVersion) return

    val dismiss = { dismissedVersion = release.version }
    PlayerAlertDialog(
        onDismissRequest = dismiss,
        title = { Text(if (ready) "更新已就绪" else "发现新版本") },
        text = {
            Text(
                if (ready) "版本 ${release.version} 的安装包已通过校验。"
                else "版本 ${release.version} 可下载（${storageBytes(release.size)}）。"
            )
        },
        dismissButton = { MotionTextButton(dismiss) { Text("稍后") } },
        confirmButton = {
            MotionButton({
                dismiss()
                openUpdates()
            }) { Text("查看更新") }
        },
    )
}
