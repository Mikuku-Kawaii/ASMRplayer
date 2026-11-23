package com.asmrplayer.android.online

import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class AccountLibraryPaginationTest {
    @Test fun sitePlaylistReadsBothPagesInOrder() {
        val input = requireNotNull(javaClass.getResourceAsStream("/asmr-one-playlist-pages.json"))
        val pages = input.bufferedReader().use { JSONArray(it.readText()) }
        val ids = mutableListOf<String>()
        for (index in 0 until pages.length()) {
            val page = accountPage(pages.getJSONObject(index), "works", index + 1)
            assertEquals(index == 0, page.hasMore)
            for (row in 0 until page.rows.length()) ids.add(AsmrJson.work(page.rows.getJSONObject(row)).sourceId)
        }
        assertEquals((1..100).map(Int::toString), ids)
    }

    @Test fun numericStringsAndReviewPagesPreserveValidation() {
        val rows = JSONObject().put("works", JSONArray().put(JSONObject().put("id", 1)))
        val meta = JSONObject().put("page", "2").put("pageSize", "1").put("totalCount", "2")
        rows.put("pagination", meta)
        assertFalse(accountPage(rows, "works", 2).hasMore)
        assertThrows(JSONException::class.java) { accountPage(rows, "works", 1) }
        meta.put("currentPage", 1)
        assertThrows(JSONException::class.java) { accountPage(rows, "works", 2) }
        meta.remove("page")
        assertTrue(accountPage(rows, "works", 1).hasMore)
        meta.remove("currentPage")
        assertThrows(JSONException::class.java) { accountPage(rows, "works", 1) }
        rows.put("works", JSONArray())
        meta.put("page", 1)
        assertThrows(JSONException::class.java) { accountPage(rows, "works", 1) }
        rows.put("pagination", JSONObject().put("totalCount", 0))
        assertFalse(accountPage(rows, "works", 1).hasMore)
    }
}
