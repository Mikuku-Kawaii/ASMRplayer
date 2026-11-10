package com.asmrplayer.core

import org.junit.Assert.*
import org.junit.Test
import kotlin.math.*

class DspEngineTest {
    private fun render(input: FloatArray, channels: Int, cfg: DspConfig): FloatArray {
        val dsp = DspEngine(48000, channels, cfg)
        return dsp.process(input) + dsp.drain()
    }

    @Test fun bypassPreservesEverySampleAndTailAcrossSmallBuffers() {
        val input = FloatArray(2054) { sin(it * 0.07).toFloat() }
        val dsp = DspEngine(48000, 2, DspConfig(bypass = true))
        val output = input.toList().chunked(34).flatMap { dsp.process(it.toFloatArray()).toList() }.toFloatArray() + dsp.drain()
        assertArrayEquals(input, output, 0f)
        assertEquals(0, dsp.drain().size)
    }

    @Test fun shortFileDoesNotLoseItsFinalSamples() {
        val input = floatArrayOf(0.2f, -0.3f, 0.7f)
        assertArrayEquals(input, render(input, 1, DspConfig(bypass = true)), 0f)
    }

    @Test fun limiterCatchesImpulseAndLinksStereoGain() {
        val input = FloatArray(9600) { if (it % 2 == 0) 0.4f else 0.2f }
        input[2000] = 3f; input[2001] = 1.5f
        val output = render(input, 2, DspConfig(highPass = false))
        val ceiling = 10.0.pow(-1.0/20)
        assertTrue(output.all { abs(it) <= ceiling + 1e-6 })
        for (i in output.indices step 2) assertEquals(output[i]/2, output[i+1], 1e-6f)
    }

    @Test fun highPassAttenuatesRumbleWithoutChangingStereoImage() {
        fun rms(freq: Double, hp: Boolean): Double {
            val input = FloatArray(96000) { sin(2*PI*freq*(it/2)/48000).toFloat()*0.1f }
            val output = render(input, 2, DspConfig(highPass = hp, limiter = false))
            for (i in output.indices step 2) assertEquals(output[i], output[i+1], 0f)
            return sqrt(output.drop(24000).sumOf { it.toDouble()*it }/72000)
        }
        assertTrue(rms(10.0, true) < rms(10.0, false)*0.06)
        assertTrue(rms(1000.0, true) > rms(1000.0, false)*0.99)
    }

    @Test fun swapAndBalanceRetainTheOriginalChannelRelationship() {
        val output = render(floatArrayOf(0.2f, 0.4f), 2, DspConfig(highPass = false, limiter = false, swap = true, balanceDb = 6.0))
        assertEquals((0.4*10.0.pow(-6.0/20)).toFloat(), output[0], 1e-6f)
        assertEquals(0.2f, output[1], 1e-6f)
    }

    @Test fun hotUpdatesKeepLengthAndProduceFiniteSamples() {
        val dsp = DspEngine(48000, 2)
        val first = dsp.process(FloatArray(2000) { 0.2f })
        dsp.update(DspConfig(bands = listOf(PeqBand(1000.0, 6.0)), deEsser = true, air = true))
        val second = dsp.process(FloatArray(2000) { 0.2f })
        val output = first + second + dsp.drain()
        assertEquals(4000, output.size)
        assertTrue(output.all { it.isFinite() })
    }

    @Test fun invalidParametersCannotEnterTheAudioThread() {
        assertThrows(IllegalArgumentException::class.java) { DspConfig(preampDb = Double.NaN) }
        assertThrows(IllegalArgumentException::class.java) { PeqBand(0.0, 1.0) }
    }
}
