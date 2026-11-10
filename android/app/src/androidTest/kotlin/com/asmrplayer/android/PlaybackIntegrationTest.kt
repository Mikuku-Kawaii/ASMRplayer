package com.asmrplayer.android

import android.content.ComponentName
import android.content.ContentValues
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import androidx.lifecycle.Lifecycle
import androidx.media3.common.*
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.*
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.playback.*
import com.asmrplayer.core.DspConfig
import com.google.common.util.concurrent.ListenableFuture
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.TimeUnit
import kotlin.math.sin
import java.net.ServerSocket
import java.net.InetAddress
import java.net.SocketException
import kotlin.concurrent.thread

/** Synthetic media only; never reads the user's library. */
@UnstableApi
@RunWith(AndroidJUnit4::class)
class PlaybackIntegrationTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun <T> main(block: () -> T): T {
        var value: T? = null
        instrumentation.runOnMainSync { value = block() }
        @Suppress("UNCHECKED_CAST") return value as T
    }
    private fun await(message: String, predicate: () -> Boolean) {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(15)
        while (System.nanoTime() < end) { if (predicate()) return; Thread.sleep(100) }
        fail(message)
    }
    private fun command(p: MediaController, action: String, args: Bundle = Bundle.EMPTY): SessionResult =
        main { p.sendCustomCommand(SessionCommand(action, Bundle.EMPTY), args) }.get(5, TimeUnit.SECONDS)

    @Test fun serviceOwnsPlaybackAcrossBackgroundSeekAndDspUpdates() {
        val uri = audio(20)
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        var future: ListenableFuture<MediaController>? = null
        var profileId: String? = null
        try {
            future = main { MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync() }
            val p = future.get(10, TimeUnit.SECONDS)
            // Explicit fixture setup: production privacy defaults to off.
            command(p,DspSettings.SET_PRIVACY,Bundle().apply{putBoolean("private",true)})
            main {
                p.setMediaItem(MediaItem.Builder().setMediaId("synthetic-p0").setUri(uri).setMediaMetadata(MediaMetadata.Builder().setTitle("Private fixture title").build()).build())
                p.prepare(); p.seekTo(1000); p.play()
            }
            await("Local content URI did not play: ${main { p.playerError?.errorCodeName }}") { main { p.isPlaying && p.currentPosition >= 1000 } }
            val info = command(p, DspSettings.GET_DSP)
            assertEquals(SessionResult.RESULT_SUCCESS, info.resultCode)
            assertTrue("Real AudioSink did not process PCM", info.extras.getLong("processedFrames") > 0)
            assertEquals(48000, info.extras.getInt("sampleRate"))
            assertEquals(2, info.extras.getInt("channels"))
            assertEquals("ASMR Audio Track", main { p.mediaMetadata.title.toString() })
            assertNotNull("Neutral artwork missing", main { p.mediaMetadata.artworkData })
            await("Actual AudioTrack output was not reported") {
                HeadphoneCommands.decode(command(p, HeadphoneCommands.GET).extras.getString("headphones")!!).route != null
            }
            val manager = context.getSystemService(android.app.NotificationManager::class.java)
            await("MediaStyle notification missing") { manager.activeNotifications.any { it.notification.channelId == PlaybackService.MEDIA_CHANNEL } }
            val notification = manager.activeNotifications.first { it.notification.channelId == PlaybackService.MEDIA_CHANNEL }.notification
            assertNotNull(notification.extras.getParcelable<android.media.session.MediaSession.Token>(android.app.Notification.EXTRA_MEDIA_SESSION))
            assertNotNull(notification.contentIntent)
            assertTrue(notification.actions.isNotEmpty())
            val setting = command(p, DspSettings.SET_DSP, Bundle().apply { putString(DspSettings.JSON, DspSettings.encode(DspConfig(highPass = false, ceilingDb = -6.0, warm = true))) })
            assertEquals(SessionResult.RESULT_SUCCESS, setting.resultCode)
            val saved = command(p, HeadphoneCommands.EDIT, Bundle().apply { putString("operation", "save"); putString("name", "Integration headphones") })
            assertEquals(SessionResult.RESULT_SUCCESS, saved.resultCode)
            profileId = HeadphoneCommands.decode(saved.extras.getString("headphones")!!).activeId!!
            command(p, DspSettings.SET_DSP, Bundle().apply { putString(DspSettings.JSON, DspSettings.encode(DspConfig())) })
            val discontinuities = java.util.concurrent.atomic.AtomicInteger()
            val continuity = object : Player.Listener {
                override fun onPositionDiscontinuity(oldPosition: Player.PositionInfo, newPosition: Player.PositionInfo, reason: Int) { discontinuities.incrementAndGet() }
            }
            main { p.addListener(continuity) }
            val switchPosition = main { p.currentPosition }
            val applied = command(p, HeadphoneCommands.EDIT, Bundle().apply { putString("operation", "select"); putString("id", profileId) })
            assertEquals(-6.0, DspSettings.decode(applied.extras.getString(DspSettings.JSON)!!).ceilingDb, 0.0)
            assertTrue(DspSettings.decode(applied.extras.getString(DspSettings.JSON)!!).warm)
            val calibrated=DspConfig(preampDb=-2.0,balanceDb=1.0,bands=listOf(com.asmrplayer.core.PeqBand(250.0,-2.0)),leveler=true,ceilingDb=-6.0)
            com.asmrplayer.core.TonePreset.entries.forEach { preset ->
                val next=preset.applyTo(calibrated)
                val result=command(p,DspSettings.SET_DSP,Bundle().apply{putString(DspSettings.JSON,DspSettings.encode(next))})
                assertEquals(SessionResult.RESULT_SUCCESS,result.resultCode)
                assertEquals(next,DspSettings.decode(command(p,DspSettings.GET_DSP).extras.getString(DspSettings.JSON)!!))
            }
            val afterSwitch = main { p.currentPosition }
            assertEquals("Profile switch caused a seek/transition", 0, discontinuities.get())
            // Controller position extrapolation can adjust by a few milliseconds on a session update.
            assertTrue("Profile switch reset progress: $switchPosition -> $afterSwitch", afterSwitch >= switchPosition - 100)
            assertTrue("Profile switch stopped playback", main { p.isPlaying })
            main { p.removeListener(continuity) }
            val before = main { p.currentPosition }
            scenario.moveToState(Lifecycle.State.CREATED)
            await("Playback stopped when Activity left foreground") { main { p.isPlaying && p.currentPosition > before + 600 } }
            instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_MEDIA_PAUSE").use { it.close() }
            await("System media-key pause failed") { main { !p.isPlaying } }
            instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_MEDIA_PLAY").use { it.close() }
            await("System media-key play failed") { main { p.isPlaying } }
            main { p.pause() }
            await("MediaController pause failed") { main { !p.isPlaying } }
            main { p.seekTo(5000); p.play() }
            await("Seek/play failed") { main { p.isPlaying && p.currentPosition >= 5000 } }
            assertEquals("synthetic-p0", main { p.currentMediaItem?.mediaId })
            command(p, DspSettings.SET_PRIVACY, Bundle().apply { putBoolean("private", false) })
            await("Metadata did not update") { main { p.mediaMetadata.title.toString() == "Private fixture title" } }
            main { p.stop() }
            await("Stop failed") { main { p.playbackState == Player.STATE_IDLE } }
            command(p, DspSettings.SET_DSP, Bundle().apply { putString(DspSettings.JSON, DspSettings.encode(DspConfig())) })
            command(p, DspSettings.SET_PRIVACY, Bundle().apply { putBoolean("private", true) })
        } finally {
            future?.let { f ->
                if (f.isDone && !f.isCancelled) profileId?.let { id -> command(f.get(), HeadphoneCommands.EDIT, Bundle().apply { putString("operation", "remove"); putString("id", id) }) }
                main { if (f.isDone && !f.isCancelled) f.get().stop(); MediaController.releaseFuture(f) }
            }
            scenario.close()
            context.contentResolver.delete(uri, null, null)
        }
    }

    @Test fun naturalEofAdvancesQueueButManualStopDoesNot() {
        val uri = audio(1)
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        val future = main { MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync() }
        try {
            val p = future.get(10, TimeUnit.SECONDS)
            main {
                p.setMediaItems(listOf(MediaItem.Builder().setMediaId("one").setUri(uri).build(), MediaItem.Builder().setMediaId("two").setUri(uri).build()))
                p.prepare(); p.play()
            }
            await("Natural EOF did not advance to second item") { main { p.currentMediaItemIndex == 1 } }
            main { p.pause(); p.seekTo(0, 0); p.prepare(); p.stop() }
            Thread.sleep(1200)
            assertEquals(0, main { p.currentMediaItemIndex })
            assertEquals(Player.STATE_IDLE, main { p.playbackState })
        } finally {
            main { future.get().stop(); MediaController.releaseFuture(future) }
            scenario.close(); context.contentResolver.delete(uri, null, null)
        }
    }

    @Test fun systemArtworkFollowsPrivacyWithoutChangingPlayback() {
        val uri = audio(15)
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        val future = main { MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync() }
        val cover = java.io.File(context.cacheDir, "media-cover-${System.nanoTime()}.png")
        val backend = com.asmrplayer.android.data.AndroidBackend.get(context)
        val id = "artwork-${System.nanoTime()}"
        val original = com.asmrplayer.android.data.LibraryWork(id, null, id, "Private cover fixture")
        try {
            val bitmap = android.graphics.Bitmap.createBitmap(24, 24, android.graphics.Bitmap.Config.ARGB_8888)
            bitmap.eraseColor(android.graphics.Color.MAGENTA)
            cover.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
            kotlinx.coroutines.runBlocking {
                val dao = backend.database.library()
                dao.putWork(original.copy(coverUri = Uri.fromFile(cover).toString()))
                dao.putTrack(com.asmrplayer.android.data.LibraryTrack(id, id, "Private cover fixture", "WAV"))
                dao.putVersion(com.asmrplayer.android.data.TrackVersion(id, id, "wav"))
                dao.putSource(com.asmrplayer.android.data.MediaSource(id, id, null, id, uri.toString()))
            }
            val p = future.get(10, TimeUnit.SECONDS)
            command(p, DspSettings.SET_PRIVACY, Bundle().apply { putBoolean("private", false) })
            main { p.setMediaItem(MediaItem.Builder().setMediaId(id).setUri(uri).build()); p.prepare(); p.play() }
            await("Current work cover was not published") {
                main { p.mediaMetadata.artworkData }?.let { bytes ->
                    android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.let { b ->
                        val matches = b.getPixel(0, 0) == android.graphics.Color.MAGENTA; b.recycle(); matches
                    }
                } == true
            }
            val publicCover = main { p.mediaMetadata.artworkData!!.copyOf() }
            val position = main { p.currentPosition }
            command(p, DspSettings.SET_PRIVACY, Bundle().apply { putBoolean("private", true) })
            await("Cover privacy did not take effect") { main { !p.mediaMetadata.artworkData.contentEquals(publicCover) && p.mediaMetadata.title == "ASMR Audio Track" } }
            assertNull(main { p.mediaMetadata.artworkUri })
            assertTrue(main { p.isPlaying && p.currentPosition >= position })
            command(p, DspSettings.SET_PRIVACY, Bundle().apply { putBoolean("private", false) })
            command(p, DspSettings.SET_PRIVACY, Bundle().apply { putBoolean("private", true) })
            Thread.sleep(500)
            assertFalse("Stale public artwork returned after privacy toggle", main { p.mediaMetadata.artworkData.contentEquals(publicCover) })
        } finally {
            main { if (future.isDone) future.get().stop(); MediaController.releaseFuture(future) }
            scenario.close()
            kotlinx.coroutines.runBlocking { backend.database.library().putWork(original) }
            cover.delete(); context.contentResolver.delete(uri, null, null)
        }
    }

    @Test fun networkStreamTraversesSameDspAndSupportsSeek() {
        val uri = audio(8)
        val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
        val server = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
        val worker = thread(isDaemon = true) {
            try {
                while (!server.isClosed) server.accept().use { socket ->
                    val reader = socket.getInputStream().bufferedReader()
                    var start = 0
                    var range = false
                    while (true) {
                        val line = reader.readLine() ?: break
                        if (line.isEmpty()) break
                        if (line.startsWith("Range:", true)) { range = true; start = line.substringAfter("bytes=").substringBefore('-').trim().toInt() }
                    }
                    val count = bytes.size - start
                    val headers = buildString {
                        append(if (range) "HTTP/1.1 206 Partial Content\r\n" else "HTTP/1.1 200 OK\r\n")
                        append("Content-Type: audio/wav\r\nAccept-Ranges: bytes\r\nContent-Length: $count\r\nConnection: close\r\n")
                        if (range) append("Content-Range: bytes $start-${bytes.lastIndex}/${bytes.size}\r\n")
                        append("\r\n")
                    }
                    try { socket.getOutputStream().apply { write(headers.toByteArray()); write(bytes, start, count); flush() } }
                    catch (_: SocketException) { /* Player may close its old request when seeking. */ }
                }
            } catch (e: SocketException) { if (!server.isClosed) throw e }
        }
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        val future = main { MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync() }
        try {
            val p = future.get(10, TimeUnit.SECONDS)
            main { p.setMediaItem(MediaItem.Builder().setMediaId("network-fixture").setUri("http://127.0.0.1:${server.localPort}/fixture.wav").build()); p.prepare(); p.play() }
            await("Network stream failed") { main { p.isPlaying } }
            assertTrue(command(p, DspSettings.GET_DSP).extras.getLong("processedFrames") > 0)
            main { p.seekTo(4000) }
            await("Network seek failed") { main { p.isPlaying && p.currentPosition >= 4000 } }
        } finally {
            main { future.get().stop(); MediaController.releaseFuture(future) }
            scenario.close(); server.close(); worker.join(2000); context.contentResolver.delete(uri, null, null)
        }
    }

    private fun audio(seconds: Int): Uri {
        val frames = 48000 * seconds
        val dataSize = frames * 4
        val header = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
        header.put("RIFF".toByteArray()).putInt(dataSize+36).put("WAVEfmt ".toByteArray()).putInt(16)
            .putShort(1).putShort(2).putInt(48000).putInt(192000).putShort(4).putShort(16)
            .put("data".toByteArray()).putInt(dataSize)
        val uri = requireNotNull(context.contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, ContentValues().apply {
            put(MediaStore.Audio.Media.DISPLAY_NAME, "ASMR-P0-${System.nanoTime()}.wav")
            put(MediaStore.Audio.Media.MIME_TYPE, "audio/wav")
            put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/ASMR-P0")
        }))
        context.contentResolver.openOutputStream(uri)!!.use { stream ->
            stream.write(header.array())
            val block = ByteBuffer.allocate(48000*4).order(ByteOrder.LITTLE_ENDIAN)
            repeat(48000) { i -> val sample = (sin(i * 2 * Math.PI * 220 / 48000) * 1200).toInt().toShort(); block.putShort(sample).putShort(sample) }
            repeat(seconds) { stream.write(block.array()) }
        }
        return uri
    }
}
