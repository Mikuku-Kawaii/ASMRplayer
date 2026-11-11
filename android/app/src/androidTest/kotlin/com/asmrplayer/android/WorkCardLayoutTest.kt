package com.asmrplayer.android

import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.ui.CardTitle
import com.asmrplayer.android.ui.WorkCard
import com.asmrplayer.android.ui.PlayerTypography
import com.asmrplayer.android.ui.CardCodeScore
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

@UnstableApi
class WorkCardLayoutTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun ratingStartsAtTheSameColumnAfterShortAndLongCodes() {
        ui.runOnUiThread { ui.activity.setContent {
            MaterialTheme(typography = PlayerTypography) {
                Column {
                    Box(Modifier.width(170.dp)) { CardCodeScore("RJ123456", 4.85) }
                    Box(Modifier.width(220.dp)) { CardCodeScore("RJ12345678", 4.23) }
                }
            }
        } }
        val shortRating = ui.onNodeWithText("4.85").fetchSemanticsNode().boundsInRoot
        val longRating = ui.onNodeWithText("4.23").fetchSemanticsNode().boundsInRoot
        assertEquals(shortRating.left, longRating.left, 1f)
        assertTrue(shortRating.left > ui.onNodeWithText("RJ123456").fetchSemanticsNode().boundsInRoot.right)
        assertTrue(longRating.left > ui.onNodeWithText("RJ12345678").fetchSemanticsNode().boundsInRoot.right)
    }

    @Test fun titleSlotsStayEqualForShortTwoLineAndEllipsizedMixedTitles() {
        val titles = listOf("短标题", "中文 ASMR\nあいう Ag", "中日文 あいう ASMR 长标题".repeat(12))
        var fontScale by mutableFloatStateOf(1f)
        ui.runOnUiThread { ui.activity.setContent {
            val density = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(density.density, fontScale)) {
                MaterialTheme(typography = PlayerTypography) {
                    Column(Modifier.width(160.dp)) {
                        titles.forEachIndexed { index, title -> CardTitle(title, Modifier.testTag("title$index")) }
                    }
                }
            }
        } }
        for (scale in listOf(1f, 1.5f)) {
            ui.runOnIdle { fontScale = scale }
            val heights = titles.indices.map { ui.onNodeWithTag("title$it").fetchSemanticsNode().boundsInRoot.height }
            assertEquals(heights[0], heights[1], 1f)
            assertEquals(heights[1], heights[2], 1f)
            titles.indices.forEach { index ->
                val results = mutableListOf<TextLayoutResult>()
                ui.onNodeWithTag("title$index").performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(results) }
                val layout = results.single()
                assertTrue(layout.lineCount <= 2)
                assertTrue("Visible lines must fit the shared slot: scale=$scale index=$index height=${layout.size.height}",
                    layout.getLineBottom(layout.lineCount - 1) <= layout.size.height + 1)
                if (index == 1) { assertEquals(2, layout.lineCount); assertFalse(layout.isLineEllipsized(1)) }
                if (index == 2) { assertEquals(2, layout.lineCount); assertTrue(layout.isLineEllipsized(1)) }
            }
        }
    }

    @Test fun missingMetadataKeepsActionsAlignedAndOverflowTagsDoNotExpandCard() {
        var saved = 0
        var liked = 0
        var chosen = ""
        val tags = listOf("一个很长的作品标签", "治愈", "睡眠", "耳语")
        ui.runOnUiThread { ui.activity.setContent {
            MaterialTheme(typography = PlayerTypography) {
                Row {
                    Box(Modifier.width(170.dp).testTag("full")) {
                        WorkCard("完整作品", null, "RJ123456", listOf("在线"), "",
                            tags, {}, favorite=false, like={liked++}, onTag={chosen=it},
                            voices=listOf("声优"), inLibrary=false, save={saved++}, score=4.8)
                    }
                    Box(Modifier.width(170.dp).testTag("empty")) {
                        WorkCard("缺少资料", null, null, emptyList(), "", open={},
                            favorite=false, like={}, inLibrary=false, save={})
                    }
                }
            }
        } }
        val before = ui.onNodeWithTag("full").fetchSemanticsNode().boundsInRoot
        assertEquals(before.height, ui.onNodeWithTag("empty").fetchSemanticsNode().boundsInRoot.height, 1f)
        val first = ui.onNodeWithContentDescription("喜欢 完整作品").fetchSemanticsNode().boundsInRoot
        val second = ui.onNodeWithContentDescription("喜欢 缺少资料").fetchSemanticsNode().boundsInRoot
        assertEquals(first.top, second.top, 1f)
        ui.onNodeWithContentDescription("加入音频库 完整作品").performClick()
        ui.onNodeWithContentDescription("喜欢 完整作品").performClick()
        ui.runOnIdle { assertEquals(1, saved); assertEquals(1, liked) }
        ui.onNodeWithContentDescription("展开其余 3 个标签").performClick()
        ui.onNodeWithText("作品标签").assertIsDisplayed()
        ui.onNodeWithText("耳语").performClick()
        ui.runOnIdle { assertEquals("耳语", chosen) }
        ui.onNodeWithText("作品标签").assertDoesNotExist()
        assertEquals(before.height, ui.onNodeWithTag("full").fetchSemanticsNode().boundsInRoot.height, 1f)
    }
}
