package com.asmrplayer.android.online

import android.net.Uri
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.LibraryIndex
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

internal fun installAccountLists(server: OnlineFixtureServer) {
    fun work(id: Int, title: String, code: String) = JSONObject().put("id", id).put("title", title).put("source_id", code).put("nsfw", false)
    val local = work(101, "原有作品", "RJ000101")
    val both = work(102, "重复出现的作品", "RJ000102")
    val onlyInList = work(100000041, "歌单作品", "VJ01002150")
    server.metadataBody = onlyInList.toString()
    server.accountListsResponse = { request ->
        val uri = Uri.parse(request.path)
        val page = uri.getQueryParameter("page")!!.toInt()
        val (key, row, total) = when (uri.path) {
            "/api/review" -> Triple("works", if (page == 1) local else both, 2)
            "/api/playlist/get-playlists" -> Triple("playlists", JSONObject().put("id", uri.getQueryParameter("filterBy"))
                .put("name", if (uri.getQueryParameter("filterBy") == "owned") "自建歌单" else "收藏歌单"), 1)
            "/api/playlist/get-playlist-works" -> Triple("works", if (uri.getQueryParameter("id") == "owned") both else onlyInList, 1)
            else -> error("Unexpected account route")
        }
        if (request.headers["authorization"] != "Bearer fixture") 401 to "{}" else
            200 to JSONObject().put(key, JSONArray().put(row)).put("pagination",
                JSONObject().put(if (uri.path == "/api/review") "currentPage" else "page", page).put("pageSize", 1).put("totalCount", total)).toString()
    }
}

@RunWith(AndroidJUnit4::class)
class AccountLibraryImportTest {
    @Test fun confirmationIsScopedAtomicAndIdempotent() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        val http = HttpTransport(debugLoopback = true)
        val provider = AsmrOneProvider(ProviderAccount(context, http), http)
        val online = OnlineRepository(db, provider)
        val oldBase = online.account.state.value.apiBase
        OnlineFixtureServer().use { server ->
            try {
                installAccountLists(server)
                online.account.setApiBase(server.base)
                online.account.setToken("fixture")
                db.library().putWork(LibraryWork("local", "root", ".", "本地标题", "RJ000101"))
                val cached = LibraryIndex.id("asmr_one", server.base, "101")
                db.library().putWork(LibraryWork(cached, null, "", "曾浏览的作品", "RJ000101", inLibrary = false))
                db.metadata().put(CatalogMetadata(cached, origin = server.base, sourceId = "101"))
                val preview = online.accountLibrary.preview()
                assertEquals(listOf(2,2,2,1), listOf(preview.favorites,preview.playlists,preview.playlistWorks,preview.alreadyInLibrary))
                assertEquals(2, preview.works.size)
                assertEquals(2, db.metadata().accountLibraryEntries().size)
                online.account.setToken("fixture")
                assertEquals(OnlineError.SOURCE_CHANGED, (runCatching { online.accountLibrary.confirm(preview) }.exceptionOrNull() as OnlineException).code)
                val next = online.accountLibrary.preview()
                val owned = next.lists.first { it.name == "自建歌单" }
                val justVj = AccountLibrarySelection(setOf("100000041"), setOf(owned.id))
                assertTrue(runCatching { online.accountLibrary.confirm(next, justVj.copy(playlistIds = setOf("foreign"))) }.isFailure)
                assertTrue(db.queueGroups().all().isEmpty())
                assertEquals(1, online.accountLibrary.confirm(next, justVj))
                assertEquals(listOf("102"), online.accountLibrary.preview().works.map { it.sourceId })
                assertEquals(listOf(owned.id), db.queueGroups().all().map { it.id })
                assertTrue(db.queueGroups().get(owned.id)!!.works().isEmpty())
                assertEquals(1,online.accountLibrary.confirm(next))
                assertEquals(0,online.accountLibrary.confirm(next))
                val existing=online.accountLibrary.preview()
                assertTrue(existing.works.isEmpty())
                assertTrue(db.queueGroups().get(owned.id)!!.works().isEmpty())
                assertEquals(0,online.accountLibrary.confirm(existing,true))
                val original=db.queueGroups().all()
                assertEquals(2,original.size)
                assertTrue(original.all { it.isWorks && it.works().size==1 })
                online.accountLibrary.confirm(online.accountLibrary.preview(),true)
                assertEquals(original,db.queueGroups().all())
                val shared = original.single { it.id == owned.id }.works().single().id
                db.library().membership(shared, false)
                val missingAgain = online.accountLibrary.preview()
                assertEquals(0, online.accountLibrary.confirm(missingAgain, AccountLibrarySelection(playlistIds = setOf(owned.id))))
                assertEquals(original, db.queueGroups().all())
                assertFalse(db.library().work(shared)!!.inLibrary)
                assertEquals(1, online.accountLibrary.confirm(missingAgain, AccountLibrarySelection(workIds = setOf("102"))))
                val empty=existing.lists.first().copy(name="空的官网歌单",sources=emptyList())
                val repo=com.asmrplayer.android.data.QueueGroupRepository(db)
                val manual=com.asmrplayer.android.data.CollectionWork("local")
                repo.addWorks(empty.id,listOf(manual))
                online.accountLibrary.confirm(existing.copy(lists=listOf(empty)),true)
                val updated=db.queueGroups().get(empty.id)!!
                assertEquals("空的官网歌单",updated.name)
                assertEquals(listOf(manual),updated.works())
                repo.remove(empty.id,manual.id)
                online.accountLibrary.confirm(existing.copy(lists=listOf(empty)),true)
                assertTrue(db.queueGroups().get(empty.id)!!.works().isEmpty())
                val imported = LibraryIndex.id("asmr_one", server.base, "100000041")
                assertTrue(db.library().work(imported)!!.inLibrary)
                assertFalse(db.library().work(imported)!!.favorite)
                assertEquals("本地标题",db.library().work("local")!!.title)
                assertEquals(0,server.treeRequests.get())
                val opened = online.importWork("100000041", false)
                assertEquals(imported, opened.workId)
                assertEquals(1,opened.versionIds.size)
                val good = server.accountListsResponse!!
                server.accountListsResponse = { req -> if (req.path.startsWith("/api/review?") && req.path.endsWith("page=2")) 401 to "{}" else good(req) }
                assertEquals(OnlineError.AUTH_REQUIRED,(runCatching { online.accountLibrary.preview() }.exceptionOrNull() as OnlineException).code)
                val entries = db.metadata().accountLibraryEntries()
                assertEquals(4, entries.size)
                assertEquals(3, entries.count { it.inLibrary })
            } finally {
                online.account.setToken(null)
                online.account.setApiBase(oldBase)
                db.close()
            }
        }
    }
}
