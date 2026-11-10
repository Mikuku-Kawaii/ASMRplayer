package com.asmrplayer.android.ui

import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp

/** Official Lucide geometry; state changes keep the same centered 24-unit canvas.
 * Callers own the accessible name and >=48dp touch target.
 */
@Composable
fun AppGlyph(kind: Glyph, modifier: Modifier = Modifier, tint: Color = LocalContentColor.current, filled: Boolean = false) {
    val color = tint
    Crossfade(kind to filled, modifier.size(24.dp),
        animationSpec = if (LocalUiMotion.current) tween(100) else snap(), label = "control icon") { (glyph, selected) ->
        Image(painterResource(glyph.lucideResource(selected)), contentDescription = null,
            modifier = Modifier.fillMaxSize(), contentScale = ContentScale.Fit,
            colorFilter = ColorFilter.tint(color))
    }
}
