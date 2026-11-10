package com.asmrplayer.android

import androidx.activity.compose.setContent
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.online.BrowseFilters
import com.asmrplayer.android.ui.MotionButton
import com.asmrplayer.android.ui.OnlineFilterSheet
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

@UnstableApi
class FilterEditorTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun voiceSearchAndSubtitleChipsCommitTogether() {
        var applied = BrowseFilters()
        ui.runOnUiThread { ui.activity.setContent {
            OnlineFilterSheet(BrowseFilters(), emptyList(), emptyMap(), null, listOf("声优 A", "声优 B"),
                emptyList(), "popular", false, listOf("popular" to "热门作品"), {}) { filters, _, _ -> applied = filters }
        } }
        ui.onNodeWithText("仅带字幕").performClick()
        ui.onNodeWithText("声优").performClick()
        ui.onNode(hasSetTextAction()).performTextInput("B")
        ui.onNodeWithText("声优 A").assertDoesNotExist()
        ui.onNodeWithText("声优 B").performClick()
        ui.onNodeWithText("完成选择").performClick()
        ui.runOnIdle { assertEquals(BrowseFilters(), applied) }
        ui.onNodeWithText("应用筛选").performClick()
        ui.runOnIdle { assertEquals(listOf("声优 B"), applied.voices); assertTrue(applied.subtitleOnly) }
    }

    @Test fun switchingTagDirectionAtLimitAndClosingDoesNotCommitDraft() {
        val initial = BrowseFilters(tags = listOf("A", "B", "C", "D"), excludedTags = listOf("E"))
        var applied by mutableStateOf(initial)
        var commits = 0
        ui.runOnUiThread { ui.activity.setContent {
            var open by remember { mutableStateOf(true) }
            if (open) OnlineFilterSheet(applied, listOf("A", "B", "C", "D", "E", "F"), emptyMap(),
                null, emptyList(), emptyList(), "popular", false, listOf("popular" to "热门作品"),
                { open = false }) { filters, _, _ -> applied = filters; commits++ }
            else MotionButton({ open = true }) { Text("重新打开") }
        } }
        fun tagPage() {
            ui.waitUntil(5000) { ui.onAllNodesWithText("标签").fetchSemanticsNodes().isNotEmpty() }
            ui.onNodeWithText("标签").performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnClick)
            ui.waitUntil(5000) { ui.onAllNodesWithText("排除").fetchSemanticsNodes().isNotEmpty() }
        }
        tagPage()
        ui.onNodeWithText("F").assertIsNotEnabled()
        ui.onNodeWithText("排除").performClick()
        ui.onNodeWithText("A").performClick()
        ui.onNodeWithText("排除 A").assertExists()
        ui.onNodeWithContentDescription("关闭筛选").performClick()
        ui.runOnIdle { assertEquals(initial, applied); assertEquals(0, commits) }
        ui.onNodeWithText("重新打开").performClick()
        tagPage()
        ui.onNodeWithText("排除").performClick()
        ui.onNodeWithText("A").performClick()
        ui.onNodeWithText("完成选择").performClick()
        ui.runOnIdle { assertEquals(0, commits) }
        ui.onNodeWithText("应用筛选").performClick()
        ui.runOnIdle {
            assertEquals(1, commits)
            assertEquals(listOf("B", "C", "D"), applied.tags)
            assertEquals(listOf("E", "A"), applied.excludedTags)
        }
    }
}
