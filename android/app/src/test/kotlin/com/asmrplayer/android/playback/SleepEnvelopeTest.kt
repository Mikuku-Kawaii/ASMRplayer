package com.asmrplayer.android.playback

import org.junit.Assert.*
import org.junit.Test

class SleepEnvelopeTest {
    @Test fun fadeOnlyAttenuatesInsideFinalWindow() {
        assertEquals(1f, SleepEnvelope.gain(null, 30_000), 0f)
        assertEquals(1f, SleepEnvelope.gain(40_000, 30_000), 0f)
        assertEquals(.5f, SleepEnvelope.gain(15_000, 30_000), .0001f)
        assertEquals(0f, SleepEnvelope.gain(-500, 30_000), 0f)
        assertEquals(1f, SleepEnvelope.gain(0, 0), 0f)
    }
    @Test fun extensionPreservesRemainingTimeAndCanRestartExpiredDeadline() {
        assertEquals(320_000, SleepEnvelope.extend(20_000, 10_000, 5))
        assertEquals(310_000, SleepEnvelope.extend(5_000, 10_000, 5))
        assertEquals(310_000, SleepEnvelope.extend(null, 10_000, 5))
    }
    @Test(expected = IllegalArgumentException::class) fun extensionRejectsNegativeDuration() {
        SleepEnvelope.extend(null, 0, -1)
    }
}
