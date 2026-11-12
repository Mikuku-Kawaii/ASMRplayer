package com.asmrplayer.android.ui

import android.os.Build
import androidx.compose.material3.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.unit.sp
import com.asmrplayer.android.R

internal const val DEFAULT_APPEARANCE_MODE = "light"
internal const val DEFAULT_COLOR_PRESET = "wisteria"

/** Surface colors are authored for each mode so large areas stay softly tinted. */
internal enum class ColorPreset(
    val key: String, val label: String,
    val background: Color, val panel: Color, val lightBackground: Color, val lightPanel: Color,
    val lightField: Color, val accent: Color, val ink: Color, val muted: Color, val lightAccent: Color
) {
    InkBlue("ink_blue", "墨蓝", Color(0xFF252C36), Color(0xFF303945), Color(0xFFE7EBF0), Color(0xFFF1F5FA),
        Color(0xFFDEE4EB), Color(0xFFB4D5FD), Color(0xFFDEE3EB), Color(0xFFAAB3C2), Color(0xFF28558A)),
    Wisteria(DEFAULT_COLOR_PRESET, "藤紫", Color(0xFF282532), Color(0xFF34303E), Color(0xFFEEE8F7), Color(0xFFF7F1FA),
        Color(0xFFE6DDF1), Color(0xFFB5A0FF), Color(0xFFF0EDFF), Color(0xFFBBB5D1), Color(0xFF6947B5)),
    Berry("berry", "莓粉", Color(0xFF29232C), Color(0xFF322A35), Color(0xFFEEE8EC), Color(0xFFF8F0F5),
        Color(0xFFE5DDE3), Color(0xFFF18BB5), Color(0xFFF7EDF4), Color(0xFFCBB7C6), Color(0xFF9A3965)),
    Amber("amber", "琥珀烛光", Color(0xFF2C2723), Color(0xFF38312A), Color(0xFFEEE9E3), Color(0xFFF8F1E8),
        Color(0xFFE5DFD7), Color(0xFFE5B887), Color(0xFFF5ECE3), Color(0xFFB8ABA0), Color(0xFF825321));

    companion object { fun from(key: String) = entries.firstOrNull { it.key == key } ?: Wisteria }
}

internal fun playerColors(dark: Boolean, preset: String = DEFAULT_COLOR_PRESET): ColorScheme {
    val p = ColorPreset.from(preset)
    val base = wisteriaColors(dark)
    val page = if (dark) p.background else p.lightBackground
    val panel = if (dark) p.panel else p.lightPanel
    val ink = if (dark) p.ink else p.panel
    val muted = if (dark) p.muted else lerp(ink, p.lightAccent, .20f)
    val accent = if (dark) p.accent else p.lightAccent
    val selected = lerp(panel, accent, if (dark) .20f else .24f)
    val raised = if (dark) lerp(panel, p.ink, .055f) else p.lightField
    val colors = if (p == ColorPreset.Wisteria) base else base.copy(
        primary = accent, onPrimary = if (dark) page else Color.White,
        primaryContainer = selected, onPrimaryContainer = if (dark) p.ink else p.panel,
        secondary = muted, onSecondary = if (dark) page else Color.White,
        secondaryContainer = if (dark) panel else lerp(panel, accent, .14f), onSecondaryContainer = ink,
        tertiary = accent, onTertiary = if (dark) page else Color.White,
        tertiaryContainer = selected, onTertiaryContainer = ink,
        inverseSurface = ink, inverseOnSurface = page,
        inversePrimary = if (dark) p.lightAccent else p.accent,
        surfaceTint = accent
    )
    return colors.copy(
        background = page, onBackground = ink, surface = if (dark) page else panel, onSurface = ink,
        surfaceVariant = if (dark) panel else p.lightField, onSurfaceVariant = muted,
        surfaceContainerLowest = if (dark) lerp(page, Color.Black, .08f) else Color.White,
        surfaceContainerLow = if (dark) lerp(page, panel, .65f) else panel,
        surfaceContainer = if (dark) panel else lerp(panel, page, .36f), surfaceContainerHigh = raised,
        surfaceContainerHighest = if (dark) lerp(panel, p.ink, .11f) else lerp(p.lightField, accent, .14f),
        surfaceDim = if (dark) page else p.lightField, surfaceBright = if (dark) raised else panel,
        outline = if (dark) lerp(page, muted, .85f) else lerp(page, ink, .62f),
        outlineVariant = if (dark) lerp(panel, muted, .17f) else lerp(page, ink, .30f)
    )
}

private fun wisteriaColors(dark: Boolean): ColorScheme = if (dark) darkColorScheme(
    primary = Color(0xFFB5A0FF), onPrimary = Color(0xFF241747),
    primaryContainer = Color(0xFF3C3158), onPrimaryContainer = Color(0xFFE9DFFF),
    secondary = Color(0xFFCBC0E4), onSecondary = Color(0xFF302640),
    secondaryContainer = Color(0xFF393147), onSecondaryContainer = Color(0xFFE9E0F8),
    tertiary = Color(0xFFE3B6C6), onTertiary = Color(0xFF442735),
    tertiaryContainer = Color(0xFF563646), onTertiaryContainer = Color(0xFFFFD9E7),
    background = Color(0xFF161522), onBackground = Color(0xFFF0EDFF),
    surface = Color(0xFF161522), onSurface = Color(0xFFF0EDFF),
    surfaceVariant = Color(0xFF302C43), onSurfaceVariant = Color(0xFFBBB5D1),
    surfaceContainerLowest = Color(0xFF12111C), surfaceContainerLow = Color(0xFF201D30),
    surfaceContainer = Color(0xFF252238), surfaceContainerHigh = Color(0xFF312D44),
    surfaceContainerHighest = Color(0xFF3B364F), surfaceDim = Color(0xFF161522), surfaceBright = Color(0xFF3B364F),
    outline = Color(0xFF9189A7), outlineVariant = Color(0xFF494159),
    inverseSurface = Color(0xFFF0EDFF), inverseOnSurface = Color(0xFF292339), inversePrimary = Color(0xFF6947B5),
    surfaceTint = Color(0xFFB5A0FF), error = Color(0xFFFFB4AB), onError = Color(0xFF690005),
    errorContainer = Color(0xFF7D2929), onErrorContainer = Color(0xFFFFDAD6), scrim = Color.Black,
) else lightColorScheme(
    primary = Color(0xFF6947B5), onPrimary = Color.White,
    primaryContainer = Color(0xFFDFCEF8), onPrimaryContainer = Color(0xFF38215F),
    secondary = Color(0xFF625673), onSecondary = Color.White,
    secondaryContainer = Color(0xFFE9DFF4), onSecondaryContainer = Color(0xFF3F344E),
    tertiary = Color(0xFF825063), onTertiary = Color.White,
    tertiaryContainer = Color(0xFFFFD9E7), onTertiaryContainer = Color(0xFF63394A),
    background = Color(0xFFF8F7FC), onBackground = Color(0xFF272333),
    surface = Color(0xFFFFFBFF), onSurface = Color(0xFF272333),
    surfaceVariant = Color(0xFFECE8F2), onSurfaceVariant = Color(0xFF655E73),
    surfaceContainerLowest = Color.White, surfaceContainerLow = Color(0xFFF2EFF8),
    surfaceContainer = Color(0xFFEDE9F4), surfaceContainerHigh = Color(0xFFE6E1EF),
    surfaceContainerHighest = Color(0xFFDFD9E9), surfaceDim = Color(0xFFDFD9E9), surfaceBright = Color(0xFFFFFBFF),
    outline = Color(0xFF81778F), outlineVariant = Color(0xFFD6CFDF),
    inverseSurface = Color(0xFF302A3E), inverseOnSurface = Color(0xFFF4EFFA), inversePrimary = Color(0xFFD0BCFF),
    surfaceTint = Color(0xFF6947B5), error = Color(0xFFBA1A1A), onError = Color.White,
    errorContainer = Color(0xFFFFDAD6), onErrorContainer = Color(0xFF7D2929), scrim = Color.Black,
)

@OptIn(ExperimentalTextApi::class)
internal val PlayerFontFamily = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) FontFamily(
    listOf(400, 500, 600, 700).map { weight -> Font(R.font.noto_sans_sc,
        weight = FontWeight(weight), variationSettings = FontVariation.Settings(FontVariation.weight(weight))) }
) else FontFamily.SansSerif

private fun type(size: Int, height: Int, weight: FontWeight = FontWeight.Medium) =
    TextStyle(fontFamily = PlayerFontFamily, fontSize = size.sp, lineHeight = height.sp, fontWeight = weight,
        letterSpacing = 0.sp, fontFeatureSettings = "tnum",
        platformStyle = PlatformTextStyle(includeFontPadding = false),
        lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.None))

internal val PlayerTypography = Typography(
    displayLarge = type(36, 44, FontWeight.SemiBold), displayMedium = type(30, 38, FontWeight.SemiBold),
    displaySmall = type(26, 34, FontWeight.SemiBold), headlineLarge = type(24, 32, FontWeight.SemiBold),
    headlineMedium = type(22, 30, FontWeight.SemiBold), headlineSmall = type(20, 28, FontWeight.SemiBold),
    titleLarge = type(18, 26, FontWeight.SemiBold), titleMedium = type(15, 22, FontWeight.SemiBold),
    titleSmall = type(14, 20, FontWeight.SemiBold), bodyLarge = type(15, 23), bodyMedium = type(13, 20),
    bodySmall = type(12, 18), labelLarge = type(13, 18),
    labelMedium = type(12, 16), labelSmall = type(11, 15),
)
