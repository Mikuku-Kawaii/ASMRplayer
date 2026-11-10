package com.asmrplayer.android.online

import android.content.ComponentName
import android.net.Uri
import androidx.activity.compose.setContent
import androidx.compose.runtime.*
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSpec
import androidx.media3.session.*
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.artwork.CoverArtwork
import com.asmrplayer.android.data.CacheMetrics
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.playback.*
import com.asmrplayer.android.ui.rememberArtwork
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayOutputStream
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

@UnstableApi
@RunWith(AndroidJUnit4::class)
class PlaybackCacheTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext.also { check(it.packageName.endsWith(".uitest")) }
    private val backend = AndroidBackend.get(context)
    private fun <T> main(block: () -> T): T {
        val task = java.util.concurrent.FutureTask(block)
        instrumentation.runOnMainSync(task)
        return task.get()
    }

    @Test fun remountCardAndPlayer100TimesWithWarmFirstFramesAndOneImageTransfer() {
        OnlineFixtureServer().use { server ->
            val url = server.base + "/media/image"
            runBlocking {
                // Concurrent reads at different sizes share the compressed source on disk.
                coroutineScope { listOf(384, 768, 1536).map { edge ->
                    async { requireNotNull(CoverArtwork.best(context, url, null, edge)) }
                }.awaitAll() }
            }
            assertEquals(1, server.mediaRequests.get())
            server.mode = "unauthorized"
            ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                val page = mutableIntStateOf(0)
                val firstFrames = LinkedBlockingQueue<Pair<Int, Boolean>>()
                scenario.onActivity { activity -> activity.setContent {
                    key(page.intValue) {
                        val index = page.intValue
                        val art = rememberArtwork(url, highResolution = index % 2 == 1)
                        val first = remember { art.image != null && !art.loading }
                        DisposableEffect(index) { firstFrames.add(index to first); onDispose {} }
                    }
                } }
                val warmDecodes = CacheMetrics.imageDecodes.get()
                for (index in 0..100) {
                    if (index > 0) main { page.intValue = index }
                    assertEquals(index to true, firstFrames.poll(8, TimeUnit.SECONDS))
                }
                assertEquals(1, server.mediaRequests.get())
                assertEquals(warmDecodes, CacheMetrics.imageDecodes.get())
            }
        }
    }

    @Test fun libraryAndFullPlayerNavigate100TimesWhileSameAudioKeepsPlaying() {
        val server = OnlineFixtureServer(durationSeconds = 360, sampleRate = 8000).apply { attachments = true }
        val oldBase = backend.online.account.state.value.apiBase
        val version = runBlocking {
            backend.online.account.setApiBase(server.base)
            backend.online.importWork("123456").versionIds.single()
        }
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        val future = main { MediaController.Builder(context, SessionToken(context,
            ComponentName(context, PlaybackService::class.java))).buildAsync() }
        fun waitUntil(message: String, condition: () -> Boolean) {
            val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(12)
            while (System.nanoTime() < end) { if (condition()) return; Thread.sleep(30) }
            fail(message)
        }
        fun findClickTarget(label: String): android.view.accessibility.AccessibilityNodeInfo? {
            fun find(node: android.view.accessibility.AccessibilityNodeInfo): android.view.accessibility.AccessibilityNodeInfo? {
                if (node.isVisibleToUser && (node.contentDescription?.toString() == label ||
                    node.text?.toString()?.contains(label) == true)) {
                    var target: android.view.accessibility.AccessibilityNodeInfo? = node
                    while (target != null && !target.isClickable) target = target.parent
                    if (target != null) return target
                }
                for (i in 0 until node.childCount) node.getChild(i)?.let { find(it) }?.let { return it }
                return null
            }
            return instrumentation.uiAutomation.rootInActiveWindow?.let(::find)
        }
        fun click(label: String) = waitUntil("Missing visible control: $label") {
            findClickTarget(label)?.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK) == true
        }
        fun roundTrip() {
            click("正在播放 ·")
            click("收起播放页")
            waitUntil("Mini player did not return") { findClickTarget("正在播放 ·") != null }
        }
        try {
            val controller = future.get(10, TimeUnit.SECONDS)
            main { LibraryPlayback.play(controller, listOf(version), resume = false) }.get(10, TimeUnit.SECONDS)
            waitUntil("Silent fixture did not play") { main { controller.isPlaying && controller.currentPosition > 300 } }
            roundTrip()
            val opens = CacheMetrics.streamOpens.get()
            val start = main { controller.currentPosition }
            repeat(50) { roundTrip() }
            assertTrue(main { controller.isPlaying && controller.currentMediaItem?.mediaId == version &&
                controller.currentPosition > start + 1000 })
            assertEquals("Navigation reopened the audio source", opens, CacheMetrics.streamOpens.get())
        } finally {
            main {
                if (future.isDone && !future.isCancelled) { future.get().stop(); future.get().clearMediaItems() }
                MediaController.releaseFuture(future)
            }
            scenario.close(); server.close()
            runBlocking { backend.online.account.setApiBase(oldBase) }
        }
    }

    @Test fun repeatedDetailsAndImportShareOneTreeAndOfflineReplayUsesDisk() {
        val server = OnlineFixtureServer()
        val account = backend.online.account
        val oldBase = account.state.value.apiBase
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        val future = main { MediaController.Builder(context, SessionToken(context,
            ComponentName(context, PlaybackService::class.java))).buildAsync() }
        try {
            val version = runBlocking {
                account.setApiBase(server.base)
                val details = coroutineScope { List(8) { async { backend.online.detail("123456") } }.awaitAll() }
                repeat(100) { assertSame(details.first(), backend.online.cachedDetail(server.base, "RJ123456")) }
                backend.online.importWork("123456", reuseDetail = true).versionIds.single()
            }
            assertEquals(1, server.treeRequests.get())
            val controller = future.get(10, TimeUnit.SECONDS)
            fun play() {
                val result = main { LibraryPlayback.play(controller, listOf(version), resume = false) }
                assertEquals(SessionResult.RESULT_SUCCESS, result.get(10, TimeUnit.SECONDS).resultCode)
                val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(12)
                while (System.nanoTime() < deadline) {
                    if (main { controller.isPlaying && controller.currentPosition >= 400 }) return
                    Thread.sleep(30)
                }
                fail("Playback did not advance: " + main { controller.playerError?.errorCodeName })
            }
            play()
            main { controller.stop(); controller.clearMediaItems() }
            val requests = server.requests.size
            // Take the only source completely offline. A new data source cannot use ExoPlayer's RAM buffer.
            server.close()
            val networkBytes = CacheMetrics.networkBytes.get()
            val source = VersionDataSource.Factory(context, backend).createDataSource()
            val start = 8192
            val count = 16000
            val spec = DataSpec.Builder().setUri(Uri.parse("asmr://version/$version"))
                .setPosition(start.toLong()).setLength(count.toLong()).build()
            val bytes = ByteArrayOutputStream()
            try {
                source.open(spec)
                val buffer = ByteArray(4096)
                while (true) {
                    val n = source.read(buffer, 0, buffer.size)
                    if (n < 0) break
                    bytes.write(buffer, 0, n)
                }
            } finally { source.close() }
            assertArrayEquals(server.bytes.copyOfRange(start, start + count), bytes.toByteArray())
            play()
            main { controller.seekTo(100) }
            val loop = main { LibraryPlayback.setAb(controller, version, 200, 900) }
            assertEquals(SessionResult.RESULT_SUCCESS, loop.get(5, TimeUnit.SECONDS).resultCode)
            var previous = -1L
            var wraps = 0
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(6)
            while (wraps < 2 && System.nanoTime() < deadline) {
                val position = main { controller.currentPosition }
                if (position + 250 < previous) wraps++
                previous = position
                Thread.sleep(50)
            }
            assertTrue("No repeated offline A–B playback", wraps >= 2)
            assertEquals(requests, server.requests.size)
            assertEquals(networkBytes, CacheMetrics.networkBytes.get())
        } finally {
            main {
                if (future.isDone && !future.isCancelled) { future.get().stop(); future.get().clearMediaItems() }
                MediaController.releaseFuture(future)
            }
            scenario.close(); server.close()
            runBlocking { account.setApiBase(oldBase) }
        }
    }
}
