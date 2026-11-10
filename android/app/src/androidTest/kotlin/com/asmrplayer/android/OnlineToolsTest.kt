package com.asmrplayer.android

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.online.OnlineFixtureServer
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.net.InetAddress
import java.net.ServerSocket
import java.net.SocketException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

@RunWith(AndroidJUnit4::class)
class OnlineToolsTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun main(action: () -> Unit) = instrumentation.runOnMainSync(action)
    private var step=0
    private suspend fun wait(tools: OnlineTools, predicate: (OnlineScreenState) -> Boolean): OnlineScreenState {
        val n=++step
        return try {withTimeout(15000) {tools.state.first(predicate)}} catch(e:TimeoutCancellationException) {
            val state=tools.state.value
            throw AssertionError("Step $n: query=${state.query}, error=${state.error}, action=${state.actionError}, account=${state.accountError}, notice=${state.notice}, tasks=${state.tasks.map{it.state}}, detail=${state.detailError}, working=${state.working}",e)
        }
    }

    @Test fun browseCredentialsImportPlaybackDownloadsAndSubtitleBinding() = runBlocking {
        val scenario = androidx.test.core.app.ActivityScenario.launch(MainActivity::class.java)
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        val oldFilter = backend.library.preferences.first()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        // Exercise UI/repository execution deterministically; OEM JobScheduler timing is a separate device check.
        val worker=scope.launch(Dispatchers.IO) {while(isActive){backend.downloads.runQueue();delay(100)}}
        var played = emptyList<String>()
        var started: String? = null
        val tools = OnlineTools(context, scope) { ids, _, start -> played = ids; started = start }
        OnlineFixtureServer().use { server ->
            server.attachments = true
            server.secondTrack = true
            try {
                backend.library.setContentFilter(false, true)
                main { tools.saveOrigin(server.base) }
                wait(tools) { !it.accountSaving && it.account.apiBase == server.base }
                main { tools.browsing(true) }
                val page = wait(tools) { !it.loading && it.page != null }.page!!
                assertEquals(listOf("123456"), page.items.map { it.sourceId })
                main { tools.search("Voice", "dl_count", true) }
                wait(tools) { !it.loading && it.query == "Voice" && it.page != null }
                assertTrue(server.requests.any { it.path.contains("order=dl_count&sort=asc") })
                server.mode = "unauthorized"
                main { tools.search("locked") }
                assertTrue(wait(tools) { !it.loading && it.error != null }.error!!.contains("Token"))
                assertNull(tools.state.value.page)
                server.mode = "normal"
                var cleared = false
                main { tools.saveToken(server.base, "synthetic-ui-token") { cleared = true } }
                wait(tools) { !it.accountSaving && it.account.hasToken }
                assertTrue(cleared)
                main { tools.browsing(false); tools.loadDetail(server.base, "123456") }
                wait(tools) { it.detail != null && !it.detailLoading }
                var completed = false
                main { tools.act("play", listOf("WAV/02.wav", "WAV/01.wav"), startPath = "WAV/01.wav") { completed = true } }
                wait(tools) { !it.working && it.notice != null }
                val work = backend.library.works.first().single { it.title == "Synthetic online work" && it.id ==
                    com.asmrplayer.android.library.LibraryIndex.id("asmr_one", server.base, "123456") }
                val versions = backend.library.versions(work.id).first().associateBy { it.title }
                val version = versions.getValue("01.wav")
                assertEquals(listOf(versions.getValue("02.wav").id, version.id), played)
                assertEquals(version.id, started)
                main { assertTrue(completed) }
                assertFalse(played.any { it.contains("http") })
                main { tools.act("favorite-track",listOf("WAV/01.wav")) }
                wait(tools) { !it.working && it.notice == "已加入喜欢。" }
                assertTrue(backend.library.collections.favoriteVersions().first().any{it.versionId==version.id})
                main { tools.act("favorite") }; wait(tools) { !it.working && it.notice == "已加入喜欢。" }
                server.mode = "slow"
                main { tools.act("download", listOf("WAV/01.wav")) }
                val downloading = wait(tools) { it.tasks.any { task -> task.versionId == version.id && task.state == "DOWNLOADING" } }
                val task = downloading.tasks.single { it.versionId == version.id }
                main { tools.taskAction(task.id, "pause") }
                wait(tools) { it.tasks.any { t -> t.id == task.id && t.state == "PAUSED" } && task.id !in it.taskBusy }
                main { tools.taskAction(task.id, "resume") }
                wait(tools) { it.tasks.any { t -> t.id == task.id && t.state == "COMPLETED" } }
                main { tools.act("assets", listOf("WAV/01.srt")) }
                val subtitle = wait(tools) { it.tasks.any { t -> t.assetId != null && t.state == "COMPLETED" } }.tasks.first { it.assetId != null && it.state == "COMPLETED" }
                val library = LibraryTools(context, scope)
                main { library.selectVersion(version.id, work.id) }
                val asset = withTimeout(10000) { library.state.first { it.assets.any { a -> a.id == subtitle.assetId } } }.assets.single { it.id == subtitle.assetId }
                main { library.attachSubtitle(version.id, false, android.net.Uri.parse(asset.uri), asset) }
                withTimeout(10000) { library.state.first { !it.subtitleSaving && it.subtitles?.original != null } }
                assertTrue(backend.subtitles.load(version.id).original!!.cues.isNotEmpty())
                main { tools.taskAction(task.id, "remove") }
                wait(tools) { it.tasks.none { t -> t.id == task.id } }
                assertTrue(backend.library.work(work.id)!!.favorite)
                main { tools.loadDetail("https://old.invalid", "123456") }
                assertTrue(wait(tools) { !it.detailLoading && it.detailError != null }.detailError!!.contains("来源已切换"))
                assertNull(tools.state.value.detail)
            } finally {
                main { tools.browsing(false); tools.leaveDetail() }
                backend.online.account.setToken(null)
                worker.cancelAndJoin()
                tools.state.value.tasks.forEach { backend.downloads.remove(it.id) }
                scope.cancel()
                backend.online.account.setApiBase(oldBase)
                backend.library.setContentFilter(oldFilter.includeNsfw, oldFilter.includeUnrated)
                scenario.close()
            }
        }
    }

    @Test fun lateSearchAndExitedDetailsCannotPublishOldResults() = runBlocking {
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        val tools = OnlineTools(context, scope) { _, _, _ -> }
        val socket = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
        val gate = CountDownLatch(1)
        val entered = CountDownLatch(1)
        val worker = thread(isDaemon = true) {
            try { while (!socket.isClosed) {
                val client = socket.accept()
                thread(isDaemon = true) { client.use { peer ->
                    try {
                        val reader = peer.getInputStream().bufferedReader()
                        val path = reader.readLine().split(' ')[1]
                        while (!reader.readLine().isNullOrEmpty()) { }
                        val old = path.contains("old")
                        if (old) { entered.countDown(); gate.await(10, TimeUnit.SECONDS) }
                        val title = if (old) "old" else "new"
                        val body = """{"works":[{"id":123456,"title":"$title","age_category_string":"general"}],"pagination":{"currentPage":1,"pageSize":20,"totalCount":1}}""".toByteArray()
                        peer.getOutputStream().apply { write("HTTP/1.1 200 OK\r\nContent-Length: ${body.size}\r\nConnection: close\r\n\r\n".toByteArray()); write(body); flush() }
                    } catch (_: java.io.IOException) { }
                } }
            } } catch (_: SocketException) { }
        }
        try {
            backend.online.account.setApiBase("http://127.0.0.1:${socket.localPort}")
            wait(tools) { it.account.apiBase.endsWith(":${socket.localPort}") }
            main { tools.search("old") }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            main { tools.search("new") }
            wait(tools) { !it.loading && it.page?.items?.firstOrNull()?.title == "new" }
            gate.countDown(); delay(300)
            assertEquals("new", tools.state.value.page!!.items.single().title)
            main { tools.loadDetail(tools.state.value.account.apiBase, "123456"); tools.leaveDetail() }
            delay(200)
            assertNull(tools.state.value.detail); assertNull(tools.state.value.detailId)
        } finally {
            gate.countDown(); socket.close(); worker.join(1000); scope.cancel()
            backend.online.account.setApiBase(oldBase)
        }
    }
}
