package com.asmrplayer.android

import android.app.Application
import android.content.ComponentName
import android.content.ContentValues
import android.net.Uri
import android.provider.MediaStore
import androidx.lifecycle.ViewModelStore
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.*
import com.asmrplayer.android.playback.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.TimeUnit

/** UI state-holder tests use synthetic rows/media on a dedicated emulator, never user files. */
@UnstableApi
@RunWith(AndroidJUnit4::class)
class CatalogUiToolsTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val backend = AndroidBackend.get(context)
    private fun <T> main(block: () -> T): T {
        var value: T? = null
        instrumentation.runOnMainSync { value = block() }
        @Suppress("UNCHECKED_CAST") return value as T
    }
    private suspend fun await(tools: CollectionTools, test: (CollectionScreenState) -> Boolean) =
        withTimeout(15000) { tools.state.first(test) }
    private suspend fun await(model: PlayerViewModel, test: (PlayerScreenState) -> Boolean): PlayerScreenState {
        val caller = Throwable()
        return withTimeoutOrNull(15000) { model.state.first(test) }
            ?: throw AssertionError("Timed out waiting for playback state", caller)
    }

    @Test fun historyOrdersPositionsByLastListenAcrossVersions() = runBlocking {
        check(context.packageName.endsWith(".uitest"))
        val preview = InstrumentationRegistry.getArguments().getString("cardPreview") == "true"
        val prefix = if (preview) "mixed-card-preview" else "mixed-card-${System.nanoTime()}"
        val dao = backend.database.library()
        val heard = "$prefix-heard"
        val fresh = "$prefix-fresh"
        try {
            for ((work, title) in listOf(heard to "夜雨与耳语", fresh to "森林环境音")) {
                dao.putWork(LibraryWork(work, null, "", title, favorite = true, rating = "SAFE"))
                backend.database.metadata().put(CatalogMetadata(work, tagsJson = if (work == heard) """["雨声","耳语"]""" else """["自然音"]"""))
                for (i in 1..2) {
                    val id = "$work-$i"
                    dao.putTrack(LibraryTrack(id, work, "音轨 $i", "WAV"))
                    dao.putVersion(TrackVersion(id, id, "wav"))
                    dao.putSource(MediaSource(id, id, null, "$i.wav", "content://synthetic/unused"))
                }
            }
            dao.putPosition(PlaybackPosition("$heard-1", 920_000L, 1_800_000L, updatedMs = 100))
            dao.putPosition(PlaybackPosition("$heard-2", 754_000L, 1_800_000L, updatedMs = 200))
            val history = backend.library.collections.history().first()
            assertEquals(754_000L, history.first { it.workId == heard }.positionMs)
            assertNull(history.firstOrNull { it.workId == fresh })
        } finally {
            if (!preview) for (id in listOf(heard, fresh)) {
                backend.library.setFavorite(id, false)
                backend.library.setMembership(id, false)
            }
        }
    }

    @Test fun catalogQueriesTrackFavoritesHistoryAndBookmarksStayLive() = runBlocking {
        val prefix = "catalog-ui-${System.nanoTime()}"
        seed(prefix, "content://synthetic/unused")
        val preferences = backend.library.preferences.first()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        val tools = main { CollectionTools(context, scope) }
        try {
            backend.library.setContentFilter(true, true)
            main { tools.query(LibraryQuery(text = prefix)) }
            assertEquals(listOf("$prefix-a", "$prefix-b"), await(tools) { it.ready && it.works.size == 2 }.works.map { it.work.id })
            main { tools.query(LibraryQuery(text = prefix, source = LibrarySourceFilter.ONLINE)) }
            await(tools) { it.works.map { w -> w.work.id } == listOf("$prefix-b") }
            main { tools.query(LibraryQuery(text = prefix, listening = ListeningFilter.IN_PROGRESS)) }
            await(tools) { it.works.map { w -> w.work.id } == listOf("$prefix-a") }
            main { tools.query(LibraryQuery(text = prefix, sort = LibrarySort.CODE, descending = true)) }
            await(tools) { it.works.map { w -> w.work.id } == listOf("$prefix-b", "$prefix-a") }
            main { tools.query(LibraryQuery(text = "$prefix absent")) }
            await(tools) { it.works.isEmpty() }
            main { tools.query(LibraryQuery(text = prefix)) }
            await(tools) { it.works.size == 2 }
            main { tools.favorite("$prefix-v1", true) }
            await(tools) { it.favorites.any { v -> v.versionId == "$prefix-v1" } && "$prefix-v1" !in it.busyIds }
            assertFalse(backend.library.work("$prefix-a")!!.favorite)
            assertFalse(tools.state.value.favorites.any { it.versionId == "$prefix-v2" })
            assertTrue(tools.state.value.history.any { it.versionId == "$prefix-v1" && it.positionMs == 5000L })
            assertTrue(tools.state.value.bookmarks.any { it.bookmark.id == "$prefix-bookmark" })
            main { tools.removeBookmark("$prefix-bookmark") }
            await(tools) { it.bookmarks.none { b -> b.bookmark.id == "$prefix-bookmark" } && "$prefix-bookmark" !in it.busyIds }
            assertEquals(5000L, backend.library.position("$prefix-v1")!!.positionMs)
            backend.library.setContentFilter(false, true)
            await(tools) { it.works.map { w -> w.work.id } == listOf("$prefix-a") && it.history.none { v -> v.versionId == "$prefix-v3" } }
            main { tools.favorite("$prefix-v1", false) }
            await(tools) { it.favorites.none { v -> v.versionId == "$prefix-v1" } && "$prefix-v1" !in it.busyIds }
            assertFalse(backend.database.collections().preference("$prefix-v1")!!.favorite)
        } finally { scope.cancel(); backend.library.setContentFilter(preferences.includeNsfw, preferences.includeUnrated) }
    }

    @Test fun queueEditsKeepCurrentVersionPausedAndBookmarkUsesItsOwnPosition(): Unit = runBlocking {
        val prefix = "queue-ui-${System.nanoTime()}"
        val uri = audio()
        seed(prefix, uri.toString())
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        val store = ViewModelStore()
        val model = main { PlayerViewModel(context.applicationContext as Application).also { store.put("catalog-test", it) } }
        val future = main { MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync() }
        val p = future.get(10, TimeUnit.SECONDS)
        try {
            await(model) { it.connected && it.libraryReady }
            val ids = listOf("$prefix-v1", "$prefix-v2")
            main { LibraryPlayback.play(p, ids, resume = false, startPositionMs = 1500) }.get(10, TimeUnit.SECONDS)
            await(model) { it.playing && it.queue.map { q -> q.id } == ids && it.position >= 1500 }
            main { p.pause() }
            await(model) { !it.playRequested }
            val before = main { p.currentPosition }
            main { model.setLibraryMembership("$prefix-a",false) }
            await(model) { it.library.any{work->work.id=="$prefix-a"&&!work.inLibrary} }
            assertEquals(ids,main{model.state.value.queue.map{it.id}})
            assertEquals(ids[0],main{p.currentMediaItem!!.mediaId})
            assertTrue(backend.database.library().sources(ids[0]).isNotEmpty())
            main { model.setLibraryMembership("$prefix-a",true) }
            await(model) { it.library.any{work->work.id=="$prefix-a"&&work.inLibrary} }
            main { model.playbackOrder(PlaybackOrder.SHUFFLE_LOOP) }
            await(model) { it.shuffle && it.repeatMode == 2 && it.canNext && it.canPrevious }
            // This second MediaController receives its own asynchronous callback.
            withTimeout(15000) { while (!main { p.shuffleModeEnabled }) delay(30) }
            assertFalse(main { p.playWhenReady })
            assertTrue(kotlin.math.abs(before-main {p.currentPosition})<=100)
            main { model.next() }
            await(model) { it.index==1 }
            main { model.next() }
            await(model) { it.index==0 }
            main { p.seekTo(0,before);model.playbackOrder(PlaybackOrder.SEQUENTIAL) }
            await(model) { !it.shuffle && it.repeatMode==0 && !it.canPrevious }
            main { model.enqueue(listOf(ids[0]), true) }
            await(model) { it.queue.map { q -> q.id } == listOf(ids[0], ids[0], ids[1]) }
            assertFalse(main { p.playWhenReady })
            assertTrue(kotlin.math.abs(before - main { p.currentPosition }) <= 100)
            main { model.removeQueue(1, listOf(ids[0], ids[0], ids[1])); model.repeat(1) }
            await(model) { it.queue.map { q -> q.id } == ids && it.repeatMode == 1 && !it.shuffle }
            main { model.repeat(0) }
            main { model.moveQueue(0, 1, ids) }
            await(model) { it.queue.map { q -> q.id } == ids.reversed() && it.index == 1 }
            assertEquals(ids[0], main { p.currentMediaItem!!.mediaId })
            assertFalse(main { p.playWhenReady })
            assertTrue(kotlin.math.abs(before - main { p.currentPosition }) <= 100)
            main { model.removeQueue(0, ids) } // Old confirmation snapshot must be rejected.
            assertEquals(ids.reversed(), model.state.value.queue.map { it.id })
            assertTrue(model.state.value.error!!.contains("队列已变化"))
            main { model.removeQueue(0, ids.reversed()) }
            await(model) { it.queue.map { q -> q.id } == listOf(ids[0]) }
            assertFalse(main { p.playWhenReady })
            main {
                model.playVersion("$prefix-missing")
                model.moveQueue(0, 0, listOf(ids[0]))
                assertFalse("Queue edit left the superseded open spinner active", model.state.value.opening)
            }
            main { model.removeQueue(0, listOf(ids[0])) }
            await(model) { it.queue.isEmpty() && !it.playing }
            main { model.playBookmark(ids[0], 3000) }
            val bookmark = await(model) { !it.opening && it.playing && it.queue.singleOrNull()?.id == ids[0] && it.position >= 3000 }
            assertTrue("Bookmark resumed a different position: ${bookmark.position}", bookmark.position < 4500)
            main { p.pause() }
            await(model) { !it.playRequested }
        } finally {
            main { p.stop(); p.clearMediaItems(); store.clear(); MediaController.releaseFuture(future) }
            scenario.close()
            context.contentResolver.delete(uri, null, null)
        }
    }

    private suspend fun seed(prefix: String, uri: String) {
        val dao = backend.database.library()
        dao.putWork(LibraryWork("$prefix-a", null, "a", "$prefix 02", "RJ2", rating = "SAFE"))
        dao.putWork(LibraryWork("$prefix-b", null, "b", "$prefix 10", "RJ10", rating = "NSFW"))
        for (i in 1..3) {
            dao.putTrack(LibraryTrack("$prefix-t$i", if (i == 3) "$prefix-b" else "$prefix-a", "Synthetic track $i", "WAV"))
            dao.putVersion(TrackVersion("$prefix-v$i", "$prefix-t$i", "wav"))
            if (i == 3) dao.putRemote(RemoteMedia("$prefix-v$i", "https://fixture.invalid", "990001", "file-$i", "$i.wav", null, "wav"))
            dao.putSource(MediaSource("$prefix-s$i", "$prefix-v$i", null, "$i.wav", uri,
                kind = if (i == 3) "online_provider" else "local_imported"))
        }
        dao.putPosition(PlaybackPosition("$prefix-v1", 5000, 30000))
        dao.putPosition(PlaybackPosition("$prefix-v3", 30000, 30000, completed = true))
        dao.putBookmark(Bookmark("$prefix-bookmark", "$prefix-v1", 3000, "Synthetic bookmark"))
    }

    private fun audio(): Uri {
        val bytes = 48000 * 30 * 4
        val header = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
        header.put("RIFF".toByteArray()).putInt(bytes + 36).put("WAVEfmt ".toByteArray()).putInt(16)
            .putShort(1).putShort(2).putInt(48000).putInt(192000).putShort(4).putShort(16).put("data".toByteArray()).putInt(bytes)
        val uri = context.contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, ContentValues().apply {
            put(MediaStore.Audio.Media.DISPLAY_NAME, "catalog-ui-${System.nanoTime()}.wav")
            put(MediaStore.Audio.Media.MIME_TYPE, "audio/wav"); put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/ASMR-UI-Test")
        })!!
        context.contentResolver.openOutputStream(uri)!!.use { stream -> stream.write(header.array()); repeat(30) { stream.write(ByteArray(192000)) } }
        return uri
    }
}
