package com.asmrplayer.android.playback

import com.asmrplayer.core.*
import org.json.JSONArray
import org.json.JSONObject

/** Local persisted settings and the same-app session command share one validated schema. */
object DspSettings {
    const val SET_DSP = "com.asmrplayer.SET_DSP"
    const val GET_DSP = "com.asmrplayer.GET_DSP"
    const val SET_PRIVACY = "com.asmrplayer.SET_PRIVACY"
    const val JSON = "config"
    const val ANALYZE = "com.asmrplayer.ANALYZE_LOUDNESS"
    const val CANCEL_ANALYSIS = "com.asmrplayer.CANCEL_LOUDNESS"

    fun encode(c: DspConfig): String = JSONObject().apply {
        put("bypass", c.bypass); put("preamp", c.preampDb)
        put("highPass", c.highPass); put("cutoff", c.cutoffHz)
        put("deEsser", c.deEsser); put("air", c.air); put("swap", c.swap)
        put("warm", c.warm)
        put("balance", c.balanceDb); put("limiter", c.limiter); put("ceiling", c.ceilingDb)
        put("leveler", c.leveler); put("normalize", c.normalize)
        put("targetLufs", c.targetLufs); put("maxBoostDb", c.maxBoostDb)
        put("bands", JSONArray().apply { c.bands.forEach { b -> put(JSONObject().apply {
            put("hz", b.frequencyHz); put("gain", b.gainDb); put("q", b.q)
            put("kind", b.kind.name); put("enabled", b.enabled)
        }) } })
    }.toString()

    fun decode(text: String): DspConfig {
        require(text.length <= 65536)
        val o = JSONObject(text)
        val a = o.getJSONArray("bands")
        require(a.length() <= 10)
        return DspConfig(
            bypass = o.getBoolean("bypass"), preampDb = o.getDouble("preamp"),
            highPass = o.getBoolean("highPass"), cutoffHz = o.getDouble("cutoff"),
            deEsser = o.getBoolean("deEsser"), air = o.getBoolean("air"), swap = o.getBoolean("swap"),
            balanceDb = o.getDouble("balance"), limiter = o.getBoolean("limiter"), ceilingDb = o.getDouble("ceiling"),
            leveler = o.optBoolean("leveler", false), normalize = o.optBoolean("normalize", false),
            targetLufs = if (o.has("targetLufs")) o.getDouble("targetLufs") else -23.0,
            maxBoostDb = if (o.has("maxBoostDb")) o.getDouble("maxBoostDb") else 6.0,
            warm = o.optBoolean("warm", false),
            bands = List(a.length()) { i -> a.getJSONObject(i).let { b ->
                PeqBand(b.getDouble("hz"), b.getDouble("gain"), b.getDouble("q"), FilterKind.valueOf(b.getString("kind")), b.getBoolean("enabled"))
            } },
        )
    }
}
