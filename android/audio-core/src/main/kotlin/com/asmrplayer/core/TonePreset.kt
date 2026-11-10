package com.asmrplayer.core

/** Tone-only presets never replace headphone calibration, loudness or channel preferences. */
enum class TonePreset(val highPass: Boolean = false, val deEsser: Boolean = false,
                      val air: Boolean = false, val warm: Boolean = false) {
    NATURAL,
    SOFT(deEsser = true),
    WHISPER(air = true),
    RUMBLE(highPass = true),
    WARM(warm = true),
    CLEAR(highPass = true, deEsser = true, air = true);

    fun applyTo(current: DspConfig) = current.copy(bypass = false, highPass = highPass,
        cutoffHz = if (highPass) 50.0 else current.cutoffHz, deEsser = deEsser, air = air, warm = warm)

    fun matches(current: DspConfig) = !current.bypass && current.highPass == highPass &&
        (!highPass || current.cutoffHz == 50.0) && current.deEsser == deEsser && current.air == air && current.warm == warm
}
