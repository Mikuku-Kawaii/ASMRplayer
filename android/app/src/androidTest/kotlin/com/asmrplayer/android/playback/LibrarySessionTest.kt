package com.asmrplayer.android.playback

import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.SessionResult
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.TimeUnit

@UnstableApi
@RunWith(AndroidJUnit4::class)
class LibrarySessionTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun <T> main(block: () -> T): T {
        var result: T? = null
        instrumentation.runOnMainSync { result = block() }
        @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun waitFor(message: String, condition: () -> Boolean) {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
        while (System.nanoTime() < end) { if (condition()) return; Thread.sleep(50) }
        fail(message)
    }
    private fun command(coordinator: LibrarySessionCoordinator, action: String, args: Bundle = Bundle.EMPTY): SessionResult =
        main { coordinator.command(action, args) }.get(10, TimeUnit.SECONDS)

    @Test fun replacingIndexZeroKeepsBothTrackOffsetsAndRestartOnlyResetsTheFirst() {
        val backend = AndroidBackend.get(context)
        val authority = InstrumentationRegistry.getArguments().getString("fixtureAuthority", "com.asmrplayer.android.test.fixture")
        val tree = DocumentsContract.buildTreeDocumentUri(authority, "session")
        context.contentResolver.call(Uri.parse("content://$authority"), "mode", "normal", null)
        val ids = runBlocking {
            val root = "session-fixture"
            backend.database.library().putRoot(LibraryRoot(root, tree.toString(), "Fixture", "WORK"))
            backend.library.scan(root)
            val work = backend.library.works.first().first { it.rootId == root }
            backend.library.versions(work.id).first().filter { it.groupPath == "WAV" }.map { it.id }
        }
        val player = main { ExoPlayer.Builder(context).build().apply { volume = 0f } }
        val coordinator = main { LibrarySessionCoordinator(player, backend, mutableMapOf(), { it }, context.getSharedPreferences("resume-test",0)) }
        fun start(queue: List<String>, index: Int = 0, position: Long? = null): Long {
            val result = command(coordinator, LibraryPlayback.PLAY_VERSIONS, Bundle().apply {
                putStringArrayList("versionIds", ArrayList(queue))
                putInt("index", index)
                position?.let { putLong("positionMs", it) }
            })
            assertEquals(SessionResult.RESULT_SUCCESS, result.resultCode)
            main { player.pause() }
            waitFor("Media not ready") { main { player.playbackState == Player.STATE_READY } }
            return main { player.currentPosition }
        }
        try {
            start(listOf(ids[0]), position = 600)
            start(listOf(ids[1]), position = 1700)
            assertTrue(start(listOf(ids[0])) in 600..1000)
            assertTrue(start(ids, index = 1) in 1700..2100)
            assertTrue(start(ids, position = 0) < 400)
            assertTrue(runBlocking { backend.library.position(ids[1])!!.positionMs } in 1700..2100)
        } finally {
            main { coordinator.close(); player.release() }
        }
    }

    @Test fun stableQueueBookmarksAbSleepAndPausedRecreation() {
        val backend = AndroidBackend.get(context)
        val provider = Uri.parse("content://${InstrumentationRegistry.getArguments().getString("fixtureAuthority", "com.asmrplayer.android.test.fixture")}")
        context.contentResolver.call(provider, "mode", "normal", null)
        val ids = runBlocking {
            backend.database.library().putRoot(LibraryRoot("session-fixture", DocumentsContract.buildTreeDocumentUri(provider.authority!!, "session").toString(), "Fixture", "WORK"))
            backend.library.scan("session-fixture")
            val work = backend.library.works.first().first { it.rootId == "session-fixture" }
            backend.library.versions(work.id).first().filter { it.groupPath == "WAV" }.map { it.id }
        }
        var player = main { ExoPlayer.Builder(context).build() }
        var coordinator = main { LibrarySessionCoordinator(player, backend, mutableMapOf(), { it }, context.getSharedPreferences("sleep-test",0)) }
        try {
            assertEquals(0, command(coordinator, LibraryPlayback.PLAY_VERSIONS, Bundle().apply {
                putStringArrayList("versionIds", ArrayList(ids)); putInt("index", 1); putBoolean("resume", false)
            }).resultCode)
            waitFor("Tapped track did not start") { main { player.isPlaying && player.currentMediaItem?.mediaId == ids[1] } }
            main { player.seekTo(0, 3650) }
            waitFor("Automatic advance did not reach the next track") { main { player.currentMediaItemIndex == 1 && player.isPlaying } }
            main { player.seekTo(3650) }
            waitFor("Last track did not finish") { main { player.playbackState == Player.STATE_ENDED } }
            assertEquals(ids, main { List(player.mediaItemCount) { player.getMediaItemAt(it).mediaId } })
            command(coordinator, LibraryPlayback.SAVE_SESSION)
            assertEquals(ids, runBlocking { SessionPersistence.decode(backend.sessions.read()!!.queueJson) })
            assertEquals(0, command(coordinator, LibraryPlayback.PLAY_VERSIONS, Bundle().apply {
                putStringArrayList("versionIds", ArrayList(ids)); putBoolean("resume", false)
            }).resultCode)
            waitFor("Library audio failed") { main { player.isPlaying && player.duration > 0 } }
            assertEquals(SessionResult.RESULT_SUCCESS, command(coordinator, LibraryPlayback.SET_AB, Bundle().apply {
                putString("versionId", ids[0]); putLong("aMs", 1000); putLong("bMs", 1500)
            }).resultCode)
            main { player.seekTo(1400) }
            Thread.sleep(350)
            assertTrue(main { player.currentPosition in 1000..1550 })
            assertNotEquals(SessionResult.RESULT_SUCCESS, command(coordinator, LibraryPlayback.SET_AB, Bundle().apply {
                putString("versionId", ids[0]); putLong("aMs", 1000); putLong("bMs", 99999)
            }).resultCode)
            main { player.seekTo(1, 1000) }
            waitFor("Version transition failed") { main { player.currentMediaItemIndex == 1 && player.isPlaying } }
            assertNull(command(coordinator, LibraryPlayback.GET_STATE).extras.getString("abVersionId"))
            // A note drafted for the previous version must not land on the newly playing track.
            assertNotEquals(SessionResult.RESULT_SUCCESS, command(coordinator, LibraryPlayback.ADD_BOOKMARK, Bundle().apply {
                putString("versionId", ids[0]); putString("note", "stale UI draft")
            }).resultCode)
            assertFalse(runBlocking { backend.library.bookmarks(ids[1]).first().any { it.note == "stale UI draft" } })
            val bookmark = command(coordinator, LibraryPlayback.ADD_BOOKMARK, Bundle().apply { putString("note", "fixture") })
            assertNotNull(bookmark.extras.getString("bookmarkId"))
            assertTrue(runBlocking { backend.library.bookmarks(ids[1]).first().any { it.note == "fixture" } })

            main { player.volume = .6f; player.seekTo(1, 0) }
            command(coordinator, LibraryPlayback.SET_SLEEP, Bundle().apply {
                putLong("deadlineMs", System.currentTimeMillis() + 1500); putLong("fadeMs", 2000)
            })
            waitFor("Sleep fade did not attenuate app volume") { main { player.volume < .5f } }
            val beforeExtension = command(coordinator, LibraryPlayback.GET_STATE).extras.getLong("deadlineMs")
            command(coordinator, LibraryPlayback.SET_SLEEP, Bundle().apply { putInt("extendMinutes", 5) })
            assertEquals(beforeExtension + 300_000, command(coordinator, LibraryPlayback.GET_STATE).extras.getLong("deadlineMs"))
            assertEquals(.6f, main { player.volume }, .01f)
            val deadline = System.currentTimeMillis() + 600
            command(coordinator, LibraryPlayback.SET_SLEEP, Bundle().apply { putLong("deadlineMs", deadline); putLong("fadeMs", 1000) })
            waitFor("Deadline did not pause playback") { main { !player.playWhenReady } }
            assertFalse(command(coordinator, LibraryPlayback.GET_STATE).extras.containsKey("deadlineMs"))
            assertEquals(.6f, main { player.volume }, .01f)

            main { player.seekTo(0, 3700) }
            command(coordinator, LibraryPlayback.SET_SLEEP, Bundle().apply { putBoolean("endOfTrack", true) })
            main { player.play() }
            waitFor("End-of-track sleep did not pause") { main { !player.playWhenReady } }
            assertEquals(0, main { player.currentMediaItemIndex })
            assertFalse(command(coordinator, LibraryPlayback.GET_STATE).extras.getBoolean("endOfTrack"))
            command(coordinator, LibraryPlayback.SAVE_SESSION)
            assertTrue(runBlocking { backend.library.position(ids[0])!!.completed })

            main { player.seekTo(1, 1250); player.play() }
            assertEquals(SessionResult.RESULT_SUCCESS, command(coordinator, LibraryPlayback.PREPARE_UPDATE).resultCode)
            assertFalse(main { player.playWhenReady })
            main { coordinator.close(); player.release() }
            // Media3 may publish its final paused frame after pause(); compare restoration to the
            // ordered, durable close snapshot rather than an earlier transient controller read.
            val updatePosition = runBlocking { backend.sessions.read()!! }.positionMs
            assertTrue(updatePosition >= 1250)
            player = main { ExoPlayer.Builder(context).build() }
            coordinator = main { LibrarySessionCoordinator(player, backend, mutableMapOf(), { it }, context.getSharedPreferences("sleep-test",0)) }
            command(coordinator, LibraryPlayback.GET_STATE) // Wait for restoration.
            assertFalse(main { player.playWhenReady })
            assertEquals(Player.STATE_IDLE, main { player.playbackState })
            assertEquals(2, main { player.mediaItemCount })
            assertEquals(ids[1], main { player.currentMediaItem?.mediaId })
            assertEquals(updatePosition, main { player.currentPosition })
            runBlocking { backend.database.library().putPosition(PlaybackPosition(ids[1], 4000, 4000, completed = true)) }
            command(coordinator, LibraryPlayback.SAVE_SESSION)
            assertTrue("Restored unprepared timeline erased the completion marker", runBlocking { backend.library.position(ids[1])!!.completed })
            main { player.moveMediaItem(1, 0); player.removeMediaItem(1) }
            command(coordinator, LibraryPlayback.SAVE_SESSION)
            val edited = runBlocking { backend.sessions.read()!! }
            assertEquals(listOf(ids[1]), SessionPersistence.decode(edited.queueJson))
            assertEquals(0, edited.index); assertEquals(updatePosition, edited.positionMs)
            main { coordinator.close(); player.release() }
            player = main { ExoPlayer.Builder(context).build() }
            coordinator = main { LibrarySessionCoordinator(player, backend, mutableMapOf(), { it }, context.getSharedPreferences("sleep-test",0)) }
            command(coordinator, LibraryPlayback.GET_STATE)
            assertEquals(1, main { player.mediaItemCount })
            assertEquals(ids[1], main { player.currentMediaItem?.mediaId })
            assertFalse(main { player.playWhenReady })
        } finally {
            main { player.clearMediaItems(); coordinator.close(); player.release() }
        }
    }

    @Test fun cancelledResolveCannotReplaceNewerQueue() {
        val backend = AndroidBackend.get(context)
        val id = runBlocking {
            val provider = Uri.parse("content://${InstrumentationRegistry.getArguments().getString("fixtureAuthority", "com.asmrplayer.android.test.fixture")}")
            context.contentResolver.call(provider, "mode", "normal", null)
            backend.database.library().putRoot(LibraryRoot("race-fixture", DocumentsContract.buildTreeDocumentUri(provider.authority!!, "race").toString(), "Fixture", "WORK"))
            backend.library.scan("race-fixture")
            val work = backend.library.works.first().first { it.rootId == "race-fixture" }
            backend.library.versions(work.id).first().first().id
        }
        val player = main { ExoPlayer.Builder(context).build() }
        val coordinator = main { LibrarySessionCoordinator(player, backend, mutableMapOf(), { it }, context.getSharedPreferences("sleep-test",0)) }
        try {
            command(coordinator, LibraryPlayback.GET_STATE)
            main { player.clearMediaItems() }
            // A pending request loses ownership as soon as a standard player intent arrives.
            val future = main {
                val value = coordinator.command(LibraryPlayback.PLAY_VERSIONS, Bundle().apply {
                    putStringArrayList("versionIds", arrayListOf(id))
                })
                coordinator.playbackIntent()
                value
            }
            assertEquals(androidx.media3.session.SessionError.INFO_CANCELLED, future.get(10, TimeUnit.SECONDS).resultCode)
            assertEquals(0, main { player.mediaItemCount })
        } finally { main { coordinator.close(); player.release() } }
    }
}
