package com.asmrplayer.android

import com.asmrplayer.android.subtitles.*
import org.junit.Assert.*
import org.junit.Test

class SubtitleParserTest {
    @Test fun bilingualPairingIncludesLateAndSpanningTranslationsButExcludesTouchingEdges() {
        val track = SubtitleTimeline(listOf(SubtitleCue(0, 600, "before"), SubtitleCue(200, 4100, "spans"),
            SubtitleCue(900, 1600, "late"), SubtitleCue(2300, 3000, "after")))
        // Original [1000, 2700), translation delayed by 400 ms.
        assertEquals(listOf("spans", "late"), track.overlapping(1000, 2700, 400).map { it.text })
        assertEquals(listOf("spans", "after"), track.overlapping(2000, Long.MAX_VALUE, -400).map { it.text })
        assertEquals(listOf("spans"), track.active(1000, 400).map { it.text })
    }
    @Test fun excerptRetainsSentenceAcrossGapsAndReplacesAtNextStart() {
        val timeline = SubtitleTimeline(listOf(SubtitleCue(1000, 2000, "First"), SubtitleCue(4000, 5000, "Next")))
        assertEquals("First", timeline.display(3500).single().text)
        assertTrue(timeline.active(3500).isEmpty()) // Timed subtitle semantics remain unchanged.
        assertEquals("Next", timeline.display(4000).single().text)
        assertEquals("Next", timeline.display(9000).single().text)
    }
    @Test fun excerptRecomputesOnBackwardSeekAndOffsetWithoutStaleText() {
        val timeline = SubtitleTimeline(listOf(SubtitleCue(1000, 2000, "First"), SubtitleCue(4000, 5000, "Next")))
        assertEquals("Next", timeline.display(4500, 500).single().text)
        assertEquals("First", timeline.display(4499, 500).single().text)
        assertTrue(timeline.display(1499, 500).isEmpty())
        assertTrue(SubtitleTimeline(emptyList()).display(1000).isEmpty())
    }
    @Test fun excerptKeepsSimultaneousLinesAsOneSentence() {
        val timeline = SubtitleTimeline(listOf(SubtitleCue(0, 500, "A"), SubtitleCue(0, 600, "B"), SubtitleCue(900, 1200, "C")))
        assertEquals(listOf("A", "B"), timeline.display(800).map { it.text })
        assertEquals(listOf("C"), timeline.display(900).map { it.text })
    }
    @Test fun srtKeepsBilingualLinesAndFindsOverlapsWithOffsets() {
        val timeline = SubtitleParser.parse("\uFEFF1\r\n00:00:01,000 --> 00:00:03,500\r\n原文\r\n译文\r\n\r\n2\r\n00:00:02,000 --> 00:00:04,000\r\n声", "srt")
        assertEquals("原文\n译文", timeline.cues[0].text)
        assertEquals(2, timeline.active(2500).size)
        assertEquals(1, timeline.active(2500, 1000).size)
        assertTrue(timeline.active(4000).isEmpty())
    }
    @Test fun vttSettingsAndNotesDoNotBecomeDialogue() {
        val timeline = SubtitleParser.parse("WEBVTT\n\nNOTE example\nnot dialogue\n\nid\n00:01.000 --> 00:02.500 align:start\n<v Reader><i>A &amp; B</i></v>", "vtt")
        assertEquals(listOf(SubtitleCue(1000, 2500, "A & B")), timeline.cues)
    }
    @Test fun lrcMultipleTagsOffsetAndLastCue() {
        val timeline = SubtitleParser.parse("[offset:200]\n[00:01.20][00:03.00]Hello\n[00:04.000]End", "lrc")
        assertEquals(listOf(1000L, 2800L, 3800L), timeline.cues.map { it.startMs })
        assertEquals("End", timeline.active(5000).single().text)
    }
    @Test fun assRespectsEventFormatCommasAndNewlines() {
        val timeline = SubtitleParser.parse("[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.20,0:00:03.45,Default,,0,0,0,,{\\i1}Hello, world\\N译文", "ass")
        assertEquals(SubtitleCue(1200, 3450, "Hello, world\n译文"), timeline.cues.single())
    }
    @Test(expected = IllegalArgumentException::class) fun malformedSubtitleDoesNotPretendToLoad() {
        SubtitleParser.parse("no timeline", "srt")
    }
}
