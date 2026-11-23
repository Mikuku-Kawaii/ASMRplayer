package com.asmrplayer.android

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.media3.common.util.UnstableApi
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@UnstableApi
@RunWith(AndroidJUnit4::class)
class PlaylistNavigationTest {
    @get:Rule val ui=createAndroidComposeRule<MainActivity>()

    @Test fun convertsSelectedChaptersKeepingLegacyOrderAndQueueEntry() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend=AndroidBackend.get(context)
        val ids=listOf("playlist-fixture-b","playlist-fixture-a")
        val dao=backend.database.library()
        val work="playlist-ui-work"
        val legacy=runBlocking {
            dao.putWork(LibraryWork(work,null,"","测试作品",rating="SAFE"))
            ids.forEach { id->
                dao.putTrack(LibraryTrack("t-$id",work,id,"Disc"))
                dao.putVersion(TrackVersion(id,"t-$id","wav",25000))
            }
            backend.queueGroups.create("测试旧歌单",ids)
        }
        try {
            ui.onNode(hasText("歌单") and isSelectable()).performClick()
            ui.onNodeWithText("当前队列",substring=true).assertExists()
            ui.onNodeWithText("音轨歌单").performClick()
            ui.onNodeWithText("测试旧歌单").performClick()
            ui.onNodeWithContentDescription("歌单操作").performClick()
            ui.onNodeWithText("整理为作品集").performClick()
            ui.onNodeWithText("已收 2 / 2 轨").assertExists()
            ui.onNodeWithText("另存为作品集（1）").performClick()
            ui.waitUntil(10000) { ui.onAllNodesWithText("测试旧歌单 · 作品集").fetchSemanticsNodes().isNotEmpty() }
            ui.onNodeWithText("测试作品").assertExists()
            runBlocking {
                assertEquals(ids,backend.database.queueGroups().get(legacy)!!.ids())
                assertEquals(listOf(work),backend.database.queueGroups().all().single { it.isWorks && it.name=="测试旧歌单 · 作品集" }.ids())
            }
        } finally {
            runBlocking {
                backend.database.queueGroups().all().filter { it.id==legacy || it.name=="测试旧歌单 · 作品集" }.forEach { backend.queueGroups.delete(it.id) }
                val sql=backend.database.openHelper.writableDatabase
                ids.forEach { sql.delete("track_versions","id=?",arrayOf(it));sql.delete("tracks","id=?",arrayOf("t-$it")) }
                sql.delete("works","id=?",arrayOf(work))
            }
        }
    }
}
