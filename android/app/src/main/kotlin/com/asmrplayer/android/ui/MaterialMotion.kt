package com.asmrplayer.android.ui

import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.window.DialogWindowProvider
import com.asmrplayer.android.R

/** Window-level exit survives removal from composition, without delaying an action or save. */
@Composable
internal fun DialogMotion() {
    val view = LocalView.current
    val enabled = LocalUiMotion.current
    DisposableEffect(view, enabled) {
        val window = (view.parent as? DialogWindowProvider)?.window
        window?.setWindowAnimations(if (enabled) R.style.PlayerDialogMotion else 0)
        onDispose { /* Keep the exit style until the window is removed by Dialog. */ }
    }
}
