package com.asmrplayer.core

import kotlin.math.*

/** Mono/stereo BS.1770 K weighting. Coefficients use the bilinear-transform analogue parameters.
 * Measurement only: never filter the audible signal with this weighting. */
internal class KWeighting(private val rate: Int, private val channels: Int) {
    private class Filter(val b0: Double, val b1: Double, val b2: Double, val a1: Double, val a2: Double, channels: Int) {
        val z1 = DoubleArray(channels); val z2 = DoubleArray(channels)
        fun sample(x: Double, ch: Int): Double {
            val y = b0*x + z1[ch]; z1[ch] = b1*x - a1*y + z2[ch]; z2[ch] = b2*x - a2*y
            return y
        }
    }
    private fun filter(hp: Boolean): Filter {
        val k = tan(PI * (if (hp) 38.13547087602444 else 1681.974450955533) / rate)
        val q = if (hp) 0.5003270373238773 else 0.7071752369554196
        val a0 = 1 + k/q + k*k
        val vh = 10.0.pow(3.999843853973347 / 20)
        val vb = vh.pow(0.4996667741545416)
        return Filter(if (hp) 1.0 else (vh + vb*k/q + k*k)/a0,
            if (hp) -2.0 else 2*(k*k-vh)/a0, if (hp) 1.0 else (vh-vb*k/q+k*k)/a0,
            2*(k*k-1)/a0, (1-k/q+k*k)/a0, channels)
    }
    private val shelf = filter(false); private val highPass = filter(true)
    fun sample(x: Double, channel: Int) = highPass.sample(shelf.sample(x, channel), channel)
}

data class LoudnessMeasurement(val lufs: Double?, val peak: Double, val frames: Long) {
    fun gainDb(target: Double, maxBoost: Double): Double = lufs?.let { (target-it).coerceIn(-36.0, maxBoost) } ?: 0.0
}

/** 400 ms blocks / 100 ms hops; -70 LUFS absolute and -10 LU relative gates.
 * Fixed histogram (0.01 LU bins) bounds memory independently of track duration.
 * Peak is a sample peak, NOT an oversampled true-peak measurement. */
class IntegratedLoudness(private val rate: Int, private val channels: Int) {
    init { require(rate in 8000..384000 && channels in 1..2) }
    private val weighting = KWeighting(rate, channels)
    private val hop = max(1, (rate*0.1).roundToInt())
    private val ring = DoubleArray(4)
    private val sums = DoubleArray(10001)
    private val counts = LongArray(10001)
    private var partial = 0.0; private var partFrames = 0; private var hops = 0L
    private var peak = 0.0; private var frames = 0L
    fun add(input: FloatArray, count: Int = input.size) {
        require(count in 0..input.size && count % channels == 0)
        for (i in 0 until count step channels) {
            for (ch in 0 until channels) {
                val x = input[i+ch].toDouble(); require(x.isFinite())
                peak = max(peak, abs(x)); val y = weighting.sample(x, ch); partial += y*y
            }
            frames++; partFrames++
            if (partFrames == hop) {
                ring[(hops % 4).toInt()] = partial/hop; hops++; partial = 0.0; partFrames = 0
                if (hops >= 4) {
                    val energy = ring.sum()/4
                    val lu = -0.691 + 10*log10(max(energy, 1e-30))
                    if (lu >= -70) {
                        val bin = ((lu+70)*100).roundToInt().coerceIn(0, sums.lastIndex)
                        sums[bin] += energy; counts[bin]++
                    }
                }
            }
        }
    }
    fun result(): LoudnessMeasurement {
        val count = counts.sum()
        if (count == 0L) return LoudnessMeasurement(null, peak, frames)
        val gate = max(-70.0, -0.691 + 10*log10(sums.sum()/count) - 10)
        var energy = 0.0; var kept = 0L
        for (i in sums.indices) if (counts[i] > 0 && -0.691 + 10*log10(sums[i]/counts[i]) >= gate) {
            energy += sums[i]; kept += counts[i]
        }
        return LoudnessMeasurement(if (kept > 0) -0.691 + 10*log10(energy/kept) else null, peak, frames)
    }
}

/** Linked, gated leveler. Silence/very quiet material is never chased upwards.
 * Detects before tone processing; all channels receive exactly the same gain. */
internal class LoudnessLeveler(private val rate: Int, channels: Int) {
    private val weighting = KWeighting(rate, channels)
    private val window = DoubleArray(max(1, (rate*0.4).roundToInt()))
    private var at = 0; private var filled = 0; private var sum = 0.0
    private var blockEnergy = 0.0
    private var target = 0.0
    private var tick = 0
    private val attack = 1-exp(-1.0/(rate*0.08))
    private val release = 1-exp(-1.0/(rate*1.5))
    var gainDb = 0.0; private set
    fun detect(sample: Double, channel: Int) { val y = weighting.sample(sample, channel); blockEnergy += y*y }
    fun frame(config: DspConfig, normalizationDb: Double): Double {
        sum += blockEnergy - window[at]; window[at] = blockEnergy; blockEnergy = 0.0
        at = (at+1) % window.size; filled = min(filled+1, window.size)
        if (++tick >= max(1, rate/100)) {
            tick = 0
            val lu = -0.691 + 10*log10(max(sum/filled, 1e-30))
            // Half correction retains some intended dynamics; max boost includes track normalization.
            target = if (!config.leveler || config.bypass || filled < window.size || lu < -50) 0.0
                else ((config.targetLufs - lu - normalizationDb)*0.5).coerceIn(-24.0, max(0.0, config.maxBoostDb-normalizationDb))
        }
        gainDb += (target-gainDb)*(if (target < gainDb) attack else release)
        return if (config.leveler && !config.bypass) 10.0.pow(gainDb/20) else 1.0
    }
}
