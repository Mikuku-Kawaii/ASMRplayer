package com.asmrplayer.android.backup

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.*
import com.asmrplayer.android.subtitles.SubtitleRepository
import com.asmrplayer.android.online.*
import com.asmrplayer.android.download.DownloadRepository
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class BackupRepositoryTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val provider = Uri.parse("content://com.asmrplayer.android.test.fixture")
    private lateinit var source: LibraryDatabase
    private lateinit var target: LibraryDatabase
    private lateinit var library: LibraryRepository
    private lateinit var backup: BackupRepository
    private lateinit var restored: BackupRepository
    private lateinit var workId: String
    private lateinit var versionId: String
    private lateinit var subtitleId: String
    private val grants = mutableListOf<Uri>()
    private fun mode(value: String) { context.contentResolver.call(provider, "mode", value, null) }
    private fun grant(uri: Uri): Uri {
        context.contentResolver.call(provider, "grant", context.packageName, Bundle().apply { putString("uri", uri.toString()) })
        grants.add(uri); return uri
    }
    private fun tree(name: String) = DocumentsContract.buildTreeDocumentUri(provider.authority!!, name)
    @Before fun setup() = runBlocking {
        mode("normal")
        source = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        target = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        library = LibraryRepository(context, source)
        backup = BackupRepository(context, source); restored = BackupRepository(context, target)
        source.library().putRoot(LibraryRoot("portable-root", tree("backupOriginal").toString(), "Fixture", "AUTO"))
        library.scan("portable-root")
        val work = library.works.first().single(); workId = work.id
        versionId = library.versions(workId).first().first { it.groupPath == "WAV" && it.title == "01" }.id
        library.setFavorite(workId, true); library.setRating(workId, "NSFW")
        source.library().putPosition(PlaybackPosition(versionId, 1234, 4000))
        library.addBookmark(versionId, 999, "Portable note")
        subtitleId = SubtitleRepository(context, source).sources(versionId).first().single().id
        source.library().putSubtitlePreference(SubtitlePreference(versionId, subtitleId, offsetMs = 321))
        source.library().putSession(SessionRecord(queueJson = "[\"$versionId\"]", index = 0, positionMs = 1234, sleepDeadlineMs = 9999999999999))
    }
    @After fun cleanup() {
        mode("normal")
        grants.forEach { try { context.contentResolver.releasePersistableUriPermission(it, Intent.FLAG_GRANT_READ_URI_PERMISSION) } catch (_: SecurityException) { } }
        source.close(); target.close()
    }
    @Test fun portableRoundTripOmitsDevicePathsAndRequiresReauthorization() = runBlocking {
        val bytes = backup.exportBytes(); val text = bytes.toString(Charsets.UTF_8)
        assertFalse(text.contains(provider.authority!!)); assertFalse(text.contains("treeUri")); assertFalse(text.contains("sleepDeadline"))
        val plan = restored.prepare(bytes)
        assertEquals(4, plan.preview.version); assertEquals(1, plan.preview.queueSize)
        assertTrue(target.library().observeWorks().first().isEmpty()) // preview never mutates
        val result = restored.apply(plan)
        assertEquals(listOf(versionId), result.queueVersionIds)
        assertNull(target.library().session()) // importing never commandeers the live queue
        assertTrue(target.library().work(workId)!!.favorite)
        assertEquals(1234L, target.library().position(versionId)!!.positionMs)
        val restoredLibrary = LibraryRepository(context, target)
        assertEquals(listOf("portable-root"), restoredLibrary.reauthorizationNeeds().directoryIds)
        assertTrue(target.library().sourcesForRoot("portable-root").none { it.available })
        val replacement = grant(tree("backupRelocated"))
        val preview = restoredLibrary.previewReauthorization("portable-root", replacement)
        assertEquals(RootMode.WORK, preview.mode); assertEquals(3, preview.matchedAudio)
        restoredLibrary.reauthorizeRoot("portable-root", replacement)
        assertTrue(restoredLibrary.resolve(versionId).uri.contains("backupRelocated"))
        assertEquals(321L, SubtitleRepository(context, target).load(versionId).preference.offsetMs)
        assertEquals("Portable note", restoredLibrary.bookmarks(versionId).first().single().note)
        assertTrue(restoredLibrary.reauthorizationNeeds().directoryIds.isEmpty())
    }
    @Test fun repeatedMergeIsIdempotentAndExplicitPolicyControlsUserRecords() = runBlocking {
        val plan = restored.prepare(backup.exportBytes()); restored.apply(plan)
        target.library().favorite(workId, false)
        target.library().putPosition(PlaybackPosition(versionId, 3000, 4000))
        val kept = restored.apply(plan)
        assertEquals(0, kept.inserted); assertEquals(0, kept.updated)
        assertFalse(target.library().work(workId)!!.favorite); assertEquals(3000L, target.library().position(versionId)!!.positionMs)
        val overwritten = restored.apply(plan, ImportPolicy.REPLACE_USER_DATA)
        assertTrue(overwritten.updated > 0); assertTrue(target.library().work(workId)!!.favorite)
        assertEquals(1234L, target.library().position(versionId)!!.positionMs)
        assertEquals(1, target.library().observeBookmarks(versionId).first().size)
    }
    @Test fun favoriteVersionsRoundTripAndOldV1ImportPreservesExistingPreferences() = runBlocking {
        library.collections.setVersionFavorite(versionId, true)
        val bytes = backup.exportBytes()
        val plan = restored.prepare(bytes); restored.apply(plan)
        assertTrue(target.collections().preference(versionId)!!.favorite)
        target.collections().putPreference(VersionPreference(versionId, false))
        restored.apply(plan)
        assertFalse(target.collections().preference(versionId)!!.favorite)
        restored.apply(plan, ImportPolicy.REPLACE_USER_DATA)
        assertTrue(target.collections().preference(versionId)!!.favorite)
        val old = JSONObject(bytes.toString(Charsets.UTF_8)).put("version", 1)
        old.getJSONObject("records").remove("version_preferences")
        val oldPlan = restored.prepare(old.toString().toByteArray())
        assertEquals(0, oldPlan.preview.counts.getValue("version_preferences"))
        restored.apply(oldPlan, ImportPolicy.REPLACE_USER_DATA)
        assertTrue(target.collections().preference(versionId)!!.favorite)
    }
    @Test fun membershipRoundTripsAndLegacyBundlesKeepExistingWorksInLibrary() = runBlocking {
        library.setMembership(workId,false)
        val bytes=backup.exportBytes()
        restored.apply(restored.prepare(bytes))
        assertFalse(target.library().work(workId)!!.inLibrary)
        for(version in listOf(1,2)) {
            val legacy=JSONObject(bytes.toString(Charsets.UTF_8)).put("version",version)
            val records=legacy.getJSONObject("records")
            if(version==1)records.remove("version_preferences")
            val works=records.getJSONArray("works")
            for(i in 0 until works.length())works.getJSONObject(i).remove("inLibrary")
            restored.apply(restored.prepare(legacy.toString().toByteArray()),ImportPolicy.REPLACE_USER_DATA)
            assertTrue(target.library().work(workId)!!.inLibrary)
        }
    }
    @Test fun invalidVersionsReferencesDuplicatesAndPathsAreRejectedBeforeWriting() = runBlocking {
        val original = backup.exportBytes().toString(Charsets.UTF_8)
        val mutations: List<(JSONObject) -> Unit> = listOf(
            { it.put("version", 99) },
            { it.getJSONObject("records").getJSONArray("tracks").getJSONObject(0).put("workId", "absent") },
            { val rows = it.getJSONObject("records").getJSONArray("works"); rows.put(rows.getJSONObject(0)) },
            { it.getJSONObject("records").getJSONArray("media_sources").getJSONObject(0).put("relativePath", "../escape") },
            { it.getJSONObject("records").getJSONArray("subtitles").getJSONObject(0).put("uri", "file:///private") },
        )
        mutations.forEach { mutate ->
            val changed = JSONObject(original); mutate(changed)
            try { restored.prepare(changed.toString().toByteArray()); fail("invalid bundle accepted") } catch (_: IllegalArgumentException) { }
        }
        try { restored.prepare(original.dropLast(10).toByteArray()); fail("truncated accepted") } catch (_: org.json.JSONException) { }
        assertTrue(target.library().observeWorks().first().isEmpty())
    }
    @Test fun lateIdentityCollisionRollsBackEarlierRowsAndNeverRebindsExistingSource() = runBlocking {
        val track = source.library().version(versionId)!!
        target.library().putWork(LibraryWork("unrelated", null, "", "Other"))
        target.library().putTrack(LibraryTrack(track.trackId, "unrelated", "Collision", ""))
        val plan = restored.prepare(backup.exportBytes())
        try { restored.apply(plan); fail("conflict accepted") } catch (_: IllegalArgumentException) { }
        assertNull(target.library().root("portable-root")); assertNull(target.library().work(workId))
        assertEquals("unrelated", target.library().observeWorks().first().single().id)
    }
    @Test fun missingOrIncompleteRelinkDoesNotReplaceCurrentBinding() = runBlocking {
        val replacement = grant(tree("backupPartial"))
        val before = source.library().root("portable-root")!!
        mode("missing")
        assertEquals(listOf("WAV/02.wav"), library.previewReauthorization("portable-root", replacement).missingPaths)
        try { library.reauthorizeRoot("portable-root", replacement); fail("missing accepted silently") } catch (_: IllegalArgumentException) { }
        assertEquals(before, source.library().root("portable-root"))
        mode("loading")
        try { library.reauthorizeRoot("portable-root", replacement, true); fail("incomplete committed") } catch (_: java.io.IOException) { }
        assertEquals(before, source.library().root("portable-root"))
        mode("slow")
        val job = launch(Dispatchers.IO) { library.reauthorizeRoot("portable-root", replacement) }
        withTimeout(3000) { library.scanStates.first { it["portable-root"]?.running == true } }
        library.cancelScan("portable-root"); job.join()
        assertEquals(before, source.library().root("portable-root"))
        mode("missing"); library.reauthorizeRoot("portable-root", replacement, allowMissing = true)
        assertEquals(1, source.library().sourcesForRoot("portable-root").count { !it.available })
        assertTrue(source.library().work(workId)!!.favorite)
    }
    @Test fun standaloneAudioAndSubtitleCanBeRelinkedWithoutChangingIds() = runBlocking {
        val audio = grant(DocumentsContract.buildDocumentUri(provider.authority!!, "single.wav"))
        val standalone = library.importDocument(audio, "single.wav")
        val subUri = grant(DocumentsContract.buildDocumentUri(provider.authority!!, "single.srt"))
        val sub = SubtitleRepository(context, source).attach(standalone.versionId, subUri, "single.srt")
        source.library().putSubtitlePreference(SubtitlePreference(standalone.versionId, sub.id, offsetMs = 12))
        restored.apply(restored.prepare(backup.exportBytes()))
        val importedLibrary = LibraryRepository(context, target)
        assertTrue(importedLibrary.reauthorizationNeeds().audioSourceIds.contains(standalone.sourceId))
        assertTrue(importedLibrary.reauthorizationNeeds().subtitleSourceIds.contains(sub.id))
        importedLibrary.reauthorizeAudioSource(standalone.sourceId, audio)
        SubtitleRepository(context, target).reauthorize(sub.id, subUri)
        assertEquals(standalone.versionId, importedLibrary.resolve(standalone.versionId).versionId)
        assertEquals(12L, SubtitleRepository(context, target).load(standalone.versionId).preference.offsetMs)
    }
    @Test fun safExportAndPreviewReadTheSameVersionedDocument() = runBlocking {
        val uri = DocumentsContract.buildDocumentUri(provider.authority!!, "backup.json")
        assertTrue(backup.exportTo(uri) > 0)
        val plan = restored.prepare(uri)
        assertEquals(listOf("portable-root"), plan.preview.directoryIds)
        restored.apply(plan)
        assertEquals(3, target.library().observeVersions(workId).first().size)
    }
    @Test fun remoteReferencesAndDownloadedSubtitleBindingsSurviveWithoutCachedMediaOrCredentials() = runBlocking {
        val assetId = LibraryIndex.id("portable-remote-asset")
        val origin = "https://fixture.invalid"
        val remote = RemoteAsset(assetId, workId, origin, "123456", "fixture-subtitle", "WAV/01.srt", "subtitle", "srt", null)
        source.library().putRemoteAsset(remote)
        source.library().putRemote(RemoteMedia(versionId, origin, "123456", "fixture-01", "WAV/01.wav", null, "wav"))
        source.library().putSource(MediaSource("remote-source", versionId, null, "WAV/01.wav", "https://fixture.invalid/media?signature=do-not-export", kind = "online_provider"))
        source.library().putAsset(WorkAsset(assetId, workId, "online:$workId", remote.relativePath, "file:///private/device/subtitle.media", "srt"))
        source.library().putSubtitle(SubtitleSource("remote-subtitle", versionId, null, remote.relativePath, "file:///private/device/subtitle.media", "Downloaded", "srt", manual = true))
        source.library().putSubtitlePreference(SubtitlePreference(versionId, "remote-subtitle", offsetMs = 456))
        val bytes = backup.exportBytes()
        assertFalse(bytes.toString(Charsets.UTF_8).contains("signature="))
        assertFalse(bytes.toString(Charsets.UTF_8).contains("/private/device"))
        restored.apply(restored.prepare(bytes))
        assertEquals("unbound://asset/$assetId", target.library().subtitle("remote-subtitle")!!.uri)
        assertTrue(target.library().sources(versionId).single { it.kind == "online_provider" }.available)
        assertTrue(target.library().downloadTasks().isEmpty())
        OnlineFixtureServer().use { server ->
            server.attachments = true
            // Redirect only this synthetic imported reference to the loopback fixture, never a live origin.
            target.library().putRemoteAsset(remote.copy(origin = server.base))
            val http = HttpTransport(debugLoopback = true)
            val account = ProviderAccount(context, http)
            val downloads = DownloadRepository(context, target, AsmrOneProvider(account, http), schedule = {})
            try {
                downloads.enqueueAssets(listOf(assetId)); downloads.runQueue()
                assertTrue(target.library().subtitle("remote-subtitle")!!.available)
                assertEquals(456L, SubtitleRepository(context, target).load(versionId).preference.offsetMs)
            } finally { downloads.tasks.first().forEach { downloads.remove(it.id) } }
        }
    }
}
