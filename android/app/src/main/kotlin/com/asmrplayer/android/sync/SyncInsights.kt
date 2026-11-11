package com.asmrplayer.android.sync

import org.json.JSONArray
import org.json.JSONObject

internal object SyncInsights {
    private val kinds = linkedMapOf("library" to "音频库作品", "work" to "喜欢作品", "track" to "收藏音轨",
        "position" to "收听进度", "bookmark" to "时间标记", "playlist" to "歌单")
    private fun normalized(value: Any?): String = when(value) {
        is JSONObject -> value.keys().asSequence().filter { it!="deviceName" }.sorted().joinToString(prefix="{",postfix="}") { JSONObject.quote(it)+":"+normalized(value.get(it)) }
        is JSONArray -> (0 until value.length()).joinToString(prefix="[",postfix="]") { normalized(value.get(it)) }
        is String -> JSONObject.quote(value)
        else -> value.toString()
    }
    fun describe(doc: JSONObject, before: JSONObject, catalog: SyncCatalog, device: String): Pair<JSONObject, JSONObject?> {
        val selected = SyncProtocol.selected(doc)
        val records = doc.getJSONObject("records")
        val keys = selected.keys().asSequence().toList()
        fun pending(key: String) = !key.startsWith("playlist|") && key.split('|')[1] !in catalog.targets
        val summary = JSONArray()
        for ((kind,label) in kinds) {
            val rows = keys.filter { it.startsWith("$kind|") }
            summary.put(JSONObject().put("kind",kind).put("label",label)
                .put("total",rows.count { !selected.isNull(it) && selected.get(it) != false })
                .put("received",rows.count { normalized(before.opt(it)) != normalized(selected.get(it)) && SyncProtocol.rows(records.getJSONArray(it)).last().getString("device") != device })
                .put("pending",rows.count(::pending)))
        }
        val descriptors=doc.optJSONObject("onlineWorks") ?: JSONObject()
        fun label(target:String):String = catalog.names[target] ?: descriptors.optJSONObject(target)?.optString("title")
            ?: descriptors.keys().asSequence().mapNotNull { key ->
                val work=descriptors.getJSONObject(key)
                work.optJSONObject("tracks")?.optJSONObject(target)?.let { "${work.optString("title")} · ${it.optString("path")}" }
            }.firstOrNull() ?: "尚未匹配的本地内容"
        val pendingItems=JSONArray()
        keys.filter(::pending).take(50).forEach { key -> pendingItems.put(JSONObject().put("key",key).put("label",label(key.split('|')[1]))
            .put("kind",key.substringBefore('|')).put("reason","未找到相同音源，请导入原文件或补全在线作品")) }
        val resume=keys.filter { it.startsWith("position|") }.mapNotNull { key ->
            val rows=records.getJSONArray(key)
            if(rows.length()!=1) return@mapNotNull null
            val r=rows.getJSONObject(0);val v=r.getJSONObject("value")
            if(r.getString("device")==device || v.getBoolean("completed") || v.getLong("positionMs")<=0 ||
                v.getLong("updatedMs") <= (before.optJSONObject(key)?.optLong("updatedMs") ?: 0)) return@mapNotNull null
            val target=key.split('|')[1]
            val version=catalog.versions.entries.find { it.value==target }?.key ?: return@mapNotNull null
            JSONObject().put("id","${r.getString("device")}:${r.getLong("seq")}:$target").put("versionId",version)
                .put("title",label(target)).put("positionMs",v.getLong("positionMs")).put("updatedMs",v.getLong("updatedMs"))
                .put("deviceName",v.optString("deviceName").takeIf { it.isNotBlank() && it.length<=80 } ?: "其他设备")
        }.maxByOrNull { it.getLong("updatedMs") }
        return JSONObject().put("summary",summary).put("pendingItems",pendingItems) to resume
    }
}
