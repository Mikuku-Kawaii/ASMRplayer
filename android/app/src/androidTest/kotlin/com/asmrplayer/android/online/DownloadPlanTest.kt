package com.asmrplayer.android.online

import androidx.room.Room
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.download.*
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.flow.first
import org.junit.*
import org.junit.Assert.*
import java.io.File

class DownloadPlanTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private lateinit var db: LibraryDatabase
    private lateinit var server: OnlineFixtureServer
    private lateinit var online: OnlineRepository
    private lateinit var downloads: DownloadRepository
    private lateinit var plans: DownloadPlanRepository
    @Before fun setup() = runBlocking {
        db = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        server = OnlineFixtureServer().apply { attachments = true; documents = true }
        val http = HttpTransport(debugLoopback = true)
        online = OnlineRepository(db, AsmrOneProvider(ProviderAccount(context, http), http))
        online.account.setApiBase(server.base)
        downloads = DownloadRepository(context, db, online.provider, schedule = {})
        downloads.awaitReady()
        plans = DownloadPlanRepository(db, online, downloads)
    }
    @After fun cleanup() = runBlocking {
        db.library().downloadTasks().forEach { downloads.remove(it.id) }
        online.account.setApiBase(ProviderAccount.DEFAULT_BASE)
        server.close(); db.close()
    }
    @Test fun previewDoesNotQueueAndSelectedAudioAndDocumentsDownloadOnce() = runBlocking {
        val preview = plans.prepare(origin=server.base, sourceId="123456")
        assertEquals(5, preview.items.size) // audio, subtitle, picture, document, cover
        assertTrue(db.library().downloadTasks().isEmpty())
        assertFalse(db.library().work(preview.workId)!!.inLibrary)
        assertEquals(0, server.mediaRequests.get())
        val chosen = preview.items.filter { it.format in setOf("wav", "txt") }.map { it.id }.toSet()
        assertEquals(2, plans.enqueue(preview, chosen))
        assertEquals(0, plans.enqueue(preview, chosen))
        downloads.runQueue()
        assertEquals(2, db.library().downloadTasks().count { it.state == "COMPLETED" })
        val doc = preview.items.single { it.format == "txt" }
        val file = db.library().asset(doc.id)!!
        assertArrayEquals(server.documentBytes, File(android.net.Uri.parse(file.uri).path!!).readBytes())
        assertEquals(1, db.library().observeVersions(preview.workId).first().size)
        val again = plans.prepare(workId=preview.workId)
        assertEquals(2, again.items.count { it.downloaded })
        assertTrue(again.items.filter { it.id !in chosen }.all { it.selectable })
        assertTrue(db.library().work(preview.workId)!!.inLibrary)
    }
    @Test fun queuedAndPausedAreExcludedFailedCanRetryAndSourceChangeCannotSubmit() = runBlocking {
        val preview = plans.prepare(origin=server.base, sourceId="123456")
        val audio = preview.items.single { !it.attachment }
        val taskId = downloads.enqueue(listOf(audio.id)).single()
        downloads.pause(taskId)
        assertFalse(plans.prepare(workId=preview.workId).items.single { it.id == audio.id }.selectable)
        db.library().downloadState(taskId, "FAILED", "NETWORK")
        assertTrue(plans.prepare(workId=preview.workId).items.single { it.id == audio.id }.selectable)
        online.account.setApiBase(ProviderAccount.DEFAULT_BASE)
        try { plans.enqueue(preview, setOf(audio.id)); fail("Must reject changed source") } catch (_: IllegalStateException) { }
        assertEquals("FAILED", db.library().download(taskId)!!.state)
    }
    @Test fun mixedSelectionRollsBackIfAnyTargetIsInvalid() = runBlocking {
        val preview = plans.prepare(origin=server.base, sourceId="123456")
        try { downloads.enqueueSelection(listOf(preview.items.single { !it.attachment }.id), listOf("missing")); fail("Must reject missing asset") }
        catch (_: IllegalArgumentException) { }
        assertTrue(db.library().downloadTasks().isEmpty())
    }
    @Test fun invalidDocumentCanBeSelectedAgainAndRetriesFromCleanFile() = runBlocking {
        val preview = plans.prepare(origin=server.base, sourceId="123456")
        val doc = preview.items.single { it.format == "txt" }
        server.invalidAttachment = true
        plans.enqueue(preview, setOf(doc.id)); downloads.runQueue()
        assertEquals("INVALID_CONTENT", db.library().downloadTasks().single().errorCode)
        server.invalidAttachment = false
        plans.enqueue(preview, setOf(doc.id)); downloads.runQueue()
        assertEquals("COMPLETED", db.library().downloadTasks().single().state)
        assertArrayEquals(server.documentBytes, File(android.net.Uri.parse(db.library().asset(doc.id)!!.uri).path!!).readBytes())
    }
}
