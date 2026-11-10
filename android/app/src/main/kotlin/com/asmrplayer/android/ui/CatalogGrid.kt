package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

private val LocalGridWidth = staticCompositionLocalOf<Dp?> { null }
private val LocalMetrics = staticCompositionLocalOf<TextMeasurer?> { null }

@Composable
internal fun cardMeasurer() = LocalMetrics.current ?: rememberTextMeasurer()

/** Grid cards share one measured width. Standalone artwork retain their own constraints. */
@Composable
internal fun CardWidth(modifier: Modifier, inset: Dp = 0.dp, content: @Composable (width: Dp, height: Dp) -> Unit) {
    val gridWidth = LocalGridWidth.current
    if (gridWidth == null) BoxWithConstraints(modifier) { content(maxWidth, maxHeight) }
    else Box(modifier) { content((gridWidth - inset).coerceAtLeast(0.dp), gridWidth * .75f) }
}

@Composable
internal fun CatalogGrid(state: LazyGridState, bottom: Dp, content: LazyGridScope.() -> Unit) {
    val metrics = rememberTextMeasurer(cacheSize = 128)
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val minWidth = 145.dp * LocalDensity.current.fontScale.coerceIn(1f, 1.8f)
        val width = with(LocalDensity.current) {
            val available = (constraints.maxWidth - 2 * 16.dp.roundToPx()).coerceAtLeast(1)
            val gap = 10.dp.roundToPx()
            val columns = ((available + gap) / (minWidth.roundToPx() + gap)).coerceAtLeast(1)
            ((available - (columns - 1) * gap) / columns).toDp()
        }
        CompositionLocalProvider(LocalGridWidth provides width, LocalMetrics provides metrics) {
            LazyVerticalGrid(GridCells.Adaptive(minWidth), modifier = Modifier.fillMaxSize(), state = state,
                contentPadding = PaddingValues(start=16.dp,end=16.dp,top=8.dp,bottom=bottom),
                horizontalArrangement=Arrangement.spacedBy(10.dp),verticalArrangement=Arrangement.spacedBy(10.dp),content=content)
        }
    }
}
