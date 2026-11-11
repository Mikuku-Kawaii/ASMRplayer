package com.asmrplayer.android

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.online.OnlineFixtureServer
import com.asmrplayer.android.ui.Destination
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class HomeFeedLayoutTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun homeTitlesShareBoundsAndDiscoverAppendsWhenScrolled() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        OnlineFixtureServer().use { server ->
            server.browseResponse = { request ->
                val page = request.browsePage()
                200 to feedPage(page, (123450 + (page - 1) * 20 until 123450 + page * 20).toList(), 60, 20)
            }
            try {
                runBlocking { backend.online.account.setApiBase(server.base) }
                val bounds = Destination.entries.map { page ->
                    ui.onNode(hasText(page.title) and isSelectable()).performClick()
                    ui.waitUntil(5000) {
                        ui.onAllNodes(hasText(page.title) and isSelected()).fetchSemanticsNodes().isNotEmpty()
                    }
                    ui.waitForIdle()
                    ui.onNode(hasTestTag("home-header") and hasAnyAncestor(hasTestTag("home-page-${page.name}")))
                        .fetchSemanticsNode().boundsInRoot
                }
                bounds.drop(1).forEach { rect ->
                    assertEquals(bounds.first().top, rect.top, 1f)
                    assertEquals(bounds.first().height, rect.height, 1f)
                    assertEquals(bounds.first().left, rect.left, 1f)
                    assertEquals(bounds.first().width, rect.width, 1f)
                }
                ui.onNode(hasText("发现") and isSelectable()).performClick()
                ui.waitUntil(8000) { ui.onAllNodesWithText("Feed work 123450").fetchSemanticsNodes().isNotEmpty() }
                val grid = ui.onNode(hasScrollToIndexAction() and
                    SemanticsMatcher.keyIsDefined(androidx.compose.ui.semantics.SemanticsProperties.VerticalScrollAxisRange) and
                    hasAnyAncestor(hasTestTag("home-page-Discover")))
                assertFalse(server.requests.any { it.browsePage() == 2 })
                grid.performScrollToIndex(17)
                ui.waitUntil(8000) { server.requests.any { it.browsePage() == 2 } }
                ui.waitForIdle()
                // Appending must not scroll back to the first work.
                ui.onNodeWithText("Feed work 123450").assertDoesNotExist()
                grid.performScrollToIndex(21)
                ui.onNodeWithText("Feed work 123470").assertIsDisplayed()
                grid.performScrollToIndex(0)
                ui.onNodeWithText("Feed work 123450").assertIsDisplayed()
                ui.onNodeWithText("上一页").assertDoesNotExist()
                ui.onNodeWithText("下一页").assertDoesNotExist()
            } finally {
                ui.onNode(hasText("音频库") and isSelectable()).performClick()
                runBlocking { backend.online.account.setApiBase(oldBase) }
            }
        }
    }
}
