package com.asmrplayer.android.online

import android.net.Uri
import androidx.room.Room
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.subtitles.SubtitleRepository
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class SubtitlePartialReadTest {
    @Test fun retryTranslationDoesNotReopenTheHealthyOriginal() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val db = Room.inMemoryDatabaseBuilder(context, LibraryDatabase::class.java).build()
        val original = File.createTempFile("original", ".srt", context.cacheDir)
        val translation = File.createTempFile("translation", ".srt", context.cacheDir)
        val sample = "1\n00:00:00,000 --> 00:00:01,000\nSynthetic cue\n"
        try {
            val dao = db.library()
            dao.putWork(LibraryWork("work", null, "", "Synthetic work"))
            dao.putTrack(LibraryTrack("track", "work", "Track", ""))
            dao.putVersion(TrackVersion("version", "track", "wav"))
            for ((role, file) in listOf("original" to original, "translation" to translation)) {
                dao.putSubtitle(SubtitleSource(role, "version", null, file.name, Uri.fromFile(file).toString(), role, "srt", manual = true))
            }
            original.writeText(sample); translation.writeText("invalid SRT")
            val repo = SubtitleRepository(context, db)
            repo.select(SubtitlePreference("version", "original", "translation", translationOffsetMs = 250))
            val partial = repo.load("version", allowPartial = true)
            assertNotNull(partial.original); assertNull(partial.originalError)
            assertNull(partial.translation); assertNotNull(partial.translationError)
            original.delete(); translation.writeText(sample)
            val repaired = repo.retryRole(partial, translation = true)
            assertSame(partial.original, repaired.original)
            assertEquals("Synthetic cue", repaired.translation!!.cues.single().text)
            assertNull(repaired.translationError); assertEquals(250L, repaired.preference.translationOffsetMs)
        } finally { db.close(); original.delete(); translation.delete() }
    }
}
