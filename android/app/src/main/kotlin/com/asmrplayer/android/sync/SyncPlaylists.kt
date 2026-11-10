package com.asmrplayer.android.sync

import androidx.sqlite.db.SupportSQLiteDatabase
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
    fun referenced(values:JSONObject):Set<String> = values.keys().asSequence().filter{it.startsWith("playlist|")&&!values.isNull(it)}.flatMap {
        val items=values.optJSONObject(it)?.optJSONArray("items") ?: JSONArray()
        (0 until items.length()).asSequence().map(items::getString)
    }.toSet()
    fun snapshot(db:SupportSQLiteDatabase,versions:Map<String,String>,result:JSONObject) {
        db.query("SELECT id,name,versionIds,sortOrder FROM queue_groups ORDER BY sortOrder,id").use { rows->
            while(rows.moveToNext()) {
                val ids=com.asmrplayer.android.data.QueueGroup.decode(rows.getString(2)).map{key(it,versions)}.distinct()
                val id=rows.getString(0)
                result.put("playlist|${hash(id)}|$id",JSONObject().put("name",rows.getString(1)).put("items",JSONArray(ids)).put("sortOrder",rows.getLong(3)))
            }
        }
    }
    fun apply(db:SupportSQLiteDatabase,values:JSONObject,versions:Map<String,String>) {
        val lookup=mutableMapOf<String,String>()
        db.query("SELECT v.id FROM track_versions v ORDER BY EXISTS(SELECT 1 FROM media_sources s WHERE s.versionId=v.id AND s.available=1) DESC,v.id").use { rows->while(rows.moveToNext()) {
            val id=rows.getString(0);lookup.putIfAbsent(key(id,versions),id);lookup[fallback(id)]=id
        } }
        for(record in values.keys().asSequence().filter{it.startsWith("playlist|")}) {
            val id=record.split('|')[2]
            if(values.isNull(record)){db.execSQL("DELETE FROM queue_groups WHERE id=?",arrayOf(id));continue}
            val value=values.getJSONObject(record);val items=value.getJSONArray("items")
            val ids=(0 until items.length()).map{items.getString(it).let{key->lookup[key] ?: "sync:$key"}}
            db.execSQL("INSERT OR REPLACE INTO queue_groups(id,name,versionIds,sortOrder) VALUES(?,?,?,?)",arrayOf(id,value.getString("name"),JSONArray(ids).toString(),value.getLong("sortOrder")))
        }
    }
}
