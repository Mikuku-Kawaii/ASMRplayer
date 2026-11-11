package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import kotlinx.coroutines.launch
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.layout.layout
import kotlin.math.roundToInt
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.PlayerScreenState
import com.asmrplayer.android.PlayerViewModel
import com.asmrplayer.android.UiFeedback
import com.asmrplayer.core.DspConfig
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.text.style.TextOverflow

@Composable
internal fun RemovableChip(onClick: () -> Unit, label: @Composable () -> Unit) {
    InputChip(true, onClick, label, trailingIcon = { AppGlyph(Glyph.Close, Modifier.size(16.dp)) })
}

@Composable
internal fun ChoiceChip(selected: Boolean, onClick: () -> Unit, label: @Composable () -> Unit,
    modifier: Modifier = Modifier, enabled: Boolean = true, glyph: Glyph? = null) {
    FilterChip(selected, onClick, label, modifier, enabled,
        leadingIcon = if (glyph != null || selected) ({ AppGlyph(glyph ?: Glyph.Check, Modifier.size(16.dp)) }) else null,
        colors = FilterChipDefaults.filterChipColors(selectedContainerColor=MaterialTheme.colorScheme.primaryContainer,
            selectedLabelColor=MaterialTheme.colorScheme.onPrimaryContainer))
}

/** Cap the content, not ModalBottomSheet itself: its drag anchors need the full window height. */
internal fun Modifier.contentSheetHeight() = layout { measurable, constraints ->
    val limit = if (constraints.hasBoundedHeight) (constraints.maxHeight * .75f).roundToInt() else constraints.maxHeight
    val sheet = measurable.measure(constraints.copy(minHeight = 0, maxHeight = limit))
    layout(sheet.width, sheet.height) { sheet.placeRelative(0, 0) }
}

/** A transient task keeps the player underneath; long tools own their scrolling inside this slot. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun PlayerSheet(title: String, close: () -> Unit, contentScrolls: Boolean = false, content: @Composable () -> Unit) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded=true)
    val scope = rememberCoroutineScope()
    val motion = LocalUiMotion.current
    fun dismiss() {
        if (!motion) close() else scope.launch {
            sheetState.hide()
            if (!sheetState.isVisible) close()
        }
    }
    ModalBottomSheet(onDismissRequest=close, sheetState=sheetState,
        containerColor=MaterialTheme.colorScheme.surfaceContainer, contentColor=MaterialTheme.colorScheme.onSurface) {
        Box(Modifier.fillMaxWidth().imePadding().contentSheetHeight().sizeMotion()) {
        Column(Modifier.fillMaxWidth()) {
            Row(Modifier.fillMaxWidth().padding(start=20.dp,end=8.dp),verticalAlignment=Alignment.CenterVertically) {
                Text(title,Modifier.weight(1f),style=MaterialTheme.typography.titleMedium)
                GlyphButton(Glyph.Close,"关闭$title",::dismiss)
            }
            val body = Modifier.fillMaxWidth().weight(1f, fill = false)
            if (contentScrolls) Box(body) { content() }
            else Column(body.verticalScroll(rememberScrollState()).padding(horizontal=20.dp,vertical=12.dp),
                verticalArrangement=Arrangement.spacedBy(12.dp)) { content() }
            NoticeOverlay(modal = true)
        }
        }
    }
}

internal fun trackDisplayTitle(title: String): String = title.replace(Regex("(?i)\\.(mp3|flac|wav|m4a|aac|ogg|opus|mp4|wma|aiff)$"), "")

/** Translation is measured in viewport pixels about its centre, after ContentScale.Fit. */
internal data class CoverTransform(val scale: Float = 1f, val offset: Offset = Offset.Zero) {
    fun gesture(view: IntSize, image: IntSize, centroid: Offset, pan: Offset, zoom: Float): CoverTransform {
        if (view.width == 0 || view.height == 0) return this
        val next = (scale * zoom).coerceIn(1f, 4f)
        if (next == 1f) return CoverTransform()
        val fit = minOf(view.width.toFloat() / image.width, view.height.toFloat() / image.height)
        val pivot = centroid - Offset(view.width / 2f, view.height / 2f)
        val moved = pivot + pan + (offset - pivot) * (next / scale)
        val maxX = ((image.width * fit * next - view.width) / 2f).coerceAtLeast(0f)
        val maxY = ((image.height * fit * next - view.height) / 2f).coerceAtLeast(0f)
        return CoverTransform(next, Offset(moved.x.coerceIn(-maxX, maxX), moved.y.coerceIn(-maxY, maxY)))
    }
}

@Composable
internal fun FullscreenArtwork(uri: String?, workId:String?=null, close: () -> Unit) {
    val artwork=rememberArtwork(uri, highResolution=true, workId=workId, fullscreen=true)
    Dialog(onDismissRequest=close,properties=DialogProperties(usePlatformDefaultWidth=false,decorFitsSystemWindows=false)) {
        DialogMotion()
        NoticeOverlay(modal = true)
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            if (artwork.loading) CircularProgressIndicator(Modifier.align(Alignment.Center).size(28.dp))
            artwork.image?.let { bitmap -> key(uri, workId) { ZoomableCover(bitmap, Modifier.fillMaxSize()) } }
            CompositionLocalProvider(LocalContentColor provides Color.White) {
                Box(Modifier.align(Alignment.TopEnd).statusBarsPadding().padding(12.dp)) { GlyphButton(Glyph.Close,"关闭封面",close) }
            }
        }
    }
}


@Composable
internal fun ZoomableCover(bitmap: ImageBitmap, modifier: Modifier = Modifier) {
    BoxWithConstraints(modifier.clipToBounds()) {
        val viewport = IntSize(constraints.maxWidth, constraints.maxHeight)
        val pixels = IntSize(bitmap.width, bitmap.height)
        var transform by remember(viewport, pixels) { mutableStateOf(CoverTransform()) }
        Image(bitmap, "作品封面", Modifier.fillMaxSize().pointerInput(viewport, pixels) {
            detectTransformGestures { centroid, pan, zoom, _ ->
                transform = transform.gesture(viewport, pixels, centroid, pan, zoom)
            }
        }.graphicsLayer {
            scaleX = transform.scale; scaleY = transform.scale
            translationX = transform.offset.x; translationY = transform.offset.y
        }, contentScale = ContentScale.Fit)
    }
}
