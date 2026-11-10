package com.asmrplayer.android.ui

import com.asmrplayer.android.artwork.*

import org.junit.Assert.assertEquals
import org.junit.Test

class ArtworkDecodeTest {
    @Test fun fullPlayerKeepsTypicalCoverAtSourceResolution() {
        assertEquals(1,artworkSampleSize(2240,1680,3072))
        assertEquals(1,artworkSampleSize(2400,1800,3072))
        assertEquals(4,artworkSampleSize(2240,1680,768))
    }
    @Test fun oversizedAndPortraitSourcesRemainBounded() {
        assertEquals(4,artworkSampleSize(8000,6000,3072))
        assertEquals(4,artworkSampleSize(6000,8000,3072))
        assertEquals(1,artworkSampleSize(640,480,3072))
    }
}
