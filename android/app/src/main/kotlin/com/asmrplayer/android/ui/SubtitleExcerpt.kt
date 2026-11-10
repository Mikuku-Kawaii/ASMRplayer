package com.asmrplayer.android.ui

import androidx.compose.animation.*
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.subtitles.BilingualSubtitles
import com.asmrplayer.android.subtitles.SubtitleTimeline

@Composable
internal fun SubtitleExcerpt(subtitles: BilingualSubtitles, position: Long) {
    val original = subtitles.original?.takeIf { it.cues.isNotEmpty() }
    val translation = subtitles.translation?.takeIf { it.cues.isNotEmpty() }
    val offset = subtitles.preference.offsetMs
    val translatedOffset = offset + subtitles.preference.translationOffsetMs
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(2.dp)) {
        ExcerptLine(original ?: translation, position, if (original == null) translatedOffset else offset,
            retain = true, style = MaterialTheme.typography.bodyLarge,
            alignment = if (original != null && translation != null) Alignment.BottomCenter else Alignment.Center)
        if (original != null && translation != null) {
            ExcerptLine(translation, position, translatedOffset, retain = false, style = MaterialTheme.typography.bodyMedium,
                alignment = Alignment.TopCenter)
        }
    }
}

@Composable
private fun ExcerptLine(track: SubtitleTimeline?, position: Long, offset: Long, retain: Boolean, style: TextStyle, alignment: Alignment) {
    val active = track?.active(position, offset).orEmpty()
    val lines = if (active.isEmpty() && retain) track?.display(position, offset).orEmpty() else active
    val value = lines.joinToString("\n") { it.text }
    val colors = MaterialTheme.colorScheme
    val ink = if (retain && active.isNotEmpty()) colors.onSurface else colors.onSurfaceVariant.copy(alpha = if (retain) .72f else .90f)
    val line: @Composable (String) -> Unit = { text ->
        Text(text, Modifier.fillMaxWidth(), maxLines = 2, overflow = TextOverflow.Ellipsis,
            style = style, textAlign = TextAlign.Center, color = ink)
    }
    Box(Modifier.fillMaxWidth(), contentAlignment = alignment) {
        // Reserve two actual font lines, keeping short originals and translations close together.
        Text("", Modifier.clearAndSetSemantics {}, minLines = 2, maxLines = 2, style = style)
        if (LocalUiMotion.current) {
            val lift = with(LocalDensity.current) { 4.dp.roundToPx() }
            AnimatedContent(value, contentAlignment = alignment, transitionSpec = {
                ((fadeIn(tween(180)) + slideInVertically(tween(180)) { lift }) togetherWith fadeOut(tween(90))).using(null)
            }, label = "字幕换句") { line(it) }
        } else line(value)
    }
}
