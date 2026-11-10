package com.asmrplayer.android.sync

import org.json.JSONObject
import org.json.JSONArray

internal object SyncProtocol {
    const val LIMIT = 16 * 1024 * 1024
    fun empty() = JSONObject().put("schema", 1).put("records", JSONObject())
    fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it.toInt() and 255) }
    fun rows(array: JSONArray) = (0 until array.length()).map(array::getJSONObject)
    fun wire(doc: JSONObject):JSONObject {
        val wire=JSONObject().put("schema",doc.getInt("schema"))
        val ordinary=JSONObject()
        val library=JSONObject()
        val playlists=JSONObject()
        val records=doc.getJSONObject("records")
        for (key in records.keys()) (if(key.startsWith("library|")) library else if(key.startsWith("playlist|")) playlists else ordinary).put(key,records.get(key))
        if(library.length()>0) wire.put("libraryRecords",library)
        if(playlists.length()>0)wire.put("playlistRecords",playlists)
        doc.optJSONObject("onlineWorks")?.let { wire.put("onlineWorks",it) }
        return wire.put("records",ordinary)
    }
    fun expand(doc:JSONObject):JSONObject {
        val records=doc.getJSONObject("records")
        for((field,kind) in listOf("libraryRecords" to "library|","playlistRecords" to "playlist|")) {
            val extra=doc.optJSONObject(field) ?: continue
            for(key in extra.keys()){require(key.startsWith(kind)){"同步扩展记录无效"};records.put(key,extra.get(key))}
            doc.remove(field)
        }
        return doc
    }
    fun validate(doc: JSONObject) {
        require(doc.getInt("schema") == 1) { "不支持的同步格式" }
        doc.optJSONObject("onlineWorks")?.let { works ->
            require(works.length() <= 100000 && works.keys().asSequence().all { SyncCatalog.validDescriptor(it, works.getJSONObject(it)) }) { "在线作品身份无效" }
        }
        val records = doc.getJSONObject("records")
        require(records.length() <= 100000)
        records.keys().forEach { key ->
            val p = key.split('|'); val rows = records.getJSONArray(key)
            require(p.size == 3 && p[0] in listOf("work", "library", "track", "position", "bookmark", "playlist") && p[1].matches(Regex("[0-9a-f]{64}")) && p[2].length <= 128 && rows.length() in 1..100)
            rows(rows).forEach { r ->
                require(r.getString("device").length in 1..64 && r.getLong("seq") in 1..1_000_000_000_000 && r.getLong("clock") in 0..1_000_000_000_000)
                val ctx = r.getJSONObject("context"); require(ctx.length() <= 1000)
                ctx.keys().forEach { require(ctx.getLong(it) in 0..1_000_000_000_000) }
                val v = r.get("value")
                when (p[0]) {
                    "playlist" -> require(SyncPlaylists.valid(p[1],p[2],v))
                    "work", "library", "track" -> require(v is Boolean)
                    "position" -> { require(v is JSONObject && v.get("completed") is Boolean); listOf("positionMs", "durationMs", "updatedMs").forEach { require(v.getLong(it) in 0..9_000_000_000_000) } }
                    "bookmark" -> if (v != JSONObject.NULL) { require(v is JSONObject && v.getLong("positionMs") in 0..9_000_000_000_000 && v.getLong("createdMs") >= 0 && v.getString("note").toByteArray().size <= 64000) }
                }
            }
        }
    }
    fun vector(doc: JSONObject): JSONObject {
        val v = JSONObject(); val records = doc.getJSONObject("records")
        records.keys().forEach { key -> rows(records.getJSONArray(key)).forEach { r ->
            val ctx = r.getJSONObject("context")
            ctx.keys().forEach { v.put(it, maxOf(v.optLong(it), ctx.getLong(it))) }
            val d = r.getString("device"); v.put(d, maxOf(v.optLong(d), r.getLong("seq")))
        } }; return v
    }
    fun fillUnknownRating(item: JSONObject, known: JSONObject?) {
        if (item.optString("rating") == "UNRATED" && known != null) item.put("rating",known.getString("rating"))
    }
    fun completeDescriptor(item:JSONObject, known:JSONObject?) {
        fillUnknownRating(item,known)
        if(!item.has("localKey") && known?.has("localKey")==true) item.put("localKey",known.getString("localKey"))
        val incoming=known?.optJSONObject("tracks") ?: return
        val tracks=item.optJSONObject("tracks") ?: JSONObject().also { item.put("tracks",it) }
        for(key in incoming.keys()) {
            val previous=incoming.getJSONObject(key)
            val ref=tracks.optJSONObject(key)
            if(ref==null) tracks.put(key,previous)
            else if(!ref.has("localKey") && previous.has("localKey")) ref.put("localKey",previous.getString("localKey"))
        }
    }
    fun merge(local: JSONObject, remote: JSONObject) {
        expand(remote)
        validate(remote)
        remote.optJSONObject("onlineWorks")?.let { source ->
            val dest=local.optJSONObject("onlineWorks") ?: JSONObject().also { local.put("onlineWorks",it) }
            for (key in source.keys()) {
                val incoming=source.getJSONObject(key)
                val localWork=dest.optJSONObject(key)
                if (localWork==null) dest.put(key,JSONObject(incoming.toString()))
                else completeDescriptor(localWork,incoming)
            }
        }
        val target = local.getJSONObject("records"); val source = remote.getJSONObject("records")
        source.keys().forEach { key ->
            val all = rows(target.optJSONArray(key) ?: JSONArray()).toMutableList()
            rows(source.getJSONArray(key)).forEach { r -> if (all.none { it.getString("device") == r.getString("device") && it.getLong("seq") == r.getLong("seq") }) all.add(r) }
            val live = all.filter { r -> all.none { it.getJSONObject("context").optLong(r.getString("device")) >= r.getLong("seq") } }
                .sortedWith(compareBy<JSONObject> { it.getLong("clock") }.thenBy { it.getString("device") }.thenBy { it.getLong("seq") })
            target.put(key, JSONArray(live))
        }; validate(local)
    }
    fun selected(doc: JSONObject): JSONObject {
        val result = JSONObject(); val records = doc.getJSONObject("records")
        records.keys().forEach { key -> val rows = records.getJSONArray(key); result.put(key, rows.getJSONObject(rows.length() - 1).get("value")) }
        return result
    }
    fun edit(j: JSONObject, key: String, value: Any) {
        val doc = j.getJSONObject("doc"); val context = vector(doc); val device = j.getString("device")
        val records = doc.getJSONObject("records"); var clock = 0L
        records.keys().forEach { k -> rows(records.getJSONArray(k)).forEach { clock = maxOf(clock, it.getLong("clock")) } }
        val r = JSONObject().put("device", device).put("seq", context.optLong(device) + 1).put("clock", clock + 1).put("context", context).put("value", value)
        records.put(key, JSONArray().put(r))
    }
    fun capture(j: JSONObject, current: JSONObject, targets: Set<String>) {
        val baseline = j.getJSONObject("baseline")
        val changes = mutableListOf<Pair<String, Any>>()
        current.keys().forEach { key -> val v = current.get(key)
            if (baseline.opt(key)?.toString() != v.toString() && (baseline.has(key) || v != false)) changes.add(key to v)
        }
        baseline.keys().forEach { key -> if (!current.has(key) && (key.startsWith("playlist|") || (key.startsWith("bookmark|") && key.split('|')[1] in targets))) changes.add(key to JSONObject.NULL) }
        val doc = j.getJSONObject("doc"); val context = vector(doc); val device = j.getString("device"); val records = doc.getJSONObject("records"); var clock = 0L
        records.keys().forEach { key -> rows(records.getJSONArray(key)).forEach { clock = maxOf(clock, it.getLong("clock")) } }
        changes.forEach { (key, value) ->
            val seq = context.optLong(device) + 1; clock++
            records.put(key, JSONArray().put(JSONObject().put("device", device).put("seq", seq).put("clock", clock).put("context", JSONObject(context.toString())).put("value", value)))
            context.put(device, seq)
        }
        j.put("baseline", current)
    }
}
