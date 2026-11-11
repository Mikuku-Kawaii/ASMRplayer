package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.background
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

internal object PageSpace {
    val edge = 20.dp
    val gap = 12.dp
    val padding = PaddingValues(horizontal = edge, vertical = 8.dp)
}

/** A single-line field, shared by library, discovery, history and settings. */
@Composable
internal fun PageSearch(value: String, change: (String) -> Unit, placeholder: String,
    modifier: Modifier = Modifier, search: (() -> Unit)? = null) {
    val colors = MaterialTheme.colorScheme
    TextField(value, change, modifier.fillMaxWidth(), singleLine = true,
        textStyle=MaterialTheme.typography.bodyMedium,
        placeholder = { Text(placeholder, maxLines = 1, overflow = TextOverflow.Ellipsis,style=MaterialTheme.typography.bodyMedium) },
        leadingIcon = { if (search == null) AppGlyph(Glyph.Search) else GlyphButton(Glyph.Search, "提交搜索", search) },
        trailingIcon = if (value.isNotEmpty()) ({ GlyphButton(Glyph.Close, "清空搜索", { change("") }) }) else null,
        shape = RoundedCornerShape(14.dp),
        colors = TextFieldDefaults.colors(focusedContainerColor = colors.surfaceContainerLow,
            unfocusedContainerColor = colors.surfaceContainerLow,
            focusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
            unfocusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = { search?.invoke() }))
}

/** A compact home search field with a full-height input and touch target. */
@Composable
internal fun HomeSearch(value: String, change: (String) -> Unit, hint: String, modifier: Modifier = Modifier,
                        submit: (() -> Unit)? = null) {
    val colors = MaterialTheme.colorScheme
    val keyboard = LocalSoftwareKeyboardController.current
    val style = MaterialTheme.typography.bodyMedium
    BasicTextField(value, change, modifier.fillMaxWidth().heightIn(min = 48.dp)
        .background(colors.surfaceContainerLow, RoundedCornerShape(14.dp)).semantics { contentDescription = hint },
        singleLine = true, textStyle = style.copy(color = colors.onSurface), cursorBrush = SolidColor(colors.primary),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = { submit?.invoke(); keyboard?.hide() }),
        decorationBox = { field ->
            Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = if (submit == null) 14.dp else 0.dp, end = 6.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (submit == null) AppGlyph(Glyph.Search, Modifier.size(20.dp), colors.onSurfaceVariant)
                else GlyphButton(Glyph.Search, "提交搜索", { submit(); keyboard?.hide() })
                Box(Modifier.weight(1f)) {
                    if (value.isEmpty()) Text(hint, style = style, color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    field()
                }
                if (value.isNotEmpty()) GlyphButton(Glyph.Close, "清空搜索", { change(""); submit?.invoke() })
            }
        })
}

@Composable
internal fun PageIntro(title: String, description: String? = null, glyph: Glyph? = null) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically) {
        if (glyph != null) Surface(shape = RoundedCornerShape(10.dp), color = MaterialTheme.colorScheme.primaryContainer) {
            Box(Modifier.size(32.dp), contentAlignment = Alignment.Center) { AppGlyph(glyph, tint = MaterialTheme.colorScheme.onPrimaryContainer) }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            description?.let { Text(it, maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}


/** All home pages share title baseline, edge spacing and action height. */
@Composable
internal fun HomeHeader(content: @Composable RowScope.() -> Unit) {
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).testTag("home-header").padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically, content = content)
}
