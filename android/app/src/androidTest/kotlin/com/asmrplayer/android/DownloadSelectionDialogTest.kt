package com.asmrplayer.android

import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.text.TextLayoutResult
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.download.*
import com.asmrplayer.android.online.ProviderAccountState
import com.asmrplayer.android.ui.DownloadSelectionDialog
import com.asmrplayer.android.ui.PlayerTypography
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

@UnstableApi
class DownloadSelectionDialogTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()
    @Test fun formatAndFolderSelectionExcludeQueuedFilesAndCancelNeverSubmits() {
        val files = listOf(
            DownloadPlanItem("a", "A/one.wav", "wav", 197446861, false),
            DownloadPlanItem("b", "A/Sub/two.mp3", "mp3", null, false),
            DownloadPlanItem("c", "B/queued.wav", "wav", 1024, false, taskState="PAUSED"),
            DownloadPlanItem("d", "B/readme.txt", "txt", 12, true))
        val plan = DownloadPlan("fixture", "合成下载选择测试", ProviderAccountState("https://example.com", false), files)
        var state by mutableStateOf(DownloadPlanState(visible=true, plan=plan, selected=setOf("a", "b", "d")))
        var submissions = 0
        ui.runOnUiThread { ui.activity.setContent {
            MaterialTheme(typography = PlayerTypography) { DownloadSelectionDialog(state, { ids ->
                val eligible = files.filter { it.id in ids && it.selectable }.map { it.id }.toSet()
                state = state.copy(selected=if(state.selected.containsAll(eligible)) state.selected-eligible else state.selected+eligible)
            }, { state=state.copy(visible=false) }, {}, { submissions++ }) }
        } }
        ui.onNodeWithText("one.wav").assertDoesNotExist()
        ui.onNodeWithText("two.mp3").assertDoesNotExist()
        val footer = ui.onNodeWithText("取消").fetchSemanticsNode().boundsInRoot
        ui.onNodeWithContentDescription("展开目录 A").performClick()
        ui.onNodeWithText("one.wav").assertIsDisplayed()
        val sizeLayout = mutableListOf<TextLayoutResult>()
        ui.onNodeWithText("188.3 MB", useUnmergedTree = true).performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(sizeLayout) }
        val measuredSize = sizeLayout.single()
        // With softWrap=false the paragraph can be wider than its painted glyphs.
        // Verify the actual characters, including the unit, stay inside the text box.
        "188.3 MB".indices.forEach { index ->
            val glyph = measuredSize.getBoundingBox(index)
            assertTrue("Clipped size character at $index: $glyph / ${measuredSize.size}",
                glyph.left >= -1f && glyph.right <= measuredSize.size.width + 1f)
        }
        ui.onNodeWithText("two.mp3").assertDoesNotExist()
        ui.onNodeWithContentDescription("展开目录 A/Sub").performClick()
        ui.onNodeWithContentDescription("下载文件列表").performScrollToNode(hasText("two.mp3"))
        ui.onNodeWithText("two.mp3").assertIsDisplayed()
        assertEquals(footer, ui.onNodeWithText("取消").fetchSemanticsNode().boundsInRoot)
        ui.onNodeWithContentDescription("下载文件列表").performScrollToNode(hasContentDescription("收起目录 A"))
        ui.onNodeWithContentDescription("收起目录 A").performClick()
        ui.onNodeWithText("one.wav").assertDoesNotExist()
        assertEquals(footer, ui.onNodeWithText("取消").fetchSemanticsNode().boundsInRoot)
        ui.onNodeWithText("取消全选").performClick()
        ui.onNodeWithText("下载").assertIsNotEnabled()
        ui.onNodeWithText("wav").performClick()
        ui.runOnIdle { assertEquals(setOf("a"), state.selected) }
        ui.onNodeWithContentDescription("选择目录 A").performClick()
        ui.runOnIdle { assertEquals(setOf("a", "b"), state.selected) }
        ui.onNodeWithText("下载").assertIsEnabled()
        ui.onNodeWithText("取消").performClick()
        ui.onNodeWithText("创建下载任务").assertDoesNotExist()
        ui.runOnIdle { assertEquals(0, submissions) }
    }
}
