package com.asmrplayer.android.playback

import androidx.media3.common.C
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.core.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.abs

@UnstableApi
@RunWith(AndroidJUnit4::class)
class DspAudioProcessorTest {
    private fun processor(config: AtomicReference<DspConfig>, rate: Int = 48000, channels: Int = 2) = DspAudioProcessor(config).apply {
        configure(AudioProcessor.AudioFormat(rate, channels, C.ENCODING_PCM_16BIT)); flush()
    }
    private fun consume(p: DspAudioProcessor, samples: ShortArray): ShortArray {
        val buffer = ByteBuffer.allocateDirect(samples.size*2).order(ByteOrder.LITTLE_ENDIAN)
        samples.forEach { buffer.putShort(it) }; buffer.flip()
        val output = ArrayList<Short>()
        while (buffer.hasRemaining()) { p.queueInput(buffer); take(p, output) }
        p.queueEndOfStream(); take(p, output)
        assertTrue(p.isEnded)
        return output.toShortArray()
    }
    private fun take(p: DspAudioProcessor, target: MutableList<Short>) {
        val result = p.output.order(ByteOrder.LITTLE_ENDIAN)
        while (result.hasRemaining()) target.add(result.short)
    }
    @Test fun pcm16BypassIsExactForExtremaLargeAndTinyBuffers() {
        for (size in listOf(2, 16, 384, 20000)) {
            val input = ShortArray(size) { listOf(-32768, -1000, 0, 32767)[it%4].toShort() }
            assertArrayEquals(input, consume(processor(AtomicReference(DspConfig(bypass = true))), input))
        }
    }
    @Test fun flushDiscardsOldTrackTailAndReconfiguresSampleRate() {
        val p = processor(AtomicReference(DspConfig(bypass = true)))
        val buffer = ByteBuffer.allocateDirect(100).order(ByteOrder.LITTLE_ENDIAN)
        repeat(50) { buffer.putShort(30000) }; buffer.flip(); p.queueInput(buffer)
        p.configure(AudioProcessor.AudioFormat(8000, 1, C.ENCODING_PCM_16BIT)); p.flush()
        assertArrayEquals(ShortArray(40), consume(p, ShortArray(40)))
        assertEquals(8000, p.sampleRate)
    }
    @Test fun limiterRunsOnActualProcessorOutputAndLinksStereo() {
        val p = processor(AtomicReference(DspConfig(highPass = false, ceilingDb = -6.0)))
        val input = ShortArray(12000) { if (it%2 == 0) 30000 else 15000 }
        val out = consume(p, input)
        assertEquals(input.size, out.size)
        assertTrue(out.all { abs(it.toInt()) <= 16423 })
        for (i in out.indices step 2) assertTrue(abs(out[i]/2.0 - out[i+1]) <= 1)
        assertEquals(6000L, p.processedFrames)
    }
    @Test fun hotConfigIsAppliedWithoutReconfigureAndResetReleasesEngine() {
        val config = AtomicReference(DspConfig(bypass = true))
        val p = processor(config)
        config.set(DspConfig(highPass = false, ceilingDb = -12.0))
        val result = consume(p, ShortArray(10000) { 32000 })
        assertTrue(result.all { abs(it.toInt()) <= 8232 })
        p.reset(); assertEquals(0, p.sampleRate)
    }
    @Test fun rejectsUnsupportedPcmRatherThanSilentlySkippingEffects() {
        val p = DspAudioProcessor(AtomicReference(DspConfig()))
        assertThrows(AudioProcessor.UnhandledAudioFormatException::class.java) { p.configure(AudioProcessor.AudioFormat(48000, 2, C.ENCODING_PCM_FLOAT)) }
        assertThrows(AudioProcessor.UnhandledAudioFormatException::class.java) { p.configure(AudioProcessor.AudioFormat(48000, 6, C.ENCODING_PCM_16BIT)) }
    }
    @Test fun settingsRoundTripAndRejectNonFiniteParameters() {
        val config = DspConfig(bypass = true, bands = listOf(PeqBand(1000.0, 3.0)))
        assertEquals(config, DspSettings.decode(DspSettings.encode(config)))
        assertThrows(Exception::class.java) { DspSettings.decode("{}") }
    }
}
