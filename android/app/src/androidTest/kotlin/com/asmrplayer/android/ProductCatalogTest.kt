package com.asmrplayer.android

import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.*
import com.asmrplayer.android.online.*
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Run on the project test emulator: all catalog rows are synthetic and in-memory. */
@RunWith(AndroidJUnit4::class)
class ProductCatalogTest {
    private val context=InstrumentationRegistry.getInstrumentation().targetContext
    @Test fun combinedMetadataFiltersRespectOfflineAndContentVisibility()=runBlocking {
        val db=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        try {
            val dao=db.library()
            for(id in listOf("local","lost","online","adult")) {
                dao.putWork(LibraryWork(id,null,"",id,favorite=id=="local",rating=if(id=="adult")"NSFW" else "SAFE"))
                dao.putTrack(LibraryTrack("t$id",id,"track","wav"));dao.putVersion(TrackVersion("v$id","t$id","wav"))
                dao.putSource(MediaSource("s$id","v$id",null,"track.wav","content://fixture/$id",available=id!="lost",kind=if(id=="online")"online_provider" else "local_imported"))
                db.metadata().put(CatalogMetadata(id,"Circle","[\"Voice\"]","[\"ASMR\",\"Sleep\"]",hasSubtitle=id!="online"))
            }
            val repo=LibraryCollections(db)
            val query=LibraryQuery(tags=listOf("ASMR","sleep"),voices=listOf("voice"),circle="circle",subtitleOnly=true,offlineOnly=true)
            assertEquals(listOf("local"),repo.catalog(query).first().map{it.work.id})
            assertEquals(listOf("local","lost","online"),repo.catalog(LibraryQuery(text="Voice")).first().map{it.work.id})
            assertTrue(repo.catalog(query.copy(tags=listOf("missing"))).first().isEmpty())
            assertTrue(repo.catalog(query.copy(favorite=2)).first().isEmpty())
            assertTrue(repo.catalog(query.copy(rating="NSFW")).first().isEmpty())
            dao.putPreferences(LibraryPreference(includeNsfw=true))
            assertEquals(listOf("adult"),repo.catalog(query.copy(rating="NSFW")).first().map{it.work.id})
        } finally{db.close()}
    }
    @Test fun discoveryUsesRecommenderAndStructuredServerSideFilters()=runBlocking {
        val http=HttpTransport(true);val account=ProviderAccount(context,http);val before=account.state.value.apiBase
        OnlineFixtureServer().use {server->
            try {
                account.setApiBase(server.base)
                val provider=AsmrOneProvider(account,http)
                provider.browse(order="popular",includeNsfw=true)
                assertTrue(server.requests.any{it.path=="/api/recommender/popular"&&it.method=="POST"})
                assertEquals(listOf("123456"), provider.browse(order="popular",includeNsfw=false).items.map { it.sourceId })
                assertEquals(listOf("123457"), provider.browse(order="popular",includeNsfw=true,filters=BrowseFilters(rating="adult")).items.map { it.sourceId })
                server.mode = "popular-unavailable"
                val failure = runCatching { provider.browse(order="popular",includeNsfw=true) }.exceptionOrNull()
                assertEquals(OnlineError.NETWORK, (failure as OnlineException).code)
                assertTrue(server.requests.all { it.method=="POST" && it.path=="/api/recommender/popular" })
                server.mode = "normal"
                val filters=BrowseFilters(tags=listOf("耳かき"),voices=listOf("Voice A"),excludedTags=listOf("大声"),subtitleOnly=true)
                provider.browse("sleep",order="review_count",includeNsfw=true,filters=filters)
                val path=java.net.URLDecoder.decode(server.requests.last().path,"UTF-8")
                assertTrue(path.contains("\$tag:耳かき\$"));assertTrue(path.contains("\$va:Voice A\$"));assertTrue(path.contains("\$-tag:大声\$"))
                assertTrue(path.contains("order=review_count"));assertTrue(path.contains("subtitle=1"))
                val count=server.requests.size
                assertTrue(provider.browse(filters=BrowseFilters(rating="adult"),includeNsfw=false).items.isEmpty())
                assertEquals(count,server.requests.size)
                listOf("dl_count","rate_average_2dp","create_date","release","review_count").forEach {provider.browse(order=it,includeNsfw=true)}
            } finally{account.setApiBase(before)}
        }
    }
    @Test fun importedMetadataIsAvailableToLocalCatalogWithoutRefetch()=runBlocking {
        val db=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        val http=HttpTransport(true);val account=ProviderAccount(context,http);val before=account.state.value.apiBase
        try {OnlineFixtureServer().use{server->
            account.setApiBase(server.base);server.attachments=true
            val online=OnlineRepository(db,AsmrOneProvider(account,http))
            val imported=online.importWork("123456")
            val meta=db.metadata().observe().first().single()
            assertEquals(imported.workId,meta.workId);assertEquals(server.base,meta.origin);assertTrue(meta.hasSubtitle)
            val count=server.requests.size
            assertEquals(imported.workId,LibraryCollections(db).catalog(LibraryQuery(subtitleOnly=true)).first().single().work.id)
            assertEquals(count,server.requests.size)
        }}finally{account.setApiBase(before);db.close()}
    }
}
