package com.asmrplayer.android.online

import android.content.Context
import android.database.sqlite.SQLiteDatabase
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.download.DownloadRepository
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.library.LibraryRepository
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.security.MessageDigest

@RunWith(AndroidJUnit4::class)
class OnlineDownloadTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var db: LibraryDatabase
    private lateinit var server: OnlineFixtureServer
    private lateinit var online: OnlineRepository
    private lateinit var downloads: DownloadRepository
    private lateinit var library: LibraryRepository
    private lateinit var versionId: String
    @Before fun setup() = runBlocking {
        db = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        server = OnlineFixtureServer()
        val http = HttpTransport(debugLoopback = true)
        online = OnlineRepository(db, AsmrOneProvider(ProviderAccount(context, http), http))
        online.account.setApiBase(server.base)
        library = LibraryRepository(context, db, online::resolve)
        downloads = DownloadRepository(context, db, online.provider, schedule = {})
        downloads.awaitReady()
        versionId = online.importWork("RJ123456").versionIds.single()
    }
    @After fun cleanup() = runBlocking {
        downloads.tasks.first().forEach { downloads.remove(it.id) }
        online.account.setToken(null); online.account.setApiBase(ProviderAccount.DEFAULT_BASE)
        server.close(); db.close()
    }
    private suspend fun task(offset: Int = 0): DownloadTask {
        val id = downloads.enqueue(listOf(versionId)).single()
        if (offset > 0) {
            val part = File(context.noBackupFilesDir, "offline/$id.part"); part.parentFile!!.mkdirs()
            part.writeBytes(server.bytes.copyOf(offset))
            db.library().putDownload(db.library().download(id)!!.copy(downloadedBytes = offset.toLong(), etag = "\"fixture-v1\"", totalBytes = server.bytes.size.toLong()))
        }
        return db.library().download(id)!!
    }
    private suspend fun verifyCompleted(id: String) {
        assertEquals("COMPLETED", db.library().download(id)!!.state)
        val source = library.resolve(versionId)
        assertTrue(source.uri.startsWith("file:")); assertEquals(versionId, source.versionId)
        assertArrayEquals(server.bytes, File(android.net.Uri.parse(source.uri).path!!).readBytes())
        assertFalse(File(context.noBackupFilesDir, "offline/$id.part").exists())
    }
    @Test fun playbackCacheDoesNotBecomeLibraryMembershipUntilExplicitlyAdded() = runBlocking {
        val workId=online.importWork("123456").workId
        library.setMembership(workId,false)
        db.library().putPosition(PlaybackPosition(versionId,1234,3000))
        library.setFavorite(workId,true)
        online.importWork("123456",addToLibrary=false)
        assertFalse(library.work(workId)!!.inLibrary)
        assertTrue(library.work(workId)!!.favorite)
        assertEquals(1234L,library.position(versionId)!!.positionMs)
        online.importWork("123456",addToLibrary=true)
        assertTrue(library.work(workId)!!.inLibrary)
        online.importWork("123456",addToLibrary=false)
        assertTrue(library.work(workId)!!.inLibrary)
    }
    @Test fun resume206KeepsIdentityAndProgressWhileSwitchingOffline() = runBlocking {
        val first = online.importWork("123456")
        assertEquals(versionId, first.versionIds.single())
        db.library().putPosition(PlaybackPosition(versionId, 1234, 3000))
        library.addBookmark(versionId, 1200, "fixture")
        library.setFavorite(first.workId, true)
        val task = task(4096); downloads.runQueue(); verifyCompleted(task.id)
        assertTrue(server.requests.any { it.headers["range"] == "bytes=4096-" })
        assertEquals(1234L, library.position(versionId)!!.positionMs)
        assertEquals(1, library.bookmarks(versionId).first().size)
        assertTrue(library.work(first.workId)!!.favorite)
        assertFalse(db.library().sources(versionId).any { "signature" in it.uri })
        assertEquals(2, db.library().sources(versionId).size)
    }
    @Test fun ignoredRange200OverwritesInsteadOfAppending() = runBlocking {
        server.mode = "ignore-range"; val task = task(4096)
        downloads.runQueue(); verifyCompleted(task.id)
    }
    @Test fun validCappedRangesContinueUntilWholeFileArrives() = runBlocking {
        server.mode = "capped-range"; val task = task(4096)
        downloads.runQueue(); verifyCompleted(task.id)
        assertTrue(server.mediaRequests.get() > 3)
    }
    @Test fun invalid206NeverAddsToPartialFile() = runBlocking {
        server.mode = "bad-range"; val task = task(4096)
        downloads.runQueue()
        assertEquals("FAILED", db.library().download(task.id)!!.state)
        assertEquals(4096L, File(context.noBackupFilesDir, "offline/${task.id}.part").length())
        assertFalse(db.library().sources(versionId).any { it.kind == "local_downloaded" })
        server.mode = "normal"; downloads.resume(task.id); downloads.runQueue(); verifyCompleted(task.id)
    }
    @Test fun exact416ValidatesBeforeCompletion() = runBlocking {
        val task = task(server.bytes.size); downloads.runQueue(); verifyCompleted(task.id)
        assertTrue(server.requests.any { it.headers["range"] == "bytes=${server.bytes.size}-" })
    }
    @Test fun interruptedBodyResumesAndExpiredLinkRefreshes() = runBlocking {
        server.mode = "drop"; val task = task(); assertTrue(downloads.runQueue())
        assertEquals("QUEUED", db.library().download(task.id)!!.state)
        assertTrue(File(context.noBackupFilesDir, "offline/${task.id}.part").length() in 1 until server.bytes.size.toLong())
        server.mode = "expired"; server.mediaRequests.set(0)
        downloads.runQueue(); verifyCompleted(task.id)
        assertTrue(server.mediaRequests.get() >= 2)
        assertTrue(server.treeRequests.get() >= 3)
    }
    @Test fun pauseIsDurableAndDoesNotPublishPartialSource() = runBlocking {
        server.mode = "slow"; val task = task()
        val transfer = launch(Dispatchers.IO) { downloads.runQueue() }
        withTimeout(10_000) { downloads.tasks.first { list -> list.any { it.id == task.id && it.downloadedBytes > 0 } } }
        downloads.pause(task.id); transfer.join()
        assertEquals("PAUSED", db.library().download(task.id)!!.state)
        assertFalse(db.library().sources(versionId).any { it.kind == "local_downloaded" })
        server.mode = "normal"; downloads.resume(task.id); downloads.runQueue(); verifyCompleted(task.id)
    }
    @Test fun renamedFileReconcilesWithoutNetworkAndRemovalKeepsHistory() = runBlocking {
        val task = task()
        val final = File(context.noBackupFilesDir, "offline/${task.id}.media"); final.parentFile!!.mkdirs(); final.writeBytes(server.bytes)
        val digest = MessageDigest.getInstance("SHA-256").digest(server.bytes).joinToString("") { "%02x".format(it) }
        db.library().putDownload(task.copy(state = "VERIFYING", totalBytes = server.bytes.size.toLong(), downloadedBytes = server.bytes.size.toLong(), sha256 = digest))
        val recovered = DownloadRepository(context, db, online.provider, schedule = {})
        recovered.awaitReady(); verifyCompleted(task.id)
        db.library().putPosition(PlaybackPosition(versionId, 500, 3000))
        recovered.remove(task.id)
        assertFalse(final.exists()); assertEquals(500L, library.position(versionId)!!.positionMs)
        assertTrue(library.resolve(versionId).uri.startsWith("http://127.0.0.1"))
    }
    @Test fun accountEncryptionOriginIsolationAndApiErrors() = runBlocking {
        online.account.setToken("fixture-secret-token")
        val prefs = context.getSharedPreferences("online-account", Context.MODE_PRIVATE)
        assertFalse(prefs.all.values.any { it.toString().contains("fixture-secret-token") })
        val page = online.browse()
        assertEquals(1, page.items.size)
        assertTrue(server.requests.any { it.headers["authorization"] == "Bearer fixture-secret-token" })
        assertTrue(server.requests.filter { it.path.startsWith("/media") }.none { it.headers.containsKey("authorization") })
        OnlineFixtureServer().use { other ->
            server.redirect = "${other.base}/api/works"
            online.browse()
            assertTrue(other.requests.none { it.headers.containsKey("authorization") })
            server.redirect = null
            online.account.setApiBase(other.base); assertFalse(online.account.state.value.hasToken)
            online.account.setApiBase(server.base); assertTrue(online.account.state.value.hasToken)
        }
        server.mode = "unauthorized"
        try { online.browse(); fail("401 accepted") } catch (e: OnlineException) { assertEquals(OnlineError.AUTH_REQUIRED, e.code) }
    }
    @Test fun migrationOneToCurrentPreservesExistingRows() = runBlocking {
        val name = "migration-${java.util.UUID.randomUUID()}.db"
        val path = context.getDatabasePath(name); path.parentFile!!.mkdirs()
        try {
            val schema = JSONObject(instrumentation.context.assets.open("com.asmrplayer.android.data.LibraryDatabase/1.json").bufferedReader().use { it.readText() }).getJSONObject("database")
            SQLiteDatabase.openOrCreateDatabase(path, null).use { old ->
                val entities = schema.getJSONArray("entities")
                for (i in 0 until entities.length()) {
                    val entity = entities.getJSONObject(i); val table = entity.getString("tableName")
                    old.execSQL(entity.getString("createSql").replace("\${TABLE_NAME}", table))
                    val indices = entity.optJSONArray("indices") ?: org.json.JSONArray()
                    for (j in 0 until indices.length()) old.execSQL(indices.getJSONObject(j).getString("createSql").replace("\${TABLE_NAME}", table))
                }
                val setup = schema.getJSONArray("setupQueries"); for (i in 0 until setup.length()) old.execSQL(setup.getString(i))
                old.execSQL("INSERT INTO works(id,rootId,relativePath,title,code,coverUri,favorite,rating) VALUES('preserved',NULL,'','Fixture',NULL,NULL,1,'UNRATED')")
                old.version = 1
            }
            val migrated = Room.databaseBuilder(context, LibraryDatabase::class.java, name).addMigrations(MIGRATION_1_2, MIGRATION_2_3, MIGRATION_3_4, MIGRATION_4_5, com.asmrplayer.android.sync.MIGRATION_5_6, MIGRATION_6_7, MIGRATION_7_8).build()
            try { assertTrue(migrated.library().work("preserved")!!.favorite); assertTrue(migrated.library().downloadTasks().isEmpty()) }
            finally { migrated.close() }
        } finally { context.deleteDatabase(name) }
    }
    @Test fun corruptRenamedFileIsNotPublishedAndCanBeRetried() = runBlocking {
        val task = task()
        val final = File(context.noBackupFilesDir, "offline/${task.id}.media"); final.parentFile!!.mkdirs(); final.writeBytes(byteArrayOf(1, 2, 3))
        db.library().putDownload(task.copy(state = "VERIFYING", totalBytes = server.bytes.size.toLong(), sha256 = "wrong"))
        val recovered = DownloadRepository(context, db, online.provider, schedule = {})
        recovered.awaitReady()
        assertEquals("FAILED", db.library().download(task.id)!!.state)
        assertEquals("INTEGRITY", db.library().download(task.id)!!.errorCode)
        assertFalse(db.library().sources(versionId).any { it.kind == "local_downloaded" && it.available })
        recovered.resume(task.id); recovered.runQueue(); verifyCompleted(task.id)
    }
}
