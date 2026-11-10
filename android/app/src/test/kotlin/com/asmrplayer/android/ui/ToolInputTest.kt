package com.asmrplayer.android.ui

import org.junit.Assert.*
import org.junit.Test

class ToolInputTest {
    @Test fun signedSubsecondOffsetsAndLoopPositionsKeepTheirUnits() {
        assertEquals(-125L, secondsToMillis("-0.125"))
        assertEquals(1500L, secondsToMillis("1,5"))
        assertEquals(120001L, secondsToMillis("120.001"))
        assertEquals(172800000L, secondsToMillis("172800"))
        assertEquals("-0.125", secondsText(-125))
    }
    @Test fun invalidOrOverflowingDraftsNeverBecomeSavedZero() {
        listOf("", "-", "NaN", "Infinity", "1e99", "00:01", "1.2.3").forEach { assertNull(it, secondsToMillis(it)) }
    }
}
