package com.asmrplayer.android

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.room.withTransaction
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.data.CatalogMetadata
import com.asmrplayer.android.data.LibraryTrack
import com.asmrplayer.android.data.LibraryWork
import com.asmrplayer.android.data.MediaSource
import com.asmrplayer.android.data.TrackVersion
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.online.OnlineFixtureServer
import com.asmrplayer.android.ui.Destination
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class HomeFeedLayoutTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun detailJoinsVisiblePrefetchAndNeverShowsPartialLocalTrackList() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        val releaseTree = CountDownLatch(1)
        OnlineFixtureServer().use { server ->
            server.secondTrack = true
            server.beforeTreeResponse = { releaseTree.await(8, TimeUnit.SECONDS) }
            server.browseResponse = { 200 to feedPage(1, listOf(123456), total = 1) }
            val work = LibraryIndex.id("asmr_one", server.base, "123456")
            try {
                runBlocking {
                    backend.online.account.setApiBase(server.base)
                    backend.database.withTransaction {
                        val dao = backend.database.library()
                        dao.putWork(LibraryWork(work, null, "", "已保存的目录", rating = "SAFE"))
                        backend.database.metadata().put(CatalogMetadata(work, origin = server.base, sourceId = "123456"))
                        dao.putTrack(LibraryTrack("partial-track-$work", work, "不完整的缓存音轨", "WAV"))
                        dao.putVersion(TrackVersion("partial-version-$work", "partial-track-$work", "wav"))
                        dao.putSource(MediaSource("partial-source-$work", "partial-version-$work", null, "WAV/01.wav",
                            "asmr-online://partial-version-$work", kind = "online_provider"))
                    }
                }
                ui.onNode(hasText("发现") and isSelectable()).performClick()
                ui.waitUntil(6_000) { server.treeRequests.get() == 1 }
                ui.onNodeWithText("Feed work 123456").performClick()
                ui.waitUntil(6_000) { ui.onAllNodesWithText("音轨").fetchSemanticsNodes().isNotEmpty() }
                ui.onNodeWithText("音轨 1").assertDoesNotExist()
                ui.onNodeWithText("不完整的缓存音轨").assertDoesNotExist()

                releaseTree.countDown()
                ui.waitUntil(8_000) { ui.onAllNodesWithText("音轨 2").fetchSemanticsNodes().isNotEmpty() }
                assertEquals(1, server.treeRequests.get())

                ui.onNodeWithContentDescription("返回").performClick()
                ui.onNode(hasText("音频库") and isSelectable()).performClick()
                ui.onNodeWithText("已保存的目录").performClick()
                ui.waitUntil(6_000) { ui.onAllNodesWithText("音轨 2").fetchSemanticsNodes().isNotEmpty() }
                assertEquals(1, server.treeRequests.get())
            } finally {
                releaseTree.countDown()
                runBlocking { backend.online.account.setApiBase(oldBase) }
            }
        }
    }

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
