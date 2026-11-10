package com.asmrplayer.android.ui

import com.asmrplayer.android.artwork.*
import org.junit.Assert.*
import org.junit.Test

class CoverComparisonTest {
    private val sample=IntArray(768){i->val x=i%32;val y=i/32;0xff000000.toInt() or ((x*7) shl 16) or ((y*10) shl 8) or (if((x/4+y/3)%2==0)220 else 40)}
    @Test fun sameArtworkWithCompressionNoiseAndSmallLogoChangesMatches() {
        val candidate=sample.copyOf()
        for(i in candidate.indices)if(i%7==0)candidate[i]=candidate[i] xor 0x030303
        for(i in 0..30)candidate[i]=0xffffffff.toInt()
        assertTrue(matchingCoverSamples(sample,candidate,4f/3,4f/3))
    }
    @Test fun anotherCompositionWithTheSamePaletteDoesNotMatch() {
        assertFalse(matchingCoverSamples(sample,sample.reversedArray(),4f/3,4f/3))
        assertFalse(matchingCoverSamples(sample,sample,4f/3,9f/16))
        val plain=IntArray(768){0xffeeeeee.toInt()}
        assertFalse(matchingCoverSamples(plain,plain,4f/3,4f/3))
    }
    @Test fun bonusImagesCanBeComparedButWallpapersAndTrackListsCannot() {
        assertTrue(coverComparisonCandidate("購入特典/特典画像.png"))
        assertFalse(coverComparisonCandidate("wallpapers/01.png"))
        assertFalse(coverComparisonCandidate("トラックリスト.png"))
    }
}
