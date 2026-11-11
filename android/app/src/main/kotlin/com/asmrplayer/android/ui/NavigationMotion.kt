@file:OptIn(androidx.compose.animation.ExperimentalSharedTransitionApi::class)

package com.asmrplayer.android.ui

import androidx.compose.animation.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.zIndex
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.*
import kotlin.math.roundToInt

internal val MotionEase = CubicBezierEasing(.2f, 0f, 0f, 1f)
internal val LocalRouteActive = compositionLocalOf { true }
internal val LocalSharedArtworkEnabled = compositionLocalOf { true }
private val LocalSharedScope = compositionLocalOf<SharedTransitionScope?> { null }
private val LocalPageScope = compositionLocalOf<AnimatedVisibilityScope?> { null }

internal data class MotionDestination(val route: String?, val depth: Int)

/** Keep both pages alive until the transition settles; only the destination accepts input. */
@Composable
internal fun RouteMotionHost(destination: MotionDestination, modifier: Modifier = Modifier,
    content: @Composable (MotionDestination) -> Unit) {
    val motion = LocalUiMotion.current
    val direction = if (LocalLayoutDirection.current == LayoutDirection.Rtl) -1 else 1
    val distance = with(LocalDensity.current) { 32.dp.roundToPx() }
    val pages = remember { mutableStateListOf(destination) }
    var previousDepth by remember { mutableIntStateOf(destination.depth) }
    val sign = remember(destination) { if (destination.depth >= previousDepth) direction else -direction }
    SideEffect {
        previousDepth = destination.depth
        if (pages.none { it.route == destination.route }) pages.add(destination)
    }
    SharedTransitionLayout(modifier) {
        val shared = this
        Box(Modifier.fillMaxSize()) {
            // Explicit visibility owners make A → B → A → B reversible even before B settles.
            // A page is released only once its own exit has finished, never by a delayed callback.
            (pages.toList() + destination).distinctBy { it.route }.forEach { page ->
                key(page.route ?: "home") {
                    val active = page.route == destination.route
                    val visibility = remember { MutableTransitionState(active && pages.size == 1 && pages[0].route == page.route) }
                    visibility.targetState = active
                    LaunchedEffect(visibility.isIdle, visibility.currentState, active) {
                        if (visibility.isIdle && !visibility.currentState && !active) pages.removeAll { it.route == page.route }
                    }
                    AnimatedVisibility(visibility, Modifier.fillMaxSize().zIndex(if (active) 1f else 0f),
                        enter = if (motion) slideInHorizontally(tween(280, easing = MotionEase)) { distance * sign } + fadeIn(tween(220)) else EnterTransition.None,
                        exit = if (motion) slideOutHorizontally(tween(280, easing = MotionEase)) { -distance * sign / 2 } + fadeOut(tween(180)) else ExitTransition.None) {
                        CompositionLocalProvider(LocalSharedScope provides shared, LocalPageScope provides this,
                            LocalRouteActive provides active) {
                            Box(Modifier.fillMaxSize().inactivePage(!active), contentAlignment = Alignment.TopCenter) { content(page) }
                        }
                    }
                }
            }
        }
    }
}

/** No invisible/outgoing controls in accessibility or hit testing during a reversed transition. */
internal fun Modifier.inactivePage(inactive: Boolean): Modifier = if (!inactive) this else
    clearAndSetSemantics { }.pointerInput(Unit) {
        awaitPointerEventScope { while (true) awaitPointerEvent(PointerEventPass.Initial).changes.forEach { it.consume() } }
    }

/** Freeze the departing detail view while its model is released or reused by the destination. */
@Composable
internal fun <T> routeSnapshot(value: T): T {
    val active = LocalRouteActive.current
    var last by remember { mutableStateOf(value) }
    SideEffect { if (active) last = value }
    return if (active) value else last
}

@Composable
internal fun Modifier.sharedWorkArtwork(workId: String?, hero: Boolean = false): Modifier {
    val shared = LocalSharedScope.current
    val page = LocalPageScope.current
    if (workId == null || shared == null || page == null || !LocalUiMotion.current || !LocalSharedArtworkEnabled.current) return this
    val radius = page.transition.animateDp(transitionSpec = { tween(300, easing = MotionEase) }, label = "cover corners") {
        if ((it == EnterExitState.Visible) == hero) 18.dp else 16.dp
    }
    return with(shared) {
        sharedBounds(rememberSharedContentState("work-cover:$workId"), page,
            boundsTransform = { _, _ -> tween(300, easing = MotionEase) },
            enter = fadeIn(tween(180)), exit = fadeOut(tween(180)),
            clipInOverlayDuringTransition = OverlayClip(RoundedCornerShape(radius.value)))
    }
}

/** The mini player is always the first child: navigation changes never recreate its host.
 * Measure the controls at full size, animate only the space and placement around them. */
@Composable
internal fun PlayerDockLayout(navigationVisible: Boolean, horizontal: Boolean, modifier: Modifier = Modifier,
    player: @Composable () -> Unit, navigation: @Composable () -> Unit) {
    val motion = LocalUiMotion.current
    val fraction = animateFloatAsState(if (navigationVisible) 1f else 0f,
        if (motion) tween(280, easing = MotionEase) else snap(), label = "dock navigation space")
    Layout(modifier = modifier.clipToBounds(), content = {
        Box { player() }
        Box(Modifier.inactivePage(!navigationVisible)) { navigation() }
    }) { children, constraints ->
        val progress = fraction.value
        val width = constraints.maxWidth
        val gap = (if (horizontal) 16.dp else 12.dp).roundToPx()
        val navWidth = if (horizontal) ((width - gap) * .58f).roundToInt() else width
        val playerWidth = if (horizontal) (width - (navWidth + gap) * progress).roundToInt() else width
        val mini = children[0].measure(constraints.copy(minWidth = playerWidth, maxWidth = playerWidth, minHeight = 0))
        val nav = children[1].measure(constraints.copy(minWidth = navWidth, maxWidth = navWidth, minHeight = 0))
        val height = if (horizontal) maxOf(mini.height, nav.height) else mini.height + ((nav.height + gap) * progress).roundToInt()
        layout(width, height) {
            mini.placeRelative(0, height - mini.height - if (horizontal) 0 else ((nav.height + gap) * progress).roundToInt())
            if (progress > 0f) nav.placeRelativeWithLayer(if (horizontal) playerWidth + gap else 0,
                if (horizontal) height - nav.height else mini.height + gap) { alpha = progress }
        }
    }
}
