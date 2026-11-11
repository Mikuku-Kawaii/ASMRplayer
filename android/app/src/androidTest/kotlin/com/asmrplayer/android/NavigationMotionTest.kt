package com.asmrplayer.android

import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.asmrplayer.android.ui.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NavigationMotionTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun dockMovesContinuouslyAndReversesWithoutRecreatingThePlayer() {
        var home by mutableStateOf(true)
        var mounted = 0
        var disposed = 0
        ui.runOnUiThread { ui.activity.setContent {
            Box(Modifier.size(340.dp, 600.dp)) {
                PlayerDockLayout(home, false, Modifier.align(Alignment.BottomCenter), player = {
                    DisposableEffect(Unit) { mounted++; onDispose { disposed++ } }
                    Box(Modifier.fillMaxWidth().height(100.dp).testTag("persistent-mini"))
                }, navigation = { Box(Modifier.fillMaxWidth().height(64.dp)) })
            }
        } }
        fun top() = ui.onNodeWithTag("persistent-mini").fetchSemanticsNode().positionInRoot.y
        val start = top()
        ui.mainClock.autoAdvance = false
        try {
            ui.runOnUiThread { home = false }
            ui.mainClock.advanceTimeBy(96)
            val middle = top()
            assertTrue("Mini player jumped or stayed still", middle > start)
            ui.runOnUiThread { home = true }
            ui.mainClock.advanceTimeBy(80)
            val reversed = top()
            assertTrue("Reverse jumped back to the endpoint", reversed > start)
            assertTrue("Reverse continued in the wrong direction: $middle to $reversed", reversed <= middle + 1)
            ui.mainClock.advanceTimeBy(400)
            assertEquals(start, top(), 1f)
            ui.runOnUiThread { home = false }
            ui.mainClock.advanceTimeBy(400)
            assertTrue(top() > middle)
            assertEquals(1, mounted)
            assertEquals(0, disposed)
        } finally { ui.mainClock.autoAdvance = true }
    }

    @Test fun reducedMotionAndHorizontalDockKeepTheSamePlayerAndUsableWidth() {
        var home by mutableStateOf(true)
        ui.runOnUiThread { ui.activity.setContent {
            CompositionLocalProvider(LocalUiMotion provides false) {
                PlayerDockLayout(home, true, Modifier.width(700.dp),
                    player = { Box(Modifier.fillMaxWidth().height(100.dp).testTag("wide-mini")) },
                    navigation = { Box(Modifier.fillMaxWidth().height(64.dp)) })
            }
        } }
        val initial = ui.onNodeWithTag("wide-mini").fetchSemanticsNode().boundsInRoot
        ui.runOnIdle { home = false }
        val expanded = ui.onNodeWithTag("wide-mini").fetchSemanticsNode().boundsInRoot
        assertTrue(expanded.width > initial.width * 2)
        assertEquals(initial.top, expanded.top, 1f)
        ui.runOnIdle { home = true }
        assertEquals(initial.width, ui.onNodeWithTag("wide-mini").fetchSemanticsNode().boundsInRoot.width, 1f)
    }

    @Test fun artworkTravelsThroughIntermediateBoundsAndOutgoingPageIsInaccessible() {
        var detail by mutableStateOf(false)
        ui.runOnUiThread { ui.activity.setContent {
            MaterialTheme {
                RouteMotionHost(MotionDestination(if (detail) "detail" else null, if (detail) 1 else 0),
                    Modifier.size(320.dp, 500.dp).testTag("transition-root")) { page ->
                    val target = page.route != null
                    Box(Modifier.fillMaxSize()) {
                        Box(Modifier.padding(start = if (target) 150.dp else 20.dp, top = if (target) 30.dp else 130.dp)
                            .sharedWorkArtwork("fixture").size(if (target) 80.dp else 140.dp).background(Color.Red))
                        Text(if (target) "详情" else "首页", Modifier.align(Alignment.BottomCenter))
                    }
                }
            }
        } }
        fun redX(): Float {
            val image = ui.onNodeWithTag("transition-root").captureToImage().toPixelMap()
            var total = 0L; var count = 0
            for (y in 0 until image.height step 2) for (x in 0 until image.width step 2) {
                val p = image[x, y]
                if (p.red > .65f && p.green < .25f && p.blue < .25f) { total += x; count++ }
            }
            assertTrue("Shared cover vanished", count > 50)
            return total.toFloat() / count
        }
        val start = redX()
        ui.mainClock.autoAdvance = false
        try {
            ui.runOnUiThread { detail = true }
            ui.mainClock.advanceTimeBy(96)
            ui.onNodeWithText("首页").assertDoesNotExist()
            val intermediate = redX()
            ui.mainClock.advanceTimeBy(400)
            val end = redX()
            assertTrue("No continuous artwork movement: $start / $intermediate / $end", intermediate > start && intermediate < end)
            ui.runOnUiThread { detail = false }
            ui.mainClock.advanceTimeBy(80)
            ui.runOnUiThread { detail = true }
            ui.mainClock.advanceTimeBy(400)
            assertEquals(end, redX(), 2f)
            ui.onNodeWithText("详情").assertIsDisplayed()
        } finally { ui.mainClock.autoAdvance = true }
    }
}
