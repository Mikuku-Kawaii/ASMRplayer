package com.asmrplayer.android.online

import android.content.ComponentName
import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.data.CacheMetrics
import com.asmrplayer.android.playback.LibraryPlayback
import com.asmrplayer.android.playback.PlaybackService
import com.asmrplayer.android.ui.Destination
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.math.abs

@UnstableApi
@RunWith(AndroidJUnit4::class)
class HomePagerTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    private fun touch(action: Int, point: Offset, down: Long) {
        val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, point.x, point.y, 0)
        event.source = InputDevice.SOURCE_TOUCHSCREEN
        try { instrumentation.sendPointerSync(event) } finally { event.recycle() }
    }

    private fun tap(point: Offset) {
        val down = SystemClock.uptimeMillis()
        touch(MotionEvent.ACTION_DOWN, point, down)
        touch(MotionEvent.ACTION_UP, point, down)
    }

    private fun tab(page: Destination) = ui.onNode(hasText(page.title) and isSelectable())
    private fun awaitPage(page: Destination) {
        ui.waitUntil(8000) { ui.onAllNodes(hasText(page.title) and isSelected()).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test fun reverseDragAndRapidDockTapsKeepWarmArtworkAndPlayback() {
        val context = instrumentation.targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        val server = OnlineFixtureServer(durationSeconds = 240, sampleRate = 8000).apply { attachments = true }
        val future = ui.runOnUiThread {
            MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync()
        }
        var fixtureId: String? = null
        try {
            val version = runBlocking {
                backend.online.account.setApiBase(server.base)
                val work = backend.online.importWork("123456")
                fixtureId = work.workId
                backend.library.setFavorite(work.workId, true)
                work.versionIds.single()
            }
            val controller = future.get(10, TimeUnit.SECONDS)
            ui.runOnUiThread { LibraryPlayback.play(controller, listOf(version), resume = false) }.get(10, TimeUnit.SECONDS)
            ui.waitUntil(10000) { ui.runOnUiThread { controller.isPlaying && controller.currentPosition > 300 } }
            val pages = Destination.entries
            val centers = pages.map { tab(it).fetchSemanticsNode().boundsInRoot.center }
            val library = ui.onNodeWithTag("home-page-Library", useUnmergedTree = true)
            val lens = ui.onNodeWithTag("home-lens", useUnmergedTree = true)
            val bounds = library.fetchSemanticsNode().boundsInRoot
            val start = Offset(bounds.left + bounds.width * .84f, bounds.top + bounds.height * .51f)
            val lensStart = lens.fetchSemanticsNode().positionInRoot.x
            val step = centers[1].x - centers[0].x
            val down = SystemClock.uptimeMillis()
            fun move(distance: Float) {
                touch(MotionEvent.ACTION_MOVE, start.copy(x = start.x - bounds.width * distance), down)
            }
            fun checkLens(range: ClosedFloatingPointRange<Float>) {
                ui.waitUntil(5000) {
                    val slide = (bounds.left - library.fetchSemanticsNode().positionInRoot.x) / bounds.width
                    val highlight = (lens.fetchSemanticsNode().positionInRoot.x - lensStart) / step
                    (slide in range && abs(slide - highlight) < .035f).also {
                        if (it) android.util.Log.i("HomePagerTest", "page=$slide lens=$highlight")
                    }
                }
            }
            fun discoveryRequests() = server.requests.count { it.path.startsWith("/api/recommender") || it.path.startsWith("/api/search") || it.path.startsWith("/api/works") }
            val beforePreview = discoveryRequests()
            touch(MotionEvent.ACTION_DOWN, start, down)
            move(.08f); move(.52f)
            checkLens(.38f.. .55f)
            move(.20f)
            checkLens(.08f.. .26f)
            move(.01f)
            touch(MotionEvent.ACTION_UP, start, down)
            awaitPage(Destination.Library)
            assertEquals("Preview activated discovery", beforePreview, discoveryRequests())

            // Commit a second drag, this time crossing the threshold and releasing on discovery.
            val nextDown = SystemClock.uptimeMillis()
            touch(MotionEvent.ACTION_DOWN, start, nextDown)
            for (n in 1..8) {
                touch(MotionEvent.ACTION_MOVE, start.copy(x = start.x - bounds.width * n / 10f), nextDown)
                SystemClock.sleep(16)
            }
            touch(MotionEvent.ACTION_UP, start.copy(x = start.x - bounds.width * .8f), nextDown)
            awaitPage(Destination.Discover)
            ui.waitUntil(6000) { discoveryRequests() > beforePreview }
            for (page in listOf(Destination.Favorites, Destination.Queue, Destination.Library)) {
                tap(centers[page.ordinal]); awaitPage(page)
            }
            ui.waitUntil(6000) { CacheMetrics.imageDecodes.get() > 0 && server.requests.any { it.path.startsWith("/media/cover") } }
            val interruptions = AtomicInteger()
            val listener = object : Player.Listener {
                override fun onIsPlayingChanged(isPlaying: Boolean) { if (!isPlaying) interruptions.incrementAndGet() }
            }
            ui.runOnUiThread { controller.addListener(listener) }
            val startMs = ui.runOnUiThread { controller.currentPosition }
            val opens = CacheMetrics.streamOpens.get()
            val fetches = CacheMetrics.imageFetches.get()
            val decodes = CacheMetrics.imageDecodes.get()
            val buffering = CacheMetrics.bufferingMs.get()
            repeat(12) {
                for (i in listOf(1, 3, 2)) { tap(centers[i]); SystemClock.sleep(35) }
                awaitPage(Destination.Favorites)
                tap(centers[0]); awaitPage(Destination.Library)
            }
            ui.runOnUiThread {
                controller.removeListener(listener)
                assertEquals(version, controller.currentMediaItem?.mediaId)
                assertTrue(controller.isPlaying && controller.currentPosition > startMs + 1000)
            }
            android.util.Log.i("HomePagerTest", "12 rapid sequences: opens=$opens->${CacheMetrics.streamOpens.get()} " +
                "fetches=$fetches->${CacheMetrics.imageFetches.get()} decodes=$decodes->${CacheMetrics.imageDecodes.get()} " +
                "bufferMs=$buffering->${CacheMetrics.bufferingMs.get()} interruptions=${interruptions.get()}")
            assertEquals(0, interruptions.get())
            assertEquals(opens, CacheMetrics.streamOpens.get())
            assertEquals(buffering, CacheMetrics.bufferingMs.get())
            assertEquals(fetches, CacheMetrics.imageFetches.get())
            assertEquals(decodes, CacheMetrics.imageDecodes.get())
        } finally {
            ui.runOnUiThread {
                if (future.isDone && !future.isCancelled) { future.get().stop(); future.get().clearMediaItems() }
                MediaController.releaseFuture(future)
            }
            server.close()
            runBlocking {
                fixtureId?.let { backend.library.setFavorite(it, false); backend.library.setMembership(it, false) }
                backend.online.account.setApiBase(oldBase)
            }
        }
    }
}
