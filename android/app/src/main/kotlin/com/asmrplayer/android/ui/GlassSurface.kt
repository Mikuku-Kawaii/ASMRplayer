package com.asmrplayer.android.ui

import android.os.Build
import android.graphics.RenderEffect
import android.graphics.RuntimeShader
import android.graphics.Shader
import androidx.annotation.RequiresApi
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp

/** Only the page records into this layer. Controls are drawn as siblings, never sampled. */
class GlassBackdrop(val layer: GraphicsLayer, val filtered: GraphicsLayer, val blur: androidx.compose.ui.graphics.RenderEffect?, val margin: Int, val refraction: Boolean) {
    var origin by mutableStateOf(Offset.Zero)
    var dockBounds by mutableStateOf(Rect.Zero)
    // Drawing coordinates are consumed in the same frame; they are not UI state.
    var filteredOrigin = Offset.Zero
    val sampleScale = if (Build.VERSION.SDK_INT >= 33) .5f else 1f
}

@Composable
fun rememberGlassBackdrop(refraction: Boolean = true): GlassBackdrop {
    val layer = rememberGraphicsLayer()
    val filtered = rememberGraphicsLayer()
    val density = LocalDensity.current
    val radius = with(density) { 24.dp.toPx() } * (if (Build.VERSION.SDK_INT >= 33) .5f else 1f)
    val margin = with(density) { 76.dp.roundToPx() }
    val blur = remember(radius) { if(Build.VERSION.SDK_INT>=31) {
        val saturation = android.graphics.ColorMatrix().apply {
            setSaturation(.78f)
            postConcat(android.graphics.ColorMatrix(floatArrayOf(1.05f,0f,0f,0f,0f, 0f,1.05f,0f,0f,0f,
                0f,0f,1.05f,0f,0f, 0f,0f,0f,1f,0f)))
        }
        RenderEffect.createColorFilterEffect(android.graphics.ColorMatrixColorFilter(saturation),
            RenderEffect.createBlurEffect(radius,radius,Shader.TileMode.CLAMP)).asComposeRenderEffect()
    } else null }
    return remember(layer,filtered,blur,margin,refraction) { GlassBackdrop(layer,filtered,blur,margin,refraction) }
}

fun Modifier.glassDockBounds(source: GlassBackdrop): Modifier = onGloballyPositioned {
    val p=it.positionInRoot()
    source.dockBounds=Rect(p.x,p.y,p.x+it.size.width,p.y+it.size.height)
}

fun Modifier.recordBackdrop(source: GlassBackdrop, enabled: Boolean, background: Color): Modifier =
    onGloballyPositioned { source.origin = it.positionInRoot() }.drawWithContent {
        if (!enabled) { drawContent(); return@drawWithContent }
        source.layer.record { drawRect(background); this@drawWithContent.drawContent() }
        drawLayer(source.layer)
        val dock=source.dockBounds
        if(dock.height>0) {
            val pad=source.margin
            val start=Offset(dock.left-source.origin.x-pad,dock.top-source.origin.y-pad)
            source.filteredOrigin=start
            source.filtered.renderEffect=source.blur
            val ratio = source.sampleScale
            source.filtered.record(size=IntSize(kotlin.math.ceil((dock.width+pad*2)*ratio).toInt().coerceAtLeast(1),kotlin.math.ceil((dock.height+pad*2)*ratio).toInt().coerceAtLeast(1))) {
                drawRect(background)
                scale(ratio, ratio, Offset.Zero) { translate(-start.x,-start.y) {
                    if(Build.VERSION.SDK_INT>=33) drawLayer(source.layer)
                    else this@drawWithContent.drawContent()
                } }
            }
        }
    }

@Composable
fun GlassSurface(source: GlassBackdrop, lowEffects: Boolean, modifier: Modifier = Modifier,
                 active: Boolean = false, baseTint: Color = MaterialTheme.colorScheme.surfaceContainer,
                 content: @Composable BoxScope.() -> Unit) {
    val material = rememberGraphicsLayer()
    var position by remember { mutableStateOf(Offset.Zero) }
    val density = LocalDensity.current
    val margin = with(density) { 22.dp.roundToPx() }
    val edge = with(density) { 14.dp.toPx() }
    val strength = with(density) { (if(active) 6.8.dp else 4.dp).toPx() }
    val optical = remember(source.refraction) { if (source.refraction && Build.VERSION.SDK_INT >= 33) GlassRefraction() else null }
    val shape = RoundedCornerShape(50)
    val scheme = MaterialTheme.colorScheme
    val sampling = Build.VERSION.SDK_INT >= 31 && !lowEffects
    val dark = scheme.background.luminance() < .5f
    val tint = if (active) scheme.primaryContainer else baseTint
    val fillAlpha by androidx.compose.animation.core.animateFloatAsState(
        if(!sampling) 1f else if(active) .46f else .34f,
        animationSpec=if(dockMotionEnabled(lowEffects)) androidx.compose.animation.core.tween(180) else androidx.compose.animation.core.snap(),
        label="glass fill")
    val rim = scheme.onSurface.copy(alpha = if(active) .22f else .10f)
    Box(modifier.clip(shape).onGloballyPositioned { position = it.positionInRoot() }
        .drawWithCache {
            val bounds = IntSize(size.width.toInt() + 2 * margin, size.height.toInt() + 2 * margin)
            val effect = if (sampling && Build.VERSION.SDK_INT >= 33 && optical != null)
                optical.effect(size.width, size.height, margin.toFloat(), strength, edge, if (active) .08f else 0f)
                    .asComposeRenderEffect() else null
            val highlight = Color.White.copy(alpha = if (dark) { if (active) .40f else .20f } else { if (active) .75f else .50f })
            val sheen = Brush.linearGradient(listOf(Color.White.copy(alpha = if (dark) .035f else .11f),
                Color.Transparent, scheme.primary.copy(alpha = if (active) .09f else .015f)))
            val border = Brush.linearGradient(listOf(highlight, rim, Color.Transparent, rim))
            val radius = CornerRadius(size.height / 2)
            val stroke = Stroke((if (active) 2.4.dp else 1.6.dp).toPx())
            val edgeStroke = Stroke(3.dp.toPx())
            onDrawWithContent {
                if (sampling && source.filtered.size.width > 0) {
                    material.record(size = bounds) {
                        drawRect(scheme.background)
                        scale(if (active && Build.VERSION.SDK_INT < 33) 1.06f else 1f) {
                            translate(margin - (position.x - source.origin.x) + source.filteredOrigin.x,
                                margin - (position.y - source.origin.y) + source.filteredOrigin.y) {
                                scale(1f / source.sampleScale, 1f / source.sampleScale, Offset.Zero) { drawLayer(source.filtered) }
                            }
                        }
                    }
                    material.renderEffect = effect
                    translate(-margin.toFloat(), -margin.toFloat()) { drawLayer(material) }
                }
                drawRect(tint.copy(alpha = fillAlpha))
                drawRect(sheen)
                drawContent()
                drawRoundRect(border, cornerRadius = radius, style = stroke)
                if (active && !lowEffects) clipRect(top = size.height * .83f) {
                    drawRoundRect(scheme.primary.copy(alpha = .22f), cornerRadius = radius, style = edgeStroke)
                }
            }
        }, content = content)
}

@RequiresApi(33)
private class GlassRefraction {
    private val shader = RuntimeShader("""
        uniform shader content;
        uniform float2 extent;
        uniform float padding;
        uniform float strength;
        uniform float edgeWidth;
        uniform float magnification;
        half4 main(float2 p) {
            float2 local = p - padding;
            float2 halfSize = extent * 0.5;
            float radius = min(halfSize.x, halfSize.y);
            float2 q = abs(local - halfSize) - (halfSize - radius);
            float distance = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
            float edge = pow(clamp(1.0 - abs(distance) / edgeWidth, 0.0, 1.0), 2.0);
            float2 centerline = clamp(local - halfSize, -(halfSize-radius), halfSize-radius);
            float2 normal = local - halfSize - centerline;
            normal /= max(length(normal), 1.0);
            float2 lens = (local-halfSize) / (1.0+magnification) + halfSize + padding;
            return content.eval(lens - normal * edge * strength);
        }
    """.trimIndent())
    fun effect(width: Float, height: Float, padding: Float, strength: Float, edge: Float, magnification: Float): RenderEffect {
        shader.setFloatUniform("extent",width,height)
        shader.setFloatUniform("padding",padding)
        shader.setFloatUniform("strength",strength)
        shader.setFloatUniform("edgeWidth",edge)
        shader.setFloatUniform("magnification",magnification)
        return RenderEffect.createRuntimeShaderEffect(shader, "content")
    }
}
