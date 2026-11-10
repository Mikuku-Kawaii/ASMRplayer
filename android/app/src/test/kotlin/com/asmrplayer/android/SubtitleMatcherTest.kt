package com.asmrplayer.android

import com.asmrplayer.android.subtitles.SubtitleMatcher as Match
import org.junit.Assert.*
import org.junit.Test

class SubtitleMatcherTest {
    @Test fun doubleExtensionAndLanguageTagsMatchAcrossFormats() {
        assertEquals("#1.序章", Match.stem("字幕/#1.序章.wav.vtt"))
        assertEquals("01_耳かき", Match.stem("01_耳かき.wav.ja.srt"))
        assertEquals(1030, Match.score("WAV/#1.序章.wav.vtt", "#1.序章.mp3", "MP3", 13, 26))
        assertEquals(1040, Match.score("WAV/#1.序章.wav.vtt", "#1.序章.wav", "WAV", 13, 26))
    }
    @Test fun formatSiblingsPreferTheSameSoundEffectEdition() {
        val same = Match.score("Work/03_无音效版/01_WAV（无音效）/#1.序章.wav.vtt", "#1.序章.mp3", "Work/03_无音效版/02_MP3（无音效版）", 13, 26)
        val different = Match.score("Work/01_WAV/#1.序章.wav.vtt", "#1.序章.mp3", "Work/03_无音效版/02_MP3（无音效版）", 13, 26)
        assertTrue(same > different)
    }
    @Test fun desktopTagsAndNumberFallbackAreSupported() {
        assertEquals(990, Match.score("01_耳かき【中文字幕】.srt", "01_耳かき（SEあり）.wav", "WAV", 10, 10))
        assertEquals(640, Match.score("#2.翻译标题.vtt", "track02 Japanese.wav", "WAV", 10, 10))
        assertEquals(0, Match.score("track01.srt", "track010.wav", "", 10, 10))
        assertEquals(0, Match.score("02.srt", "01.wav", "", 1, 1))
    }
    @Test fun singleAttachmentFallbackDoesNotLeakAcrossMultipleTracks() {
        assertEquals(440, Match.score("翻译.srt", "Audio.wav", "WAV", 1, 1))
        assertEquals(0, Match.score("翻译.srt", "Audio.wav", "WAV", 2, 1))
        assertEquals(0, Match.score(".srt", "Audio.wav", "", 1, 1))
    }
}
