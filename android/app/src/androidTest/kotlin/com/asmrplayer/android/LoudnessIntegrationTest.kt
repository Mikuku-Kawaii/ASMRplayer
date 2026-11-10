package com.asmrplayer.android

import android.content.ComponentName
import android.content.Context
import android.net.Uri
import android.os.Bundle
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.*
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.playback.*
import com.asmrplayer.core.DspConfig
import kotlinx.coroutines.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.TimeUnit
import kotlin.math.*

@UnstableApi
@RunWith(AndroidJUnit4::class)
class LoudnessIntegrationTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun <T> main(block: () -> T): T {
        var result: T? = null; instrumentation.runOnMainSync { result = block() }
        @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun wav(file: File, amplitude: Double, seconds: Int = 8) {
        val bytes = 48000*seconds*4
        val header = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
        header.put("RIFF".toByteArray()).putInt(bytes+36).put("WAVEfmt ".toByteArray()).putInt(16)
            .putShort(1).putShort(2).putInt(48000).putInt(192000).putShort(4).putShort(16).put("data".toByteArray()).putInt(bytes)
        file.outputStream().use { stream ->
            stream.write(header.array())
            val block = ByteBuffer.allocate(192000).order(ByteOrder.LITTLE_ENDIAN)
            repeat(48000) { val sample = (sin(2*PI*1000*it/48000)*amplitude*32767).roundToInt().toShort(); block.putShort(sample).putShort(sample) }
            repeat(seconds) { stream.write(block.array()) }
        }
    }
    @Test fun scannerDecodesWavAndMp3AndHonorsCancellation() = runBlocking {
        val wav = File(context.cacheDir, "loudness-synthetic.wav")
        val mp3 = File(context.cacheDir, "loudness-synthetic.mp3")
        try {
            wav(wav, 10.0.pow(-23.0/20))
            var progress = -1
            val result = LoudnessScanner(context).scan(Uri.fromFile(wav)) { progress = it }
            assertEquals(-23.0, result.lufs!!, 0.1); assertEquals(48000L*8, result.frames); assertTrue(progress > 50)
            instrumentation.context.assets.open("stereo_lr_check.mp3").use { input -> mp3.outputStream().use { input.copyTo(it) } }
            val compressed = LoudnessScanner(context).scan(Uri.fromFile(mp3)) {}
            assertTrue(compressed.frames > 48000); assertNotNull(compressed.lufs)
            val task = launch(Dispatchers.Default) {
                LoudnessScanner(context).scan(Uri.fromFile(wav)) { cancel() }
            }
            task.join(); assertTrue(task.isCancelled)
        } finally { wav.delete(); mp3.delete() }
    }
    @Test fun oldSettingsAndIndependentSwitchesSurviveHeadphoneProfiles() {
        val old = JSONObject(DspSettings.encode(DspConfig())).apply {
            remove("leveler"); remove("normalize"); remove("targetLufs"); remove("maxBoostDb")
        }
        assertEquals(DspConfig(), DspSettings.decode(old.toString()))
        val desired = DspConfig(leveler = true, normalize = true, targetLufs = -25.0, maxBoostDb = 3.0)
        assertEquals(desired, DspSettings.decode(DspSettings.encode(desired)))
        val prefs = context.getSharedPreferences("loudness-test-${System.nanoTime()}", Context.MODE_PRIVATE)
        try {
            var actual = DspConfig()
            val first = HeadphonePreferences(prefs, actual) { actual = it }
            first.edited(desired); first.save("Loudness"); val id = first.state.activeId
            val restored = HeadphonePreferences(prefs, actual) { actual = it }
            restored.select(id); assertEquals(desired, actual)
            restored.edited(desired.copy(leveler = false)); restored.save("Track only", id)
            assertTrue(restored.state.profiles.single().config.normalize)
            assertFalse(restored.state.profiles.single().config.leveler)
        } finally { prefs.edit().clear().commit() }
    }
    @Test fun serviceAppliesVersionGainWithoutChangingQueueOrPausedPosition() = runBlocking {
        val prefix = "loudness-${System.nanoTime()}"
        val folder = File(context.noBackupFilesDir, "offline").apply { mkdirs() }
        val file = File(folder, "$prefix.wav"); wav(file, 0.2, 20)
        val dao = AndroidBackend.get(context).database.library()
        dao.putWork(LibraryWork(prefix, null, prefix, "Loudness synthetic", rating = "SAFE"))
        dao.putTrack(LibraryTrack("$prefix-t", prefix, "Synthetic 1kHz", ""))
        dao.putVersion(TrackVersion(prefix, "$prefix-t", "wav"))
        dao.putSource(MediaSource(prefix, prefix, null, "test.wav", Uri.fromFile(file).toString(), kind = "local_downloaded"))
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        val future = main { MediaController.Builder(context, SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync() }
        val p = future.get(15, TimeUnit.SECONDS)
        fun command(action: String, args: Bundle = Bundle.EMPTY): SessionResult = main {
            p.sendCustomCommand(SessionCommand(action, Bundle.EMPTY), args)
        }.get(15, TimeUnit.SECONDS)
        val original = command(DspSettings.GET_DSP).extras.getString(DspSettings.JSON)!!
        try {
            main { LibraryPlayback.play(p, listOf(prefix), resume = false, startPositionMs = 1500) }.get(15, TimeUnit.SECONDS)
            withTimeout(10000) { while (!main { p.isPlaying }) delay(50) }
            main { p.pause() }
            // Controller positions are extrapolated between remote updates. Compare the
            // service's persisted position, not an estimated UI clock sampled before its update.
            assertEquals(0, command(LibraryPlayback.SAVE_SESSION).resultCode)
            var position = requireNotNull(dao.session()).positionMs
            withTimeout(5000) {
                var stable = 0
                while (stable < 3) {
                    delay(100)
                    assertEquals(0, command(LibraryPlayback.SAVE_SESSION).resultCode)
                    val next = requireNotNull(dao.session()).positionMs
                    stable = if (next == position) stable+1 else 0
                    position = next
                }
            }
            val cfg = DspConfig(normalize = true, leveler = false)
            assertEquals(0, command(DspSettings.SET_DSP, Bundle().apply { putString(DspSettings.JSON, DspSettings.encode(cfg)) }).resultCode)
            var state = Bundle()
            withTimeout(20000) {
                do { delay(100); state = command(DspSettings.GET_DSP).extras } while (state.getBoolean("loudnessBusy"))
            }
            assertTrue(state.getString("loudnessStatus").orEmpty(), state.getString("loudnessStatus")!!.contains("LUFS"))
            assertEquals(-9.0, state.getDouble("normalizationGainDb"), 0.2)
            assertEquals(prefix, main { p.currentMediaItem!!.mediaId }); assertFalse(main { p.playWhenReady })
            assertEquals(0, command(LibraryPlayback.SAVE_SESSION).resultCode)
            assertEquals(position, requireNotNull(dao.session()).positionMs)
            command(DspSettings.SET_DSP, Bundle().apply { putString(DspSettings.JSON, DspSettings.encode(cfg.copy(normalize = false, leveler = true))) })
            assertEquals(0.0, command(DspSettings.GET_DSP).extras.getDouble("normalizationGainDb"), 0.0)
            // Cache hit and a new target must adjust the same measured version without rescan.
            command(DspSettings.SET_DSP, Bundle().apply { putString(DspSettings.JSON, DspSettings.encode(cfg.copy(targetLufs = -26.0))) })
            withTimeout(10000) { while (command(DspSettings.GET_DSP).extras.getBoolean("loudnessBusy")) delay(50) }
            assertEquals(-12.0, command(DspSettings.GET_DSP).extras.getDouble("normalizationGainDb"), 0.2)
            command(DspSettings.ANALYZE)
            command(DspSettings.CANCEL_ANALYSIS)
            assertFalse(command(DspSettings.GET_DSP).extras.getBoolean("loudnessBusy"))
            main { p.clearMediaItems() }
            assertEquals(0.0, command(DspSettings.GET_DSP).extras.getDouble("normalizationGainDb"), 0.0)
        } finally {
            command(DspSettings.SET_DSP, Bundle().apply { putString(DspSettings.JSON, original) })
            main { p.stop(); p.clearMediaItems(); MediaController.releaseFuture(future) }; scenario.close(); file.delete()
        }
    }
}
