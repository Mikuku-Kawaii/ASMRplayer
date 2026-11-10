package com.asmrplayer.android.update

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class UpdateManifestTest {
    private fun manifest(): JSONObject = JSONObject().put("version", "1.4.0").put("platforms", JSONObject()
        .put("windows-x86_64", JSONObject().put("url", "existing.exe"))
        .put("android-universal", JSONObject().put("url", "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v1.4.0/release.apk")
            .put("versionCode", 14).put("minSdk", 23).put("size", 1024).put("sha256", "a".repeat(64)).put("signature", "fixture")))
    @Test fun sharedPcAndAndroidManifestAndEligibility() {
        val release = UpdateManifest.parse(manifest().toString())!!
        assertTrue(UpdateManifest.eligible(release, "1.3.0", 13, 31))
        assertFalse(UpdateManifest.eligible(release, "1.4.0", 13, 31))
        assertFalse(UpdateManifest.eligible(release, "1.3.0", 14, 31))
        assertFalse(UpdateManifest.eligible(release, "1.3.0", 13, 22))
        val pcOnly = manifest().apply { getJSONObject("platforms").remove("android-universal") }
        assertNull(UpdateManifest.parse(pcOnly.toString()))
    }
    @Test fun rejectsUntrustedOriginsAndUnstableReleases() {
        for (url in listOf("http://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v1.4.0/a.apk",
            "https://github.com/Other/repo/releases/download/v1.4.0/a.apk",
            "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v1.3.0/a.apk")) {
            val json = manifest(); json.getJSONObject("platforms").getJSONObject("android-universal").put("url", url)
            assertThrows(IllegalArgumentException::class.java) { UpdateManifest.parse(json.toString()) }
        }
        assertThrows(IllegalArgumentException::class.java) { UpdateManifest.parse(manifest().put("draft", true).toString()) }
        assertThrows(IllegalArgumentException::class.java) { UpdateManifest.parse(manifest().put("prerelease", true).toString()) }
    }
}
