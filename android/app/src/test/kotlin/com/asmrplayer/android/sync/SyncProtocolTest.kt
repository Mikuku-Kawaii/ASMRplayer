package com.asmrplayer.android.sync

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SyncProtocolTest {
    private fun journal(device: String) = JSONObject().put("device", device).put("doc", SyncProtocol.empty()).put("baseline", JSONObject())
    @Test fun concurrentRevisionsAndResolutionAreCommutative() {
        val a = journal("a"); val b = journal("b"); val key = "work|${"a".repeat(64)}|"
        SyncProtocol.edit(a, key, true); SyncProtocol.merge(b.getJSONObject("doc"), a.getJSONObject("doc"))
        SyncProtocol.edit(a, key, false); SyncProtocol.edit(b, key, true)
        val left = JSONObject(a.getJSONObject("doc").toString()); val right = JSONObject(b.getJSONObject("doc").toString())
        SyncProtocol.merge(left, b.getJSONObject("doc")); SyncProtocol.merge(right, a.getJSONObject("doc"))
        assertEquals(left.toString(), right.toString()); assertEquals(2, left.getJSONObject("records").getJSONArray(key).length())
        a.put("doc", left); SyncProtocol.edit(a, key, false); SyncProtocol.merge(right, a.getJSONObject("doc"))
        assertEquals(false, SyncProtocol.selected(right).get(key)); assertEquals(1, right.getJSONObject("records").getJSONArray(key).length())
    }
    @Test fun deletedBookmarksDoNotReturnAndMissingMediaDoesNotDelete() {
        val a = journal("a"); val target = "b".repeat(64); val key = "bookmark|$target|mark"
        val current = JSONObject().put(key, JSONObject().put("positionMs", 1200).put("note", "note").put("createdMs", 1000))
        SyncProtocol.capture(a, current, setOf(target)); val old = JSONObject(a.getJSONObject("doc").toString())
        SyncProtocol.capture(a, JSONObject(), emptySet()); assertFalse(SyncProtocol.selected(a.getJSONObject("doc")).isNull(key))
        a.put("baseline", current); SyncProtocol.capture(a, JSONObject(), setOf(target)); SyncProtocol.merge(a.getJSONObject("doc"), old)
        assertTrue(SyncProtocol.selected(a.getJSONObject("doc")).isNull(key))
    }
}
