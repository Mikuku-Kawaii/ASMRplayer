package com.asmrplayer.android.playback

import android.content.Context
import android.media.AudioFormat
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.net.Uri
import android.os.SystemClock
import com.asmrplayer.core.IntegratedLoudness
import com.asmrplayer.core.LoudnessMeasurement
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Decode an entire authorized local source without rendering or changing the playback session. */
internal class LoudnessScanner(private val context: Context) {
    suspend fun scan(uri: Uri, progress: (Int) -> Unit): LoudnessMeasurement {
        require(uri.scheme in setOf("content", "file"))
        val extractor = MediaExtractor()
        var codec: MediaCodec? = null
        try {
            if (uri.scheme == "file") extractor.setDataSource(requireNotNull(uri.path))
            else extractor.setDataSource(context, uri, null)
            currentCoroutineContext().ensureActive()
            val track = (0 until extractor.trackCount).firstOrNull {
                extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true
            } ?: error("No audio track")
            extractor.selectTrack(track)
            val source = extractor.getTrackFormat(track)
            val duration = if (source.containsKey(MediaFormat.KEY_DURATION)) source.getLong(MediaFormat.KEY_DURATION) else 0L
            var rate = source.getInteger(MediaFormat.KEY_SAMPLE_RATE)
            var channels = source.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
            var encoding = if (source.containsKey("pcm-encoding")) source.getInteger("pcm-encoding") else AudioFormat.ENCODING_PCM_16BIT
            var meter: IntegratedLoudness? = null
            val floats = FloatArray(4096)
            fun consume(buffer: ByteBuffer) {
                val current = meter ?: IntegratedLoudness(rate, channels).also { meter = it }
                buffer.order(ByteOrder.LITTLE_ENDIAN)
                val bytes = when (encoding) {
                    AudioFormat.ENCODING_PCM_8BIT -> 1
                    AudioFormat.ENCODING_PCM_16BIT -> 2
                    AudioFormat.ENCODING_PCM_FLOAT, 22 /* PCM32 */ -> 4
                    21 /* PCM24 packed */ -> 3
                    else -> error("Unsupported PCM")
                }
                require(buffer.remaining() % (bytes*channels) == 0)
                while (buffer.hasRemaining()) {
                    val count = minOf(buffer.remaining()/bytes, floats.size).let { it-it%channels }
                    for (i in 0 until count) floats[i] = when (encoding) {
                        AudioFormat.ENCODING_PCM_8BIT -> ((buffer.get().toInt() and 255)-128)/128f
                        AudioFormat.ENCODING_PCM_16BIT -> buffer.short/32768f
                        AudioFormat.ENCODING_PCM_FLOAT -> buffer.float
                        22 -> (buffer.int/2147483648.0).toFloat()
                        else -> {
                            val low = buffer.get().toInt() and 255; val mid = buffer.get().toInt() and 255; val high = buffer.get().toInt()
                            (low or (mid shl 8) or (high shl 16))/8388608f
                        }
                    }
                    current.add(floats, count)
                }
            }
            fun report(timeUs: Long) { if (duration > 0) progress((timeUs.coerceAtLeast(0)*100/duration).coerceIn(0, 99).toInt()) }
            val mime = requireNotNull(source.getString(MediaFormat.KEY_MIME))
            if (mime == "audio/raw") {
                val maxSize = if (source.containsKey(MediaFormat.KEY_MAX_INPUT_SIZE)) source.getInteger(MediaFormat.KEY_MAX_INPUT_SIZE) else 262144
                require(maxSize <= 4*1024*1024)
                val buffer = ByteBuffer.allocateDirect(maxOf(maxSize, 262144))
                while (true) {
                    currentCoroutineContext().ensureActive(); buffer.clear()
                    val count = extractor.readSampleData(buffer, 0)
                    if (count < 0) break
                    buffer.position(0); buffer.limit(count); consume(buffer); report(extractor.sampleTime); extractor.advance()
                }
            } else {
                val decoder = MediaCodec.createDecoderByType(mime).also { codec = it }
                decoder.configure(source, null, null, 0); decoder.start()
                val info = MediaCodec.BufferInfo()
                var inputDone = false; var outputDone = false; var lastProgress = SystemClock.elapsedRealtime()
                while (!outputDone) {
                    currentCoroutineContext().ensureActive()
                    check(SystemClock.elapsedRealtime()-lastProgress < 15000) { "Decoder stalled" }
                    if (!inputDone) {
                        val index = decoder.dequeueInputBuffer(10000)
                        if (index >= 0) {
                            val buffer = requireNotNull(decoder.getInputBuffer(index)); buffer.clear()
                            val count = extractor.readSampleData(buffer, 0)
                            if (count < 0) { decoder.queueInputBuffer(index, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM); inputDone = true }
                            else { decoder.queueInputBuffer(index, 0, count, extractor.sampleTime, 0); extractor.advance() }
                            lastProgress = SystemClock.elapsedRealtime()
                        }
                    }
                    val index = decoder.dequeueOutputBuffer(info, 10000)
                    if (index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                        val format = decoder.outputFormat
                        val nextRate = format.getInteger(MediaFormat.KEY_SAMPLE_RATE); val nextChannels = format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
                        check(meter == null || (rate == nextRate && channels == nextChannels))
                        rate = nextRate; channels = nextChannels
                        encoding = if (format.containsKey("pcm-encoding")) format.getInteger("pcm-encoding") else AudioFormat.ENCODING_PCM_16BIT
                    } else if (index >= 0) {
                        try {
                            if (info.size > 0 && info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0) {
                                val buffer = requireNotNull(decoder.getOutputBuffer(index))
                                buffer.position(info.offset); buffer.limit(info.offset+info.size); consume(buffer); report(info.presentationTimeUs)
                            }
                            outputDone = info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0
                        } finally { decoder.releaseOutputBuffer(index, false) }
                        lastProgress = SystemClock.elapsedRealtime()
                    }
                }
            }
            currentCoroutineContext().ensureActive()
            return requireNotNull(meter).result()
        } finally {
            try { codec?.release() } finally { extractor.release() }
        }
    }
}
