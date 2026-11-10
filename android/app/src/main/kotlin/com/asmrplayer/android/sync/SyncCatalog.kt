package com.asmrplayer.android.sync

import android.content.Context
import android.net.Uri
import androidx.sqlite.db.SupportSQLiteDatabase
import com.asmrplayer.android.data.LibraryDatabase
import org.json.JSONObject
import java.security.MessageDigest

internal class SyncCatalog(val versions: MutableMap<String, String>, val works: MutableMap<String, String>, val names: MutableMap<String, String>) {
    val targets get() = (versions.values + works.values).toSet()
    fun describeLikes(db: SupportSQLiteDatabase, doc: JSONObject) {
        val descriptors = doc.optJSONObject("onlineWorks") ?: JSONObject().also { doc.put("onlineWorks", it) }
        val tracked=doc.getJSONObject("records").keys().asSequence().map { it.split('|')[1] }.toHashSet()
        tracked.addAll(SyncPlaylists.referenced(SyncProtocol.selected(doc)))
        val byWork=mutableMapOf<String,JSONObject>()
        db.query("SELECT t.workId,r.versionId,r.sourceWorkId,r.remoteFileId,r.relativePath,r.byteSize FROM remote_media r JOIN track_versions v ON v.id=r.versionId JOIN tracks t ON t.id=v.trackId").use { rows ->
            while(rows.moveToNext()){
                val key=versions[rows.getString(1)] ?: continue
                if(key !in tracked) continue
                val ref=JSONObject().put("fileId",rows.getString(3)).put("path",rows.getString(4)).put("bytes",if(rows.isNull(5)) JSONObject.NULL else rows.getLong(5))
                if(validTrack(rows.getString(2).trimStart('0'),key,ref)) byWork.getOrPut(rows.getString(0)){JSONObject()}.put(key,ref)
            }
        }
        db.query("SELECT w.id,m.sourceId,w.title,w.rating,(w.favorite=1 OR w.inLibrary=1) FROM works w JOIN catalog_metadata m ON m.workId=w.id WHERE m.origin<>''").use { rows ->
            while (rows.moveToNext()) {
                val key = works[rows.getString(0)] ?: continue
                val tracks=byWork[rows.getString(0)]
                if(tracks==null && rows.getInt(4)==0) continue
                val item = JSONObject().put("sourceId",rows.getString(1).trimStart('0')).put("title",rows.getString(2)).put("rating",rows.getString(3))
                if(tracks!=null) item.put("tracks",tracks)
                if (validDescriptor(key,item)) {
                    SyncProtocol.completeDescriptor(item,descriptors.optJSONObject(key))
                    descriptors.put(key,item)
                }
            }
        }
    }
    // A publisher can attach a verified source to an existing file fingerprint.
    // Keep its revision history under that fingerprint when the online ID has no competing edits.
    fun useVerifiedKeys(doc: JSONObject):Map<String,String> {
        val entries=doc.optJSONObject("onlineWorks") ?: return emptyMap()
        val recorded=doc.getJSONObject("records").keys().asSequence().map { it.split('|')[1] }.toHashSet()
        recorded.addAll(SyncPlaylists.referenced(SyncProtocol.selected(doc)))
        val replacements=mutableMapOf<String,String>()
        fun accept(key:String,item:JSONObject) {
            val local=item.optString("localKey")
            if(local in recorded && key !in recorded) replacements[key]=local
        }
        for(key in entries.keys()) {
            val item=entries.getJSONObject(key)
            accept(key,item)
            item.optJSONObject("tracks")?.let { refs -> for(ref in refs.keys()) accept(ref,refs.getJSONObject(ref)) }
        }
        val ambiguous=replacements.values.groupingBy { it }.eachCount().filterValues { it>1 }.keys
        replacements.entries.removeAll { it.value in ambiguous }
        if(replacements.isNotEmpty()) {
            for(map in listOf(works,versions)) for(entry in map.entries) replacements[entry.value]?.let { entry.setValue(it) }
            for((source,local) in replacements) names[source]?.let { names[local]=it }
        }
        return replacements
    }
    fun restoreLikes(db: SupportSQLiteDatabase, doc: JSONObject, values: JSONObject, base: String) {
        val descriptors = doc.optJSONObject("onlineWorks") ?: return
        val replacements=useVerifiedKeys(doc)
        val knownVersions=versions.values.toMutableSet()
        val needed=values.keys().asSequence().filter {
            val kind=it.substringBefore('|')
            kind=="position" || (kind=="bookmark" && !values.isNull(it)) || (kind=="track" && values.optBoolean(it))
        }.map { it.split('|')[1] }.toHashSet()
        needed.addAll(SyncPlaylists.referenced(values).map { replacements[it] ?: it })
        val owners=works.entries.associate { it.value to it.key }
        for (canonical in descriptors.keys()) {
            val key=replacements[canonical] ?: canonical
            val item=descriptors.getJSONObject(canonical)
            val source=item.getString("sourceId")
            val tracks=item.optJSONObject("tracks")
            val missing=tracks?.keys()?.asSequence()?.filter {
                val effective=replacements[it] ?: it
                (effective in needed || (item.has("localKey") && values.optBoolean("library|$key|"))) && effective !in knownVersions
            }?.toList().orEmpty()
            if(missing.isEmpty() && (key in owners || !(values.optBoolean("work|$key|") || values.optBoolean("library|$key|")))) continue
            val id=owners[key] ?: com.asmrplayer.android.library.LibraryIndex.id("asmr_one",base,source)
            if(key !in owners) {
                db.execSQL("INSERT OR IGNORE INTO works(id,rootId,relativePath,title,code,coverUri,favorite,rating,inLibrary) VALUES(?,NULL,'',?,?,NULL,0,?,0)",
                arrayOf(id,item.getString("title"),"RJ"+source.padStart(8,'0'),item.getString("rating")))
                db.execSQL("INSERT OR IGNORE INTO catalog_metadata(workId,circle,voicesJson,tagsJson,description,release,hasSubtitle,origin,sourceId) VALUES(?,'','[]','[]','','',0,?,?)",arrayOf(id,base,source))
                works[id]=key
                names[key]=item.getString("title")
            }
            for(trackKey in missing) {
                val ref=requireNotNull(tracks).getJSONObject(trackKey)
                val effective=replacements[trackKey] ?: trackKey
                restoreOnlineVersion(db,id,base,source,effective,ref,item.getString("title"))
                knownVersions.add(effective)
            }
        }
    }
    private fun restoreOnlineVersion(db:SupportSQLiteDatabase, work:String, base:String, source:String, key:String, ref:JSONObject, title:String) {
        val path=ref.getString("path")
        val file=ref.getString("fileId")
        val track=com.asmrplayer.android.library.LibraryIndex.id(work,"track",path.substringBeforeLast('.'))
        val version=com.asmrplayer.android.library.LibraryIndex.id(work,"version",file,path)
        val name=path.substringAfterLast('/')
        val format=name.substringAfterLast('.',"bin").lowercase()
        val size=if(ref.isNull("bytes")) null else ref.getLong("bytes")
        db.execSQL("INSERT OR IGNORE INTO tracks(id,workId,title,groupPath) VALUES(?,?,?,?)",arrayOf(track,work,name,path.substringBeforeLast('/',"")))
        db.execSQL("INSERT OR IGNORE INTO track_versions(id,trackId,format) VALUES(?,?,?)",arrayOf(version,track,format))
        db.execSQL("INSERT OR IGNORE INTO remote_media(versionId,origin,sourceWorkId,remoteFileId,relativePath,byteSize,format) VALUES(?,?,?,?,?,?,?)",arrayOf(version,base,source,file,path,size,format))
        db.execSQL("INSERT OR IGNORE INTO media_sources(id,versionId,rootId,relativePath,uri,size,modifiedMs,available,kind) VALUES(?,?,NULL,?,?,?,0,1,'online_provider')",arrayOf(com.asmrplayer.android.library.LibraryIndex.id(version,"online"),version,path,"asmr-online://$version",size ?: 0L))
        versions[version]=key
        names[key]="$title · $name"
    }
    fun snapshot(db: SupportSQLiteDatabase): JSONObject {
        val result = JSONObject()
        works.forEach { (id, key) -> db.query("SELECT favorite,inLibrary FROM works WHERE id=?", arrayOf<Any>(id)).use { if (it.moveToFirst()) {
            result.put("work|$key|", it.getInt(0) != 0)
            result.put("library|$key|", it.getInt(1) != 0)
        } } }
        versions.forEach { (id, key) ->
            db.query("SELECT favorite FROM version_preferences WHERE versionId=?", arrayOf<Any>(id)).use { result.put("track|$key|", it.moveToFirst() && it.getInt(0) != 0) }
            db.query("SELECT positionMs,durationMs,completed,updatedMs FROM playback_positions WHERE versionId=?", arrayOf<Any>(id)).use {
                if (it.moveToFirst()) result.put("position|$key|", JSONObject().put("positionMs", it.getLong(0)).put("durationMs", it.getLong(1)).put("completed", it.getInt(2) != 0).put("updatedMs", it.getLong(3)))
            }
            db.query("SELECT id,positionMs,note,createdMs FROM bookmarks WHERE versionId=?", arrayOf<Any>(id)).use {
                while (it.moveToNext()) result.put("bookmark|$key|${it.getString(0)}", JSONObject().put("positionMs", it.getLong(1)).put("note", it.getString(2)).put("createdMs", it.getLong(3)))
            }
        }; SyncPlaylists.snapshot(db,versions,result); return result
    }
    fun apply(db: SupportSQLiteDatabase, values: JSONObject) {
        SyncPlaylists.apply(db,values,versions)
        values.keys().forEach { key -> val p = key.split('|'); val v = values.get(key)
            val ids = if (p[0] in setOf("work","library")) works else versions
            ids.filterValues { it == p[1] }.keys.forEach { id ->
                when (p[0]) {
                    "work" -> db.execSQL("UPDATE works SET favorite=? WHERE id=?", arrayOf<Any>(if (v == true) 1 else 0, id))
                    "library" -> db.execSQL("UPDATE works SET inLibrary=? WHERE id=?", arrayOf<Any>(if (v == true) 1 else 0, id))
                    "track" -> db.execSQL("INSERT OR REPLACE INTO version_preferences(versionId,favorite,updatedMs) VALUES(?,?,?)", arrayOf<Any>(id, if (v == true) 1 else 0, System.currentTimeMillis()))
                    "position" -> { v as JSONObject; db.execSQL("INSERT OR REPLACE INTO playback_positions(versionId,positionMs,durationMs,completed,updatedMs) VALUES(?,?,?,?,?)", arrayOf<Any>(id, v.getLong("positionMs"), v.getLong("durationMs"), if (v.getBoolean("completed")) 1 else 0, v.getLong("updatedMs"))) }
                    "bookmark" -> if (v == JSONObject.NULL) db.execSQL("DELETE FROM bookmarks WHERE id=? AND versionId=?", arrayOf<Any>(p[2], id)) else {
                        v as JSONObject
                        db.execSQL("INSERT OR IGNORE INTO bookmarks(id,versionId,positionMs,note,createdMs) VALUES(?,?,?,?,?)", arrayOf<Any>(p[2], id, v.getLong("positionMs"), v.getString("note"), v.getLong("createdMs")))
                        db.execSQL("UPDATE bookmarks SET positionMs=?,note=? WHERE id=? AND versionId=?", arrayOf<Any>(v.getLong("positionMs"), v.getString("note"), p[2], id))
                    }
                }
            }
        }
    }
    companion object {
        fun validDescriptor(key: String, item: JSONObject): Boolean {
            val id=item.optString("sourceId")
            if(!validLocalKey(item))return false
            if(item.has("tracks")) {
                val tracks=item.optJSONObject("tracks") ?: return false
                if(tracks.length()>50_000 || !tracks.keys().asSequence().all { k->tracks.optJSONObject(k)?.let { validTrack(id,k,it) }==true })return false
            }
            return id.matches(Regex("[1-9][0-9]{0,11}")) && hash("asmr_one|$id")==key &&
                item.optString("title").toByteArray().size in 1..4096 && item.optString("rating") in setOf("SAFE","NSFW","UNRATED")
        }
        private fun validTrack(source:String,key:String,ref:JSONObject):Boolean {
            val file=ref.optString("fileId")
            val path=ref.optString("path")
            return validLocalKey(ref) && file.toByteArray().size in 1..8192 && path.toByteArray().size in 1..4096 &&
                '\\' !in path && '\u0000' !in path && path.split('/').none { it.isEmpty() || it=="." || it==".." } &&
                (ref.isNull("bytes") || ref.optLong("bytes",-1)>=0) && key==hash("asmr_one|$source|$file")
        }
        private fun validLocalKey(item:JSONObject)=!item.has("localKey") || item.optString("localKey").matches(Regex("[0-9a-f]{64}"))
        private fun hash(text: String) = SyncProtocol.hex(MessageDigest.getInstance("SHA-256").digest(text.toByteArray()))
        fun load(context: Context, database: LibraryDatabase, cache: JSONObject): SyncCatalog {
            val db = database.openHelper.readableDatabase
            val versions = mutableMapOf<String, String>(); val workOnline = mutableMapOf<String, String>(); val groups = mutableMapOf<String, MutableSet<String>>()
            db.query("SELECT r.versionId,t.workId,r.sourceWorkId,r.remoteFileId FROM remote_media r JOIN track_versions v ON v.id=r.versionId JOIN tracks t ON t.id=v.trackId").use {
                while (it.moveToNext()) { val work = it.getString(2).trimStart('0'); versions[it.getString(0)] = hash("asmr_one|$work|${it.getString(3)}"); workOnline[it.getString(1)] = hash("asmr_one|$work") }
            }
            db.query("SELECT s.id,s.versionId,t.workId,s.uri,s.size,s.modifiedMs,s.available,s.kind FROM media_sources s JOIN track_versions v ON v.id=s.versionId JOIN tracks t ON t.id=v.trackId").use { cursor ->
                while (cursor.moveToNext()) {
                    val id = cursor.getString(0); val version = cursor.getString(1)
                    if (!versions.containsKey(version)) {
                        val uri = cursor.getString(3); val size = cursor.getLong(4); val modified = cursor.getLong(5)
                        val saved = cache.optJSONObject(id)
                        if (cursor.getInt(6) != 0 && cursor.getString(7) != "online_provider" && !uri.endsWith(".part")) {
                            if (saved == null || saved.optString("uri") != uri || saved.optLong("size") != size || saved.optLong("modified") != modified) {
                                val digest = MessageDigest.getInstance("SHA-256")
                                val input = if (uri.startsWith("/")) java.io.File(uri).inputStream() else context.contentResolver.openInputStream(Uri.parse(uri))
                                requireNotNull(input) { "无法读取作品文件，请重新授权目录" }.use { stream -> val buffer = ByteArray(65536); while (true) { val n = stream.read(buffer); if (n < 0) break; digest.update(buffer, 0, n) } }
                                cache.put(id, JSONObject().put("uri", uri).put("size", size).put("modified", modified).put("sha", SyncProtocol.hex(digest.digest())))
                            }
                        }
                        cache.optJSONObject(id)?.getString("sha")?.let { versions[version] = it }
                    }
                    versions[version]?.let { groups.getOrPut(cursor.getString(2)) { sortedSetOf() }.add(it) }
                }
            }
            db.query("SELECT workId,sourceId FROM catalog_metadata WHERE origin<>'' AND sourceId<>''").use {
                while (it.moveToNext()) workOnline[it.getString(0)] = hash("asmr_one|${it.getString(1).trimStart('0')}")
            }
            val works = (groups.mapValues { (id, group) -> workOnline[id] ?: hash("files|${group.sorted().joinToString("|")}") } + workOnline).toMutableMap()
            val aliases = db.query("SELECT json FROM device_sync_state WHERE id=1").use { if(it.moveToFirst()) JSONObject(it.getString(0)).optJSONObject("aliases") else null }
            for ((table, mapping, key) in listOf(Triple("works",works,"works"),Triple("track_versions",versions,"versions"))) {
                val saved = aliases?.optJSONObject(key) ?: continue
                db.query("SELECT id FROM $table").use { c -> while(c.moveToNext()) saved.optString(c.getString(0)).takeIf { it.matches(Regex("[0-9a-f]{64}")) }?.let { mapping[c.getString(0)] = it } }
            }
            val names = mutableMapOf<String, String>()
            db.query("SELECT id,title FROM works").use { while (it.moveToNext()) works[it.getString(0)]?.let { key -> names[key] = it.getString(1) } }
            db.query("SELECT v.id,w.title,t.title FROM track_versions v JOIN tracks t ON t.id=v.trackId JOIN works w ON w.id=t.workId").use { while (it.moveToNext()) versions[it.getString(0)]?.let { key -> names[key] = "${it.getString(1)} · ${it.getString(2)}" } }
            return SyncCatalog(versions, works, names)
        }
    }
}
