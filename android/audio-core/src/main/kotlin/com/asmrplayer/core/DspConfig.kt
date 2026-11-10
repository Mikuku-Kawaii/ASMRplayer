package com.asmrplayer.core

enum class FilterKind { PEAK, LOW_SHELF, HIGH_SHELF }

data class PeqBand(
    val frequencyHz: Double, val gainDb: Double, val q: Double = 0.7071,
    val kind: FilterKind = FilterKind.PEAK, val enabled: Boolean = true,
) {
    init {
        require(frequencyHz.isFinite() && frequencyHz in 20.0..20000.0)
        require(gainDb.isFinite() && gainDb in -24.0..24.0)
        require(q.isFinite() && q in 0.1..20.0)
    }
}

// AN003: Android owns this model; no dependency on the Windows host or database.
data class DspConfig(
    val bypass: Boolean = false,
    val preampDb: Double = 0.0,
    val highPass: Boolean = true,
    val cutoffHz: Double = 50.0,
    val bands: List<PeqBand> = emptyList(),
    val deEsser: Boolean = false,
    val air: Boolean = false,
    val swap: Boolean = false,
    val balanceDb: Double = 0.0,
    val limiter: Boolean = true,
    val ceilingDb: Double = -1.0,
    val leveler: Boolean = false,
    val normalize: Boolean = false,
    val targetLufs: Double = -23.0,
    val maxBoostDb: Double = 6.0,
    val warm: Boolean = false,
) {
    init {
        require(preampDb.isFinite() && preampDb in -24.0..12.0)
        require(cutoffHz.isFinite() && cutoffHz in 20.0..100.0)
        require(bands.size <= 10)
        require(balanceDb.isFinite() && balanceDb in -6.0..6.0)
        require(ceilingDb.isFinite() && ceilingDb in -12.0..-0.5)
        require(targetLufs.isFinite() && targetLufs in -30.0..-16.0)
        require(maxBoostDb.isFinite() && maxBoostDb in 0.0..12.0)
    }
}
