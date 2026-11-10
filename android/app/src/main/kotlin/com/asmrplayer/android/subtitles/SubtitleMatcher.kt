package com.asmrplayer.android.subtitles

import java.util.Locale

/** Desktop library.rs matching rules. Identity remains version-scoped; only attachments are shared. */
object SubtitleMatcher {
    private val extensions = setOf("srt", "vtt", "lrc", "ass", "ssa", "txt", "wav", "mp3", "flac", "m4a", "ogg", "opus", "aac")
    private val languages = setOf("ja-zh", "jp-cn", "zh-ja", "jpn", "zho", "zh", "chs", "cht", "sc", "tc", "cn", "jp", "ja", "en", "zh-cn", "zh-tw", "zh-hans", "zh-hant", "translated", "原文", "译文")
    private val tags = listOf("seあり", "seなし", "se有り", "se無し", "有音效", "无音效", "効果音あり", "効果音なし", "中日双语", "日中双语", "中日字幕", "中文字幕", "字幕", "带字幕", "chs", "cht", "zh", "jp")
    fun stem(name: String): String {
        var value = name.replace('\\', '/').substringAfterLast('/').trim().lowercase(Locale.ROOT)
        while (value.substringAfterLast('.', "") in extensions + languages) value = value.substringBeforeLast('.')
        return value.trim()
    }
    fun normalized(name: String): String {
        var value = stem(name)
        for (tag in tags) for ((left, right) in listOf("(" to ")", "（" to "）", "[" to "]", "【" to "】")) value = value.replace(left + tag + right, "")
        for (tag in listOf("seあり", "seなし", "有音效", "无音效")) value = value.replace(tag, "")
        return value.filterNot { it.isWhitespace() || it in "_- .、，,：:（）()[]【】「」" }
    }
    fun index(name: String): Int? {
        val value = stem(name).replaceFirst(Regex("^(track|tr|part|#|第)"), "").trimStart(' ', '_', '-', '.', '#')
        val digits = value.takeWhile { it in '0'..'9' }
        return digits.takeIf { it.length in 1..4 }?.toIntOrNull()?.takeIf { it in 1..999 }
    }
    fun score(subtitle: String, track: String, directory: String, tracks: Int, subtitles: Int): Int {
        val sub = stem(subtitle); val audio = stem(track)
        if (sub.isBlank() || audio.isBlank()) return 0
        val sn = normalized(subtitle); val tn = normalized(track)
        val si = index(subtitle); val ti = index(track)
        val score = when {
            sub == audio -> 1000
            sn.isNotEmpty() && sn == tn -> 950
            sn.length >= 2 && tn.length >= 2 && (sn.contains(tn) || tn.contains(sn)) -> if (si != null && ti != null && si != ti) 0 else 820
            sub.length >= 2 && audio.length >= 2 && (sub.contains(audio) || audio.contains(sub)) -> if (si != null && ti != null && si != ti) 0 else 760
            si != null && ti != null -> if (si == ti) 600 else 0
            tracks == 1 && subtitles == 1 -> 400
            else -> 0
        }
        if (score == 0) return 0
        val parent = subtitle.replace('\\', '/').substringBeforeLast('/', "")
        return score + when {
            parent.isEmpty() || parent.equals(directory, true) -> 40
            formatSibling(parent) == formatSibling(directory) -> 30
            else -> 0
        }
    }
    // Prefer the same edition when WAV and MP3 directories share subtitles; don't borrow the other SE edition first.
    private fun formatSibling(directory: String): String = directory.lowercase(Locale.ROOT).split('/').joinToString("/") {
        it.replace(Regex("^[0-9]*[_ -]*(wav|mp3|flac|m4a|ogg|opus|aac)(?=$|[（( _-])"), "audio")
            .replace("无音效版", "无音效")
    }
}
