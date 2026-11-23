package com.asmrplayer.android.data

import org.junit.Assert.*
import org.junit.Test

class FilePreferencesTest {
    @org.junit.Test fun collectionUsesOnlyOneFormatAndDirectoryInChapterOrder() {
        val versions=listOf(VersionDetail("a","one","w","01","Disc","wav",true),VersionDetail("alt","one","w","01","Disc","mp3",true),
            VersionDetail("b","two","w","02","Disc","wav",true),VersionDetail("extra","bonus","w","特典","Bonus","wav",true))
        org.junit.Assert.assertEquals(listOf("a","b"),collectionChapters(CollectionWork("w"),versions,FilePreferences(format=AudioFormatPreference.Wav),null))
        org.junit.Assert.assertEquals(listOf("alt"),collectionChapters(CollectionWork("w","Disc","mp3"),versions,FilePreferences(),null))
        org.junit.Assert.assertTrue(collectionChapters(CollectionWork("w","missing","wav"),versions,FilePreferences(),null).isEmpty())
    }
    @Test fun sharedPcAndroidDirectoryRegressions() {
        val fixtures = org.json.JSONArray(java.io.File("../../tests/fixtures/file-preferences.json").readText())
        for (index in 0 until fixtures.length()) {
            val example = fixtures.getJSONObject(index)
            val rows = example.getJSONArray("groups")
            val groups = (0 until rows.length()).flatMap { row ->
                val group = rows.getJSONObject(row)
                val formats = group.getJSONArray("formats")
                (0 until formats.length()).map { group.getString("path") to formats.getString(it) }
            }
            val prefs = example.getJSONObject("preferences")
            val preferences = FilePreferences(EffectPreference.from(prefs.getString("effects")), AudioFormatPreference.from(prefs.getString("format")))
            assertEquals(example.getString("name"), example.getString("expected"), preferredDirectory(groups, preferences, null)?.first)
            assertEquals(groups.size, preferredDirectoryOrder(groups, preferences).size)
        }
        assertEquals(EffectPreference.With, directoryEffects("含音效/無背景音樂"))
        assertEquals(EffectPreference.Without, directoryEffects("SEあり/無 SE 音效/WAV"))
    }
    private val groups = listOf("SEあり" to "wav", "SEなし" to "wav", "SEあり" to "mp3", "SEなし" to "mp3")

    @Test fun allFourCombinationsSelectTheirVersionEvenWithAnotherResume() {
        for (effects in listOf(EffectPreference.With, EffectPreference.Without)) {
            for (format in listOf(AudioFormatPreference.Mp3, AudioFormatPreference.Wav)) {
                val preferences = FilePreferences(effects, format)
                val result = preferredDirectory(groups, preferences, groups.first())!!
                assertEquals(effects, directoryEffects(result.first))
                assertEquals(format.key, result.second)
                assertEquals(groups.toSet(), preferredDirectoryOrder(groups, preferences).toSet())
            }
        }
    }

    @Test fun noPreferencePreservesSourceOrderAndResume() {
        assertEquals(groups, preferredDirectoryOrder(groups, FilePreferences()))
        assertEquals(groups.last(), preferredDirectory(groups, FilePreferences(), groups.last()))
        assertEquals(groups.first(), preferredDirectory(groups, FilePreferences(), null))
    }

    @Test fun effectsTakePriorityWhenExactCombinationIsAbsent() {
        val candidates = listOf("SEあり" to "mp3", "SEなし" to "wav")
        assertEquals(candidates.last(), preferredDirectory(candidates,
            FilePreferences(EffectPreference.Without, AudioFormatPreference.Mp3), candidates.first()))
    }

    @Test fun commonDirectoryNamesAndNearestExplicitVersionAreRecognized() {
        listOf("3_SEなし（Hシーン_No.SE）", "SE 無し/WAV", "无效果音", "無音效", "No_SE", "without se", "ＮＯ．ＳＥ").forEach {
            assertEquals(it, EffectPreference.Without, directoryEffects(it))
        }
        listOf("SEあり", "SE 有り", "有效果音", "有音效", "with_SE", "効果音あり").forEach {
            assertEquals(it, EffectPreference.With, directoryEffects(it))
        }
        assertEquals(EffectPreference.Without, directoryEffects("SEあり/SEなし/wav"))
        assertEquals(EffectPreference.With, directoryEffects("SEなし/SEあり/mp3"))
        assertEquals(EffectPreference.Any, directoryEffects("音声/WAV"))
    }

    @Test fun unknownVersionStaysUsableAndFormatIsCaseInsensitive() {
        val candidates = listOf("1_WAV" to "WAV", "2_MP3" to "MP3")
        assertEquals(candidates.last(), preferredDirectory(candidates,
            FilePreferences(EffectPreference.Without, AudioFormatPreference.Mp3), null))
    }

    @Test fun tiedPreferredGroupsResumeAndEmptyDirectoriesAreSafe() {
        val candidates = listOf("本編/SEなし" to "wav", "続編/SEなし" to "wav")
        assertEquals(candidates.last(), preferredDirectory(candidates,
            FilePreferences(EffectPreference.Without), candidates.last()))
        assertNull(preferredDirectory(emptyList(), FilePreferences(), groups.first()))
        assertEquals(groups.first(), preferredDirectory(groups, FilePreferences(), "不存在" to "wav"))
    }

    @Test fun unknownStoredValuesUseUnspecifiedDefaults() {
        assertEquals(EffectPreference.Any, EffectPreference.from("removed-value"))
        assertEquals(AudioFormatPreference.Any, AudioFormatPreference.from(null))
    }
}
