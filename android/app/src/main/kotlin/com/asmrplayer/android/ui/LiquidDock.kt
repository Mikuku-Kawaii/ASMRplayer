package com.asmrplayer.android.ui

import android.animation.ValueAnimator
import android.os.Build
import androidx.compose.animation.core.*
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.*
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*
import androidx.compose.foundation.pager.PagerState
import kotlin.math.abs
import kotlin.math.roundToInt

@Composable
internal fun dockMotionEnabled(lowEffects: Boolean) = LocalUiMotion.current && !lowEffects && (Build.VERSION.SDK_INT<26 || ValueAnimator.areAnimatorsEnabled())

@Composable
internal fun Modifier.dockPress(source: MutableInteractionSource, lowEffects: Boolean): Modifier {
    val pressed by source.collectIsPressedAsState()
    val enabled=dockMotionEnabled(lowEffects)
    val scale by animateFloatAsState(if(pressed&&enabled) .98f else 1f,
        if(enabled) tween(if (pressed) 70 else 110) else snap(),label="dock pressure")
    return graphicsLayer { scaleX=scale; scaleY=scale }
}

@Composable
internal fun DockButton(glyph: Glyph, label: String, lowEffects: Boolean, enabled: Boolean=true, action: ()->Unit) {
    val interaction=remember { MutableInteractionSource() }
    IconButton(onClick=action,enabled=enabled,interactionSource=interaction,
        modifier=Modifier.size(48.dp).dockPress(interaction,lowEffects).semantics { contentDescription=label }) {
        AppGlyph(glyph,Modifier.size(24.dp))
    }
}

/** The lens reads the same continuous position as the pages, including during a reversed drag. */
@Composable
internal fun LiquidNavigation(source: GlassBackdrop, lowEffects: Boolean, pager: PagerState,
    choose: (Destination) -> Unit, modifier: Modifier) {
    val items = Destination.entries
    val selected = items[pager.settledPage]
    fun position() = (pager.currentPage + pager.currentPageOffsetFraction).coerceIn(0f, items.lastIndex.toFloat())
    val accent=MaterialTheme.colorScheme.primary
    BoxWithConstraints(modifier,propagateMinConstraints=true) {
        // Four mobile tabs use vertical labels; wide layouts can fit icon and label side by side.
        val horizontal=maxWidth/items.size >= 104.dp * androidx.compose.ui.platform.LocalDensity.current.fontScale
        Layout(modifier=Modifier.fillMaxWidth().clip(RoundedCornerShape(50)).selectableGroup(),content={
            GlassSurface(source,lowEffects,Modifier.fillMaxSize()) {}
            GlassSurface(source,lowEffects,Modifier.testTag("home-lens").shadow(if(lowEffects) 0.dp else 6.dp,RoundedCornerShape(50),clip=false,
                ambientColor=accent.copy(alpha=.12f),spotColor=accent.copy(alpha=.10f)),active=true) {}
            items.forEach { item ->
                val interaction=remember(item) { MutableInteractionSource() }
                val inactive = MaterialTheme.colorScheme.onSurfaceVariant
                fun weight() = (1f - abs(position() - item.ordinal)).coerceIn(0f, 1f)
                Box(Modifier.heightIn(min=48.dp).selectable(item==selected,role=Role.Tab,interactionSource=interaction,
                    indication=null,onClick={choose(item)}).dockPress(interaction,lowEffects).padding(horizontal=4.dp,vertical=6.dp),
                    contentAlignment=Alignment.Center) {
                    val icon: @Composable () -> Unit = {
                        val painter = painterResource(item.glyph.lucideResource(false))
                        Canvas(Modifier.size(22.dp).graphicsLayer {
                            scaleX = 1f + .12f * weight()
                            scaleY = scaleX
                        }) {
                            with(painter) { draw(size, colorFilter = ColorFilter.tint(lerp(inactive, accent, weight()))) }
                        }
                    }
                    val label: @Composable () -> Unit = {
                        BasicText(item.title, maxLines = 1,
                            style = LocalTextStyle.current.copy(fontSize = 12.sp, lineHeight = 16.sp,
                                fontWeight = if (item == selected) FontWeight.SemiBold else FontWeight.Medium),
                            color = { lerp(inactive, accent, weight()) })
                    }
                    if(horizontal) Row(verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) { icon(); label() }
                    else Column(horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.spacedBy(4.dp)) { icon(); label() }
                }
            }
        }) { measurables,constraints ->
            val width=constraints.maxWidth
            val edges=IntArray(items.size+1) { width*it/items.size }
            val inset=4.dp.roundToPx()
            val entries=measurables.drop(2).mapIndexed { i,m ->
                m.measure(Constraints(minWidth=edges[i+1]-edges[i],maxWidth=edges[i+1]-edges[i],
                    maxHeight=(constraints.maxHeight-inset*2).coerceAtLeast(0)))
            }
            val height=constraints.constrainHeight(entries.maxOf { it.height }+inset*2)
            val background=measurables[0].measure(Constraints.fixed(width,height))
            val target=selected.ordinal
            val lens=measurables[1].measure(Constraints.fixed(edges[target+1]-edges[target],height))
            layout(width,height) {
                background.placeRelative(0,0)
                val position=position()
                val slot=position.toInt()
                lens.placeRelative((edges[slot]+(edges[slot+1]-edges[slot])*(position-slot)).roundToInt(),0)
                entries.forEachIndexed { i,p -> p.placeRelative(edges[i],(height-p.height)/2) }
            }
        }
    }
}
