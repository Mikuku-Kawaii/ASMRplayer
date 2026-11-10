package com.asmrplayer.android.playback

/** Service clock supplies the remaining time; UI animation settings never affect the fade. */
internal object SleepEnvelope {
    fun gain(remainingMs: Long?, fadeMs: Long): Float {
        if (remainingMs == null || fadeMs <= 0) return 1f
        return (remainingMs.toDouble() / fadeMs).coerceIn(0.0, 1.0).toFloat()
    }
    fun extend(deadline: Long?, now: Long, minutes: Int): Long {
        require(minutes in 1..60)
        return maxOf(deadline ?: now, now) + minutes * 60_000L
    }
}
