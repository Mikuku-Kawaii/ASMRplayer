package com.asmrplayer.android.ui

import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Match the desktop WorkContentBadges overlay; sample only the area behind each label. */
@Composable
internal fun CardArtwork(cover: String?, modifier: Modifier, workId: String?, hasSubtitle: Boolean, nsfw: Boolean) {
    if (!hasSubtitle && !nsfw) {
        Box(modifier) { AudioArtwork(Modifier.fillMaxSize(), cover, workId = workId) }
        return
    }
    val sampling = Build.VERSION.SDK_INT >= 31
    val source = if (sampling) rememberGraphicsLayer() else null
    var origin by remember { mutableStateOf<LayoutCoordinates?>(null) }
    Box(modifier.onPlaced { origin = it }) {
        AudioArtwork(Modifier.fillMaxSize().drawWithContent {
            if (source != null) {
                source.record { this@drawWithContent.drawContent() }
                drawLayer(source)
            } else drawContent()
        }, cover, workId = workId)
        Row(Modifier.align(Alignment.TopEnd).padding(8.dp), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            if (hasSubtitle) ContentBadge(false, source) { origin }
            if (nsfw) ContentBadge(true, source) { origin }
        }
    }
}

@Composable
private fun ContentBadge(adult: Boolean, source: GraphicsLayer?, origin: () -> LayoutCoordinates?) {
    val density = LocalDensity.current
    val halo = with(density) { 16.dp.roundToPx() }
    val radius = with(density) { 8.dp.toPx() }
    val frost = if (source != null) rememberGraphicsLayer() else null
    val blur = remember(radius) {
        if (Build.VERSION.SDK_INT >= 31) android.graphics.RenderEffect.createBlurEffect(
            radius, radius, android.graphics.Shader.TileMode.CLAMP).asComposeRenderEffect() else null
    }
    var offset by remember { mutableStateOf(Offset.Zero) }
    val rose = Color(0xFFF43F5E)
    val green = Color(0xFF34D399)
    val shape = RoundedCornerShape(5.dp)
    Text(if (adult) "R18" else "含字幕",
        Modifier.onPlaced { node -> origin()?.takeIf { it.isAttached }?.let { offset = it.localPositionOf(node, Offset.Zero) } }
            .shadow(3.dp, shape).clip(shape).drawWithContent {
                if (source != null && frost != null && source.size.width > 0) {
                    frost.renderEffect = blur
                    frost.record(size = IntSize(size.width.toInt() + 2 * halo, size.height.toInt() + 2 * halo)) {
                        translate(halo - offset.x, halo - offset.y) { drawLayer(source) }
                    }
                    translate(-halo.toFloat(), -halo.toFloat()) { drawLayer(frost) }
                }
                // Keep the PC foreground legible even over white artwork and on devices without blur.
                drawRect(Color.Black.copy(alpha = if (source == null) .76f else .58f))
                drawContent()
            }.background(if (adult) rose.copy(alpha = .16f) else green.copy(alpha = .14f))
            .then(if (adult) Modifier.border(1.dp, rose.copy(alpha = .38f), shape) else Modifier)
            .padding(horizontal = 7.dp, vertical = 2.dp),
        color = if (adult) Color(0xFFFB7185) else green,
        style = TextStyle(fontSize = 11.sp, lineHeight = 14.sp, fontWeight = if (adult) FontWeight.Bold else FontWeight.SemiBold),
        maxLines = 1, softWrap = false)
}
