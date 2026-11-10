package com.asmrplayer.android.data

import android.util.Log
import java.util.concurrent.atomic.AtomicLong

/** Cumulative per-process counters. Diagnostics contain no media identifiers or URLs. */
internal object CacheMetrics {
    val imageFetches = AtomicLong()
    val imageDecodes = AtomicLong()
    val streamOpens = AtomicLong()
    val networkBytes = AtomicLong()
    val diskAudioBytes = AtomicLong()
    val bufferingMs = AtomicLong()

    fun report() {
        Log.d("AsmrCache", "imageFetches=$imageFetches imageDecodes=$imageDecodes " +
            "streamOpens=$streamOpens networkBytes=$networkBytes diskAudioBytes=$diskAudioBytes bufferingMs=$bufferingMs")
    }
}
