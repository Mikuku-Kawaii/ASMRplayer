package com.asmrplayer.android.playback

import androidx.media3.common.C
import androidx.media3.common.audio.AudioProcessor
import androidx.media3.common.audio.BaseAudioProcessor
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.core.DspConfig
import com.asmrplayer.core.DspEngine
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.roundToInt

/** AN003: Media3 1.8's processing path uses PCM16. DSP itself runs in floating point.
 * Always active, including bypass, so live toggles never reconfigure or reload the player.
 * Only the playback thread touches the engine; UI sends immutable snapshots atomically.
 */
@UnstableApi
class DspAudioProcessor(private val config: AtomicReference<DspConfig>,
    private val normalization: AtomicReference<Double> = AtomicReference(0.0)) : BaseAudioProcessor() {
    private var engine: DspEngine? = null
    private val input = FloatArray(2048)
    private val output = FloatArray(2048)
    @Volatile var processedFrames: Long = 0; private set
    @Volatile var sampleRate: Int = 0; private set
    @Volatile var channels: Int = 0; private set

    override fun onConfigure(format: AudioProcessor.AudioFormat): AudioProcessor.AudioFormat {
        if (format.encoding != C.ENCODING_PCM_16BIT || format.channelCount !in 1..2 || format.sampleRate !in 8000..384000) {
            throw AudioProcessor.UnhandledAudioFormatException(format)
        }
        return format
    }

    override fun onFlush() {
        if (inputAudioFormat == AudioProcessor.AudioFormat.NOT_SET) return
        sampleRate = inputAudioFormat.sampleRate
        channels = inputAudioFormat.channelCount
        engine = DspEngine(sampleRate, channels, config.get())
        processedFrames = 0
    }

    override fun queueInput(buffer: ByteBuffer) {
        val dsp = checkNotNull(engine)
        buffer.order(ByteOrder.LITTLE_ENDIAN)
        val count = minOf(buffer.remaining() / 2, input.size).let { it - it % channels }
        check(count > 0 || !buffer.hasRemaining()) { "Unaligned PCM frame" }
        if (count == 0) return
        for (i in 0 until count) input[i] = buffer.short / 32768f
        dsp.update(config.get())
        dsp.setNormalizationGain(normalization.get())
        val written = dsp.processInto(input, count, output)
        writeSamples(output, written)
        processedFrames += count / channels
    }

    override fun onQueueEndOfStream() {
        val tail = engine?.drain() ?: return
        writeSamples(tail, tail.size)
    }

    private fun writeSamples(samples: FloatArray, count: Int) {
        val buffer = replaceOutputBuffer(count * 2).order(ByteOrder.LITTLE_ENDIAN)
        for (i in 0 until count) {
            buffer.putShort((samples[i] * 32768f).roundToInt().coerceIn(-32768, 32767).toShort())
        }
        buffer.flip()
    }

    override fun onReset() {
        engine = null
        sampleRate = 0
        channels = 0
        processedFrames = 0
    }
}
