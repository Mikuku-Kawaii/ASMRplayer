package com.asmrplayer.core

import org.junit.Assert.*
import org.junit.Test

class AutoEqTest {
    @Test fun importsParametricFiltersAndPreservesOtherEffects() {
        val config = AutoEq.parse("""
            Preamp: -6.2 dB
            Filter 1: ON PK Fc 105 Hz Gain 5.5 dB Q 0.71
            Filter 2: OFF LSC Fc 80 Hz Gain -2 dB Q 0.7
            Filter 3: ON HSC Fc 10000 Hz Gain 1.5 dB Q 0.707
        """.trimIndent(), DspConfig(bypass = true, swap = true))
        assertEquals(-6.2, config.preampDb, 0.0)
        assertEquals(listOf(FilterKind.PEAK, FilterKind.LOW_SHELF, FilterKind.HIGH_SHELF), config.bands.map { it.kind })
        assertFalse(config.bands[1].enabled)
        assertTrue(config.swap)
        assertFalse(config.bypass)
    }
    @Test fun malformedOrUnsupportedPresetDoesNotPartiallyApply() {
        for (text in listOf("Preamp: -1 dB", "Filter 1: ON HP Fc 50 Hz", "Filter 1: ON PK Fc 1000 Hz Gain NaN dB Q 1", "Filter 1: ON PK Fc 1000 Hz Gain 4 dB Q 0", (1..11).joinToString("\n") { "Filter $it: ON PK Fc 1000 Hz Gain 0 dB Q 1" })) {
            assertThrows(IllegalArgumentException::class.java) { AutoEq.parse(text, DspConfig()) }
        }
    }
    @Test fun lowRateFiltersAboveNyquistAreSkippedInsteadOfMovedIntoAudibleBand() {
        val input = FloatArray(8000) { kotlin.math.sin(it * 0.2).toFloat() * 0.1f }
        fun render(config: DspConfig): FloatArray {
            val engine = DspEngine(8000, 1, config)
            return engine.process(input) + engine.drain()
        }
        val base = DspConfig(highPass = false, limiter = false)
        assertArrayEquals(render(base), render(base.copy(deEsser = true, bands = listOf(PeqBand(6000.0, -6.0)))), 0f)
    }
    @Test fun callerOwnedBuffersPreserveFramesAcrossBlocksAndDrain() {
        val engine = DspEngine(44100, 2, DspConfig(bypass = true))
        val input = FloatArray(1024) { it / 2048f }
        val output = FloatArray(1024)
        val result = ArrayList<Float>()
        repeat(20) { val n = engine.processInto(input, input.size, output); result.addAll(output.take(n)) }
        result.addAll(engine.drain().toList())
        assertArrayEquals(FloatArray(20480) { input[it % input.size] }, result.toFloatArray(), 0f)
        assertTrue(engine.drain().isEmpty())
    }
}
