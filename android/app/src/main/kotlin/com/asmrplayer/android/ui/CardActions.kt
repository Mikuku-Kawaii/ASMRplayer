package com.asmrplayer.android.ui

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.onLongClick
import androidx.compose.ui.semantics.semantics

internal typealias CardMenu = @Composable ColumnScope.(close: () -> Unit) -> Unit

@Composable
internal fun CardActions(title: String, enabled: Boolean = true, modifier: Modifier = Modifier, menu: CardMenu, content: @Composable () -> Unit) {
    var visible by remember { mutableStateOf(false) }
    val haptic = LocalHapticFeedback.current
    val gesture = if (!enabled) Modifier else Modifier
        .semantics { onLongClick(label = "更多操作") { visible = true; true } }
        .pointerInput(haptic) {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                // Observe without taking the down event away from nested buttons or the pager.
                val cancelled = withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) {
                    do {
                        val point = awaitPointerEvent(PointerEventPass.Final).changes.singleOrNull()
                        if (point == null || point.id != down.id || !point.pressed ||
                            (point.position - down.position).getDistance() > viewConfiguration.touchSlop ||
                            (point.isConsumed && point.position != point.previousPosition)) break
                    } while (true)
                    true
                }
                if (cancelled == null) {
                    haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                    visible = true
                    do {
                        val event = awaitPointerEvent(PointerEventPass.Initial)
                        event.changes.forEach { it.consume() }
                    } while (event.changes.any { it.pressed })
                }
            }
        }
    Box(modifier.fillMaxWidth().then(gesture)) { content() }
    if (visible && enabled) PlayerSheet(title, { visible = false }) {
        Column { menu { visible = false } }
    }
}

@Composable
internal fun CardAction(label: String, close: () -> Unit, enabled: Boolean = true, action: () -> Unit) {
    DropdownMenuItem(text = { Text(label) }, enabled = enabled, onClick = {
        close()
        action()
    })
}
