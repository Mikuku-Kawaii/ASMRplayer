package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.draw.clip
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp

@Composable
internal fun Modifier.workListCard(): Modifier {
    val shape = MaterialTheme.shapes.medium
    val colors = MaterialTheme.colorScheme
    return fillMaxWidth().clip(shape).background(colors.surfaceContainerLow)
        .border(1.dp, colors.outlineVariant.copy(alpha = .7f), shape)
}

@Composable
internal fun WorkCardStatus(badges: List<String>) {
    if (badges.isNotEmpty()) Text(badges.joinToString("·"), Modifier.horizontalScroll(rememberScrollState()),
        style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = 1, softWrap = false)
}

@Composable
internal fun WorkCardActionLine(title: String, inLibrary: Boolean?, save: (() -> Unit)?, saving: Boolean,
    favorite: Boolean?, like: (() -> Unit)?, leading: @Composable () -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = 9.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.weight(1f).padding(end = 9.dp)) { leading() }
        if (save != null) CardToggle(Glyph.Collection, inLibrary == true,
            if (inLibrary == true) "移出音频库 $title" else "加入音频库 $title", save, saving, 15)
        if (like != null) CardToggle(Glyph.Heart, favorite == true,
            if (favorite == true) "取消喜欢 $title" else "喜欢 $title", like, saving, if (save != null) -1 else 15)
    }
}

@Composable
private fun CardToggle(glyph: Glyph, checked: Boolean, label: String, click: () -> Unit, busy: Boolean, insetDp: Int) {
    MotionIconToggleButton(checked, { click() }, enabled = !busy, modifier = Modifier.size(48.dp).semantics {
        contentDescription = label
    }) {
        // Bring the artwork closer without shrinking or overlapping the two touch targets.
        val icon = Modifier.offset(x = insetDp.dp).size(18.dp)
        if (busy) CircularProgressIndicator(icon, strokeWidth = 2.dp)
        else AppGlyph(glyph, icon, filled = checked,
            tint = if (checked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
