package com.asmrplayer.android.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/** Keep all three actions in one row; longer labels grow inside their own button. */
@Composable
internal fun WorkDetailActionRow(
    resumeTime: String?, collected: Boolean, liked: Boolean,
    playEnabled: Boolean, actionsEnabled: Boolean,
    onPlay: () -> Unit, onCollect: () -> Unit, onLike: () -> Unit,
) {
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val style = MaterialTheme.typography.labelLarge
    fun textWidth(text: String) = with(density) {
        measurer.measure(text, style, softWrap = false).size.width.toDp()
    }
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        // Reserve the selected labels too, so toggling a state never shifts the row.
        // Leave a few pixels beyond the measured label for density/font rounding on devices.
        val actionWidth = (maxOf(textWidth("已收藏"), textWidth("已喜欢")) + 48.dp)
            .coerceAtMost(maxWidth * .29f)
        val playWidth = maxWidth - actionWidth * 2 - 12.dp
        val playLabel = if (resumeTime == null) "播放当前目录" else "继续收听"
        val inlineLabel = resumeTime?.let { "$playLabel · $it" } ?: playLabel
        val inlineTime = textWidth(inlineLabel) + 48.dp <= playWidth
        val collectLabel = if (collected) "已收藏" else "收藏"
        val likeLabel = if (liked) "已喜欢" else "喜欢"
        Row(Modifier.fillMaxWidth().height(IntrinsicSize.Max), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            DetailHeroButton(
                onPlay, Modifier.weight(1f).fillMaxHeight(), playEnabled, Glyph.Play,
                if (inlineTime) inlineLabel else playLabel,
                primary = true,
                stacked = textWidth(playLabel) + 48.dp > playWidth,
                subtitle = resumeTime.takeUnless { inlineTime },
            )
            DetailHeroButton(
                onCollect, Modifier.width(actionWidth).fillMaxHeight().semantics {
                    contentDescription = if (collected) "已收藏，点击取消收藏，保留文件和收听记录" else "收藏，加入音频库"
                    selected = collected
                }, actionsEnabled, Glyph.Collection, collectLabel, active = collected,
                stacked = textWidth("已收藏") + 48.dp > actionWidth,
            )
            DetailHeroButton(
                onLike, Modifier.width(actionWidth).fillMaxHeight().semantics {
                    contentDescription = if (liked) "已喜欢，取消作品喜欢" else "喜欢作品"
                    selected = liked
                }, actionsEnabled, Glyph.Heart, likeLabel, active = liked,
                stacked = textWidth("已喜欢") + 48.dp > actionWidth,
            )
        }
    }
}

@Composable
private fun DetailHeroButton(
    onClick: () -> Unit, modifier: Modifier, enabled: Boolean, glyph: Glyph, label: String,
    primary: Boolean = false, active: Boolean = false, stacked: Boolean = false, subtitle: String? = null,
) {
    val interaction = remember { MutableInteractionSource() }
    val colors = MaterialTheme.colorScheme
    val dark = colors.surface.luminance() < .5f
    val lightTone = if (dark) colors.onSurface else colors.surface
    val playContainer = lerp(colors.primary, lightTone, if (dark) .24f else .55f)
    val selectedContainer = lerp(colors.primaryContainer, lightTone, .20f)
    Button(
        onClick = onClick,
        modifier = modifier.heightIn(min = 48.dp).pressMotion(interaction),
        enabled = enabled,
        interactionSource = interaction,
        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 8.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = motionColor(when {
                primary -> playContainer
                active -> selectedContainer
                else -> colors.surfaceContainerLow
            }),
            contentColor = motionColor(when {
                primary -> if (dark) colors.onPrimary else colors.onSurface
                active -> colors.onPrimaryContainer
                else -> colors.onSurfaceVariant
            }),
        ),
        border = BorderStroke(1.dp, motionColor(if (!primary && !active) colors.outlineVariant else Color.Transparent)),
    ) {
        val text: @Composable () -> Unit = {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(label, style = MaterialTheme.typography.labelLarge, textAlign = TextAlign.Center,
                    softWrap = stacked, maxLines = if (stacked) Int.MAX_VALUE else 1)
                subtitle?.let { Text(it, style = MaterialTheme.typography.labelSmall, textAlign = TextAlign.Center) }
            }
        }
        if (stacked) {
            Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                AppGlyph(glyph, Modifier.size(20.dp), filled = active)
                text()
            }
        } else {
            AppGlyph(glyph, Modifier.size(20.dp), filled = active)
            Spacer(Modifier.width(4.dp))
            text()
        }
    }
}
