package com.asmrplayer.android

import android.net.Uri
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.online.OnlineFixtureServer
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

internal fun OnlineFixtureServer.Request.browsePage(): Int =
    if (method == "POST") JSONObject(body).optInt("page", 1)
    else Uri.parse("http://localhost$path").getQueryParameter("page")?.toInt() ?: 1

internal fun feedPage(page: Int, ids: List<Int>, total: Int = 6, pageSize: Int = 2, adult: Boolean = false): String =
    """{"works":[${ids.joinToString(",") { """{"id":$it,"title":"Feed work $it","age_category_string":"${if (adult) "r18" else "general"}"}""" }}],"pagination":{"currentPage":$page,"pageSize":$pageSize,"totalCount":$total}}"""

@RunWith(AndroidJUnit4::class)
class OnlinePaginationTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private fun main(action: () -> Unit) = instrumentation.runOnMainSync(action)
    private var step = 0
    private suspend fun wait(tools: OnlineTools, predicate: (OnlineScreenState) -> Boolean): OnlineScreenState {
        val currentStep = ++step
        return try { withTimeout(10_000) { tools.state.first(predicate) } }
        catch (e: TimeoutCancellationException) { throw AssertionError("Step $currentStep: ${tools.state.value}", e) }
    }

    private fun fixture(test: suspend (OnlineFixtureServer, OnlineTools) -> Unit) = runBlocking {
        val context = instrumentation.targetContext
        check(context.packageName.endsWith(".uitest"))
        val scenario = androidx.test.core.app.ActivityScenario.launch(MainActivity::class.java)
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        val prefs = backend.library.preferences.first()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        OnlineFixtureServer().use { server ->
            try {
                backend.library.setContentFilter(false, true)
                backend.online.account.setApiBase(server.base)
                val tools = OnlineTools(context, scope) { _, _, _ -> }
                delay(150)
                test(server, tools)
            } finally {
                scope.cancel()
                backend.online.account.setApiBase(oldBase)
                backend.library.setContentFilter(prefs.includeNsfw, prefs.includeUnrated)
                scenario.close()
            }
        }
    }

    @Test fun appendDeduplicatesRetriesSamePageAndRefreshesFromStart() = fixture { server, tools ->
        val fail = AtomicBoolean(true)
        server.browseResponse = { request ->
            val page = request.browsePage()
            if (page == 2 && fail.get()) 503 to ""
            else 200 to feedPage(page, when (page) {
                1 -> listOf(123451, 123452)
                2 -> listOf(123452, 123453)
                else -> listOf(123454, 123455)
            })
        }
        main { tools.browsing(true) }
        val first = wait(tools) { !it.loading && it.page != null }
        main { repeat(5) { tools.loadNextPage() } }
        val failed = wait(tools) { !it.loadingMore && it.moreError != null }
        assertEquals(first.page, failed.page)
        // One logical request, including the repository's three HTTP attempts.
        assertEquals(3, server.requests.count { it.browsePage() == 2 })
        fail.set(false)
        main { tools.loadNextPage() }
        val second = wait(tools) { !it.loadingMore && it.page?.page == 2 }
        assertEquals(listOf("123451", "123452", "123453"), second.page!!.items.map { it.sourceId })
        assertEquals(first.feedRevision, second.feedRevision)
        main { tools.loadNextPage() }
        val last = wait(tools) { !it.loadingMore && it.page?.page == 3 }
        assertFalse(last.hasMore)
        val requests = server.requests.size
        main { tools.loadNextPage() }
        delay(150)
        assertEquals(requests, server.requests.size)
        main { tools.search(page = 1) }
        val refreshed = wait(tools) { !it.loading && it.page?.page == 1 }
        assertEquals(2, refreshed.page!!.items.size)
        assertTrue(refreshed.feedRevision > first.feedRevision)
    }

    @Test fun filteredEmptyPageStillAllowsNextPage() = fixture { server, tools ->
        server.browseResponse = { request ->
            val page = request.browsePage()
            200 to feedPage(page, listOf(123450 + page), total = 3, pageSize = 1, adult = page == 2)
        }
        main { tools.browsing(true) }
        wait(tools) { !it.loading && it.page != null }
        main { tools.loadNextPage() }
        val second = wait(tools) { !it.loadingMore && it.page?.page == 2 }
        assertEquals(1, second.page!!.items.size)
        assertTrue(second.hasMore)
        main { tools.loadNextPage() }
        val last = wait(tools) { !it.loadingMore && it.page?.page == 3 }
        assertEquals(listOf("123451", "123453"), last.page!!.items.map { it.sourceId })
        assertFalse(last.hasMore)
    }

    @Test fun changingSearchCancelsLateAppendAndLeavingStopsPagination() = fixture { server, tools ->
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        server.browseResponse = { request ->
            val page = request.browsePage()
            if (page == 2) { entered.countDown(); release.await(5, TimeUnit.SECONDS) }
            200 to feedPage(page, listOf(if (request.path.contains("fresh")) 654321 else 123450 + page))
        }
        try {
            main { tools.browsing(true) }
            wait(tools) { !it.loading && it.page != null }
            main { tools.loadNextPage() }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            main { tools.search("fresh") }
            wait(tools) { !it.loading && it.page?.items?.firstOrNull()?.sourceId == "654321" }
            release.countDown()
            delay(250)
            assertEquals(listOf("654321"), tools.state.value.page!!.items.map { it.sourceId })
            main { tools.browsing(false) }
            val requests = server.requests.size
            main { tools.loadNextPage() }
            delay(150)
            assertEquals(requests, server.requests.size)
        } finally { release.countDown() }
    }
}
