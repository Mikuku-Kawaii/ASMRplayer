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
            val bytes=BackupRepository(context,source).exportBytes()
            val restore=BackupRepository(context,target)
            restore.apply(restore.prepare(bytes))
            assertEquals(listOf("c","a"),target.queueGroups().get(id)!!.ids())
            val legacy=JSONObject(bytes.toString(Charsets.UTF_8)).put("version",3)
            legacy.getJSONObject("records").remove("queue_groups")
            assertEquals(0,PortableFormat.parse(legacy.toString().toByteArray()).getJSONObject("records").getJSONArray("queue_groups").length())
            val broken=JSONObject(bytes.toString(Charsets.UTF_8))
            broken.getJSONObject("records").getJSONArray("queue_groups").getJSONObject(0).put("versionIds","[\"missing\"]")
            try {restore.prepare(broken.toString().toByteArray());fail("orphan accepted")}catch(_:IllegalArgumentException) {}
        } finally {source.close();target.close()}
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
            val migrated=Room.databaseBuilder(context,LibraryDatabase::class.java,name).addMigrations(MIGRATION_7_8).build()
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
