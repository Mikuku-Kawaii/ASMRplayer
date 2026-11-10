package com.asmrplayer.android.backup

import org.json.JSONArray
import org.json.JSONObject

internal data class PortableTable(val name: String, val columns: String, val userColumns: Set<String> = emptySet()) {
    val fields = columns.split(',')
    val key get() = fields.first()
}

/** Public file format is versioned independently of Room. No SQL, device paths or credentials are accepted. */
internal object PortableFormat {
    const val NAME = "asmr-player.android.library"
    const val VERSION = 4
    const val MAX_BYTES = 32 * 1024 * 1024
    val tables = listOf(
        PortableTable("library_roots", "id,title,mode"),
        PortableTable("works", "id,rootId,relativePath,title,code,favorite,rating,inLibrary", setOf("favorite", "rating", "inLibrary")),
        PortableTable("tracks", "id,workId,title,groupPath"),
        PortableTable("track_versions", "id,trackId,format"),
        PortableTable("version_preferences", "versionId,favorite,updatedMs", setOf("favorite", "updatedMs")),
        PortableTable("remote_media", "versionId,origin,sourceWorkId,remoteFileId,relativePath,byteSize,format"),
        PortableTable("remote_assets", "id,workId,origin,sourceWorkId,remoteFileId,relativePath,kind,format,byteSize"),
        PortableTable("media_sources", "id,versionId,rootId,relativePath,size,modifiedMs,kind"),
        PortableTable("work_assets", "id,workId,rootId,relativePath,format"),
        PortableTable("subtitles", "id,versionId,rootId,relativePath,title,format,manual,assetId"),
        PortableTable("subtitle_preferences", "versionId,originalId,translationId,offsetMs,translationOffsetMs", setOf("originalId", "translationId", "offsetMs", "translationOffsetMs")),
        PortableTable("playback_positions", "versionId,positionMs,durationMs,completed,updatedMs", setOf("positionMs", "durationMs", "completed", "updatedMs")),
        PortableTable("bookmarks", "id,versionId,positionMs,note,createdMs", setOf("positionMs", "note", "createdMs")),
        PortableTable("queue_groups", "id,name,versionIds,sortOrder", setOf("name", "versionIds", "sortOrder")),
        PortableTable("library_preferences", "id,includeNsfw,includeUnrated", setOf("includeNsfw", "includeUnrated")),
    )
    private val numbers = setOf("sortOrder", "size", "modifiedMs", "byteSize", "offsetMs", "translationOffsetMs", "positionMs", "durationMs", "updatedMs", "createdMs")
    private val booleans = setOf("inLibrary", "favorite", "manual", "completed", "includeNsfw", "includeUnrated")
    private val nullable = setOf("rootId", "code", "byteSize", "originalId", "translationId", "assetId")
    fun rows(bundle: JSONObject, table: PortableTable): List<JSONObject> = bundle.getJSONObject("records").getJSONArray(table.name).let { array ->
        List(array.length()) { array.getJSONObject(it) }
    }
    fun parse(bytes: ByteArray): JSONObject {
        require(bytes.size <= MAX_BYTES) { "资料包超过 32 MiB" }
        val text = Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes)).toString()
        var depth = 0; var quoted = false; var escaped = false
        for (c in text) {
            if (quoted) { if (escaped) escaped = false else if (c == '\\') escaped = true else if (c == '"') quoted = false }
            else if (c == '"') quoted = true else if (c == '{' || c == '[') { depth++; require(depth <= 16) } else if (c == '}' || c == ']') depth--
        }
        val root = JSONObject(text)
        require(root.getString("format") == NAME) { "不支持此资料包格式" }
        require(root.get("version") in setOf(1, 2, 3, VERSION)) { "不支持此资料包版本" }
        if (root.getInt("version") == 1) {
            val oldRecords = root.getJSONObject("records")
            require(!oldRecords.has("version_preferences")) { "v1 资料包字段不匹配" }
            oldRecords.put("version_preferences", JSONArray())
            root.put("version", 2)
        }
        if (root.getInt("version") == 2) {
            val oldWorks = root.getJSONObject("records").getJSONArray("works")
            for (i in 0 until oldWorks.length()) oldWorks.getJSONObject(i).put("inLibrary", 1)
            root.put("version", 3)
        }
        if(root.getInt("version")==3) {
            root.getJSONObject("records").put("queue_groups", JSONArray())
            root.put("version", VERSION)
        }
        require(root.keys().asSequence().toSet() == setOf("format", "version", "createdMs", "records", "queue"))
        require(root.get("createdMs") is Number && root.getLong("createdMs") >= 0)
        val records = root.getJSONObject("records")
        require(records.keys().asSequence().toSet() == tables.map { it.name }.toSet())
        var count = 0
        tables.forEach { table ->
            val rows = rows(root, table); count += rows.size; require(count <= 100_000) { "资料包记录过多" }
            val ids = hashSetOf<String>()
            rows.forEach { row ->
                require(row.keys().asSequence().toSet() == table.fields.toSet()) { "资料包字段不匹配" }
                table.fields.forEach { key ->
                    val value = row.get(key)
                    if (value == JSONObject.NULL) require(key in nullable)
                    else if (key in numbers) {
                        require(value is Int || value is Long)
                        val n = row.getLong(key)
                        if (key.endsWith("OffsetMs") || key == "offsetMs") require(n in -600_000L..600_000L) else require(n >= 0)
                    } else if (key in booleans) require(value is Int && value in 0..1)
                    else if (table.name == "library_preferences" && key == "id") require(value == 1)
                    else { require(value is String && value.length <= (if(key=="versionIds")65536 else 8192) && '\u0000' !in value) }
                }
                val id = row.get(table.key).toString(); require(id.isNotBlank() && id.length <= 128 && ids.add(id)) { "重复或无效标识" }
                if (row.has("relativePath")) path(row.getString("relativePath"))
                if (row.has("groupPath")) path(row.getString("groupPath"))
                if (row.has("origin")) {
                    val uri = java.net.URI(row.getString("origin"))
                    require(uri.scheme == "https" && !uri.host.isNullOrBlank() && uri.userInfo == null && uri.rawQuery == null && uri.fragment == null && uri.path.isNullOrEmpty())
                }
                if (row.has("sourceWorkId")) require(row.getString("sourceWorkId").matches(Regex("[1-9][0-9]{0,11}")))
                if (row.has("mode")) require(row.getString("mode") in setOf("AUTO", "WORK", "COLLECTION"))
                if (row.has("rating")) require(row.getString("rating") in setOf("UNRATED", "SAFE", "NSFW"))
                if (table.name == "media_sources") require(row.getString("kind") in setOf("local_imported", "online_provider"))
                if (table.name == "remote_assets") require(row.getString("kind") in setOf("subtitle", "image", "cover"))
            }
        }
        val indexed = tables.associate { table -> table.name to rows(root, table).associateBy { it.get(table.key).toString() } }
        fun ref(row: JSONObject, key: String, table: String) { if (!row.isNull(key)) require(indexed.getValue(table).containsKey(row.get(key).toString())) { "资料包关联缺失" } }
        tables.forEach { table -> rows(root, table).forEach { row ->
            if(table.name=="queue_groups") {
                require(row.getString("name").isNotBlank() && row.getString("name").length<=80)
                com.asmrplayer.android.data.QueueGroup.decode(row.getString("versionIds")).forEach { require(indexed.getValue("track_versions").containsKey(it)) }
            }
            if (row.has("rootId") && table.name != "work_assets") ref(row, "rootId", "library_roots")
            if (row.has("workId")) ref(row, "workId", "works")
            if (row.has("trackId")) ref(row, "trackId", "tracks")
            if (row.has("versionId") && table.name != "track_versions") ref(row, "versionId", "track_versions")
            if (table.name == "work_assets") ref(row, "rootId", "library_roots")
            if (table.name == "subtitles") ref(row, "assetId", "remote_assets")
            if (table.name == "subtitles" && !row.isNull("assetId")) {
                val asset = indexed.getValue("remote_assets").getValue(row.getString("assetId"))
                val version = indexed.getValue("track_versions").getValue(row.getString("versionId"))
                val track = indexed.getValue("tracks").getValue(version.getString("trackId"))
                require(asset.getString("kind") == "subtitle" && asset.getString("workId") == track.getString("workId"))
            }
            if (table.name == "subtitle_preferences") listOf("originalId", "translationId").forEach { key ->
                ref(row, key, "subtitles")
                if (!row.isNull(key)) require(indexed.getValue("subtitles").getValue(row.getString(key)).getString("versionId") == row.getString("versionId"))
            }
            if (table.name == "media_sources" && row.getString("kind") == "online_provider") require(indexed.getValue("remote_media").containsKey(row.getString("versionId")))
        } }
        val queue = root.getJSONArray("queue"); require(queue.length() <= 500)
        for (i in 0 until queue.length()) require(indexed.getValue("track_versions").containsKey(queue.getString(i)))
        return root
    }
    private fun path(value: String) {
        require(!value.startsWith('/') && value.split('/').none { it == ".." || it == "." }) { "无效相对路径" }
    }
}
