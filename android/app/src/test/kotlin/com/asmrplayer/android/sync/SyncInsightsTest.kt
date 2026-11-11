package com.asmrplayer.android.sync
import org.json.JSONObject
import org.json.JSONArray
import org.junit.Assert.*
import org.junit.Test

class SyncInsightsTest {
    private val target="a".repeat(64)
    private val key="position|$target|"
    private val catalog=SyncCatalog(mutableMapOf("local-version" to target),mutableMapOf(),mutableMapOf(target to "合成作品 · 音轨"))
    private fun value()=JSONObject().put("positionMs",4200).put("durationMs",10000).put("updatedMs",2000).put("completed",false).put("deviceName","Windows 电脑")
    private fun document()=SyncProtocol.empty().also { doc ->
        doc.getJSONObject("records").put(key,JSONArray().put(JSONObject().put("device","pc").put("seq",1).put("clock",1).put("context",JSONObject()).put("value",value())))
    }
    @Test fun onlyNewRemoteProgressCreatesAnOfferAndMetadataDoesNotCountAsAChange() {
        val doc=document()
        val (first,offer)=SyncInsights.describe(doc,JSONObject(),catalog,"android")
        assertEquals("local-version",offer!!.getString("versionId"));assertEquals(4200L,offer.getLong("positionMs"))
        val position=SyncProtocol.rows(first.getJSONArray("summary")).first {it.getString("kind")=="position"}
        assertEquals(1,position.getInt("received"))
        val old=JSONObject().put(key,JSONObject().put("completed",false).put("updatedMs",2000).put("durationMs",10000).put("positionMs",4200))
        val (again,resume)=SyncInsights.describe(doc,old,catalog,"android")
        assertNull(resume)
        assertEquals(0,SyncProtocol.rows(again.getJSONArray("summary")).first{it.getString("kind")=="position"}.getInt("received"))
        assertNull(SyncInsights.describe(doc,JSONObject(),catalog,"pc").second)
    }
    @Test fun conflictingCompletedAndUnmatchedProgressDoNotOfferPlayback() {
        val doc=document()
        val rows=doc.getJSONObject("records").getJSONArray(key)
        rows.put(JSONObject(rows.getJSONObject(0).toString()).put("device","other"))
        assertNull(SyncInsights.describe(doc,JSONObject(),catalog,"android").second)
        rows.remove(1);rows.getJSONObject(0).getJSONObject("value").put("completed",true)
        assertNull(SyncInsights.describe(doc,JSONObject(),catalog,"android").second)
        rows.getJSONObject(0).getJSONObject("value").put("completed",false)
        val (details,resume)=SyncInsights.describe(doc,JSONObject(),SyncCatalog(mutableMapOf(),mutableMapOf(),mutableMapOf()),"android")
        assertNull(resume);assertEquals(1,details.getJSONArray("pendingItems").length())
    }
}
