package com.asmrplayer.android.sync

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.security.MessageDigest

class PlaylistSyncTest {
    private val id="shared-playlist"
    private val key="playlist|${SyncProtocol.hex(MessageDigest.getInstance("SHA-256").digest(id.toByteArray()))}|$id"
    private fun journal()=JSONObject().put("device","android").put("doc",SyncProtocol.empty()).put("baseline",JSONObject())
    private fun value()=JSONObject().put("name","夜雨").put("items",JSONArray(listOf("a".repeat(64),"b".repeat(64)))).put("sortOrder",0)
    @Test fun optionalWireRoundtripAndDeletionKeepOldClientsCompatible() {
        val j=journal();val content=JSONObject().put(key,value())
        SyncProtocol.capture(j,content,emptySet())
        val wire=SyncProtocol.wire(j.getJSONObject("doc"))
        assertEquals(0,wire.getJSONObject("records").length());assertTrue(wire.getJSONObject("playlistRecords").has(key))
        val remote=SyncProtocol.expand(JSONObject(wire.toString()));SyncProtocol.validate(remote)
        assertEquals(listOf("a".repeat(64),"b".repeat(64)),SyncPlaylists.referenced(SyncProtocol.selected(remote)).toList())
        SyncProtocol.capture(j,JSONObject(),emptySet());SyncProtocol.merge(j.getJSONObject("doc"),remote)
        assertTrue(SyncProtocol.selected(j.getJSONObject("doc")).isNull(key))
    }
    @Test fun validatesMemberIdentityUniquenessAndLimits() {
        val identity=key.split('|')[1]
        assertTrue(SyncPlaylists.valid(identity,id,value()))
        assertFalse(SyncPlaylists.valid(identity,"other",value()))
        assertFalse(SyncPlaylists.valid(identity,id,value().put("items",JSONArray(listOf("a".repeat(64),"a".repeat(64))))))
        assertFalse(SyncPlaylists.valid(identity,id,value().put("items",JSONArray(listOf("private-path.wav")))))
        assertFalse(SyncPlaylists.valid(identity,id,value().put("name",true)))
        assertFalse(SyncPlaylists.valid(identity,id,value().put("sortOrder","0")))
        assertFalse(SyncPlaylists.valid(identity,id,value().put("sortOrder",0.5)))
    }
    @Test fun concurrentPlaylistChangesStayResolvable() {
        val a=journal();val b=journal().put("device","pc")
        SyncProtocol.edit(a,key,value());SyncProtocol.merge(b.getJSONObject("doc"),a.getJSONObject("doc"))
        SyncProtocol.edit(a,key,value().put("name","手机"));SyncProtocol.edit(b,key,value().put("items",JSONArray(listOf("b".repeat(64),"a".repeat(64)))))
        SyncProtocol.merge(a.getJSONObject("doc"),b.getJSONObject("doc"));assertEquals(2,a.getJSONObject("doc").getJSONObject("records").getJSONArray(key).length())
        SyncProtocol.edit(a,key,value());SyncProtocol.merge(b.getJSONObject("doc"),a.getJSONObject("doc"));assertEquals(1,b.getJSONObject("doc").getJSONObject("records").getJSONArray(key).length())
    }
    @Test fun verifiedOnlineSourceUsesTheFingerprintReferencedInsideAPlaylist() {
        val fingerprint="b".repeat(64);val canonical="c".repeat(64)
        val catalog=SyncCatalog(mutableMapOf("native" to canonical),mutableMapOf(),mutableMapOf())
        val j=journal();SyncProtocol.edit(j,key,value().put("items",JSONArray(listOf(fingerprint))))
        j.getJSONObject("doc").put("onlineWorks",JSONObject().put("a".repeat(64),JSONObject().put("tracks",JSONObject().put(canonical,JSONObject().put("localKey",fingerprint)))))
        catalog.useVerifiedKeys(j.getJSONObject("doc"));assertEquals(fingerprint,catalog.versions["native"])
    }
}
