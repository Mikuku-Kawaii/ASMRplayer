package com.asmrplayer.android

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.*
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.UUID

@UnstableApi
class DirectoryRemovalFeedbackTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun removeOnlyLibraryReferenceAndExpireNotice() {
        val context = ui.activity.applicationContext
        check(context.packageName.endsWith(".uitest"))
        val repo = AndroidBackend.get(context).library
        val id = UUID.randomUUID().toString()
        val title = "测试目录-" + id.take(6)
        val file = File.createTempFile("ring-test", ".wav", context.cacheDir)
        try {
            val root = LibraryRoot(id, "unbound://test/$id", title, "WORK")
            val version = runBlocking {
                repo.commit(root, LibraryIndex.build(title, listOf(ScannedDocument(file.name, file.toURI().toString())), RootMode.WORK))
                val work = repo.works.first().single { it.rootId == id }
                val version = repo.versions(work.id).first().single()
                repo.setFavorite(work.id, true)
                repo.addBookmark(version.id, 1000, "测试标记")
                AndroidBackend.get(context).database.library().putPosition(PlaybackPosition(version.id, 1000, 4000))
                version
            }
            ui.onNodeWithContentDescription("设置").performClick()
            ui.onNodeWithText("作品目录与扫描").performScrollTo().performClick()
            fun menu() {
                ui.onNodeWithText(title).performScrollTo().performTouchInput { longClick() }
                ui.onNodeWithText("移除目录").performClick()
                ui.onNodeWithText("移除作品目录？").assertIsDisplayed()
            }
            menu()
            ui.onNodeWithText("取消").performClick()
            ui.onNodeWithText(title).assertExists()
            menu()
            ui.onNodeWithText("移除目录").performClick()
            ui.waitUntil(5000) { ui.onAllNodesWithText("已移除作品目录").fetchSemanticsNodes().size == 1 }
            ui.onNodeWithText(title).assertDoesNotExist()
            ui.onNodeWithText("已移除作品目录").assertIsDisplayed()
            ui.mainClock.advanceTimeBy(2400)
            ui.onNodeWithText("已移除作品目录").assertIsDisplayed()
            ui.mainClock.advanceTimeBy(650)
            ui.onNodeWithText("已移除作品目录").assertDoesNotExist()
            runBlocking {
                assertFalse(repo.roots.first().any { it.id == id })
                val kept = repo.work(version.workId)!!
                assertNull(kept.rootId); assertTrue(kept.favorite); assertFalse(kept.inLibrary)
                assertFalse(repo.versions(kept.id).first().single().available)
                assertEquals(1000L, repo.position(version.id)!!.positionMs)
                assertEquals(1, repo.bookmarks(version.id).first().size)
            }
            assertTrue(file.exists())
        } finally { file.delete() }
    }
}
