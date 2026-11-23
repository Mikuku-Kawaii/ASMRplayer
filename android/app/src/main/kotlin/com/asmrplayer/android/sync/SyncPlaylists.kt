package com.asmrplayer.android.sync

import androidx.sqlite.db.SupportSQLiteDatabase
import com.asmrplayer.android.data.CollectionWork
import org.json.JSONArray
import org.json.JSONObject
import java.security.MessageDigest

internal object SyncPlaylists {
    private fun hash(value:String)=SyncProtocol.hex(MessageDigest.getInstance("SHA-256").digest(value.toByteArray()))
    private fun fallback(id:String)=hash("playlist-android|$id")
    private fun key(id:String,versions:Map<String,String>)=versions[id] ?: if(id.startsWith("sync:"))id.removePrefix("sync:") else fallback(id)
    fun valid(identity:String,id:String,value:Any):Boolean {
        if(id.isEmpty()||hash(id)!=identity)return false
        if(value==JSONObject.NULL)return true
        if(value !is JSONObject)return false
        val name=value.opt("name") as? String ?: return false
        val order=value.opt("sortOrder")
        val items=value.optJSONArray("items") ?: return false
        if(name.isBlank()||name.codePointCount(0,name.length)>80||order !is Number||order.toDouble()!=order.toLong().toDouble()||order.toLong() !in 0..1_000_000_000_000)return false
        if((0 until items.length()).any { items.opt(it) !is String })return false
        val ids=(0 until items.length()).map{items.getString(it)}
        return ids.size<=500&&ids.distinct().size==ids.size&&ids.all{it.matches(Regex("[0-9a-f]{64}"))}
    }
    fun validWorks(identity:String,id:String,value:Any):Boolean {
        if(id.isEmpty() || hash(id)!=identity)return false
        if(value==JSONObject.NULL)return true
        if(value !is JSONObject)return false
        val items=value.optJSONArray("items") ?: return false
        val name=value.opt("name") as? String ?: return false
        val order=value.opt("sortOrder") as? Number ?: return false
        if(!com.asmrplayer.android.data.QueueGroup.validName(id, name) || order.toDouble()!=order.toLong().toDouble() || order.toLong() !in 0..1_000_000_000_000)return false
        val seen=hashSetOf<String>()
        return (0 until items.length()).all { i -> runCatching { val w=CollectionWork.parse(items.getJSONObject(i)); w.id.matches(Regex("[0-9a-f]{64}")) && seen.add(w.id) }.getOrDefault(false) }
    }
    fun referenced(values:JSONObject):Set<String> = values.keys().asSequence().filter{(it.startsWith("worklist|") || it.startsWith("playlist|"))&&!values.isNull(it)}.flatMap {
        val items=values.optJSONObject(it)?.optJSONArray("items") ?: JSONArray()
        (0 until items.length()).asSequence().map { i -> items.optJSONObject(i)?.getString("id") ?: items.getString(i) }
    }.toSet()
    fun snapshot(db:SupportSQLiteDatabase,versions:Map<String,String>,result:JSONObject,works:Map<String,String> = emptyMap()) {
        db.query("SELECT id,name,versionIds,sortOrder,workItems FROM queue_groups ORDER BY sortOrder,id").use { rows->
            while(rows.moveToNext()) {
                if(!rows.isNull(4)) {
                    val raw=JSONArray(rows.getString(4))
                    val items=(0 until raw.length()).map { CollectionWork.parse(raw.getJSONObject(it)) }.map { it.copy(id=key(it.id,works)) }.distinctBy { it.id }
                    val id=rows.getString(0)
                    result.put("worklist|${hash(id)}|$id",JSONObject().put("name",rows.getString(1)).put("items",JSONArray(CollectionWork.encode(items))).put("sortOrder",rows.getLong(3)))
                    continue
                }
                val ids=com.asmrplayer.android.data.QueueGroup.decode(rows.getString(2)).map{key(it,versions)}.distinct()
                val id=rows.getString(0)
                result.put("playlist|${hash(id)}|$id",JSONObject().put("name",rows.getString(1)).put("items",JSONArray(ids)).put("sortOrder",rows.getLong(3)))
            }
        }
    }
    fun apply(db:SupportSQLiteDatabase,values:JSONObject,versions:Map<String,String>,works:Map<String,String> = emptyMap()) {
        val lookup=mutableMapOf<String,String>()
        db.query("SELECT v.id FROM track_versions v ORDER BY EXISTS(SELECT 1 FROM media_sources s WHERE s.versionId=v.id AND s.available=1) DESC,v.id").use { rows->while(rows.moveToNext()) {
            val id=rows.getString(0);lookup.putIfAbsent(key(id,versions),id);lookup[fallback(id)]=id
        } }
        val workLookup = mutableMapOf<String,String>()
        db.query("SELECT id FROM works ORDER BY inLibrary DESC,id").use { rows -> while(rows.moveToNext()) {
            val id=rows.getString(0); workLookup.putIfAbsent(key(id,works),id);workLookup[fallback(id)]=id
        } }
        for(record in values.keys().asSequence().filter{it.startsWith("playlist|") || it.startsWith("worklist|")}) {
            val isWorks=record.startsWith("worklist|")
            val id=record.split('|')[2]
            if(values.isNull(record)){db.execSQL("DELETE FROM queue_groups WHERE id=? AND (workItems IS NOT NULL)=?",arrayOf(id,if(isWorks) 1 else 0));continue}
            val value=values.getJSONObject(record);val items=value.getJSONArray("items")
            if(isWorks) {
                val members=(0 until items.length()).map { CollectionWork.parse(items.getJSONObject(it)) }.map { it.copy(id=workLookup[it.id] ?: "sync:${it.id}") }
                db.execSQL("INSERT OR REPLACE INTO queue_groups(id,name,versionIds,sortOrder,workItems) VALUES(?,?,'[]',?,?)",arrayOf(id,value.getString("name"),value.getLong("sortOrder"),CollectionWork.encode(members)))
                continue
            }
            val ids=(0 until items.length()).map{items.getString(it).let{key->lookup[key] ?: "sync:$key"}}
            db.execSQL("INSERT OR REPLACE INTO queue_groups(id,name,versionIds,sortOrder) VALUES(?,?,?,?)",arrayOf(id,value.getString("name"),JSONArray(ids).toString(),value.getLong("sortOrder")))
        }
    }
}
