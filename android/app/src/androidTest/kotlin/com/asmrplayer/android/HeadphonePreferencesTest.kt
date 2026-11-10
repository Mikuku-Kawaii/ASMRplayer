package com.asmrplayer.android

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.playback.*
import com.asmrplayer.core.DspConfig
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class HeadphonePreferencesTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val common = DspConfig()
    private val sound = DspConfig(highPass = false, deEsser = true, balanceDb = -2.0, preampDb = -4.0)
    private val speaker = OutputRoute(1, "Speaker", "port:2", true)
    private val phones = OutputRoute(2, "Headphones", "synthetic-address-hash")
    private fun fixture(test: (android.content.SharedPreferences) -> Unit) {
        val prefs = context.getSharedPreferences("test-headphones-${System.nanoTime()}", Context.MODE_PRIVATE)
        try { test(prefs) } finally { prefs.edit().clear().commit() }
    }
    @Test fun automaticRoutesRestoreCommonAndKeepManualChangesUntilRouteChanges() = fixture { prefs ->
        var actual = common
        val store = HeadphonePreferences(prefs, actual) { actual = it }
        store.edited(sound); store.save("My headphones")
        val id = store.state.activeId!!
        store.routeChanged(phones); store.bind(id, phones.key!!); store.automatic(true)
        assertEquals(sound, actual)
        val edited = sound.copy(air = true)
        store.edited(edited); actual = edited
        store.routeChanged(phones)
        assertEquals(edited, actual)
        assertTrue(store.state.modified)
        store.routeChanged(speaker)
        assertEquals(common, actual)
        store.routeChanged(phones)
        assertEquals(sound, actual)
        store.routeChanged(null)
        assertEquals(common, actual)
        store.automatic(false); store.select(id); store.routeChanged(speaker)
        assertEquals(sound, actual)
    }
    @Test fun persistenceNeverRestoresHeadphoneSoundBeforeOutputIsKnown() = fixture { prefs ->
        var actual = common
        val first = HeadphonePreferences(prefs, actual) { actual = it }
        first.edited(sound); first.save("Saved")
        val id = first.state.activeId!!
        first.routeChanged(phones); first.bind(id, phones.key!!); first.automatic(true)
        assertEquals(sound, actual)
        val restored = HeadphonePreferences(prefs, actual) { actual = it }
        assertEquals(common, actual)
        assertNull(restored.state.activeId)
        assertEquals(sound, restored.state.profiles.single().config)
        restored.routeChanged(phones)
        assertEquals(sound, actual)
        restored.remove(id)
        assertEquals(common, actual)
        assertTrue(restored.state.bindings.isEmpty())
        assertTrue(restored.state.profiles.isEmpty())
    }
    @Test fun unknownOutputAndStaleBindingAreRejectedAndStateRoundTrips() = fixture { prefs ->
        val store = HeadphonePreferences(prefs, common) {}
        store.edited(sound); store.save("Long headphone name 蓝牙")
        val id = store.state.activeId!!
        store.routeChanged(phones)
        assertThrows(IllegalArgumentException::class.java) { store.bind(id, "old-route") }
        store.routeChanged(phones.copy(key = null))
        assertThrows(IllegalArgumentException::class.java) { store.bind(id, phones.key!!) }
        store.routeChanged(phones); store.bind(id, phones.key!!)
        assertEquals(store.state, HeadphoneCommands.decode(HeadphoneCommands.encode(store.state)))
        store.saveCommon()
        assertNull(store.state.activeId)
        assertFalse(store.state.modified)
        store.edited(common)
        assertTrue(store.state.modified)
    }
}
