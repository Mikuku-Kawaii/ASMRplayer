package com.asmrplayer.android

import com.asmrplayer.android.library.*
import org.junit.Assert.*
import org.junit.Test

class LibraryIndexTest {
    private fun file(path: String) = ScannedDocument(path, "content://fixture/$path")
    @Test fun versionsStaySeparateAndAmbiguousSubtitleIsNotGuessed() {
        val preview = LibraryIndex.build("RJ123456 Work", listOf("WAV/01.wav", "MP3/01.mp3",
            "WAV/01.wav.ja.srt", "字幕/01.zh.srt", "cover.jpg", "02.wav.part").map(::file), RootMode.AUTO)
        assertEquals(RootMode.WORK, preview.mode)
        val work = preview.works.single()
        assertEquals(2, work.audio.size)
        assertEquals("cover.jpg", work.cover!!.path)
        assertEquals(listOf("WAV/01.wav.ja.srt"), work.subtitles["WAV/01.wav"]!!.map { it.path })
        assertTrue(work.subtitles["MP3/01.mp3"]!!.isEmpty())
        assertNotEquals(LibraryIndex.id("root", "WAV/01.wav"), LibraryIndex.id("root", "MP3/01.mp3"))
    }
    @Test fun collectionAndExplicitGroupingStayWithinGrantedTree() {
        val files = listOf("Work 10/01.wav", "Work 2/01.wav", "Work 2/02.wav").map(::file)
        val preview = LibraryIndex.build("Library", files, RootMode.AUTO)
        assertEquals(listOf("Work 2", "Work 10"), preview.works.map { it.title })
        assertEquals(1, LibraryIndex.build("Library", files, RootMode.WORK).works.size)
    }
    @Test fun naturalSortHasNoNumericOverflowOrIdentityCollision() {
        val names = listOf("Track1000000000000000000000", "Track10", "Track2")
        assertEquals(listOf(names[2], names[1], names[0]), names.sortedWith(NaturalOrder))
        assertNotEquals(LibraryIndex.id("ab", "c"), LibraryIndex.id("a", "bc"))
    }
    @Test(expected = IllegalArgumentException::class) fun duplicateProviderPathsRejectWholeScan() {
        LibraryIndex.build("Work", listOf(file("01.wav"), file("01.wav")), RootMode.WORK)
    }
}
