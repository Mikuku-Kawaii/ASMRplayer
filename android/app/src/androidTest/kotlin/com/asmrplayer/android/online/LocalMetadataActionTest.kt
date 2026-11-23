package com.asmrplayer.android.online

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.media3.common.util.UnstableApi
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.data.LibraryRoot
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.library.RootMode
import com.asmrplayer.android.library.ScannedDocument
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@UnstableApi
@RunWith(AndroidJUnit4::class)
class LocalMetadataActionTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun enterVjCodeFromTheLocalWorkMenu() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        val root = LibraryRoot("metadata-ui-fixture", "content://fixture/metadata", "待同步的本地作品", "WORK")
        val workId = LibraryIndex.id(root.id, "work", "")
        val exact = """{"id":100000041,"source_id":"VJ01002150","title":"来源作品标题","nsfw":false}"""
        OnlineFixtureServer().use { server ->
            try {
                server.browseResponse = { 200 to """{"works":[$exact]}""" }
                server.metadataBody = exact
                runBlocking {
                    backend.online.account.setApiBase(server.base)
                    backend.database.library().putRoot(root)
                    backend.library.commit(root, LibraryIndex.build(root.title,
                        listOf(ScannedDocument("01.wav", "content://fixture/01")), RootMode.WORK))
                    backend.library.setRating(workId, "SAFE")
                    backend.library.setMembership(workId, true)
                }
                ui.onNode(hasText("音频库") and isSelectable()).performClick()
                val open = hasContentDescription("打开 ${root.title}")
                ui.waitUntil(10000) { ui.onAllNodes(open).fetchSemanticsNodes().isNotEmpty() }
                ui.onNode(open).performClick()
                ui.onNodeWithContentDescription("作品操作").performClick()
                ui.onNodeWithText("同步元数据").performClick()
                ui.onNode(hasSetTextAction()).performTextInput("VJ01002150")
                ui.onNodeWithText("同步").performClick()
                ui.waitUntil(10000) { ui.onAllNodesWithText("来源作品标题").fetchSemanticsNodes().isNotEmpty() }
                assertEquals("100000041", runBlocking { backend.database.metadata().forWork(workId)?.sourceId })
            } finally {
                runBlocking {
                    backend.online.account.setApiBase(oldBase)
                    backend.library.removeRoot(root.id)
                    backend.database.openHelper.writableDatabase.delete("catalog_metadata", "workId=?", arrayOf(workId))
                }
            }
        }
    }
}
