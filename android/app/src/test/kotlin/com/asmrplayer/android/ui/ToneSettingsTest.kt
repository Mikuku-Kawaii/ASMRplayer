package com.asmrplayer.android.ui

import com.asmrplayer.android.playback.DspSettings
import com.asmrplayer.core.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class ToneSettingsTest {
    @Test fun warmthRoundTripsAndOldProfilesRemainCompatible() {
        val config=TonePreset.WARM.applyTo(DspConfig(leveler=true,bands=listOf(PeqBand(120.0,-2.0))))
        val encoded=DspSettings.encode(config)
        assertEquals(config,DspSettings.decode(encoded))
        val old=JSONObject(encoded).apply{remove("warm")}
        assertEquals(config.copy(warm=false),DspSettings.decode(old.toString()))
    }
}
