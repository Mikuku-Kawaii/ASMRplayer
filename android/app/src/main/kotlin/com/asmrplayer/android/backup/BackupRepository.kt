package com.asmrplayer.android.backup

import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import androidx.room.withTransaction
import com.asmrplayer.android.data.LibraryDatabase
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream

enum class ImportPolicy { KEEP_EXISTING, REPLACE_USER_DATA }
data class ImportPreview(val version: Int, val createdMs: Long, val counts: Map<String, Int>, val existingRecords: Int,
    val directoryIds: List<String>, val queueSize: Int)
class PreparedImport internal constructor(val preview: ImportPreview, internal val document: String)
data class ImportResult(val inserted: Int, val updated: Int, val kept: Int, val directoryIds: List<String>, val queueVersionIds: List<String>)

/** AN009: portable metadata only. Import never replaces a live database, queue, permission or credential. */
class BackupRepository(private val context: Context, private val db: LibraryDatabase) {
    suspend fun exportBytes(): ByteArray = withContext(Dispatchers.IO) {
        val document = db.withTransaction {
            val sql = db.openHelper.readableDatabase
            val records = JSONObject()
            var totalRows = 0; var estimatedBytes = 0L
            PortableFormat.tables.forEach { table ->
                val query = when (table.name) {
                    "subtitles" -> "SELECT s.*, COALESCE((SELECT r.id FROM remote_assets r WHERE s.uri = 'asmr-subtitle://' || r.id LIMIT 1), (SELECT r.id FROM work_assets a JOIN remote_assets r ON r.id=a.id WHERE a.uri=s.uri LIMIT 1)) AS assetId FROM subtitles s"
                    "media_sources" -> "SELECT * FROM media_sources WHERE kind IN ('local_imported','online_provider')"
                    "work_assets" -> "SELECT * FROM work_assets WHERE rootId IN (SELECT id FROM library_roots)"
                    else -> "SELECT * FROM ${table.name}"
                }
                val rows = JSONArray()
                sql.query(query).use { cursor ->
                    while (cursor.moveToNext()) {
                        currentCoroutineContext().ensureActive()
                        val row = JSONObject()
                        table.fields.forEach { key ->
                            val column = cursor.getColumnIndexOrThrow(key)
                            row.put(key, when (cursor.getType(column)) {
                                Cursor.FIELD_TYPE_NULL -> JSONObject.NULL
                                Cursor.FIELD_TYPE_INTEGER -> cursor.getLong(column)
                                else -> cursor.getString(column)
                            })
                        }
                        totalRows++; estimatedBytes += row.toString().toByteArray(Charsets.UTF_8).size
                        require(totalRows <= 100_000 && estimatedBytes <= PortableFormat.MAX_BYTES) { "资料包超出导出大小限制" }
                        rows.put(row)
                    }
                }
                records.put(table.name, rows)
            }
            val versions = PortableFormat.tables.single { it.name == "track_versions" }.let { table ->
                val rows = records.getJSONArray(table.name); (0 until rows.length()).map { rows.getJSONObject(it).getString("id") }.toSet()
            }
            val queue = db.library().session()?.queueJson?.let { JSONArray(it) } ?: JSONArray()
            val stableQueue = JSONArray((0 until queue.length()).map { queue.getString(it) }.filter { it in versions })
            JSONObject().put("format", PortableFormat.NAME).put("version", PortableFormat.VERSION)
                .put("createdMs", System.currentTimeMillis()).put("records", records).put("queue", stableQueue)
        }
        val bytes = document.toString().toByteArray(Charsets.UTF_8)
        PortableFormat.parse(bytes) // Never emit a bundle our importer cannot validate.
        bytes
    }

    /** Destination is the result of CreateDocument(application/json); SAF atomic rename is not assumed. */
    suspend fun exportTo(uri: Uri): Int = withContext(Dispatchers.IO) {
        require(uri.scheme == "content")
        val bytes = exportBytes()
        val output = context.contentResolver.openOutputStream(uri, "wt") ?: error("无法打开导出文件")
        output.use { it.write(bytes); it.flush() }
        bytes.size
    }
    suspend fun prepare(uri: Uri): PreparedImport = withContext(Dispatchers.IO) {
        require(uri.scheme == "content")
        val bytes = context.contentResolver.openInputStream(uri)?.use { input ->
            val output = ByteArrayOutputStream(); val buffer = ByteArray(8192)
            while (true) {
                currentCoroutineContext().ensureActive()
                val count = input.read(buffer); if (count < 0) break
                require(output.size() + count <= PortableFormat.MAX_BYTES) { "资料包超过 32 MiB" }
                output.write(buffer, 0, count)
            }
            output.toByteArray()
        } ?: error("无法读取资料包")
        prepare(bytes)
    }
    suspend fun prepare(bytes: ByteArray): PreparedImport = withContext(Dispatchers.IO) {
        val document = PortableFormat.parse(bytes.copyOf())
        val existing = db.withTransaction { PortableFormat.tables.sumOf { table ->
            PortableFormat.rows(document, table).count { row -> exists(table, row.get(table.key).toString()) }
        } }
        PreparedImport(ImportPreview(PortableFormat.VERSION, document.getLong("createdMs"),
            PortableFormat.tables.associate { it.name to PortableFormat.rows(document, it).size }, existing,
            PortableFormat.rows(document, PortableFormat.tables.first()).map { it.getString("id") }, document.getJSONArray("queue").length()), document.toString())
    }
    suspend fun apply(prepared: PreparedImport, policy: ImportPolicy = ImportPolicy.KEEP_EXISTING): ImportResult = withContext(Dispatchers.IO) {
        val document = PortableFormat.parse(prepared.document.toByteArray(Charsets.UTF_8))
        db.withTransaction {
            val sql = db.openHelper.writableDatabase
            var inserted = 0; var updated = 0; var kept = 0
            PortableFormat.tables.forEach { table -> PortableFormat.rows(document, table).forEach { row ->
                currentCoroutineContext().ensureActive()
                val key = row.get(table.key).toString()
                if (exists(table, key)) {
                    // An ID collision must not redirect old history to another work/version/source.
                    val identity = table.fields.filter { it in setOf("rootId", "workId", "trackId", "versionId", "origin", "sourceWorkId", "remoteFileId", "relativePath", "kind", "format") && it != table.key }
                    sql.query("SELECT * FROM ${table.name} WHERE ${table.key} = ?", arrayOf(key)).use { cursor ->
                        check(cursor.moveToFirst())
                        identity.forEach { field ->
                            val old = cursor.getString(cursor.getColumnIndexOrThrow(field))
                            val incoming = if (row.isNull(field)) null else row.getString(field)
                            require(old == incoming) { "标识冲突：资料包内容与当前库不匹配" }
                        }
                    }
                    if (policy == ImportPolicy.REPLACE_USER_DATA && table.userColumns.isNotEmpty()) {
                        sql.update(table.name, SQLiteDatabase.CONFLICT_ABORT, values(row, table.userColumns), "${table.key} = ?", arrayOf(key)); updated++
                    } else kept++
                } else {
                    val value = values(row, table.fields.toSet() - "assetId")
                    when (table.name) {
                        "library_roots" -> { value.put("treeUri", "unbound://root/${Uri.encode(key)}"); value.put("available", 0); value.put("lastScanMs", 0) }
                        "works" -> value.putNull("coverUri")
                        "remote_assets" -> value.put("available", 1)
                        "media_sources" -> {
                            val online = row.getString("kind") == "online_provider"
                            value.put("uri", if (online) "asmr-online://${row.getString("versionId")}" else "unbound://source/${Uri.encode(key)}")
                            value.put("available", if (online) 1 else 0)
                        }
                        "work_assets" -> { value.put("uri", "unbound://asset/${Uri.encode(key)}"); value.put("available", 0) }
                        "subtitles" -> {
                            value.put("uri", if (!row.isNull("assetId")) "unbound://asset/${Uri.encode(row.getString("assetId"))}" else "unbound://subtitle/${Uri.encode(key)}")
                            value.put("available", 0)
                        }
                    }
                    sql.insert(table.name, SQLiteDatabase.CONFLICT_ABORT, value); inserted++
                }
            } }
            val queue = document.getJSONArray("queue")
            ImportResult(inserted, updated, kept, prepared.preview.directoryIds, List(queue.length()) { queue.getString(it) })
        }
    }
    private fun exists(table: PortableTable, id: String): Boolean = db.openHelper.readableDatabase.query(
        "SELECT 1 FROM ${table.name} WHERE ${table.key} = ?", arrayOf(id)).use { it.moveToFirst() }
    private fun values(row: JSONObject, fields: Set<String>) = ContentValues().apply {
        fields.forEach { key -> when (val value = row.get(key)) {
            JSONObject.NULL -> putNull(key)
            is Number -> put(key, value.toLong())
            else -> put(key, value as String)
        } }
    }
}
