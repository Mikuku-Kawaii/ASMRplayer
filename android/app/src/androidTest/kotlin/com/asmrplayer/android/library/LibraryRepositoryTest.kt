package com.asmrplayer.android.library

import android.net.Uri
import android.provider.DocumentsContract
import android.os.Bundle
import android.content.Intent
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.subtitles.SubtitleRepository
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class LibraryRepositoryTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val provider = Uri.parse("content://com.asmrplayer.android.test.fixture")
    private lateinit var db: LibraryDatabase
    private lateinit var repo: LibraryRepository
    private fun mode(value: String) { context.contentResolver.call(provider, "mode", value, null) }
    private fun root(name: String = "rootA") = LibraryRoot("fixture-root", DocumentsContract.buildTreeDocumentUri(provider.authority!!, name).toString(), "Fixture", "WORK")
    @Before fun setup() { db = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build(); repo = LibraryRepository(context, db); mode("normal") }
    @After fun cleanup() { mode("normal"); db.close() }

    @Test fun scanRescanMissingAndRelocationPreserveVersionHistory() = runBlocking {
        db.library().putRoot(root())
        repo.scan(root().id)
        val work = repo.works.first().single()
        val versions = repo.versions(work.id).first()
        assertEquals(3, versions.size)
        val version = versions.single { it.groupPath == "WAV" && it.title == "02" }
        repo.setFavorite(work.id, true)
        repo.setRating(work.id, "NSFW")
        db.library().putPosition(PlaybackPosition(version.id, 1200, 4000))
        repo.addBookmark(version.id, 1000, "fixture")
        mode("missing"); repo.scan(root().id)
        assertFalse(repo.versions(work.id).first().single { it.id == version.id }.available)
        assertEquals(1200L, repo.position(version.id)!!.positionMs)
        assertEquals(1, repo.bookmarks(version.id).first().size)
        assertTrue(repo.works.first().single().favorite)
        assertTrue(repo.visibleWorks.first().isEmpty())
        repo.setContentFilter(true, true); assertEquals(1, repo.visibleWorks.first().size)
        mode("normal")
        db.library().putRoot(root("rootB")) // Same root identity, new granted provider tree.
        repo.scan(root().id)
        assertEquals(versions.map { it.id }.toSet(), repo.versions(work.id).first().map { it.id }.toSet())
        assertTrue(repo.resolve(version.id).uri.contains("rootB"))
        assertEquals(1200L, repo.position(version.id)!!.positionMs)
    }

    @Test fun partialAndCancelledScansNeverInvalidateExistingSources() = runBlocking {
        db.library().putRoot(root()); repo.scan(root().id)
        mode("loading")
        try { repo.scan(root().id); fail("Incomplete provider response committed") } catch (_: java.io.IOException) { }
        assertTrue(db.library().sourcesForRoot(root().id).all { it.available })
        mode("slow")
        val scan = launch(Dispatchers.IO) { repo.scan(root().id) }
        withTimeout(3000) { repo.scanStates.first { it[root().id]?.running == true } }
        repo.cancelScan(root().id); scan.join()
        assertTrue(db.library().sourcesForRoot(root().id).all { it.available })
        mode("denied")
        try { repo.scan(root().id); fail("Revoked provider accepted") } catch (_: SecurityException) { }
        assertTrue(db.library().sourcesForRoot(root().id).none { it.available })
        assertEquals(1, repo.works.first().size)
    }

    @Test fun subtitleSelectionsOffsetsAndVersionBoundaryPersist() = runBlocking {
        db.library().putRoot(root()); repo.scan(root().id)
        val work = repo.works.first().single()
        val versions = repo.versions(work.id).first()
        val wav = versions.single { it.groupPath == "WAV" && it.title == "01" }
        val subs = SubtitleRepository(context, db)
        val source = subs.sources(wav.id).first().single()
        subs.select(SubtitlePreference(wav.id, source.id, offsetMs = 500))
        assertTrue(subs.load(wav.id).active(200).first.isEmpty())
        assertEquals("原文\n译文", subs.load(wav.id).active(600).first.single().text)
        val other = versions.first { it.id != wav.id }
        try { subs.select(SubtitlePreference(other.id, source.id)); fail("Cross-version subtitle accepted") } catch (_: IllegalArgumentException) { }
        val translation = repo.assets(work.id).first().single { it.relativePath == "字幕/01.zh.srt" }
        val attached = subs.attach(wav.id, Uri.parse(translation.uri), "01.zh.srt", persistGrant = false)
        subs.select(SubtitlePreference(wav.id, source.id, attached.id, offsetMs = 500))
        db.library().putRoot(root("relocated-subtitles")); repo.scan(root().id)
        assertTrue(subs.sources(wav.id).first().single { it.id == attached.id }.uri.contains("relocated-subtitles"))
        assertEquals(1, subs.load(wav.id).active(600).second.size)
    }

    @Test fun persistentGrantAndReauthorizationUseSameRootIdentity() = runBlocking {
        val trees = listOf("grantA", "grantB").map { DocumentsContract.buildTreeDocumentUri(provider.authority!!, it) }
        try {
            trees.forEach { uri -> context.contentResolver.call(provider, "grant", context.packageName, Bundle().apply { putString("uri", uri.toString()) }) }
            val original = repo.authorizeRoot(trees[0], RootMode.WORK)
            assertTrue(context.contentResolver.persistedUriPermissions.any { it.uri == trees[0] && it.isReadPermission })
            repo.scan(original.id)
            val work = repo.works.first().single()
            val ids = repo.versions(work.id).first().map { it.id }
            repo.setFavorite(work.id, true)
            val moved = repo.authorizeRoot(trees[1], RootMode.WORK, original.id)
            assertEquals(original.id, moved.id)
            repo.scan(moved.id)
            assertEquals(ids, repo.versions(work.id).first().map { it.id })
            assertTrue(repo.work(work.id)!!.favorite)
        } finally {
            trees.forEach { uri ->
                try { context.contentResolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) } catch (_: SecurityException) { }
            }
        }
    }

    @Test fun databaseReopenPreservesSchemaAndUserRecords() = runBlocking {
        val name = "p1-test-${java.util.UUID.randomUUID()}.db"
        try {
            val disk = Room.databaseBuilder(context, LibraryDatabase::class.java, name).build()
            try {
                disk.library().putWork(LibraryWork("work", null, "", "Synthetic", favorite = true))
                disk.library().putTrack(LibraryTrack("track", "work", "Track", ""))
                disk.library().putVersion(TrackVersion("version", "track", "wav"))
                disk.library().putPosition(PlaybackPosition("version", 1234, 4000))
                disk.library().putBookmark(Bookmark("bookmark", "version", 1200, "saved"))
            } finally { disk.close() }
            val reopened = Room.databaseBuilder(context, LibraryDatabase::class.java, name).build()
            try {
                assertTrue(reopened.library().work("work")!!.favorite)
                assertEquals(1234L, reopened.library().position("version")!!.positionMs)
                assertEquals("saved", reopened.library().observeBookmarks("version").first().single().note)
            } finally { reopened.close() }
        } finally { context.deleteDatabase(name) }
    }

    @Test fun singleDocumentImportIsStableAndNeverScansParents() = runBlocking {
        val uri = DocumentsContract.buildDocumentUri(provider.authority!!, "single/WAV/01.wav")
        context.contentResolver.call(provider, "grant", context.packageName, Bundle().apply { putString("uri", uri.toString()) })
        try {
            val first = repo.importDocument(uri, "01.wav")
            val again = repo.importDocument(uri, "01.wav")
            assertEquals(first.versionId, again.versionId)
            assertTrue(repo.roots.first().isEmpty())
            val work = repo.works.first().single()
            assertNull(work.rootId)
            assertEquals(1, repo.versions(work.id).first().size)
        } finally { context.contentResolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
    }
}
