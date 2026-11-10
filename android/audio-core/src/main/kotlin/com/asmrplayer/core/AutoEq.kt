package com.asmrplayer.core

/** Equalizer APO / AutoEQ ParametricEQ.txt. Parse completely before replacing a preset. */
object AutoEq {
    private const val NUMBER = "([+-]?(?:[0-9]+(?:\\.[0-9]*)?|\\.[0-9]+))"
    private val preamp = Regex("Preamp:\\s*$NUMBER\\s*dB", RegexOption.IGNORE_CASE)
    private val filter = Regex("Filter\\s+\\d+:\\s+(ON|OFF)\\s+(PK|LSC?|HSC?)\\s+Fc\\s+$NUMBER\\s+Hz\\s+Gain\\s+$NUMBER\\s+dB\\s+Q\\s+$NUMBER", RegexOption.IGNORE_CASE)

    fun parse(text: String, current: DspConfig): DspConfig {
        require(text.length <= 65536) { "预设文本过长" }
        var gain = 0.0
        val bands = mutableListOf<PeqBand>()
        for ((index, raw) in text.lineSequence().withIndex()) {
            val line = raw.substringBefore('#').trim().removePrefix("\uFEFF")
            if (line.isBlank()) continue
            val p = preamp.matchEntire(line)
            if (p != null) { gain = p.groupValues[1].toDouble(); continue }
            val f = filter.matchEntire(line)
                ?: throw IllegalArgumentException("第 ${index+1} 行不是支持的 Preamp 或 PK/LS/HS 滤波器")
            val kind = when (f.groupValues[2].uppercase()) {
                "PK" -> FilterKind.PEAK
                "LS", "LSC" -> FilterKind.LOW_SHELF
                else -> FilterKind.HIGH_SHELF
            }
            require(bands.size < 10) { "最多支持 10 个频段" }
            bands += PeqBand(f.groupValues[3].toDouble(), f.groupValues[4].toDouble(), f.groupValues[5].toDouble(), kind, f.groupValues[1].equals("ON", true))
        }
        require(bands.isNotEmpty()) { "没有找到可导入的 EQ 频段" }
        return current.copy(bypass = false, preampDb = gain, bands = bands)
    }
}
