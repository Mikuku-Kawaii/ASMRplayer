package com.asmrplayer.android.backup

import android.database.sqlite.SQLiteDatabase
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class QueueGroupsTest {
    private val instrumentation=InstrumentationRegistry.getInstrumentation()
    private val context=instrumentation.targetContext
    private suspend fun seed(db:LibraryDatabase) {
        db.library().putWork(LibraryWork("work",null,"","Work"))
        listOf("a","b","c").forEach {
            db.library().putTrack(LibraryTrack("track-$it","work",it,"WAV"))
            db.library().putVersion(TrackVersion(it,"track-$it","wav"))
        }
        db.library().putPosition(PlaybackPosition("a",1234,3000))
        db.library().putSession(SessionRecord(queueJson="[\"a\",\"b\"]",index=0,positionMs=1234))
    }
    @Test fun editsPersistWithoutChangingRuntimeQueueAndRejectStaleReorders() = runBlocking {
        val name="queue-group-${UUID.randomUUID()}.db"
        var db=Room.databaseBuilder(context,LibraryDatabase::class.java,name).build()
        try {
            seed(db)
            val repo=QueueGroupRepository(db)
            val id=repo.create("Night",listOf("a","b"))
            repo.add(id,listOf("b","c"))
            assertEquals(listOf("a","b","c"),db.queueGroups().get(id)!!.ids())
            repo.move(id,"c",0,listOf("a","b","c"))
            try {repo.move(id,"a",2,listOf("a","b","c"));fail("stale move accepted")} catch(_:IllegalArgumentException) {}
            try {repo.add(id,listOf("missing"));fail("invalid version accepted")} catch(_:IllegalArgumentException) {}
            repo.rename(id,"Evening")
            db.close();db=Room.databaseBuilder(context,LibraryDatabase::class.java,name).build()
            val saved=db.queueGroups().get(id)!!
            assertEquals("Evening",saved.name);assertEquals(listOf("c","a","b"),saved.ids())
            val reopened=QueueGroupRepository(db)
            reopened.remove(id,"b");reopened.delete(id);assertNull(db.queueGroups().get(id))
            reopened.restore(saved)
            assertEquals(listOf("c","a","b"),db.queueGroups().get(id)!!.ids())
            assertEquals("[\"a\",\"b\"]",db.library().session()!!.queueJson)
            assertEquals(1234L,db.library().position("a")!!.positionMs)
        } finally {db.close();context.deleteDatabase(name)}
    }
    @Test fun portableGroupsRoundTripAndVersionThreeImportsWithEmptyGroups() = runBlocking {
        val source=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        val target=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        try {
            seed(source);val id=QueueGroupRepository(source).create("Saved",listOf("c","a"))
            val collection=QueueGroupRepository(source).create("整部",works=listOf(CollectionWork("work","WAV","wav")))
            val vanished = "a".repeat(64)
            val siteGroup = QueueGroup("asmr-list-" + "b".repeat(64), "长名称".repeat(40), sortOrder = 9,
                workItems = CollectionWork.encode(listOf(CollectionWork(vanished), CollectionWork("work"))))
            source.queueGroups().put(siteGroup)
            source.metadata().put(CatalogMetadata("work",origin="https://api.asmr.one",sourceId="123"))
            val bytes=BackupRepository(context,source).exportBytes()
            val restore=BackupRepository(context,target)
            restore.apply(restore.prepare(bytes))
            assertEquals(listOf("c","a"),target.queueGroups().get(id)!!.ids())
            assertFalse(target.queueGroups().get(id)!!.isWorks)
            assertEquals(listOf(CollectionWork("work","WAV","wav")),target.queueGroups().get(collection)!!.works())
            assertEquals("123",target.metadata().forWork("work")!!.sourceId)
            assertEquals(siteGroup, target.queueGroups().get(siteGroup.id))
            val legacy=JSONObject(bytes.toString(Charsets.UTF_8)).put("version",3)
            legacy.getJSONObject("records").remove("queue_groups")
            assertEquals(0,PortableFormat.parse(legacy.toString().toByteArray()).getJSONObject("records").getJSONArray("queue_groups").length())
            val broken=JSONObject(bytes.toString(Charsets.UTF_8))
            broken.getJSONObject("records").getJSONArray("queue_groups").getJSONObject(0).put("versionIds","[\"missing\"]")
            try {restore.prepare(broken.toString().toByteArray());fail("orphan accepted")}catch(_:IllegalArgumentException) {}
        } finally {source.close();target.close()}
    }
    @Test fun desktopPacketRestoresOnlineWorkAndKeepsMissingEntriesInOrder() = runBlocking {
        val db=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        try {
            val protocol=com.asmrplayer.android.sync.SyncProtocol
            val doc=protocol.expand(JSONObject(instrumentation.context.assets.open("playlist-exchange.json").bufferedReader().use { it.readText() }))
            protocol.validate(doc)
            val selected=protocol.selected(doc)
            val catalog=com.asmrplayer.android.sync.SyncCatalog(mutableMapOf(),mutableMapOf(),mutableMapOf())
            val sql=db.openHelper.writableDatabase
            catalog.restoreLikes(sql,doc,selected,"https://api.asmr.one")
            catalog.apply(sql,selected)
            val list=db.queueGroups().get("fixture-works")!!.works()
            assertEquals("VJ01002150",db.library().work(list[0].id)!!.code)
            assertTrue(list[0].fromSite);assertEquals("本篇",list[0].directory);assertEquals("wav",list[0].format)
            assertTrue(list[1].id.startsWith("sync:"));assertTrue(db.queueGroups().get("fixture-tracks")!!.ids().single().startsWith("sync:"))
            val roundtrip=catalog.snapshot(sql)
            for(key in selected.keys()) assertEquals(selected.getJSONObject(key).getJSONArray("items").toString(),roundtrip.getJSONObject(key).getJSONArray("items").toString())
        } finally { db.close() }
    }

    @Test fun versionFourBackupRetainsOrderAndEmptyTrackLists() = runBlocking {
        val db=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        try {
            seed(db)
            val repo=QueueGroupRepository(db)
            repo.create("旧收藏",listOf("c","a")); repo.create("空歌单")
            val doc=JSONObject(BackupRepository(context,db).exportBytes().toString(Charsets.UTF_8)).put("version",4)
            val records=doc.getJSONObject("records");records.remove("catalog_metadata")
            val groups=records.getJSONArray("queue_groups")
            for(i in 0 until groups.length()) groups.getJSONObject(i).remove("workItems")
            val versions=records.getJSONArray("track_versions")
            for(i in 0 until versions.length()) versions.getJSONObject(i).remove("durationMs")
            val restored=PortableFormat.parse(doc.toString().toByteArray()).getJSONObject("records").getJSONArray("queue_groups")
            val byName=(0 until restored.length()).map { restored.getJSONObject(it) }.associateBy { it.getString("name") }
            assertTrue(byName.values.all { it.isNull("workItems") })
            assertEquals(listOf("c","a"), QueueGroup.decode(byName.getValue("旧收藏").getString("versionIds")))
            assertEquals(emptyList<String>(), QueueGroup.decode(byName.getValue("空歌单").getString("versionIds")))
        } finally { db.close() }
    }

    @Test fun schemaSevenUpgradeKeepsSessionAndLibrary() = runBlocking {
        val name="group-migration-${UUID.randomUUID()}.db"
        val path=context.getDatabasePath(name);path.parentFile!!.mkdirs()
        try {
            val schema=JSONObject(instrumentation.context.assets.open("com.asmrplayer.android.data.LibraryDatabase/7.json").bufferedReader().use{it.readText()}).getJSONObject("database")
            SQLiteDatabase.openOrCreateDatabase(path,null).use {old->
                val entities=schema.getJSONArray("entities")
                for(i in 0 until entities.length()) {
                    val entity=entities.getJSONObject(i);val table=entity.getString("tableName")
                    old.execSQL(entity.getString("createSql").replace("\${TABLE_NAME}",table))
                    val indices=entity.optJSONArray("indices")?:org.json.JSONArray()
                    for(j in 0 until indices.length())old.execSQL(indices.getJSONObject(j).getString("createSql").replace("\${TABLE_NAME}",table))
                }
                val setup=schema.getJSONArray("setupQueries");for(i in 0 until setup.length())old.execSQL(setup.getString(i))
                old.execSQL("INSERT INTO works VALUES('work',NULL,'','Old',NULL,NULL,1,'SAFE',1)")
                old.execSQL("INSERT INTO playback_session VALUES(1,'[]',-1,123,NULL,NULL,NULL,NULL,0)")
                old.version=7
            }
            val migrated=Room.databaseBuilder(context,LibraryDatabase::class.java,name).addMigrations(MIGRATION_7_8,MIGRATION_8_9).build()
            try {
                assertTrue(migrated.library().work("work")!!.favorite)
                assertEquals(123L,migrated.library().session()!!.positionMs)
                assertTrue(migrated.queueGroups().all().isEmpty())
                QueueGroupRepository(migrated).create("After upgrade")
                assertEquals(1,migrated.queueGroups().all().size)
            } finally {migrated.close()}
        } finally {context.deleteDatabase(name)}
    }
}
