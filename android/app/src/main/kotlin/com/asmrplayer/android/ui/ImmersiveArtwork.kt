package com.asmrplayer.android.ui

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.*
import androidx.compose.ui.layout.ContentScale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

// Object identity distinguishes revisions without keeping the original full-size bitmap alive.
private val blurIds = java.util.WeakHashMap<Bitmap, Long>()
private var nextBlurId = 0L
private val blurCache = object : android.util.LruCache<Pair<Long, Int>, ImageBitmap>(4 * 1024 * 1024) {
    override fun sizeOf(key: Pair<Long, Int>, value: ImageBitmap) = value.asAndroidBitmap().allocationByteCount
}

@Composable
private fun rememberedBlur(image: ImageBitmap?, edge: Int, enabled: Boolean): ImageBitmap? {
    if (!enabled || image == null) return null
    val source = image.asAndroidBitmap()
    val key = remember(source, edge) { synchronized(blurIds) { blurIds.getOrPut(source) { ++nextBlurId } } to edge }
    val warm = remember(key) { blurCache.get(key) }
    val result by produceState(warm, source, edge) {
        if (warm == null) value = withContext(Dispatchers.Default) {
            blurCover(image, longestEdge = edge, radius = 4).also {
                blurCache.put(key, it)
            }
        }
    }
    return result
}

/** PC-style immersive backdrop with stronger cover colour: 1.7 saturation and .56 brightness.
 * A small static bitmap is prepared once per cover, off the UI thread, on every supported API.
 * Clamped edge sampling avoids a dark border; foreground artwork and controls stay sharp.
 */
@Composable
internal fun ImmersiveArtwork(image: ImageBitmap?, lowEffects: Boolean, modifier: Modifier = Modifier) {
    key(image, lowEffects) {
        val blurred = rememberedBlur(image, 192, !lowEffects)
        val filter = remember {
            val matrix = android.graphics.ColorMatrix().apply {
                setSaturation(1.7f)
                postConcat(android.graphics.ColorMatrix(floatArrayOf(
                    .56f,0f,0f,0f,0f, 0f,.56f,0f,0f,0f,
                    0f,0f,.56f,0f,0f, 0f,0f,0f,1f,0f)))
            }
            ColorFilter.colorMatrix(ColorMatrix(matrix.array))
        }
        blurred?.let {
            Image(it, null, modifier.fillMaxSize().graphicsLayer { scaleX=1.16f; scaleY=1.16f },
                contentScale=ContentScale.Crop, colorFilter=filter)
        }
    }
}

/** Align clear/soft copies exactly, then fade their composite once into the full-screen backdrop. */
@Composable
internal fun FusedCoverArtwork(image: ImageBitmap, lowEffects: Boolean, contentScale: ContentScale, modifier: Modifier = Modifier) {
    key(image, lowEffects) {
        val softened = rememberedBlur(image, 384, !lowEffects)
        val compositeFade = remember { coverFade(.22f,.42f) }
        val sharpFade = remember { coverFade(.16f,.58f) }
        Box(modifier.clipToBounds().graphicsLayer { compositingStrategy=CompositingStrategy.Offscreen }
            .drawWithContent { drawContent(); drawRect(compositeFade,blendMode=BlendMode.DstIn) }) {
            softened?.let {
                Image(it,null,Modifier.fillMaxSize(),contentScale=contentScale,alignment=Alignment.TopCenter)
            }
            Image(image,null,Modifier.fillMaxSize().graphicsLayer { compositingStrategy=CompositingStrategy.Offscreen }
                .drawWithContent { drawContent(); if(softened!=null)drawRect(sharpFade,blendMode=BlendMode.DstIn) },
                contentScale=contentScale,alignment=Alignment.TopCenter)
        }
    }
}

/** Smoothstep has a level slope at both joins, avoiding visible transition bands. */
private fun coverFade(topEnd: Float, bottomStart: Float): Brush = Brush.verticalGradient(
    *(0..64).map { step ->
        val position=step/64f
        val distance=minOf(position/topEnd,(1f-position)/(1f-bottomStart)).coerceIn(0f,1f)
        position to Color.White.copy(alpha=distance*distance*(3f-2f*distance))
    }.toTypedArray())

private fun blurCover(image: ImageBitmap, longestEdge: Int = 192, radius: Int = 6): ImageBitmap {
    val ratio = minOf(1f, longestEdge.toFloat() / maxOf(image.width, image.height))
    val width = (image.width * ratio).roundToInt().coerceAtLeast(1)
    val height = (image.height * ratio).roundToInt().coerceAtLeast(1)
    val small = Bitmap.createScaledBitmap(image.asAndroidBitmap(), width, height, true)
    var pixels = IntArray(width * height)
    var output = IntArray(pixels.size)
    small.getPixels(pixels, 0, width, 0, 0, width, height)
    if (small !== image.asAndroidBitmap()) small.recycle()
    // Three separable box passes approximate a Gaussian on this bounded thumbnail.
    repeat(6) { pass ->
        val horizontal = pass % 2 == 0
        for (y in 0 until height) for (x in 0 until width) {
            var a=0; var r=0; var g=0; var b=0
            for (delta in -radius..radius) {
                val xx = if(horizontal) (x+delta).coerceIn(0,width-1) else x
                val yy = if(horizontal) y else (y+delta).coerceIn(0,height-1)
                val pixel = pixels[yy*width+xx]
                a += pixel ushr 24; r += (pixel ushr 16) and 255
                g += (pixel ushr 8) and 255; b += pixel and 255
            }
            val samples=radius*2+1
            output[y*width+x] = ((a/samples) shl 24) or ((r/samples) shl 16) or ((g/samples) shl 8) or (b/samples)
        }
        val old = pixels; pixels = output; output = old
    }
    return Bitmap.createBitmap(pixels, width, height, Bitmap.Config.ARGB_8888).asImageBitmap()
}
