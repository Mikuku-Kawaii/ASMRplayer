package com.asmrplayer.android.online

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class OnlineCardMetadataTest {
    @Test fun cardKeepsSourceMetadataAndRealScore() {
        val work=AsmrJson.work(JSONObject("""{"id":123,"title":"森の音","rate_average_2dp":4.87,"has_subtitle":true,"nsfw":false,"vas":[{"name":"声優 A"}],"tags":[{"name":"自然音"},{"name":"睡眠"}]}"""))
        assertEquals(4.87,work.score!!,0.001)
        assertTrue(work.hasSubtitle)
        assertEquals("SAFE",work.rating)
        assertEquals(listOf("声優 A"),work.voices)
        assertEquals(listOf("自然音","睡眠"),work.tags)
    }

    @Test fun absentOrInvalidScoreIsNotInvented() {
        for(value in listOf(null,JSONObject.NULL,"unrated",0,-1,6)) {
            val json=JSONObject().put("id",123).put("title","森の音")
            if(value!=null)json.put("rate_average_2dp",value)
            assertNull(AsmrJson.work(json).score)
        }
    }
}
