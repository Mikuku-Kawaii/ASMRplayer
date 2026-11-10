package com.asmrplayer.android.subtitles

data class SubtitleCue(val startMs: Long, val endMs: Long, val text: String)

/** Immutable timeline, including overlapping dialogue. UI supplies the Media3 position, never its own clock. */
class SubtitleTimeline(val cues: List<SubtitleCue>) {
    private val maxEnds = LongArray(cues.size).also { ends ->
        var maximum = 0L
        cues.forEachIndexed { index, cue -> maximum = maxOf(maximum, cue.endMs); ends[index] = maximum }
    }
    fun lastStarted(positionMs: Long, offsetMs: Long = 0): Int {
        val time = positionMs - offsetMs
        var lo = 0; var hi = cues.size
        while (lo < hi) { val mid = (lo + hi) ushr 1; if (cues[mid].startMs <= time) lo = mid + 1 else hi = mid }
        return lo - 1
    }
    /** Player excerpt: retain the last started sentence until the next starts, including across seeks. */
    fun display(positionMs: Long, offsetMs: Long = 0): List<SubtitleCue> {
        val lo = lastStarted(positionMs, offsetMs) + 1
        if (lo == 0) return emptyList()
        val start = cues[lo - 1].startMs
        var first = lo - 1
        while (first > 0 && cues[first - 1].startMs == start) first--
        return cues.subList(first, lo)
    }
    fun active(positionMs: Long, offsetMs: Long = 0): List<SubtitleCue> {
        val time = positionMs - offsetMs
        val result = mutableListOf<SubtitleCue>()
        var i = lastStarted(positionMs, offsetMs)
        while (i >= 0 && maxEnds[i] > time) { if (cues[i].endMs > time) result.add(cues[i]); i-- }
        return result.asReversed()
    }

    /** Match a source cue against this timeline, including the open-ended final LRC cue. */
    fun overlapping(startMs: Long, endMs: Long, offsetMs: Long = 0): List<SubtitleCue> {
        if (endMs <= startMs) return emptyList()
        val start = startMs - offsetMs
        val end = if (endMs == Long.MAX_VALUE) endMs else endMs - offsetMs
        var i = lastStarted(end - 1)
        val matches = mutableListOf<SubtitleCue>()
        while (i >= 0 && maxEnds[i] > start) {
            if (cues[i].endMs > start) matches.add(cues[i])
            i--
        }
        return matches.asReversed()
    }
}

object SubtitleParser {
    fun decode(bytes: ByteArray, format: String): SubtitleTimeline {
        require(bytes.size <= 8 * 1024 * 1024) { "字幕文件超过 8 MiB" }
        val charset = when {
            bytes.size >= 2 && bytes[0] == 0xff.toByte() && bytes[1] == 0xfe.toByte() -> Charsets.UTF_16LE
            bytes.size >= 2 && bytes[0] == 0xfe.toByte() && bytes[1] == 0xff.toByte() -> Charsets.UTF_16BE
            else -> Charsets.UTF_8
        }
        return parse(charset.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes)).toString(), format)
    }
    private val timing = Regex("(\\d{1,3}:)?\\d{1,2}:\\d{2}[.,]\\d{1,3}")
    fun parse(text: String, format: String): SubtitleTimeline {
        require(text.length <= 4 * 1024 * 1024) { "字幕过大" }
        val normalized = text.removePrefix("\uFEFF").replace("\r\n", "\n").replace('\r', '\n')
        val cues = when (format.lowercase()) {
            "srt", "vtt" -> blocks(normalized)
            "lrc" -> lrc(normalized)
            "ass", "ssa" -> ass(normalized)
            else -> throw IllegalArgumentException("不支持的字幕格式")
        }.filter { it.endMs > it.startMs && it.text.isNotBlank() }.sortedBy { it.startMs }
        require(cues.isNotEmpty()) { "字幕没有可用的时间轴" }
        return SubtitleTimeline(cues)
    }

    private fun blocks(text: String): List<SubtitleCue> = text.split(Regex("\n[ \t]*\n")).mapNotNull { block ->
        val lines = block.lines()
        if (lines.firstOrNull()?.let { it.startsWith("NOTE") || it == "STYLE" || it == "REGION" } == true) return@mapNotNull null
        val row = lines.indexOfFirst { "-->" in it }
        if (row < 0) return@mapNotNull null
        val pair = lines[row].split("-->", limit = 2)
        val start = timing.find(pair[0])?.value?.let(::timestamp) ?: return@mapNotNull null
        val end = timing.find(pair[1])?.value?.let(::timestamp) ?: return@mapNotNull null
        SubtitleCue(start, end, clean(lines.drop(row + 1).joinToString("\n")))
    }

    private fun lrc(text: String): List<SubtitleCue> {
        val tags = Regex("\\[(\\d{1,3}:\\d{2}(?:[.:]\\d{1,3})?)\\]")
        val offset = Regex("(?i)\\[offset:([+-]?\\d+)\\]").find(text)?.groupValues?.get(1)?.toLongOrNull() ?: 0L
        val lines = text.lines().flatMap { line ->
            val matches = tags.findAll(line).toList()
            val body = if (matches.isEmpty()) "" else clean(line.substring(matches.last().range.last + 1))
            matches.mapNotNull { tag ->
                val value = tag.groupValues[1]
                val normalized = if (value.count { it == ':' } == 2) value.replaceRange(value.lastIndexOf(':'), value.lastIndexOf(':') + 1, ".") else value
                timestamp(normalized)?.let { (it - offset).coerceAtLeast(0) to body }
            }
        }.sortedBy { it.first }
        val starts = lines.map { it.first }.distinct()
        val ends = starts.withIndex().associate { (i, start) -> start to (starts.getOrNull(i + 1) ?: Long.MAX_VALUE) }
        return lines.map { (start, body) ->
            SubtitleCue(start, ends.getValue(start), body)
        }
    }

    private fun ass(text: String): List<SubtitleCue> {
        var inEvents = false
        var fields = listOf("layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text")
        val result = mutableListOf<SubtitleCue>()
        text.lineSequence().forEach { raw ->
            val line = raw.trim()
            if (line.startsWith('[')) inEvents = line.equals("[Events]", true)
            if (!inEvents) return@forEach
            if (line.startsWith("Format:", true)) fields = line.substringAfter(':').split(',').map { it.trim().lowercase() }
            if (line.startsWith("Dialogue:", true)) {
                val parts = line.substringAfter(':').trimStart().split(',', limit = fields.size)
                fun value(name: String) = fields.indexOf(name).takeIf { it >= 0 }?.let { parts.getOrNull(it) }
                val start = value("start")?.let(::timestamp); val end = value("end")?.let(::timestamp)
                if (start != null && end != null) result.add(SubtitleCue(start, end,
                    clean(value("text").orEmpty().replace(Regex("\\{[^}]*}"), "").replace("\\N", "\n").replace("\\n", "\n").replace("\\h", " "))))
            }
        }
        return result
    }

    private fun timestamp(value: String): Long? {
        val parts = value.trim().replace(',', '.').split(':')
        if (parts.size !in 2..3) return null
        val seconds = parts.last().toDoubleOrNull() ?: return null
        val minutes = parts[parts.lastIndex - 1].toLongOrNull() ?: return null
        val hours = if (parts.size == 3) parts[0].toLongOrNull() ?: return null else 0
        if (!seconds.isFinite() || seconds !in 0.0..<60.0 || minutes < 0 || hours < 0 || hours > 999 || (parts.size == 3 && minutes >= 60)) return null
        return hours * 3_600_000 + minutes * 60_000 + (seconds * 1000).toLong()
    }
    private fun clean(text: String) = text.replace(Regex("<[^>]*>"), "")
        .replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ")
        .replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&").trim()
}
