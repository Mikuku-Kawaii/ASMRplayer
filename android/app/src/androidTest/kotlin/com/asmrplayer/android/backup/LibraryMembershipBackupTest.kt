package com.asmrplayer.android.backup

import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Membership migration uses only isolated databases; no SAF provider or real media is needed. */
@RunWith(AndroidJUnit4::class)
class LibraryMembershipBackupTest {
    @Test fun cachedMembershipRoundTripsAndLegacyBundlesPreserveExplicitLibraries() = runBlocking {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val source=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        val target=Room.inMemoryDatabaseBuilder(context,LibraryDatabase::class.java).build()
        try {
            source.library().putWork(LibraryWork("cached",null,"","Cached",favorite=true,inLibrary=false))
            source.library().putTrack(LibraryTrack("track","cached","Track","WAV"))
            source.library().putVersion(TrackVersion("version","track","wav"))
            source.library().putPosition(PlaybackPosition("version",1234,3000))
            source.library().putBookmark(Bookmark("bookmark","version",1200,"Preserved"))
            val bytes=BackupRepository(context,source).exportBytes()
            val restore=BackupRepository(context,target)
            val plan=restore.prepare(bytes)
            assertEquals(4,plan.preview.version)
            restore.apply(plan)
            assertFalse(target.library().work("cached")!!.inLibrary)
            assertTrue(target.library().work("cached")!!.favorite)
            assertEquals(1234L,target.library().position("version")!!.positionMs)
            assertEquals("Preserved",target.library().bookmark("bookmark")!!.note)
            target.library().membership("cached",true)
            restore.apply(plan)
            assertTrue(target.library().work("cached")!!.inLibrary)
            restore.apply(plan,ImportPolicy.REPLACE_USER_DATA)
            assertFalse(target.library().work("cached")!!.inLibrary)
            for(version in listOf(1,2)) {
                val legacy=JSONObject(bytes.toString(Charsets.UTF_8)).put("version",version)
                val records=legacy.getJSONObject("records")
                if(version==1)records.remove("version_preferences")
                records.getJSONArray("works").getJSONObject(0).remove("inLibrary")
                restore.apply(restore.prepare(legacy.toString().toByteArray()),ImportPolicy.REPLACE_USER_DATA)
                assertTrue(target.library().work("cached")!!.inLibrary)
            }
        } finally {source.close();target.close()}
    }
}
