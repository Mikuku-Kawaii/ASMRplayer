package com.asmrplayer.android.library

import android.database.sqlite.SQLiteDatabase
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class LibraryCollectionsTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var db: LibraryDatabase
    private lateinit var library: LibraryRepository
    @Before fun setup() = runBlocking {
        db = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        library = LibraryRepository(context, db)
        val dao = db.library()
        for ((id, title, rating) in listOf(Triple("two", "Work 2", "SAFE"), Triple("ten", "Work 10", "SAFE"),
            Triple("hidden", "Secret", "NSFW"), Triple("unrated", "Unrated", "UNRATED"))) {
            dao.putWork(LibraryWork(id, null, "", title, "RJ$id", rating = rating))
            dao.putTrack(LibraryTrack("$id-track", id, "Track", "WAV"))
            dao.putVersion(TrackVersion("$id-wav", "$id-track", "wav"))
            dao.putSource(MediaSource("$id-source", "$id-wav", null, "01.wav", "content://fixture/$id", available = id != "hidden"))
        }
        dao.putVersion(TrackVersion("two-mp3", "two-track", "mp3"))
        dao.putSource(MediaSource("downloaded", "two-wav", null, "01.wav", "file:///fixture.wav", kind = "local_downloaded"))
        dao.putSource(MediaSource("remote", "two-wav", null, "01.wav", "asmr-online://two-wav", kind = "online_provider"))
        dao.putPosition(PlaybackPosition("two-wav", 300, 1000, updatedMs = 100))
        dao.putPosition(PlaybackPosition("hidden-wav", 400, 1000, updatedMs = 200))
        dao.putBookmark(Bookmark("bookmark", "two-wav", 250, "public note", 100))
        dao.putBookmark(Bookmark("secret", "hidden-wav", 250, "private note", 200))
    }
    @After fun cleanup() { db.close() }
    @Test fun catalogFiltersCountsVersionsOnceAndUsesNaturalOrder() = runBlocking {
        val collections = library.collections
        assertEquals(listOf("two", "ten"), collections.catalog(LibraryQuery(text = "work")).first().map { it.work.id })
        val row = collections.catalog(LibraryQuery(text = "RJtwo")).first().single()
        assertEquals(2, row.versionCount); assertEquals(1, row.playedCount)
        assertEquals(1, row.downloadedCount); assertEquals(1, row.onlineCount)
        assertEquals(listOf("two"), collections.catalog(LibraryQuery(source = LibrarySourceFilter.DOWNLOADED)).first().map { it.work.id })
        assertEquals(listOf("two"), collections.catalog(LibraryQuery(listening = ListeningFilter.IN_PROGRESS)).first().map { it.work.id })
        assertTrue(collections.catalog(LibraryQuery(listening = ListeningFilter.COMPLETED)).first().isEmpty())
        db.library().putPosition(PlaybackPosition("two-wav", 1000, 1000, true, 100))
        db.library().putPosition(PlaybackPosition("two-mp3", 1000, 1000, true, 101))
        assertEquals("two", collections.catalog(LibraryQuery(listening = ListeningFilter.COMPLETED)).first().single().work.id)
        assertEquals("two", collections.catalog(LibraryQuery(sort = LibrarySort.RECENTLY_PLAYED, descending = true)).first().first().work.id)
    }
    @Test fun favoritesHistoryAndBookmarksShareContentPolicyAndStableVersionIdentity() = runBlocking {
        val collections = library.collections
        collections.setVersionFavorite("two-wav", true); collections.setVersionFavorite("hidden-wav", true)
        assertEquals(listOf("two-wav"), collections.favoriteVersions().first().map { it.versionId })
        assertEquals(listOf("two-wav"), collections.history().first().map { it.versionId })
        assertEquals(listOf("bookmark"), collections.bookmarks().first().map { it.bookmark.id })
        assertTrue(collections.bookmarks("private").first().isEmpty())
        library.setContentFilter(includeNsfw = true, includeUnrated = false)
        assertEquals("hidden-wav", collections.history().first().first().versionId)
        assertEquals("secret", collections.bookmarks("private").first().single().bookmark.id)
        assertTrue(collections.catalog().first().none { it.work.id == "unrated" })
        collections.setVersionFavorite("two-wav", false)
        assertFalse(db.collections().preference("two-wav")!!.favorite)
        assertNull(db.collections().preference("two-mp3")) // formats do not inherit each other's preference
        try { collections.setVersionFavorite("missing", true); fail("missing version accepted") } catch (_: IllegalArgumentException) { }
    }
    @Test fun preferenceAndFavoriteChangesInvalidateObservedCollections() = runBlocking {
        val result = async(Dispatchers.Default) { withTimeout(5_000) {
            library.collections.favoriteVersions().first { entries -> entries.any { it.versionId == "hidden-wav" } }
        } }
        library.collections.setVersionFavorite("hidden-wav", true)
        library.setContentFilter(true, true)
        assertEquals("hidden-wav", result.await().single().versionId)
    }
    @Test fun libraryMembershipIsIndependentFromLikesAndPlaybackRecords() = runBlocking {
        library.setMembership("two", false)
        library.setFavorite("two", true)
        library.collections.setVersionFavorite("two-wav",true)
        assertFalse(library.collections.catalog().first().any{it.work.id=="two"})
        assertEquals("two",library.collections.catalog(LibraryQuery(favoritesOnly=true)).first().single().work.id)
        assertTrue(library.collections.favoriteVersions().first().any{it.versionId=="two-wav"})
        assertEquals(300L,library.position("two-wav")!!.positionMs)
        library.setMembership("two",true)
        assertTrue(library.collections.catalog().first().any{it.work.id=="two"})
        library.setMembership("two",false)
        assertEquals(3,db.library().sources("two-wav").size)
        assertEquals("public note",library.bookmarks("two-wav").first().single().note)
    }
    @Test fun widgetStateTracksVersionLikesAndWorkMembershipIndependently() = runBlocking {
        library.setMembership("two", false)
        val stream = db.collections().observeWidgetCollection("two-wav")
        assertEquals(WidgetCollectionState("two-wav", "two", false, false), stream.first())
        val liked = async(Dispatchers.Default) { withTimeout(5_000) { stream.first { it?.favorite == true } } }
        library.collections.setVersionFavorite("two-wav", true)
        assertFalse(liked.await()!!.inLibrary)
        val added = async(Dispatchers.Default) { withTimeout(5_000) { stream.first { it?.inLibrary == true } } }
        library.setMembership("two", true)
        assertTrue(added.await()!!.favorite)
        assertEquals(WidgetCollectionState("two-mp3", "two", false, true), db.collections().observeWidgetCollection("two-mp3").first())
        library.collections.setVersionFavorite("two-wav", false)
        assertEquals(WidgetCollectionState("two-wav", "two", false, true), stream.first())
        assertNull(db.collections().observeWidgetCollection("temporary-url").first())
    }
    @Test fun schemaThreeMigrationRetainsLibrarySessionAndHistory() = runBlocking {
        val name = "collections-migration-${UUID.randomUUID()}.db"
        val path = context.getDatabasePath(name); path.parentFile!!.mkdirs()
        try {
            val schema = JSONObject(instrumentation.context.assets.open("com.asmrplayer.android.data.LibraryDatabase/3.json")
                .bufferedReader().use { it.readText() }).getJSONObject("database")
            SQLiteDatabase.openOrCreateDatabase(path, null).use { old ->
                val entities = schema.getJSONArray("entities")
                for (i in 0 until entities.length()) {
                    val entity = entities.getJSONObject(i); val table = entity.getString("tableName")
                    old.execSQL(entity.getString("createSql").replace("\${TABLE_NAME}", table))
                    val indices = entity.optJSONArray("indices") ?: JSONArray()
                    for (j in 0 until indices.length()) old.execSQL(indices.getJSONObject(j).getString("createSql").replace("\${TABLE_NAME}", table))
                }
                val setup = schema.getJSONArray("setupQueries"); for (i in 0 until setup.length()) old.execSQL(setup.getString(i))
                old.execSQL("INSERT INTO works VALUES('work',NULL,'','Old',NULL,NULL,1,'SAFE')")
                old.execSQL("INSERT INTO tracks VALUES('track','work','Old track','WAV')")
                old.execSQL("INSERT INTO track_versions VALUES('version','track','wav')")
                old.execSQL("INSERT INTO playback_positions VALUES('version',123,1000,0,100)")
                old.execSQL("INSERT INTO playback_session VALUES(1,'[\"version\"]',0,123,NULL,NULL,NULL,NULL,0)")
                old.version = 3
            }
            val migrated = Room.databaseBuilder(context, LibraryDatabase::class.java, name).addMigrations(MIGRATION_3_4, MIGRATION_4_5, com.asmrplayer.android.sync.MIGRATION_5_6, MIGRATION_6_7, MIGRATION_7_8).build()
            try {
                assertTrue(migrated.library().work("work")!!.favorite)
                assertTrue(migrated.library().work("work")!!.inLibrary)
                assertEquals(123L, migrated.library().session()!!.positionMs)
                assertEquals(123L, migrated.collections().observeHistory().first().single().positionMs)
                migrated.collections().putPreference(VersionPreference("version", true))
                assertEquals("version", migrated.collections().observeFavorites().first().single().versionId)
            } finally { migrated.close() }
        } finally { context.deleteDatabase(name) }
    }
}
