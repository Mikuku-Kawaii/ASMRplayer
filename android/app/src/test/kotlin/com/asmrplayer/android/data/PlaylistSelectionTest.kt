package com.asmrplayer.android.data

import org.junit.Assert.*
import org.junit.Test

class PlaylistSelectionTest {
    @Test fun savesPreferredGroupWithoutAlternateFormatsBonusOrUnavailableTracks() {
        fun row(id:String,dir:String,format:String="wav",available:Boolean=true,track:String=id)=VersionDetail(id,track,"w",id,dir,format,available)
        val rows=listOf(row("nose","SEなし"),row("bonus","特典"),row("wav","SEあり"),row("flac","SEあり","flac",track="wav"),row("wav2","SEあり"),row("missing","SEあり",available=false))
        assertEquals(listOf("wav","wav2"),preferredPlaylistVersions(rows))
        assertTrue(preferredPlaylistVersions(rows.map{it.copy(available=false)}).isEmpty())
    }
}
