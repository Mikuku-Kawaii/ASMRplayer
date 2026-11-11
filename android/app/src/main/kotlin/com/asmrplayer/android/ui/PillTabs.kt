package com.asmrplayer.android.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import kotlin.math.abs

@Composable
internal fun <T : Any> PageTabs(selected: T?, options: List<Pair<T, String>>, modifier: Modifier = Modifier,
    change: (T) -> Unit) {
    if (options.isEmpty()) return
    val colors = MaterialTheme.colorScheme
    val style = MaterialTheme.typography.labelLarge
    val density = LocalDensity.current
    val scroll = rememberLazyListState()
    val motion = LocalUiMotion.current
    val inputMode = LocalInputModeManager.current
    BoxWithConstraints(modifier.fillMaxWidth().clip(CircleShape).background(colors.surfaceContainerHigh)
        .padding(horizontal = 4.dp)) {
        val viewport = constraints.maxWidth
        // Use identical pixel widths and center any rounding remainder in the track.
        val equalShare = viewport / options.size
        val segmentWidth = if (options.size <= 3) equalShare else with(density) {
            maxOf(equalShare, (80.dp * fontScale).roundToPx().coerceAtMost(viewport))
        }
        val stripWidth = minOf(viewport, segmentWidth * options.size)
        val active = options.indexOfFirst { it.first == selected }
        // Animate in slots, not pixels. Springs retain velocity when another tab interrupts the glide.
        val target = active.coerceAtLeast(0).toFloat()
        val position = animateFloatAsState(target, if (motion) spring(dampingRatio = 1f, stiffness = 900f,
            visibilityThreshold = .001f) else snap(), label = "tab glide")
        LaunchedEffect(active, segmentWidth, stripWidth, motion) {
            if (active < 0) return@LaunchedEffect
            val first = scroll.firstVisibleItemIndex * segmentWidth + scroll.firstVisibleItemScrollOffset
            val endClipped = (active + 1) * segmentWidth > first + stripWidth
            if (active * segmentWidth < first || endClipped) {
                val offset = if (endClipped) segmentWidth - stripWidth else 0
                if (motion) scroll.animateScrollToItem(active, offset) else scroll.scrollToItem(active, offset)
            }
        }
        LazyRow(Modifier.align(Alignment.Center).width(with(density) { stripWidth.toDp() }).clip(CircleShape).selectableGroup().drawBehind {
            if (active < 0) return@drawBehind
            val inset = 4.dp.toPx()
            val height = (size.height - 2 * inset).coerceAtLeast(0f)
            val scrollX = scroll.firstVisibleItemIndex * segmentWidth + scroll.firstVisibleItemScrollOffset
            val front = position.value * segmentWidth
            val start = front - scrollX
            val width = segmentWidth.toFloat()
            val x = if (layoutDirection == LayoutDirection.Ltr) start else size.width - start - width
            drawRoundRect(colors.primaryContainer, Offset(x, inset), Size(width, height), CornerRadius(height / 2))
        }, state = scroll, verticalAlignment = Alignment.CenterVertically) {
            itemsIndexed(options, key = { _, option -> option.first }) { index, (key, label) ->
                val isSelected = key == selected
                var focused by remember { mutableStateOf(false) }
                Row(Modifier.width(with(density) { segmentWidth.toDp() }).heightIn(min = 48.dp).clip(CircleShape)
                    .onFocusChanged { focused = it.isFocused }
                    .then(if (focused && inputMode.inputMode == InputMode.Keyboard) Modifier.border(2.dp, colors.primary, CircleShape) else Modifier)
                    .selectable(isSelected, interactionSource = null, indication = null, role = Role.Tab, onClick = { if (!isSelected) change(key) })
                    .padding(horizontal = 10.dp, vertical = 6.dp), horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically) {
                    BasicText(label, style = style, maxLines = 1, overflow = TextOverflow.Ellipsis, color = {
                        val coverage = if (active < 0) 0f else (1f - abs(position.value - index)).coerceIn(0f, 1f)
                        lerp(colors.onSurfaceVariant, colors.onPrimaryContainer, coverage)
                    })
                }
            }
        }
    }
}
