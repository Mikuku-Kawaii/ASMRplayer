package com.asmrplayer.android.online

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.media3.common.util.UnstableApi
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.library.LibraryIndex
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@UnstableApi
@RunWith(AndroidJUnit4::class)
class AccountLibraryDialogTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun rejectThenConfirmFromSiteAccountSettings() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        OnlineFixtureServer().use { server ->
            val ids = listOf("101","102","100000041").map { LibraryIndex.id("asmr_one",server.base,it) }
            try {
                installAccountLists(server)
                runBlocking { backend.online.account.setApiBase(server.base); backend.online.account.setToken("fixture") }
                ui.onNodeWithContentDescription("设置").performClick()
                ui.onNode(hasSetTextAction()).performTextInput("在线来源")
                ui.onNode(hasText("在线来源") and !hasSetTextAction()).performScrollTo().performClick()
                fun openPreview() {
                    ui.onNode(hasText("同步收藏与播放列表") and hasClickAction()).performScrollTo().performClick()
                }
                val validLists = requireNotNull(server.accountListsResponse)
                server.accountListsResponse = { request ->
                    val (status, body) = validLists(request)
                    if (request.path.startsWith("/api/playlist/get-playlist-works?")) {
                        val data = org.json.JSONObject(body)
                        data.getJSONObject("pagination").put("page", 2)
                        status to data.toString()
                    } else status to body
                }
                openPreview()
                ui.waitUntil(8000) {
                    ui.onAllNodesWithText("无法读取「歌单作品」第 1 页，返回的页码与请求不符。").fetchSemanticsNodes().isNotEmpty()
                }
                ui.onNodeWithText("关闭").performClick()
                assertTrue(runBlocking { ids.none { backend.library.work(it)?.inLibrary == true } })
                server.accountListsResponse = validLists
                fun preview() {
                    openPreview()
                    ui.waitUntil(8000) { ui.onAllNodesWithText("确认加入 3 部").fetchSemanticsNodes().isNotEmpty() }
                }
                preview()
                val all = ui.onNodeWithContentDescription("全选")
                all.assertIsOn().performClick()
                ui.onNodeWithText("确认加入").assertIsNotEnabled()
                ui.onNodeWithContentDescription("全选歌单").performClick()
                ui.onNodeWithText("保存 2 个作品集").assertIsEnabled()
                ui.onNodeWithText("拒绝").performClick()
                assertTrue(runBlocking { ids.none { backend.library.work(it)?.inLibrary == true } })
                preview()
                all.assertIsOn()
                ui.onNodeWithText("自建歌单").assertIsOn().performClick()
                ui.onNode(hasScrollAction() and hasAnyAncestor(isDialog())).performScrollToKey("work-102")
                ui.onNodeWithText("重复出现的作品").assertIsOn().performClick()
                ui.onNodeWithText("确认加入 2 部").performClick()
                ui.waitUntil(8000) { ui.onAllNodesWithText("已加入 2 部作品。已保存 1 个作品集。").fetchSemanticsNodes().isNotEmpty() }
                assertTrue(runBlocking { backend.library.work(ids[0])?.inLibrary == true })
                assertTrue(runBlocking { backend.library.work(ids[1])?.inLibrary != true })
                assertTrue(runBlocking { backend.library.work(ids[2])?.inLibrary == true })
                val group = runBlocking { backend.database.queueGroups().all().single { it.name == "收藏歌单" } }
                assertEquals(listOf(ids[2]), group.works().map { it.id })
                assertTrue(runBlocking { backend.database.queueGroups().all().none { it.name == "自建歌单" } })
                assertEquals(0,server.treeRequests.get())
                repeat(2) { ui.onNodeWithContentDescription("返回").performClick() }
                ui.onNode(hasText("音频库") and isSelectable()).performClick()
                val open = hasContentDescription("打开 歌单作品")
                ui.waitUntil(10000) { ui.onAllNodes(open).fetchSemanticsNodes().isNotEmpty() }
                ui.onNode(open).performClick()
                ui.waitUntil(10000) { ui.onAllNodesWithText("音轨 1").fetchSemanticsNodes().isNotEmpty() }
                assertTrue(server.treeRequests.get() > 0)
            } finally {
                runBlocking {
                    backend.online.account.setToken(null)
                    backend.online.account.setApiBase(oldBase)
                    for (list in backend.database.queueGroups().all().filter { item -> item.works().any { it.id in ids } }) {
                        backend.database.queueGroups().delete(list.id)
                    }
                    val db = backend.database.openHelper.writableDatabase
                    for (id in ids) {
                        db.delete("catalog_metadata","workId=?", arrayOf(id))
                        db.delete("works","id=?", arrayOf(id))
                    }
                }
            }
        }
    }
}
