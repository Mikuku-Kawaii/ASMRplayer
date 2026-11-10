package com.asmrplayer.android

import androidx.lifecycle.ViewModelProvider
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.asmrplayer.android.ui.*
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.room.withTransaction
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.data.CatalogMetadata
import com.asmrplayer.android.data.LibraryWork
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@UnstableApi
@RunWith(AndroidJUnit4::class)
class PlayerMotionTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun catalogGridRoutesCoverHeartAndMetadataTapsIndependently() {
        var opened = 0
        var likes = 0
        var tag = ""
        var voice = ""
        ui.runOnUiThread { ui.activity.setContent {
            Box(Modifier.fillMaxWidth().testTag("grid-entry")) {
                WorkCard("测试作品", null, "RJ123456", listOf("本地"), "", listOf("雨声"),
                    { opened++ }, onTag = { tag = it }, favorite = false, like = { likes++ },
                    voices = listOf("测试声优"), onVoice = { voice = it })
            }
        } }
        ui.onNodeWithContentDescription("喜欢 测试作品").performTouchInput { click() }
        ui.onNodeWithText("雨声", useUnmergedTree = true).performTouchInput { click() }
        ui.onNodeWithText("测试声优", useUnmergedTree = true).performTouchInput { click() }
        ui.runOnIdle {
            assertEquals(0, opened)
            assertEquals(1, likes)
            assertEquals("雨声", tag)
            assertEquals("测试声优", voice)
        }
        ui.onNodeWithContentDescription("打开 测试作品").performClick()
        ui.runOnIdle { assertEquals(1, opened) }
    }

    @Test fun workGroupChipsScrollAndSelectFolderAndFormatTogether() {
        val groups = listOf("本編" to "wav", "特典" to "wav", "特典" to "mp3")
        var selected by mutableStateOf(groups.first())
        ui.runOnUiThread { ui.activity.setContent {
            Box(Modifier.width(190.dp)) { GroupPicker(groups, selected) { selected = it } }
        } }
        val strip = ui.onNode(hasScrollToIndexAction())
        strip.performTouchInput { swipeLeft() }
        ui.onNodeWithText("特典 · MP3").assertIsDisplayed().performClick()
        ui.runOnIdle { assertEquals(groups.last(), selected) }
        strip.performTouchInput { swipeRight() }
        ui.onNodeWithText("本編 · WAV").assertIsDisplayed().performClick()
        ui.runOnIdle { assertEquals(groups.first(), selected) }
    }

    @Test fun offCentrePinchKeepsLandmarkUnderFingersAndClampsToFittedImage() {
        val bitmap = Bitmap.createBitmap(400, 400, Bitmap.Config.ARGB_8888)
        Canvas(bitmap).apply {
            drawColor(android.graphics.Color.BLACK)
            drawCircle(280f, 180f, 16f, Paint().apply { color = android.graphics.Color.RED })
        }
        ui.runOnUiThread { ui.activity.setContent {
            ZoomableCover(bitmap.asImageBitmap(), Modifier.size(220.dp).testTag("viewport"))
        } }
        val view = ui.onNodeWithTag("viewport")
        fun redWidth(): Int {
            val image = view.captureToImage().toPixelMap()
            val row = (image.height * .45f).toInt()
            assertTrue("Landmark moved away from centroid", image[(image.width * .70f).toInt(), row].red > .9f)
            return (0 until image.width).count { image[it, row].red > .9f }
        }
        val initial = redWidth()
        view.performTouchInput {
            val pivot = Offset(width * .70f, height * .45f)
            down(0, pivot - Offset(width * .08f, 0f))
            down(1, pivot + Offset(width * .08f, 0f))
            for (i in 1..12) {
                val delta = Offset(width * (.08f + .01f * i), 0f)
                updatePointerTo(0, pivot - delta)
                updatePointerTo(1, pivot + delta)
                move()
            }
            up(1); up(0)
        }
        assertTrue("Pinch did not enlarge the image", redWidth() > initial * 1.65f)
        val viewSize = IntSize(300, 600)
        val imageSize = IntSize(400, 200)
        val bounded = CoverTransform().gesture(viewSize, imageSize, Offset(150f, 300f), Offset(5000f, 5000f), 2f)
        assertEquals(Offset(150f, 0f), bounded.offset)
        assertEquals(CoverTransform(), bounded.gesture(viewSize, imageSize, Offset(50f, 90f), Offset.Zero, .2f))
    }

    @Test fun cueScrollApproachesAnchorAndSeekSkipsInterveningLines() {
        lateinit var list: LazyListState
        var cue by mutableIntStateOf(0)
        ui.runOnUiThread { ui.activity.setContent {
            list = rememberLazyListState()
            LazyColumn(Modifier.size(250.dp), state=list, contentPadding=PaddingValues(vertical=30.dp)) {
                items(80) { Box(Modifier.fillMaxWidth().height(55.dp)) }
            }
            LaunchedEffect(cue) { list.followLyric(cue, animate = cue < 10) }
        } }
        ui.waitForIdle()
        ui.mainClock.autoAdvance = false
        try {
            fun offset(index: Int) = ui.runOnUiThread { list.layoutInfo.visibleItemsInfo.first { it.index == index }.offset }
            ui.runOnUiThread { cue=1 }
            ui.mainClock.advanceTimeBy(96)
            val middle = offset(1)
            assertTrue(middle > 0)
            ui.runOnUiThread { cue=2 }
            var previous = offset(2)
            repeat(18) {
                ui.mainClock.advanceTimeByFrame()
                val current = offset(2)
                assertTrue("Unexpected reverse or overshoot: $previous to $current", current in 0..previous)
                previous = current
            }
            assertEquals(0, previous)
            ui.runOnUiThread { cue=60 }
            ui.mainClock.advanceTimeBy(48)
            assertEquals(0, offset(60))
        } finally { ui.mainClock.autoAdvance = true }
    }

    @Test fun settingsTitleAndContentTravelTogetherAndCanImmediatelyReturn() {
        ui.onNodeWithContentDescription("设置").assertIsDisplayed()
        ui.mainClock.autoAdvance = false
        try {
            ui.onNodeWithContentDescription("设置").performClick()
            ui.mainClock.advanceTimeBy(48)
            val title = ui.onNodeWithText("设置")
            val body = ui.onNodeWithText("搜索设置")
            val titleStart = title.fetchSemanticsNode().positionInRoot.x
            val bodyStart = body.fetchSemanticsNode().positionInRoot.x
            ui.mainClock.advanceTimeBy(48)
            val dx = title.fetchSemanticsNode().positionInRoot.x - titleStart
            val bodyDx = body.fetchSemanticsNode().positionInRoot.x - bodyStart
            assertTrue("Forward layer did not settle toward origin", dx < 0f)
            assertEquals(dx, bodyDx, 1f)
            ui.onNodeWithContentDescription("返回").performClick()
            ui.mainClock.advanceTimeBy(32)
            ui.onNodeWithContentDescription("设置").performClick()
            ui.mainClock.advanceTimeBy(240)
            body.assertIsDisplayed()
        } finally { ui.mainClock.autoAdvance = true }
    }

    @Test fun filteringFromScrolledWorkCardPreservesItsViewportAnchor() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend = AndroidBackend.get(context)
        val prefix = "000Motion${System.nanoTime()}"
        val ids = (0 until 36).map { "$prefix-${it.toString().padStart(2, '0')}" }
        val tag = "锚点"
        runBlocking {
            backend.database.withTransaction {
                ids.forEachIndexed { index, id ->
                    backend.database.library().putWork(LibraryWork(id, null, "", id, rating="SAFE"))
                    if (index >= 6) backend.database.metadata().put(CatalogMetadata(id, tagsJson="""["锚点"]"""))
                }
            }
        }
        try {
            val model = ui.runOnUiThread { ViewModelProvider(ui.activity)[PlayerViewModel::class.java] }
            ui.waitUntil(8000) { model.collections.state.value.works.count { it.work.id.startsWith(prefix) } == ids.size }
            val grid = ui.onNode(hasScrollToIndexAction() and hasAnyAncestor(hasTestTag("home-page-Library")))
            grid.performScrollToIndex(19)
            val nodes = ui.onAllNodes(hasContentDescription("打开 $prefix", substring=true)).fetchSemanticsNodes()
            val first = nodes.filter { it.boundsInRoot.height > 0 }.minWith(compareBy({it.boundsInRoot.top},{it.boundsInRoot.left}))
            val anchor = first.config[SemanticsProperties.ContentDescription].first()
            val top = first.positionInRoot.y
            ui.onAllNodes(hasText(tag) and hasClickAction()).onFirst().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnClick)
            try {
                ui.waitUntil(8000) { val result = model.collections.state.value
                    result.ready && tag in result.query.tags }
            } catch (timeout: ComposeTimeoutException) {
                val result = model.collections.state.value
                throw AssertionError("Filter did not settle: tags=${result.query.tags} works=${result.works.size} ready=${result.ready}", timeout)
            }
            assertEquals(30, model.collections.state.value.works.size)
            ui.waitForIdle()
            assertEquals(top, ui.onNodeWithContentDescription(anchor).fetchSemanticsNode().positionInRoot.y, 1f)
        } finally {
            runBlocking { backend.database.withTransaction { ids.forEach { backend.library.setMembership(it, false) } } }
        }
    }

    @Test fun shortSheetGrowsAndShrinksWithDisclosure() {
        ui.runOnUiThread { ui.activity.setContent {
            var visible by remember { mutableStateOf(true) }
            if (visible) PlayerSheet("动效测试", { visible=false }) {
                DetailDisclosure("选项") { Box(Modifier.size(100.dp).testTag("expanded-option")) }
            }
        } }
        val header = ui.onNodeWithText("选项")
        val before = header.fetchSemanticsNode().boundsInRoot.top
        header.performClick()
        ui.onNodeWithTag("expanded-option").assertIsDisplayed()
        ui.waitUntil(5000) { header.fetchSemanticsNode().boundsInRoot.top < before - 1f }
        header.performClick()
        ui.waitUntil(5000) { kotlin.math.abs(header.fetchSemanticsNode().boundsInRoot.top - before) < 1.1f }
        ui.onNodeWithContentDescription("关闭动效测试").performClick()
        ui.waitUntil(5000) { ui.onAllNodesWithText("动效测试").fetchSemanticsNodes().isEmpty() }
    }

    @Test fun playerModeReversalKeepsTransportInPlace() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend = AndroidBackend.get(context)
        val model = ui.runOnUiThread { ViewModelProvider(ui.activity)[PlayerViewModel::class.java] }
        val oldBase = backend.online.account.state.value.apiBase
        val script = java.io.File.createTempFile("motion-", ".lrc", context.cacheDir)
        val sourceId = script.name
        com.asmrplayer.android.online.OnlineFixtureServer(durationSeconds = 120, sampleRate = 8000).use { server ->
            try {
                val version = runBlocking {
                    backend.online.account.setApiBase(server.base)
                    val version = backend.online.importWork("123456").versionIds.single()
                    script.writeText("[00:00.00]测试台本")
                    backend.database.library().putSubtitle(com.asmrplayer.android.data.SubtitleSource(sourceId, version, null,
                        script.name, android.net.Uri.fromFile(script).toString(), script.name, "lrc"))
                    backend.database.library().putSubtitlePreference(com.asmrplayer.android.data.SubtitlePreference(version, sourceId, null))
                    version
                }
                ui.runOnUiThread { ui.activity.setContent { PlayerApp(model, playerRequest = 1) }; model.playVersion(version) }
                ui.waitUntil(10000) { model.screenState.value.playing && model.tools.state.value.subtitles?.original != null }
                ui.runOnUiThread { model.togglePlayback() }
                ui.waitUntil(5000) { !model.screenState.value.playRequested }
                assertReversibleTransport()
            } finally {
                ui.runOnUiThread { model.stop() }
                runBlocking { backend.online.account.setApiBase(oldBase) }
                script.delete()
            }
        }
    }

    private fun assertReversibleTransport() {
        val enter = ui.onNodeWithContentDescription("点击封面展开台本")
        val leave = ui.onNodeWithContentDescription("返回封面")
        enter.assertIsDisplayed()
        val seek = ui.onNode(hasContentDescription("播放进度", substring=true))
        val play = ui.onNodeWithContentDescription("播放")
        val seekBounds = seek.fetchSemanticsNode().boundsInRoot
        val playBounds = play.fetchSemanticsNode().boundsInRoot
        fun assertFixed() {
            assertEquals(seekBounds, seek.fetchSemanticsNode().boundsInRoot)
            assertEquals(playBounds, play.fetchSemanticsNode().boundsInRoot)
        }
        ui.mainClock.autoAdvance = false
        try {
            enter.performClick(); ui.mainClock.advanceTimeBy(64); assertFixed()
            leave.performClick(); ui.mainClock.advanceTimeBy(32); assertFixed()
            enter.performClick(); ui.mainClock.advanceTimeBy(240); assertFixed()
            leave.performClick(); ui.mainClock.advanceTimeBy(240); assertFixed()
        } finally { ui.mainClock.autoAdvance = true }
    }
}
