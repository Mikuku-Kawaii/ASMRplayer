package com.asmrplayer.android.ui

import android.database.ContentObserver
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.compose.animation.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp

internal val LocalUiMotion = compositionLocalOf { true }

/** Also reacts when the system animation setting changes while this screen is open. */
@Composable
internal fun rememberUiMotion(lowEffects: Boolean): Boolean {
    val resolver = LocalContext.current.contentResolver
    fun systemEnabled() = Settings.Global.getFloat(resolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) > 0f
    var system by remember(resolver) { mutableStateOf(systemEnabled()) }
    DisposableEffect(resolver) {
        val observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
            override fun onChange(selfChange: Boolean) { system = systemEnabled() }
        }
        resolver.registerContentObserver(Settings.Global.getUriFor(Settings.Global.ANIMATOR_DURATION_SCALE), false, observer)
        system = systemEnabled()
        onDispose { resolver.unregisterContentObserver(observer) }
    }
    return !lowEffects && system
}

@Composable
internal fun MotionReveal(visible: Boolean, content: @Composable () -> Unit) {
    if (!LocalUiMotion.current) { if (visible) content() }
    else AnimatedVisibility(visible,
        enter = expandVertically(tween(240, easing = MotionEase), expandFrom = androidx.compose.ui.Alignment.Top) + fadeIn(tween(180)),
        exit = shrinkVertically(tween(240, easing = MotionEase), shrinkTowards = androidx.compose.ui.Alignment.Top) + fadeOut(tween(180))) { content() }
}

@Composable
internal fun motionColor(target: Color): Color = animateColorAsState(target,
    if (LocalUiMotion.current) tween(180) else snap(), label = "state color").value

/** Animate the semantic palette together, including contrast pairs, when choosing a preset. */
@Composable
internal fun motionColors(target: ColorScheme, enabled: Boolean): ColorScheme {
    @Composable fun color(value: Color) = animateColorAsState(value,
        if (enabled) tween(220) else snap(), label = "theme color").value
    return target.copy(
        primary = color(target.primary), onPrimary = color(target.onPrimary),
        primaryContainer = color(target.primaryContainer), onPrimaryContainer = color(target.onPrimaryContainer),
        inversePrimary = color(target.inversePrimary), secondary = color(target.secondary),
        onSecondary = color(target.onSecondary), secondaryContainer = color(target.secondaryContainer),
        onSecondaryContainer = color(target.onSecondaryContainer), tertiary = color(target.tertiary),
        onTertiary = color(target.onTertiary), tertiaryContainer = color(target.tertiaryContainer),
        onTertiaryContainer = color(target.onTertiaryContainer), background = color(target.background),
        onBackground = color(target.onBackground), surface = color(target.surface), onSurface = color(target.onSurface),
        surfaceVariant = color(target.surfaceVariant), onSurfaceVariant = color(target.onSurfaceVariant),
        surfaceTint = color(target.surfaceTint), inverseSurface = color(target.inverseSurface),
        inverseOnSurface = color(target.inverseOnSurface), error = color(target.error), onError = color(target.onError),
        errorContainer = color(target.errorContainer), onErrorContainer = color(target.onErrorContainer),
        outline = color(target.outline), outlineVariant = color(target.outlineVariant), scrim = color(target.scrim),
        surfaceBright = color(target.surfaceBright), surfaceDim = color(target.surfaceDim),
        surfaceContainer = color(target.surfaceContainer), surfaceContainerLow = color(target.surfaceContainerLow),
        surfaceContainerLowest = color(target.surfaceContainerLowest), surfaceContainerHigh = color(target.surfaceContainerHigh),
        surfaceContainerHighest = color(target.surfaceContainerHighest))
}

@Composable
internal fun Modifier.sizeMotion(): Modifier = if (LocalUiMotion.current)
    animateContentSize(tween(240, easing = MotionEase)) else this

/** Ripple stays native. Only compact buttons compress, never full-width actions or cards. */
@Composable
internal fun Modifier.pressMotion(source: MutableInteractionSource, pressedScale: Float = .98f): Modifier {
    val pressed by source.collectIsPressedAsState()
    val enabled = LocalUiMotion.current
    val limit = with(LocalDensity.current) { 200.dp.toPx() }
    var compact by remember { mutableStateOf(true) }
    val scale by animateFloatAsState(if (pressed && enabled && compact) pressedScale else 1f,
        if (enabled) tween(if (pressed) 70 else 110, easing = LinearOutSlowInEasing) else snap(), label = "button pressure")
    return onSizeChanged { compact = it.width <= limit }
        .graphicsLayer { scaleX = scale; scaleY = scale }
}

@Composable
internal fun MotionButton(onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit) {
    val source = remember { MutableInteractionSource() }
    Button(onClick, modifier.pressMotion(source), enabled, interactionSource = source, content = content)
}

@Composable
internal fun MotionTextButton(onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit) {
    val source = remember { MutableInteractionSource() }
    TextButton(onClick, modifier.pressMotion(source), enabled, interactionSource = source, content = content)
}

@Composable
internal fun MotionOutlinedButton(onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit) {
    val source = remember { MutableInteractionSource() }
    OutlinedButton(onClick, modifier.pressMotion(source), enabled, interactionSource = source, content = content)
}

@Composable
internal fun MotionIconButton(onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
    content: @Composable () -> Unit) {
    val source = remember { MutableInteractionSource() }
    IconButton(onClick, modifier.pressMotion(source,.97f), enabled, interactionSource = source, content = content)
}

@Composable
internal fun MotionFilledIconButton(onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
    content: @Composable () -> Unit) {
    val source = remember { MutableInteractionSource() }
    FilledIconButton(onClick, modifier.pressMotion(source,.97f), enabled, interactionSource = source, content = content)
}

@Composable
internal fun MotionIconToggleButton(checked: Boolean, onCheckedChange: (Boolean) -> Unit, modifier: Modifier = Modifier,
    enabled: Boolean = true, content: @Composable () -> Unit) {
    val source = remember { MutableInteractionSource() }
    IconToggleButton(checked, onCheckedChange, modifier.pressMotion(source,.97f), enabled, interactionSource = source, content = content)
}
