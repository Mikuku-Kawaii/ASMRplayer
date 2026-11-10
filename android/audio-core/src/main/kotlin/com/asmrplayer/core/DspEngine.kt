package com.asmrplayer.core

import kotlin.math.*

/** AN003: bounded streaming DSP. Fixed 4ms latency, including bypass; drain preserves the final frames. */
class DspEngine(val sampleRate: Int, val channels: Int, initial: DspConfig = DspConfig()) {
    init { require(sampleRate in 8000..384000); require(channels in 1..2) }
    val latencyFrames = max(1, (sampleRate * 0.004).roundToInt())
    private val capacity = latencyFrames + 2
    private val delay = Array(channels) { DoubleArray(capacity) }
    private val peakValues = DoubleArray(capacity)
    private val peakIds = LongArray(capacity)
    private var head = 0L
    private var tail = 0L
    private var frame = 0L
    private var pendingFrames = 0
    private var gain = 1.0
    private val release = exp(-1.0 / (sampleRate * 0.080))
    private var config = initial
    private var chain = ToneChain(initial)
    private var oldChain: ToneChain? = null
    private var transition = 0
    private val leveler = LoudnessLeveler(sampleRate, channels)
    private var normalizationTargetDb = 0.0
    private var normalizationDb = 0.0
    private val normalizationSmoothing = 1-exp(-1.0/(sampleRate*0.25))
    private var lastLoudnessGain = 1.0
    private var transitionLoudnessGain = 1.0

    fun setNormalizationGain(db: Double) {
        require(db.isFinite() && db in -36.0..12.0)
        normalizationTargetDb = db
    }

    fun update(next: DspConfig) {
        if (next == config) return
        oldChain = chain
        transitionLoudnessGain = lastLoudnessGain
        chain = ToneChain(next)
        transition = 256
        config = next
    }

    fun process(input: FloatArray): FloatArray {
        val output = FloatArray(input.size)
        val written = processInto(input, input.size, output)
        return output.copyOf(written)
    }

    /** Caller-owned bounded buffers; all methods belong to the audio thread. */
    fun processInto(input: FloatArray, sampleCount: Int, output: FloatArray): Int {
        require(sampleCount in 0..input.size && sampleCount % channels == 0)
        require(output.size >= sampleCount)
        var written = 0
        val ceiling = 10.0.pow(config.ceilingDb/20)
        for (offset in 0 until sampleCount step channels) {
            val target = if (config.normalize && !config.bypass) normalizationTargetDb.coerceAtMost(config.maxBoostDb) else 0.0
            normalizationDb += (target-normalizationDb)*normalizationSmoothing
            for (ch in 0 until channels) leveler.detect(input[offset+ch].toDouble(), ch)
            val requestedGain = leveler.frame(config, normalizationDb) *
                if (config.normalize && !config.bypass) 10.0.pow(normalizationDb/20) else 1.0
            val blend = if (transition > 0) 1.0 - transition / 256.0 else 1.0
            val loudnessGain = transitionLoudnessGain*(1-blend) + requestedGain*blend
            lastLoudnessGain = loudnessGain
            val protect = (!config.bypass && (config.limiter || config.leveler || config.normalize)) || (transition > 0 && loudnessGain > 1.0)
            var peak = 0.0
            val slot = (frame % capacity).toInt()
            for (ch in 0 until channels) {
                val sourceCh = if (chain.swap) 1-ch else ch
                val value = input[offset+sourceCh].toDouble()
                require(value.isFinite())
                val next = chain.process(value, ch)
                val oldSourceCh = if (oldChain?.swap == true) 1-ch else ch
                val previous = oldChain?.process(input[offset+oldSourceCh].toDouble(), ch) ?: next
                val y = (previous * (1-blend) + next * blend) * loudnessGain
                delay[ch][slot] = y
                peak = max(peak, abs(y))
            }
            if (transition > 0 && --transition == 0) oldChain = null
            while (tail > head && peakValues[((tail-1) % capacity).toInt()] <= peak) tail--
            peakValues[(tail % capacity).toInt()] = peak
            peakIds[(tail % capacity).toInt()] = frame
            tail++
            val oldest = frame - latencyFrames
            while (tail > head && peakIds[(head % capacity).toInt()] < oldest) head++
            if (oldest >= 0) {
                val maximum = peakValues[(head % capacity).toInt()]
                val required = if (protect && maximum > ceiling) ceiling/maximum else 1.0
                gain = if (required < gain) required else min(required, 1 - (1-gain)*release)
                val applied = if (protect) gain else 1.0
                for (ch in 0 until channels) output[written++] = (delay[ch][(oldest % capacity).toInt()] * applied).toFloat()
            }
            frame++
            pendingFrames = min(latencyFrames, pendingFrames+1)
        }
        return written
    }

    fun drain(): FloatArray {
        if (pendingFrames == 0) return FloatArray(0)
        val count = pendingFrames
        val missing = (latencyFrames.toLong() - frame).coerceAtLeast(0).toInt()
        val result = process(FloatArray((count+missing)*channels))
        pendingFrames = 0
        return result
    }

    private inner class ToneChain(private val cfg: DspConfig) {
        val swap = cfg.swap && !cfg.bypass && channels == 2
        private val filters = buildList {
            if (!cfg.bypass) {
                if (cfg.highPass) add(Biquad.create(sampleRate, channels, cfg.cutoffHz, 0.0, sqrt(0.5)))
                for (b in cfg.bands.filter { it.enabled && it.frequencyHz < sampleRate * 0.5 }) add(Biquad.create(sampleRate, channels, b.frequencyHz, b.gainDb, b.q, b.kind))
                if (cfg.deEsser && 6500 < sampleRate * 0.5) add(Biquad.create(sampleRate, channels, 6500.0, -3.5, 4.0, FilterKind.PEAK))
                if (cfg.warm && 4000 < sampleRate * 0.5) add(Biquad.create(sampleRate, channels, 4000.0, -2.0, sqrt(0.5), FilterKind.HIGH_SHELF))
                if (cfg.air && 12000 < sampleRate * 0.5) add(Biquad.create(sampleRate, channels, 12000.0, 1.5, sqrt(0.5), FilterKind.HIGH_SHELF))
            }
        }
        // Reserve the sum of positive band gains (conservative for overlapping bands).
        private val headroom = cfg.bands.filter { it.enabled && it.frequencyHz < sampleRate * 0.5 }.sumOf { max(0.0, it.gainDb) } + if (cfg.air && 12000 < sampleRate * 0.5) 1.5 else 0.0
        private val scale = DoubleArray(channels) { ch ->
            val trim = if (channels == 1) 0.0 else if (ch == 0) -max(0.0, cfg.balanceDb) else min(0.0, cfg.balanceDb)
            10.0.pow((cfg.preampDb-headroom+trim)/20)
        }
        fun process(x: Double, ch: Int): Double {
            if (cfg.bypass) return x
            var y = x * scale[ch]
            for (filter in filters) y = filter.process(y, ch)
            return y
        }
    }
}
