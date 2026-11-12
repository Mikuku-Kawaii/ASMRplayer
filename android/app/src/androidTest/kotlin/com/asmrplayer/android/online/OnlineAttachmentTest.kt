package com.asmrplayer.android.online

import android.database.sqlite.SQLiteDatabase
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.download.DownloadRepository
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.subtitles.SubtitleRepository
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.security.MessageDigest

@RunWith(AndroidJUnit4::class)
class OnlineAttachmentTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var db: LibraryDatabase
    private lateinit var server: OnlineFixtureServer
    private lateinit var online: OnlineRepository
    private lateinit var downloads: DownloadRepository
    private lateinit var subtitles: SubtitleRepository
    private lateinit var imported: ImportedOnlineWork
    private lateinit var subtitle: RemoteAsset
    private lateinit var picture: RemoteAsset
    @Before fun setup() = runBlocking {
        db = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        server = OnlineFixtureServer().apply { attachments = true }
        val http = HttpTransport(debugLoopback = true)
        online = OnlineRepository(db, AsmrOneProvider(ProviderAccount(context, http), http))
        online.account.setApiBase(server.base)
        downloads = DownloadRepository(context, db, online.provider, schedule = {})
        downloads.awaitReady()
        subtitles = SubtitleRepository(context, db)
        imported = online.importWork("123456")
        val assets = online.assets(imported.workId).first()
        subtitle = assets.single { it.kind == "subtitle" }
        picture = assets.single { it.kind == "image" }
    }
    @After fun cleanup() = runBlocking {
        downloads.tasks.first().forEach { downloads.remove(it.id) }
        online.account.setToken(null); online.account.setApiBase(ProviderAccount.DEFAULT_BASE)
        server.close(); db.close()
    }
    private suspend fun downloadAll(): List<String> {
        val ids = downloads.enqueueAssets(imported.assetIds) + downloads.cacheCover(imported.workId)
        downloads.runQueue()
        assertTrue(ids.all { db.library().download(it)?.state == "COMPLETED" })
        return ids
    }
    @Test fun videoExtensionOverridesProviderAudioClassification() {
        val files = AsmrJson.tree(
            """[{"title":"bonus.MP4","type":"audio"},{"title":"track.m4a","type":"audio"}]""",
            "123456",
        )
        assertEquals("other", files[0].kind)
        assertEquals("audio", files[1].kind)
    }

    @Test fun offlineAttachmentsAndCoverKeepStableIdsAcrossMetadataRefresh() = runBlocking {
        val ids = downloadAll()
        assertEquals(3, ids.size)
        assertTrue(downloads.tasks.first().all { it.versionId == null && it.assetId != null })
        assertEquals(1, db.library().sources(imported.versionIds.single()).size) // no fake audio sources
        val cover = db.library().work(imported.workId)!!.coverUri!!
        assertTrue(cover.startsWith("file:"))
        assertEquals(cover, db.library().observeWorks().first().single().coverUri)
        db.library().favorite(imported.workId, true)
        val refreshed = online.importWork("RJ123456")
        assertEquals(imported.assetIds, refreshed.assetIds); assertEquals(imported.coverAssetId, refreshed.coverAssetId)
        assertEquals(cover, db.library().work(imported.workId)!!.coverUri)
        assertTrue(db.library().work(imported.workId)!!.favorite)
        server.mode = "unauthorized"
        val before = server.requests.size
        val image = db.library().asset(picture.id)!!
        assertArrayEquals(server.imageBytes, context.contentResolver.openInputStream(Uri.parse(image.uri))!!.use { it.readBytes() })
        val bitmap = BitmapFactory.decodeFile(Uri.parse(cover).path); assertNotNull(bitmap); bitmap.recycle()
        val source = subtitles.attachDownloaded(imported.versionIds.single(), subtitle.id)
        subtitles.select(SubtitlePreference(source.versionId, originalId = source.id, offsetMs = 250))
        assertEquals(12_000, subtitles.load(source.versionId).original!!.cues.size)
        assertEquals(before, server.requests.size)
    }
    @Test fun storageAccountingAndCoverCleanupLeaveMediaAndHistoryUntouched() = runBlocking {
        downloadAll()
        downloads.enqueue(imported.versionIds); downloads.runQueue()
        db.library().favorite(imported.workId, true)
        db.library().putPosition(PlaybackPosition(imported.versionIds.single(), 123, 4000))
        val before = downloads.storageUsage()
        assertEquals(server.bytes.size.toLong(), before.bytesFor("audio"))
        assertEquals(server.imageBytes.size.toLong(), before.bytesFor("cover"))
        assertEquals(server.subtitleBytes.size.toLong(), before.bytesFor("subtitle"))
        assertEquals(server.imageBytes.size.toLong(), before.bytesFor("image"))
        val cleared = downloads.clearCompletedCoverCache()
        assertEquals(1, cleared.removedIds.size); assertTrue(cleared.failedIds.isEmpty())
        val after = downloads.storageUsage()
        assertEquals(0L, after.bytesFor("cover"))
        assertEquals(before.totalBytes - before.bytesFor("cover"), after.totalBytes)
        assertEquals(before.bytesFor("audio"), after.bytesFor("audio"))
        assertEquals(123L, db.library().position(imported.versionIds.single())!!.positionMs)
        assertTrue(db.library().work(imported.workId)!!.favorite)
        assertTrue(db.library().asset(subtitle.id)!!.available)
        assertTrue(downloads.clearCompletedCoverCache().removedIds.isEmpty())
    }
    @Test fun subtitleRemovalAndRedownloadPreserveBindingAndOffsets() = runBlocking {
        val id = downloads.enqueueAssets(listOf(subtitle.id)).single(); downloads.runQueue()
        val versionId = imported.versionIds.single()
        val source = subtitles.attachDownloaded(versionId, subtitle.id)
        assertEquals(source.id, subtitles.attachDownloaded(versionId, subtitle.id).id)
        subtitles.select(SubtitlePreference(versionId, originalId = source.id, translationId = source.id, offsetMs = 111, translationOffsetMs = 222))
        db.library().putPosition(PlaybackPosition(versionId, 500, 3000))
        downloads.remove(id)
        assertFalse(db.library().asset(subtitle.id)!!.available)
        assertFalse(subtitles.sources(versionId).first().single().available)
        try { subtitles.load(versionId); fail("removed file is usable") } catch (_: IllegalStateException) { }
        downloads.resume(id); downloads.runQueue()
        // Discovery can also register the remote subtitle; the explicit binding keeps its identity.
        assertTrue(subtitles.sources(versionId).first().single { it.id == source.id }.available)
        assertEquals(111L, subtitles.load(versionId).preference.offsetMs)
        assertEquals(222L, subtitles.load(versionId).preference.translationOffsetMs)
        assertEquals(500L, db.library().position(versionId)!!.positionMs)
        assertEquals(1, subtitles.sources(versionId).first().count { it.manual })
    }
    @Test fun downloadedSubtitleCannotBindAcrossWorksOrBeforeCompletion() = runBlocking {
        try { subtitles.attachDownloaded(imported.versionIds.single(), subtitle.id); fail("undownloaded bound") } catch (_: IllegalArgumentException) { }
        downloads.enqueueAssets(listOf(subtitle.id)); downloads.runQueue()
        db.library().putWork(LibraryWork("other", null, "", "Other"))
        db.library().putTrack(LibraryTrack("track-other", "other", "Audio", ""))
        db.library().putVersion(TrackVersion("version-other", "track-other", "wav"))
        try { subtitles.attachDownloaded("version-other", subtitle.id); fail("cross-work bound") } catch (_: IllegalArgumentException) { }
        try { subtitles.attachDownloaded(imported.versionIds.single(), picture.id); fail("image bound") } catch (_: IllegalArgumentException) { }
    }
    @Test fun pausedAttachmentIsNotPublishedAndResumesWithFreshAddress() = runBlocking {
        server.mode = "slow"
        val id = downloads.enqueueAssets(listOf(subtitle.id)).single()
        val transfer = launch(Dispatchers.IO) { downloads.runQueue() }
        withTimeout(10_000) { downloads.tasks.first { items -> items.any { it.id == id && it.downloadedBytes > 0 } } }
        downloads.pause(id); transfer.join()
        assertEquals("PAUSED", db.library().download(id)!!.state)
        assertNull(db.library().asset(subtitle.id))
        val offset = File(context.noBackupFilesDir, "offline/$id.part").length()
        server.mode = "expired"; server.mediaRequests.set(0)
        downloads.resume(id); downloads.runQueue()
        assertEquals("COMPLETED", db.library().download(id)!!.state)
        assertTrue(server.requests.any { it.path.startsWith("/media/subtitle") && it.headers["range"] == "bytes=$offset-" })
        assertTrue(server.mediaRequests.get() >= 2)
    }
    @Test fun htmlDisguisedAsSubtitleImageOrCoverIsRejectedAndRetryWorks() = runBlocking {
        server.invalidAttachment = true
        val ids = downloads.enqueueAssets(imported.assetIds) + downloads.cacheCover(imported.workId)
        downloads.runQueue()
        ids.forEach { assertEquals("INVALID_CONTENT", db.library().download(it)!!.errorCode) }
        assertTrue(db.library().observeAssets(imported.workId).first().isEmpty())
        assertFalse(db.library().work(imported.workId)!!.coverUri!!.startsWith("file:"))
        server.invalidAttachment = false
        ids.forEach { downloads.resume(it) }; downloads.runQueue()
        assertTrue(ids.all { db.library().download(it)!!.state == "COMPLETED" })
    }
    @Test fun renamedCoverRecoversOfflineAndMissingCacheFallsBackWithoutDeletingMetadata() = runBlocking {
        val id = downloads.cacheCover(imported.workId)
        val final = File(context.noBackupFilesDir, "offline/$id.media").apply { parentFile!!.mkdirs(); writeBytes(server.imageBytes) }
        val digest = MessageDigest.getInstance("SHA-256").digest(server.imageBytes).joinToString("") { "%02x".format(it) }
        db.library().putDownload(db.library().download(id)!!.copy(state = "VERIFYING", totalBytes = final.length(), sha256 = digest))
        server.mode = "unauthorized"
        val recovered = DownloadRepository(context, db, online.provider, schedule = {})
        recovered.awaitReady()
        assertEquals("COMPLETED", db.library().download(id)!!.state)
        assertTrue(db.library().work(imported.workId)!!.coverUri!!.startsWith("file:"))
        assertTrue(final.delete())
        val missing = DownloadRepository(context, db, online.provider, schedule = {})
        missing.awaitReady()
        assertEquals("MISSING_FILE", db.library().download(id)!!.errorCode)
        assertTrue(db.library().work(imported.workId)!!.coverUri!!.startsWith(server.base))
        assertNotNull(online.assets(imported.workId).first().single { it.kind == "cover" })
        server.mode = "normal"; missing.resume(id); missing.runQueue()
        missing.remove(id)
        assertTrue(db.library().work(imported.workId)!!.coverUri!!.startsWith(server.base))
    }
    @Test fun remoteRemovalRetainsCompletedLocalAssets() = runBlocking {
        downloadAll()
        server.hideAttachments = true; online.importWork("123456")
        assertTrue(online.assets(imported.workId).first().none { it.available })
        assertTrue(db.library().observeAssets(imported.workId).first().all { it.available })
        assertTrue(db.library().work(imported.workId)!!.coverUri!!.startsWith("file:"))
        assertNotNull(subtitles.attachDownloaded(imported.versionIds.single(), subtitle.id))
    }
    @Test fun migrationTwoToThreePreservesDownloadStateAndStableAudioIdentity() = runBlocking {
        val name = "asset-migration-${java.util.UUID.randomUUID()}.db"
        val path = context.getDatabasePath(name); path.parentFile!!.mkdirs()
        try {
            val schema = JSONObject(instrumentation.context.assets.open("com.asmrplayer.android.data.LibraryDatabase/2.json").bufferedReader().use { it.readText() }).getJSONObject("database")
            SQLiteDatabase.openOrCreateDatabase(path, null).use { old ->
                val entities = schema.getJSONArray("entities")
                for (i in 0 until entities.length()) {
                    val entity = entities.getJSONObject(i); val table = entity.getString("tableName")
                    old.execSQL(entity.getString("createSql").replace("\${TABLE_NAME}", table))
                    val indices = entity.optJSONArray("indices") ?: JSONArray()
                    for (j in 0 until indices.length()) old.execSQL(indices.getJSONObject(j).getString("createSql").replace("\${TABLE_NAME}", table))
                }
                val setup = schema.getJSONArray("setupQueries"); for (i in 0 until setup.length()) old.execSQL(setup.getString(i))
                old.execSQL("INSERT INTO download_tasks VALUES('audio-task','audio-version','PAUSED',123,456,'etag','modified','hash',NULL,2,10,20)")
                old.version = 2
            }
            val migrated = Room.databaseBuilder(context, LibraryDatabase::class.java, name).addMigrations(MIGRATION_2_3, MIGRATION_3_4, MIGRATION_4_5, com.asmrplayer.android.sync.MIGRATION_5_6, MIGRATION_6_7, MIGRATION_7_8).build()
            try {
                val task = migrated.library().download("audio-task")!!
                assertEquals("audio-version", task.versionId); assertNull(task.assetId)
                assertEquals("PAUSED", task.state); assertEquals(123L, task.downloadedBytes)
                assertEquals("etag", task.etag); assertEquals(2, task.attempts)
                migrated.library().putDownload(DownloadTask(LibraryIndex.id("test-asset"), null, assetId = "attachment"))
                assertEquals(2, migrated.library().downloadTasks().size)
            } finally { migrated.close() }
        } finally { context.deleteDatabase(name) }
    }
}
