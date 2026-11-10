package com.asmrplayer.android.ui

import kotlin.math.roundToLong

/** Signed seconds for subtitle offsets and loop fields; invalid input never becomes zero. */
internal fun secondsToMillis(text: String): Long? {
    val seconds = text.trim().replace(',', '.').toDoubleOrNull() ?: return null
    if (!seconds.isFinite() || kotlin.math.abs(seconds) >= Long.MAX_VALUE.toDouble() / 1000) return null
    return (seconds * 1000).roundToLong()
}
internal fun secondsText(ms: Long): String = java.lang.String.format(java.util.Locale.ROOT, "%.3f", ms / 1000.0)
