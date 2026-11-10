package com.asmrplayer.android.online

import android.content.ComponentName
import android.os.Bundle
import androidx.lifecycle.Lifecycle
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.*
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.playback.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.TimeUnit

@UnstableApi
@RunWith(AndroidJUnit4::class)
class OnlinePlaybackAndWorkerTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun <T> main(block: () -> T): T {
        var value: T? = null; instrumentation.runOnMainSync { value = block() }
        @Suppress("UNCHECKED_CAST") return value as T
    }
    private fun await(message: String, condition: () -> Boolean) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(20)
        while (System.nanoTime() < deadline) { if (condition()) return; Thread.sleep(50) }
        fail(message)
    }
    @Test fun serviceResolvesFreshUrlsAndBackgroundWorkerCreatesSameVersionOfflineSource() {
        val backend = AndroidBackend.get(context)
        val server = OnlineFixtureServer().apply { attachments = true }
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        val future = main { MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync() }
        var downloadId: String? = null
        var attachmentTasks = emptyList<String>()
        try {
            val controller = future.get(10, TimeUnit.SECONDS)
            val imported = runBlocking {
                backend.online.account.setApiBase(server.base)
                backend.online.importWork("RJ123456")
            }
            val version = imported.versionIds.single()
            server.mode = "expired"
            assertEquals(SessionResult.RESULT_SUCCESS, main { LibraryPlayback.play(controller, listOf(version), resume = false) }.get(10, TimeUnit.SECONDS).resultCode)
            await("Online version did not play: ${main { controller.playerError?.errorCodeName }}") { main { controller.isPlaying && controller.currentPosition >= 100 } }
            assertTrue("Expired stream URL was not refreshed", server.mediaRequests.get() >= 2)
            val info = main { controller.sendCustomCommand(SessionCommand(DspSettings.GET_DSP, Bundle.EMPTY), Bundle.EMPTY) }.get(5, TimeUnit.SECONDS)
            assertTrue(info.extras.getLong("processedFrames") > 0)
            assertEquals(version, main { controller.currentMediaItem?.mediaId })
            main { controller.pause(); controller.seekTo(1200) }
            main { LibraryPlayback.save(controller) }.get(5, TimeUnit.SECONDS)
            server.mode = "normal"
            downloadId = runBlocking { backend.downloads.enqueue(listOf(version)).single() }
            attachmentTasks = runBlocking { backend.downloads.enqueueAssets(imported.assetIds) + backend.downloads.cacheCover(imported.workId) }
            scenario.moveToState(Lifecycle.State.CREATED)
            runBlocking { withTimeout(40_000) { backend.downloads.tasks.first { tasks -> (attachmentTasks + downloadId!!).all { id -> tasks.any { it.id == id && it.state == "COMPLETED" } } } } }
            assertTrue(runBlocking { backend.library.resolve(version).uri.startsWith("file:") })
            assertTrue(runBlocking { backend.library.work(imported.workId)!!.coverUri!!.startsWith("file:") })
            val before = server.mediaRequests.get()
            server.mode = "unauthorized"
            main { controller.stop(); controller.clearMediaItems() }
            assertEquals(SessionResult.RESULT_SUCCESS, main { LibraryPlayback.play(controller, listOf(version)) }.get(10, TimeUnit.SECONDS).resultCode)
            await("Offline source did not play without network") { main { controller.isPlaying && controller.currentPosition >= 1200 } }
            assertEquals(before, server.mediaRequests.get())
            assertEquals(version, main { controller.currentMediaItem?.mediaId })
        } finally {
            main { if (future.isDone && !future.isCancelled) { future.get().stop(); future.get().clearMediaItems() }; MediaController.releaseFuture(future) }
            runBlocking {
                downloadId?.let { backend.downloads.remove(it) }
                attachmentTasks.forEach { backend.downloads.remove(it) }
                backend.online.account.setToken(null); backend.online.account.setApiBase(ProviderAccount.DEFAULT_BASE)
            }
            scenario.close(); server.close()
        }
    }
}
