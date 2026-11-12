package com.asmrplayer.android.ui

import android.graphics.Bitmap
import com.asmrplayer.android.artwork.CoverArtwork
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import java.util.WeakHashMap
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

internal data class ArtworkState(val image: ImageBitmap? = null, val scene: Color? = null, val loading: Boolean = false)

private val sceneColors = WeakHashMap<Bitmap, Color>()

/** Warm results are visible during composition. Disk reads, original selection and sampling stay on IO. */
@Composable
internal fun rememberArtwork(uri: String?, highResolution: Boolean = false, workId: String? = null,
    fullscreen: Boolean = false, thumbnailEdge: Int = 384): ArtworkState {
    val context = LocalContext.current.applicationContext
    val width = LocalConfiguration.current.screenWidthDp * LocalDensity.current.density
    val edge = when {
        fullscreen -> 3072
        highResolution -> if (width <= 1536) 1536 else 2048
        else -> thumbnailEdge
    }
    if (uri == null && workId == null) return ArtworkState()
    val request = CoverArtwork.request(context, uri, workId, edge)
    return key(request) {
        val exact = remember(request) { CoverArtwork.peek(request) }
        val warm = remember(request) { exact ?: CoverArtwork.preview(request) }
        val initial = remember(warm) {
            ArtworkState(warm?.asImageBitmap(), warm?.let { synchronized(sceneColors) { sceneColors[it] } }, warm == null && !CoverArtwork.recentlyFailed(request))
        }
        if (exact != null && (!highResolution || initial.scene != null)) initial else {
            val result by produceState(initial, request) {
                value = withContext(Dispatchers.IO) {
                    try {
                        val bitmap = CoverArtwork.best(context, request) ?: return@withContext initial.copy(loading = false)
                        val scene = if (!highResolution) null else synchronized(sceneColors) { sceneColors[bitmap] } ?: coverSceneColor(bitmap).also {
                            synchronized(sceneColors) { sceneColors[bitmap] = it }
                        }
                        ArtworkState(bitmap.asImageBitmap(), scene)
                    } catch (e: CancellationException) { throw e }
                    catch (_: Exception) { initial.copy(loading = false) }
                }
            }
            result
        }
    }
}

/** Keep the cover's dominant chromatic hue instead of averaging its darkest pixels into grey. */
private fun coverSceneColor(bitmap: Bitmap): Color {
    val sample = Bitmap.createScaledBitmap(bitmap, 64, 48, true)
    val pixels = IntArray(64 * 48)
    sample.getPixels(pixels, 0, 64, 0, 0, 64, 48)
    if (sample !== bitmap) sample.recycle()
    val weights = FloatArray(24)
    val saturation = FloatArray(24)
    val hue = FloatArray(24)
    val hsv = FloatArray(3)
    pixels.forEach { pixel ->
        if (android.graphics.Color.alpha(pixel) > 128) {
            android.graphics.Color.colorToHSV(pixel, hsv)
            // Ignore white lettering and black outlines, favour substantial coloured regions.
            if (hsv[1] >= .12f && hsv[2] >= .20f) {
                val bucket = (hsv[0] / 15f).toInt().coerceIn(0, 23)
                val weight = hsv[1] * hsv[2]
                weights[bucket] += weight
                saturation[bucket] += hsv[1] * weight
                hue[bucket] += hsv[0] * weight
            }
        }
    }
    val dominant = weights.indices.maxBy { weights[it] }
    if (weights[dominant] == 0f) return Color(0xFF494649)
    // A readable mid-dark shade with visible colour, shared by the full page and both fades.
    hsv[0] = hue[dominant] / weights[dominant]
    hsv[1] = (saturation[dominant] / weights[dominant]).coerceIn(.32f, .62f)
    hsv[2] = .48f
    return Color(android.graphics.Color.HSVToColor(hsv))
}
