package com.asmrplayer.android

import android.graphics.Bitmap
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.ui.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File
import kotlin.math.abs

@UnstableApi
class MiniPlayerProgressTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun titleStaysAlignedAndNavigationKeepsFourEqualSlots() {
        var position by mutableLongStateOf(0)
        var state by mutableStateOf(fixture())
        var width by mutableStateOf(360.dp)
        var dark by mutableStateOf(true)
        ui.runOnUiThread { ui.activity.setContent {
            MaterialTheme(colorScheme = playerColors(dark), typography = PlayerTypography) {
                val backdrop = rememberGlassBackdrop()
                val pager = rememberPagerState { Destination.entries.size }
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                        Column(Modifier.width(width), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                            HomeHeader {
                                Text("音频库", Modifier.weight(1f))
                            }
                            if (state.queue.isNotEmpty()) MiniPlayerDock(backdrop, state, position, {}, {}, {}, {})
                            LiquidNavigation(backdrop, state.lowEffects, pager, {}, Modifier.fillMaxWidth().testTag("fixture-navigation"))
                        }
                    }
                }
            }
        } }

        val bar = ui.onNodeWithTag("mini-player")
        val lens = ui.onNodeWithTag("home-lens")
        val title = ui.onNode(hasText("音频库") and hasAnyAncestor(hasTestTag("home-header")))
        fun assertHeaderAndTabs() {
            val titleBounds = title.fetchSemanticsNode().boundsInRoot
            val header = ui.onNodeWithTag("home-header").fetchSemanticsNode().boundsInRoot
            val navigation = ui.onNodeWithTag("fixture-navigation").fetchSemanticsNode().boundsInRoot
            val selected = lens.fetchSemanticsNode().boundsInRoot
            assertTrue(abs(titleBounds.left - header.left - with(ui.density) { 16.dp.toPx() }) < 2)
            assertTrue(abs(titleBounds.center.y - header.center.y) < 2)
            assertTrue(header.bottom < navigation.top)
            assertTrue(abs(selected.left - navigation.left) < 2)
            assertTrue(abs(selected.width * 4 - navigation.width) < 4)
        }

        assertHeaderAndTabs()
        val start = title.fetchSemanticsNode().boundsInRoot
        val empty = bar.captureToImage().toPixelMap()
        ui.runOnIdle { position = 50_000 }
        assertEquals(start, title.fetchSemanticsNode().boundsInRoot)
        val half = bar.captureToImage().toPixelMap()
        val x = half.width / 4
        val y = half.height / 10
        assertNotEquals(empty[x, y], half[x, y])
        assertEquals(empty[x * 3, y], half[x * 3, y])
        capture("标题左对齐和四项底栏")
        ui.runOnIdle { position = 150_000 }
        val full = bar.captureToImage().toPixelMap()
        assertNotEquals(half[x * 3, y], full[x * 3, y])

        ui.runOnIdle { width = 280.dp; dark = false; state = state.copy(queue = emptyList()) }
        bar.assertDoesNotExist()
        assertHeaderAndTabs()
        capture("空队列窄屏浅色")
    }

    @Test fun horizontalDragPreviewsAndSeeksOnceWithoutClickingPlaybackControls() {
        var position by mutableLongStateOf(0)
        var state by mutableStateOf(fixture())
        val seeks = mutableListOf<Long>()
        var opens = 0
        var plays = 0
        var skips = 0
        ui.runOnUiThread { ui.activity.setContent {
            MaterialTheme(colorScheme = playerColors(true), typography = PlayerTypography) {
                val backdrop = rememberGlassBackdrop()
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                        MiniPlayerDock(backdrop, state, position, { opens++ }, { plays++ }, { skips++ },
                            { seeks += it; position = it }, Modifier.width(340.dp))
                    }
                }
            }
        } }

        val bar = ui.onNodeWithTag("mini-player")
        fun fraction() = bar.fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo].current
        bar.performTouchInput {
            down(Offset(width * .25f, centerY))
            moveTo(Offset(width * .75f, centerY))
        }
        assertEquals(.75f, fraction(), .02f)
        ui.runOnIdle { assertTrue(seeks.isEmpty()); assertEquals(0, opens) }
        capture("拖动预览")
        bar.performTouchInput { up() }
        ui.runOnIdle {
            assertEquals(1, seeks.size)
            assertTrue(abs(seeks.single() - 75_000) < 2_000)
            assertEquals(0, opens + plays + skips)
        }
        bar.performTouchInput {
            down(Offset(width * .75f, centerY))
            moveTo(Offset(width * .12f, centerY))
            cancel()
        }
        assertEquals(.75f, fraction(), .02f)
        ui.runOnIdle { assertEquals(1, seeks.size) }
        bar.performSemanticsAction(SemanticsActions.SetProgress) { assertTrue(it(.25f)) }
        ui.runOnIdle { assertEquals(25_000L, seeks.last()) }

        ui.onNodeWithText("雨夜助眠 · 轻声耳语").performTouchInput { click() }
        ui.onNodeWithContentDescription("播放").performTouchInput { click() }
        ui.onNodeWithContentDescription("下一轨").performTouchInput { click() }
        ui.runOnIdle {
            assertEquals(1, opens); assertEquals(1, plays); assertEquals(1, skips)
            state = state.copy(duration = 0)
        }
        bar.assert(SemanticsMatcher.keyNotDefined(SemanticsActions.SetProgress))
        bar.performTouchInput { swipeRight() }
        ui.runOnIdle { assertEquals(2, seeks.size) }
    }

    private fun fixture() = PlayerScreenState(connected = true, duration = 100_000, seekable = true,
        queue = listOf(QueueEntry("synthetic", "雨夜助眠 · 轻声耳语")), index = 0,
        showNsfw = true, lowEffects = true, canNext = true)

    private fun capture(name: String) {
        if (InstrumentationRegistry.getArguments().getString("visualFixture") != "true") return
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val dir = File(instrumentation.targetContext.getExternalFilesDir(null), "mini-progress").apply { mkdirs() }
        ui.onRoot().captureToImage().asAndroidBitmap().let { bitmap ->
            File(dir, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
    }
}
