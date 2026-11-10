package com.asmrplayer.android.sync

import androidx.room.Room
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.online.*
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test

class SourceEnrichmentTest {
    @Test fun receivedIdentityGetsLocalSourceMetadataAndRetriesFailures() = runBlocking {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        assumeTrue(context.packageName.endsWith(".drivetest"))
        val db=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        val server=OnlineFixtureServer().apply { attachments=true; requireWebMetadataRequest=true }
        val backup=OnlineFixtureServer().apply { requireWebMetadataRequest=true }
        val http=HttpTransport(debugLoopback=true)
        val online=OnlineRepository(db,AsmrOneProvider(ProviderAccount(context,http),http,listOf(server.base,backup.base)))
        try {
            online.account.setApiBase(server.base)
            db.library().putWork(LibraryWork("receiver",null,"","旧标题",favorite=true,inLibrary=true))
            db.metadata().put(CatalogMetadata("receiver",origin=server.base,sourceId="123456"))
            val enricher=SyncSourceEnrichment(context,db,online)
            server.mode="unauthorized"
            enricher.complete()
            assertEquals(1,enricher.pending())
            assertTrue(db.metadata().observe().first().single().tags().isEmpty())
            server.mode="normal"
            enricher.complete()
            assertEquals(0,enricher.pending())
            val work=requireNotNull(db.library().work("receiver"))
            val metadata=db.metadata().observe().first().single()
            assertEquals("Synthetic online work",work.title)
            assertTrue(work.favorite && work.inLibrary)
            assertTrue(work.coverUri?.startsWith(server.base)==true)
            assertEquals(listOf("Voice"),metadata.voices())
            assertEquals(listOf("General"),metadata.tags())
            assertEquals("Fixture",metadata.circle)
            assertEquals("01110085",AsmrJson.workInfoId("1110085"))
            assertEquals("299717",AsmrJson.workInfoId("299717"))
            assertEquals("100000041",AsmrJson.workInfoId("100000041"))
            assertEquals("1110085",AsmrJson.workId("RJ01110085"))
            val count=server.requests.size
            enricher.complete()
            assertEquals(count,server.requests.size)
            assertEquals(0,server.treeRequests.get())
            online.account.setToken("fixture-primary-only")
            server.mode="metadata-unavailable"
            online.refreshMetadata("receiver","123456")
            assertEquals(server.base,online.account.state.value.apiBase)
            assertTrue(backup.requests.any { it.path=="/api/workInfo/123456" })
            assertTrue(backup.requests.none { it.headers.containsKey("authorization") })
            assertTrue(server.requests.any { it.headers["authorization"]=="Bearer fixture-primary-only" })
            assertTrue(requireNotNull(db.library().work("receiver")).let { it.favorite && it.inLibrary })
        } finally {
            online.account.setToken(null)
            online.account.setApiBase(ProviderAccount.DEFAULT_BASE)
            server.close()
            backup.close()
            db.close()
        }
    }
}
