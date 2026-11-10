package com.asmrplayer.android.library

import java.util.Locale
import java.util.UUID

enum class RootMode { AUTO, WORK, COLLECTION }
data class ScannedDocument(val path: String, val uri: String, val size: Long = 0, val modifiedMs: Long = 0) {
    val name get() = path.substringAfterLast('/')
    val directory get() = path.substringBeforeLast('/', "")
    val extension get() = name.substringAfterLast('.', "").lowercase(Locale.ROOT)
    val stem get() = name.substringBeforeLast('.', name)
}
data class IndexedWork(val path: String, val title: String, val code: String?, val audio: List<ScannedDocument>,
    val assets: List<ScannedDocument>, val cover: ScannedDocument?, val subtitles: Map<String, List<ScannedDocument>>)
data class ScanPreview(val mode: RootMode, val works: List<IndexedWork>, val documentCount: Int)

/** Numeric runs are compared without integer conversion (filenames may contain arbitrarily long numbers). */
object NaturalOrder : Comparator<String> {
    private val parts = Regex("[0-9]+|[^0-9]+")
    override fun compare(a: String, b: String): Int {
        val left = parts.findAll(a.lowercase(Locale.ROOT)).map { it.value }.toList()
        val right = parts.findAll(b.lowercase(Locale.ROOT)).map { it.value }.toList()
        for (i in 0 until minOf(left.size, right.size)) {
            val x = left[i]; val y = right[i]
            val c = if (x[0].isDigit() && y[0].isDigit()) {
                val nx = x.trimStart('0').ifEmpty { "0" }; val ny = y.trimStart('0').ifEmpty { "0" }
                nx.length.compareTo(ny.length).takeIf { it != 0 } ?: nx.compareTo(ny)
            } else x.compareTo(y)
            if (c != 0) return c
        }
        return left.size.compareTo(right.size).takeIf { it != 0 } ?: a.compareTo(b)
    }
}

object LibraryIndex {
    val audioExtensions = setOf("wav", "mp3", "flac", "m4a", "aac", "ogg", "opus", "aif", "aiff", "alac", "wma")
    val subtitleExtensions = setOf("srt", "vtt", "lrc", "ass", "ssa")
    val imageExtensions = setOf("jpg", "jpeg", "png", "webp")
    private val code = Regex("(?i)(RJ|BJ|VJ)[0-9]{6,8}")
    private val structural = Regex("(?i)(wav|mp3|flac|aac|ogg|opus|hi-res|[0-9]+khz|无损|高音质|seあり|seなし|有音效|无音效|本編|本篇|特典|おまけ|bonus|extra|freetalk|disc.*|cd.*|track.*|音声|audio|字幕|subtitles|subs|images|cover)")
    fun id(vararg parts: String): String = UUID.nameUUIDFromBytes(parts.joinToString("") { "${it.length}:$it" }.toByteArray(Charsets.UTF_8)).toString()

    fun build(title: String, documents: List<ScannedDocument>, mode: RootMode): ScanPreview {
        require(documents.map { it.path }.distinct().size == documents.size) { "提供者返回重复路径，未提交扫描" }
        val files = documents.filterNot { it.name.endsWith(".part", true) }
        val audio = files.filter { it.extension in audioExtensions }
        val resolved = if (mode != RootMode.AUTO) mode else if (code.containsMatchIn(title) ||
            audio.any { it.directory.isEmpty() } || audio.any { structural.matches(it.path.substringBefore('/')) }) RootMode.WORK else RootMode.COLLECTION
        val grouped = audio.groupBy { if (resolved == RootMode.WORK || '/' !in it.path) "" else it.path.substringBefore('/') }
        val works = grouped.map { (path, tracks) ->
            val name = path.ifEmpty { title }
            val assets = files.filter { f -> f.extension !in audioExtensions && (path.isEmpty() || f.path.startsWith("$path/")) }
            val subs = assets.filter { it.extension in subtitleExtensions }
            val byStem = tracks.groupBy { it.stem.lowercase(Locale.ROOT) }
            val byName = tracks.groupBy { it.name.lowercase(Locale.ROOT) }
            val associations = tracks.associate { it.path to mutableListOf<ScannedDocument>() }
            subs.forEach { sub ->
                val target = subtitleStem(sub)
                val candidates = (byName[target] ?: byStem[target]).orEmpty()
                val sameDirectory = candidates.filter { it.directory == sub.directory }
                val match = if (sameDirectory.isNotEmpty()) sameDirectory else candidates
                if (match.size == 1) associations.getValue(match.single().path).add(sub)
            }
            val cover = assets.filter { it.extension in imageExtensions }.sortedWith(
                compareBy<ScannedDocument> { if (it.stem.lowercase(Locale.ROOT) in setOf("cover", "folder", "jacket", "main")) 0 else 1 }
                    .thenBy { it.path.count { c -> c == '/' } }.thenComparator { a, b -> NaturalOrder.compare(a.path, b.path) }).firstOrNull()
            IndexedWork(path, name, code.find(name)?.value?.uppercase(Locale.ROOT),
                tracks.sortedWith(compareBy(NaturalOrder) { it.path }), assets, cover, associations)
        }.sortedWith(compareBy(NaturalOrder) { it.title })
        return ScanPreview(resolved, works, documents.size)
    }

    private fun subtitleStem(file: ScannedDocument): String {
        var stem = file.stem.lowercase(Locale.ROOT)
        val suffixes = setOf("ja", "jp", "en", "zh", "zh-cn", "zh-tw", "zh-hans", "zh-hant", "chs", "cht", "translated", "原文", "译文")
        while (stem.substringAfterLast('.', "") in suffixes) stem = stem.substringBeforeLast('.')
        return stem
    }
}
