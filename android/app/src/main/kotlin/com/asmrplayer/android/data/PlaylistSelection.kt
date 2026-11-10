package com.asmrplayer.android.data

/** A card saves one preferred directory, never mixes alternate edits or formats. */
internal fun preferredPlaylistVersions(versions: List<VersionDetail>): List<String> {
    fun noSe(text: String) = listOf("seなし","se無し","seナシ","无音效","無効果音","効果音なし","効果音無し","nose","no_se","no-se","without se","without_se","なし版","無し版").any { text.contains(it,true) }
    fun bonus(text: String) = listOf("特典","おまけ","ボーナストラック","bonus","extra","フリートーク","free talk","試聴","体験版","予告","trailer").any { text.contains(it,true) }
    fun format(text: String) = when(text.lowercase()) { "wav"->300;"flac"->200;"mp3","m4a","ogg","opus","aac"->100;else->50 }
    val groups=versions.filter { it.available }.groupBy { it.groupPath }
    val best=groups.entries.maxWithOrNull(compareBy<Map.Entry<String,List<VersionDetail>>> {
        !noSe(it.key) && !it.value.all { v->noSe(v.title) }
    }.thenBy { it.value.maxOf { v->format(v.format) } }.thenBy { !bonus(it.key) }.thenBy { it.value.size })?.value ?: return emptyList()
    val tracks=if(best.any { !noSe(it.title) })best.filter { !noSe(it.title) } else best
    return tracks.groupBy { it.trackId }.values.map { group->group.maxBy { format(it.format) }.id }.distinct()
}
