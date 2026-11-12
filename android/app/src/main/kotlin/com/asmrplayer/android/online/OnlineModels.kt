package com.asmrplayer.android.online

import com.asmrplayer.android.library.LibraryIndex
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException

enum class OnlineError { AUTH_REQUIRED, NOT_FOUND, RATE_LIMITED, NETWORK, SCHEMA_CHANGED, SOURCE_CHANGED, STORAGE }
class OnlineException(val code: OnlineError, message: String) : IOException(message)
data class OnlineWork(val sourceId: String, val code: String, val title: String, val circle: String,
    val voices: List<String>, val tags: List<String>, val coverUrl: String?, val rating: String,
    val description:String="", val release:String="", val hasSubtitle:Boolean=false, val score:Double?=null)
data class OnlinePage(val items: List<OnlineWork>, val page: Int, val pageSize: Int, val total: Long)
data class OnlineFile(val id: String, val path: String, val title: String, val kind: String, val format: String,
    val bytes: Long?, val streamUrl: String?, val downloadUrl: String?, val durationMs:Long?=null)
data class OnlineDetail(val work: OnlineWork, val files: List<OnlineFile>)
internal fun OnlineFile.versionId(workId: String) = LibraryIndex.id(workId, "version", id, path)
data class RemoteAddress(val url: String, val bytes: Long?)

object AsmrJson {
    fun workId(raw: String): String {
        val value = raw.trim().replace(Regex("(?i)^RJ"), "")
        require(value.length in 1..12 && value.all { it in '0'..'9' }) { "请输入作品 ID 或 RJ 编号" }
        return value.trimStart('0').also { require(it.isNotEmpty()) { "作品编号不能全为零" } }
    }
    /** Format workInfo like the website; database and media identities stay canonical. */
    fun workInfoId(raw:String):String = workId(raw).let { it.padStart(if(it.length<=6) 6 else 8,'0') }
    fun work(o: JSONObject): OnlineWork {
        val id = workId(o.get("id").toString())
        fun names(key: String) = o.optJSONArray(key)?.let { a -> List(a.length()) { i -> a.optJSONObject(i)?.optString("name") ?: a.optString(i) }.filter { it.isNotBlank() } }.orEmpty()
        val tags = names("tags")
        val age = o.optString("age_category_string", o.optString("age_category", "")).lowercase()
        val rating = when {
            o.optBoolean("nsfw", false) || age in setOf("adult", "r18", "r-18") || tags.any { it.uppercase() in setOf("R18", "R-18", "NSFW", "成人向") } -> "NSFW"
            (o.has("nsfw") && !o.isNull("nsfw")) || age in setOf("general", "all", "全年龄") -> "SAFE"
            else -> "UNRATED"
        }
        return OnlineWork(id, o.optString("source_id").ifBlank { "RJ" + id.padStart(if (id.length <= 6) 6 else 8, '0') },
            o.getString("title"), o.optJSONObject("circle")?.optString("name") ?: o.optString("name"), names("vas"), tags,
            listOf("mainCoverUrl", "coverUrl", "cover", "samCoverUrl", "thumbnailCoverUrl").firstNotNullOfOrNull { key -> o.optString(key).takeIf { it.startsWith("https://") || it.startsWith("http://127.0.0.1:") } }, rating, o.optString("description").takeUnless{it=="null"}.orEmpty(), o.optString("release").takeUnless{it=="null"}.orEmpty(), o.optBoolean("has_subtitle",false),
            o.optDouble("rate_average_2dp",Double.NaN).takeIf{it.isFinite()&&it>0&&it<=5})
    }
    fun page(text: String, requested: Int): OnlinePage {
        val o = JSONObject(text); val works = o.getJSONArray("works"); val p = o.optJSONObject("pagination")
        return OnlinePage(List(works.length()) { work(works.getJSONObject(it)) }, p?.optInt("currentPage", requested) ?: requested,
            p?.optInt("pageSize", 20) ?: 20, p?.optLong("totalCount", works.length().toLong()) ?: works.length().toLong())
    }
    fun tree(text: String, workId: String): List<OnlineFile> {
        val result = mutableListOf<OnlineFile>()
        fun walk(nodes: JSONArray, prefix: String, depth: Int) {
            require(depth <= 64 && nodes.length() <= 50_000) { "目录过大" }
            for (i in 0 until nodes.length()) {
                val o = nodes.getJSONObject(i); val title = o.getString("title").trim()
                require(title.isNotEmpty() && '/' !in title && title != "." && title != "..")
                val path = if (prefix.isEmpty()) title else "$prefix/$title"
                if (o.optString("type") == "folder") { walk(o.getJSONArray("children"), path, depth + 1); continue }
                val ext = title.substringAfterLast('.', "bin").lowercase()
                val kind = when {
                    LibraryIndex.isVideoFormat(ext) -> "other"
                    o.optString("type") == "audio" || ext in LibraryIndex.audioExtensions -> "audio"
                    ext in LibraryIndex.subtitleExtensions -> "subtitle"
                    ext in LibraryIndex.imageExtensions -> "image"
                    else -> "other"
                }
                val size = if (o.has("size") && !o.isNull("size")) o.getLong("size").takeIf { it >= 0 } else null
                val id = o.optString("hash").ifBlank { "$workId:path:$path:${size ?: 0}" }
                fun address(key: String) = o.optString(key).takeIf { it.isNotBlank() && it != "null" }
                result.add(OnlineFile(id, path, title, kind, ext, size, address("mediaStreamUrl"), address("mediaDownloadUrl"),o.optDouble("duration",Double.NaN).takeIf{it.isFinite()&&it>0}?.let{(it*1000).toLong()}))
                require(result.size <= 50_000)
            }
        }
        walk(JSONArray(text), "", 0)
        require(result.map { it.path }.distinct().size == result.size) { "远端目录重复" }
        return result
    }
}
