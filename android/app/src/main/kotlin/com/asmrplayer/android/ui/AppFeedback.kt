package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.*
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import com.asmrplayer.android.UiFeedback
import kotlinx.coroutines.delay

internal const val NOTICE_DURATION_MS = 3000L

@Composable
internal fun LoadingRing(label: String? = null) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically) {
        CircularProgressIndicator(Modifier.size(24.dp).semantics { contentDescription = "加载中" }, strokeWidth = 2.dp)
        if (label != null) Text(label, Modifier.padding(start = 8.dp), style = MaterialTheme.typography.labelMedium)
    }
}

internal class NoticeHost {
    class Entry(val event: UiFeedback)
    var current by mutableStateOf<Entry?>(null)
        private set
    val windows = mutableStateListOf<Any>()
    fun show(event: UiFeedback) { current = Entry(event) }
    fun dismiss(entry: Entry) { if (entry === current) current = null }
}

internal val LocalNoticeHost = staticCompositionLocalOf<NoticeHost?> { null }

@Composable
internal fun NoticeTimer(host: NoticeHost) {
    val entry = host.current
    LaunchedEffect(entry) { if (entry != null) { delay(NOTICE_DURATION_MS); host.dismiss(entry) } }
}

/** Only the frontmost Compose window presents the event. Opening a sheet doesn't restart its timer. */
@Composable
internal fun NoticeOverlay(modal: Boolean = false) {
    val host = LocalNoticeHost.current ?: return
    val window = remember { Any() }
    if (modal) DisposableEffect(host) { host.windows.add(window); onDispose { host.windows.remove(window) } }
    if (host.windows.lastOrNull() !== (if (modal) window else null)) return
    val entry = host.current ?: return
    val density = LocalDensity.current
    val top = WindowInsets.statusBars.getTop(density) + with(density) { 16.dp.roundToPx() }
    val position = remember(top) { object : PopupPositionProvider {
        override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize) =
            IntOffset((windowSize.width - popupContentSize.width) / 2, top.coerceAtMost((windowSize.height - popupContentSize.height).coerceAtLeast(0)))
    } }
    Popup(popupPositionProvider = position, properties = PopupProperties(focusable = false, dismissOnBackPress = false, dismissOnClickOutside = false)) {
        val event = entry.event
        Surface(Modifier.padding(horizontal = 16.dp, vertical = 4.dp).widthIn(max = 360.dp)
            .semantics { liveRegion = LiveRegionMode.Polite }, shape = RoundedCornerShape(20.dp),
            color = MaterialTheme.colorScheme.inverseSurface, contentColor = MaterialTheme.colorScheme.inverseOnSurface, shadowElevation = 4.dp) {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(event.message, Modifier.weight(1f, fill = false), style = MaterialTheme.typography.bodyMedium)
                event.action?.let { action -> TextButton(onClick = { host.dismiss(entry); action() }) {
                    Text(event.actionLabel ?: "撤销", color = MaterialTheme.colorScheme.inversePrimary)
                } }
            }
        }
    }
}

@Composable
internal fun PlayerAlertDialog(onDismissRequest: () -> Unit, confirmButton: @Composable () -> Unit,
    dismissButton: (@Composable () -> Unit)? = null, title: (@Composable () -> Unit)? = null, text: (@Composable () -> Unit)? = null) {
    AlertDialog(onDismissRequest = onDismissRequest, title = title, text = text, dismissButton = dismissButton,
        confirmButton = { NoticeOverlay(modal = true); confirmButton() })
}
