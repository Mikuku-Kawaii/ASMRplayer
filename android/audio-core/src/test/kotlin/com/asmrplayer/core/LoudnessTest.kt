package com.asmrplayer.core

import org.junit.Assert.*
import org.junit.Test
import kotlin.math.*

class LoudnessTest {
    private val rate = 48000
    private fun tone(seconds: Int, amplitude: Double, stereo: Boolean = true) = FloatArray(seconds*rate*(if (stereo) 2 else 1)) {
        (amplitude*sin(2*PI*1000*(if (stereo) it/2 else it)/rate)).toFloat()
    }
    private fun measure(input: FloatArray, channels: Int = 2): LoudnessMeasurement {
        val m = IntegratedLoudness(rate, channels)
        var at = 0
        while (at < input.size) { val n = minOf(4096, input.size-at); m.add(input.copyOfRange(at, at+n)); at += n }
        return m.result()
    }
    private fun render(input: FloatArray, config: DspConfig, gain: Double = 0.0): FloatArray {
        val engine = DspEngine(rate, 2, config); engine.setNormalizationGain(gain)
        return engine.process(input) + engine.drain()
    }
    private fun rms(x: FloatArray, from: Int, until: Int): Double = sqrt((from until until).sumOf { x[it].toDouble().pow(2) }/(until-from))

    @Test fun calibratedStereoSineAndRelativeGateIgnoreLongSilence() {
        // BS.1770: equal-channel 1 kHz sine at -23 dBFS peak is about -23 LUFS.
        val signal = tone(3, 10.0.pow(-23.0/20))
        val clean = measure(signal)
        assertEquals(-23.0, clean.lufs!!, 0.08)
        val padded = measure(FloatArray(rate*2*5) + signal + FloatArray(rate*2*5))
        assertEquals(clean.lufs, padded.lufs!!, 0.6) // Edge windows contribute; silence does not dominate.
        assertNull(measure(FloatArray(rate*2)).lufs)
        assertNull(measure(tone(1, 1e-6)).lufs)
    }
    @Test fun monoAndStereoUseCorrectEnergyAndDifferentLevelsNormalizeTogether() {
        val loud = measure(tone(2, 0.2)); val quiet = measure(tone(2, 0.05))
        assertEquals(20*log10(4.0), loud.lufs!!-quiet.lufs!!, 0.01)
        val mono = measure(tone(2, 0.2, false), 1)
        assertEquals(10*log10(2.0), loud.lufs-mono.lufs!!, 0.01)
        val cfg = DspConfig(highPass = false, normalize = true, maxBoostDb = 12.0)
        val a = render(tone(3, 0.2), cfg, loud.gainDb(-23.0, 12.0))
        val b = render(tone(3, 0.05), cfg, quiet.gainDb(-23.0, 12.0))
        assertEquals(rms(a, rate*4, a.size), rms(b, rate*4, b.size), 0.0001)
    }
    @Test fun levelerReducesDifferenceWithoutChangingStereoBalanceOrBoostingNoise() {
        val input = tone(4, 0.04)+tone(4, 0.4)
        for (i in input.indices step 2) input[i+1] = input[i]*0.5f
        val output = render(input, DspConfig(highPass = false, leveler = true))
        val ratio = rms(output, rate*2*6, rate*2*8) / rms(output, rate*2*2, rate*2*4)
        assertTrue("ratio=$ratio", ratio < 6)
        for (i in output.indices step 2) assertEquals(output[i]*0.5f, output[i+1], 1e-6f)
        val noise = tone(3, 1e-5)
        assertArrayEquals(noise, render(noise, DspConfig(highPass = false, leveler = true)), 1e-7f)
    }
    @Test fun optionalModesAreIndependentAndBypassIsExactEvenWithBothEnabled() {
        val input = tone(1, 0.1)
        assertArrayEquals(input, render(input, DspConfig(bypass = true, normalize = true, leveler = true), 12.0), 0f)
        assertArrayEquals(input, render(input, DspConfig(highPass = false, limiter = false), 12.0), 0f)
        assertTrue(DspConfig(leveler = true).normalize.not())
        assertTrue(DspConfig(normalize = true).leveler.not())
    }
    @Test fun loudnessModesRetainPeakProtectionEvenIfLimiterPreferenceWasOff() {
        val input = tone(2, 0.8); input[100000] = 2f
        val output = render(input, DspConfig(highPass = false, limiter = false, normalize = true, leveler = true), 12.0)
        assertTrue(output.all { it.isFinite() && abs(it) <= 10.0.pow(-1.0/20)+1e-6 })
        assertEquals(input.size, output.size)
        assertThrows(IllegalArgumentException::class.java) { DspConfig(targetLufs = Double.NaN) }
        assertThrows(IllegalArgumentException::class.java) { DspConfig(maxBoostDb = 50.0) }
    }
    @Test fun meterHandlesCommonSampleRatesAndChunkBoundaries() {
        for (fs in listOf(8000, 44100, 48000, 96000, 192000)) {
            val input = FloatArray(fs*2*2) { (0.1*sin(2*PI*1000*(it/2)/fs)).toFloat() }
            val a = IntegratedLoudness(fs, 2); a.add(input)
            val b = IntegratedLoudness(fs, 2)
            for (start in input.indices step 510) b.add(input.copyOfRange(start, minOf(input.size, start+510)))
            assertEquals(a.result().lufs!!, b.result().lufs!!, 1e-10)
            assertTrue(a.result().lufs!! in -21.0..-19.0)
        }
    }
    @Test fun hotDisableFadesGainInsteadOfJumpingAndKeepsEveryFrame() {
        val engine = DspEngine(rate, 2, DspConfig(highPass = false, normalize = true))
        engine.setNormalizationGain(6.0)
        val first = engine.process(FloatArray(rate*4) { 0.1f })
        engine.update(DspConfig(highPass = false, limiter = false))
        val next = engine.process(FloatArray(4096) { 0.1f }) + engine.drain()
        val joined = first.takeLast(2).toFloatArray()+next
        for (i in 2 until joined.size step 2) assertTrue(abs(joined[i]-joined[i-2]) < 0.002)
        assertEquals(0.1f, next.last(), 1e-6f)
        assertEquals(rate*4+4096, first.size+next.size)
    }
}
