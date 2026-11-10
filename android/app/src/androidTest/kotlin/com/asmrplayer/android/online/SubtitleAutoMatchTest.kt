package com.asmrplayer.android.online

import android.net.Uri
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.backup.*
import com.asmrplayer.android.data.*
import com.asmrplayer.android.subtitles.SubtitleRepository
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.flow.first
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class SubtitleAutoMatchTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private lateinit var db: LibraryDatabase
    private lateinit var server: OnlineFixtureServer
    private lateinit var online: OnlineRepository
    private lateinit var subtitles: SubtitleRepository
    private lateinit var imported: ImportedOnlineWork
    private val files = mutableListOf<File>()
    @Before fun setup() = runBlocking {
        check(context.packageName.endsWith(".uitest"))
        db = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        server = OnlineFixtureServer().apply { attachments = true }
        val http = HttpTransport(true)
        online = OnlineRepository(db, AsmrOneProvider(ProviderAccount(context, http), http))
        online.account.setApiBase(server.base)
        imported = online.importWork("123456")
        subtitles = SubtitleRepository(context, db, online.provider)
    }
    @After fun cleanup() = runBlocking {
        online.account.setApiBase(ProviderAccount.DEFAULT_BASE)
        server.close(); db.close(); files.forEach { it.delete() }; Unit
    }
    @Test fun existingOnlineWorkLoadsWithoutManualSelectionAndHonorsDisable() = runBlocking {
        val id = imported.versionIds.single()
        assertTrue(subtitles.sources(id).first().isEmpty())
        val loaded = subtitles.load(id)
        assertEquals(12000, loaded.original!!.cues.size)
        assertNull(db.library().subtitlePreference(id)) // Automatic does not overwrite user settings.
        val source = subtitles.sources(id).first().single()
        assertTrue(source.uri.startsWith("asmr-subtitle://"))
        assertFalse(source.uri.contains("signature"))
        assertTrue(server.requests.any { it.path.startsWith("/media/subtitle") })
        subtitles.select(loaded.preference.copy(offsetMs = 400))
        assertEquals(400L, subtitles.load(id).preference.offsetMs)
        subtitles.select(SubtitlePreference(id))
        val before = server.requests.size
        assertNull(subtitles.load(id).original)
        assertEquals(before, server.requests.size)
    }
    @Test fun oldLocalAssetsDoubleExtensionsAndAlternateFormatsLoadWithoutRescan() = runBlocking {
        val dao = db.library()
        dao.putWork(LibraryWork("local", null, "", "Synthetic work"))
        for (format in listOf("wav", "mp3")) {
            dao.putTrack(LibraryTrack(format, "local", "#1.序章.$format", format.uppercase()))
            dao.putVersion(TrackVersion(format, format, format))
        }
        val file = File.createTempFile("subtitle", ".vtt", context.cacheDir).also { files.add(it) }
        file.writeText("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nSynthetic opening\n")
        dao.putAsset(WorkAsset("asset", "local", "local-root", "WAV/#1.序章.wav.vtt", Uri.fromFile(file).toString(), "vtt"))
        for (id in listOf("wav", "mp3")) assertEquals("Synthetic opening", subtitles.load(id).original!!.cues.single().text)
        assertNotEquals(subtitles.load("wav").preference.originalId, subtitles.load("mp3").preference.originalId)
        dao.putTrack(LibraryTrack("two", "local", "#2.別の音声", "WAV"))
        dao.putVersion(TrackVersion("two", "two", "wav"))
        assertNull(subtitles.load("two").original)
    }
    @Test fun downloadedAttachmentWinsWithoutNetworkAndRemoteRemovalStopsAutoSelection() = runBlocking {
        val id = imported.versionIds.single()
        subtitles.load(id)
        val asset = online.assets(imported.workId).first().single { it.kind == "subtitle" }
        val file = File.createTempFile("offline-subtitle", ".srt", context.cacheDir).also { files.add(it) }
        file.writeBytes(server.subtitleBytes)
        db.library().putAsset(WorkAsset(asset.id, asset.workId, "online:${asset.workId}", asset.relativePath, Uri.fromFile(file).toString(), "srt"))
        server.mode = "unauthorized"
        val before = server.requests.size
        assertEquals(12000, subtitles.load(id).original!!.cues.size)
        assertEquals(before, server.requests.size)
        db.library().assetAvailable(asset.id, false)
        db.library().subtitleUriAvailable(Uri.fromFile(file).toString(), false)
        db.library().invalidateRemoteAssets(imported.workId)
        assertNull(subtitles.load(id).original)
    }
    @Test fun invalidRemoteReportsFailureAndCanRetry() = runBlocking {
        val id = imported.versionIds.single()
        server.invalidAttachment = true
        try { subtitles.load(id); fail("Invalid subtitle was silently treated as absent") } catch (_: IllegalArgumentException) { }
        server.invalidAttachment = false
        assertNotNull(subtitles.load(id).original)
    }
    @Test fun onlineSelectionAndOffsetsSurvivePortableRoundTrip() = runBlocking {
        val id = imported.versionIds.single()
        val loaded = subtitles.load(id)
        subtitles.select(loaded.preference.copy(offsetMs = 321))
        // Portable metadata requires HTTPS; loopback HTTP is only a transport test facility.
        for (table in listOf("remote_assets", "remote_media")) db.openHelper.writableDatabase.execSQL("UPDATE $table SET origin = 'https://fixture.invalid'")
        val bytes = BackupRepository(context, db).exportBytes()
        val restored = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        try {
            val backup = BackupRepository(context, restored)
            backup.apply(backup.prepare(bytes), ImportPolicy.KEEP_EXISTING)
            for (table in listOf("remote_assets", "remote_media")) restored.openHelper.writableDatabase.execSQL("UPDATE $table SET origin = ?", arrayOf(server.base))
            val result = SubtitleRepository(context, restored, online.provider).load(id)
            assertEquals(321L, result.preference.offsetMs)
            assertEquals(12000, result.original!!.cues.size)
        } finally { restored.close() }
    }
}
