package com.asmrplayer.android.sync

import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test

class MultiStorageTest {
    private val context
        get() = InstrumentationRegistry.getInstrumentation().targetContext

    private val device = "783f3c16-62be-420d-bcc4-41297873fb20"
    private val name = "asmr-sync-v1-$device.json"
    private val key = "work|" + "a".repeat(64) + "|"

    private fun remote(): JSONObject {
        val journal = JSONObject().put("doc", SyncProtocol.empty()).put("device", "pc")
        SyncProtocol.edit(journal, key, true)
        return journal.getJSONObject("doc")
    }

    @Test fun webDavMergesAgainAfterAConcurrentCreate() {
        assumeTrue(context.packageName.endsWith(".drivetest"))
        val replies = ArrayDeque(listOf(404, 404, 412, 200, 204))
        val writes = mutableListOf<String>()
        val dav = WebDavProvider(context) { url ->
            assertEquals("https://example.invalid/dav/asmr-player-sync-v1.json", url.toString())
            val result = replies.removeFirst()
            val output = java.io.ByteArrayOutputStream()
            object : javax.net.ssl.HttpsURLConnection(url) {
                override fun usingProxy() = false
                override fun connect() = Unit
                override fun disconnect() = Unit
                override fun getCipherSuite() = "fixture"
                override fun getLocalCertificates(): Array<java.security.cert.Certificate>? = null
                override fun getServerCertificates() = emptyArray<java.security.cert.Certificate>()
                override fun getInputStream() = remote().toString().byteInputStream()
                override fun getOutputStream() = output
                override fun getHeaderField(name:String) = if (name=="ETag") "\"changed\"" else null
                override fun getResponseCode():Int {
                    assertFalse(instanceFollowRedirects)
                    assertTrue(getRequestProperty("Authorization").startsWith("Basic "))
                    if(requestMethod=="PUT"){
                        writes.add(getRequestProperty("If-None-Match") ?: getRequestProperty("If-Match"))
                        if(result==204) assertTrue(SyncProtocol.selected(JSONObject(output.toString("UTF-8"))).getBoolean(key))
                    }
                    return result
                }
            }
        }
        try {
            dav.login(LoginInput.WebDav("https://example.invalid/dav","user","fixture-password"))
            val merged=SyncProtocol.empty()
            dav.exchangeSyncData(dav.accessToken(),device,merged)
            assertTrue(SyncProtocol.selected(merged).getBoolean(key))
            assertEquals(listOf("*","\"changed\""),writes)
            assertTrue(replies.isEmpty())
        } finally { dav.logout() }
    }

    @Test
    fun gist_binding_is_independent_and_only_the_secret_gist_is_updated() {
        assumeTrue(context.packageName.endsWith(".drivetest"))
        val written = mutableListOf<String>()
        val mock =
            object : StorageTransport {
                override fun request(
                    url: String,
                    token: String?,
                    method: String,
                    body: String?,
                    limit: Int,
                ): String {
                    assertEquals("fixture-token", token)
                    val description = "ASMR Player sync v1"
                    return when {
                        url.endsWith("/user") -> "{}"
                        method == "POST" -> {
                            val created = JSONObject(body!!)
                            assertFalse(created.getBoolean("public"))
                            assertEquals(description, created.getString("description"))
                            written.add(method)
                            "{}"
                        }
                        url.contains("/gists?") ->
                            JSONArray()
                                .put(
                                    JSONObject()
                                        .put("description", description)
                                        .put("public", true)
                                        .put("id", "bb")
                                )
                                .put(
                                    JSONObject()
                                        .put("description", description)
                                        .put("public", false)
                                        .put("id", "aa")
                                )
                                .toString()
                        url.endsWith("/gists/aa") && method == "GET" ->
                            JSONObject()
                                .put("public", false)
                                .put("description", description)
                                .put(
                                    "files",
                                    JSONObject()
                                        .put(name, JSONObject().put("content", remote().toString())),
                                )
                                .toString()
                        url.endsWith("/gists/aa") && method == "PATCH" -> {
                            val stored =
                                JSONObject(
                                    JSONObject(body!!)
                                        .getJSONObject("files")
                                        .getJSONObject(name)
                                        .getString("content")
                                )
                            assertTrue(SyncProtocol.selected(stored).getBoolean(key))
                            written.add(method)
                            "{}"
                        }
                        else -> error("Unexpected Gist operation")
                    }
                }
            }
        val gist = GitHubGistProvider(context, mock)
        try {
            gist.login(LoginInput.Token("fixture-token"))
            val restored = GitHubGistProvider(context, mock)
            val token = restored.accessToken()
            assertEquals("fixture-token", token)
            assertFalse(OneDriveProvider(context).getStatus().connected)
            val merged = SyncProtocol.empty()
            restored.uploadSyncData(token, device, merged, null)
            val own = restored.downloadSyncData(token, device, merged)
            assertEquals("aa", own)
            restored.uploadSyncData(token, device, merged, own)
            assertEquals(listOf("POST", "PATCH"), written)
        } finally {
            gist.logout()
        }
        assertFalse(gist.getStatus().connected)
    }

    @Test
    fun onedrive_download_does_not_forward_bearer_and_put_targets_the_app_folder() {
        val root = "https://graph.microsoft.com/v1.0/me/drive/special/approot"
        var uploaded = false
        val mock =
            object : StorageTransport {
                override fun request(
                    url: String,
                    token: String?,
                    method: String,
                    body: String?,
                    limit: Int,
                ): String {
                    if (url == "https://download.example/signed") {
                        assertNull(token)
                        return remote().toString()
                    }
                    assertEquals("fixture-access", token)
                    return when (url) {
                        root -> "{}"
                        "$root/children" ->
                            JSONObject()
                                .put("value", JSONArray())
                                .put("@odata.nextLink", "$root/children?page=2")
                                .toString()
                        "$root/children?page=2" ->
                            JSONObject()
                                .put(
                                    "value",
                                    JSONArray()
                                        .put(
                                            JSONObject()
                                                .put("name", name)
                                                .put("file", JSONObject())
                                                .put(
                                                    "@microsoft.graph.downloadUrl",
                                                    "https://download.example/signed",
                                                )
                                        ),
                                )
                                .toString()
                        "$root:/$name:/content" -> {
                            assertEquals("PUT", method)
                            assertTrue(SyncProtocol.selected(JSONObject(body!!)).getBoolean(key))
                            uploaded = true
                            "{}"
                        }
                        else -> error("Unexpected Graph operation")
                    }
                }
            }
        val provider = OneDriveProvider(context, mock)
        val merged = SyncProtocol.empty()
        provider.downloadSyncData("fixture-access", device, merged)
        provider.uploadSyncData("fixture-access", device, merged, null)
        assertTrue(uploaded)
        assertTrue(StorageOAuth.oneDrive.acceptsScope("Files.ReadWrite.AppFolder"))
        assertFalse(StorageOAuth.oneDrive.acceptsScope("Files.ReadWrite.AppFolder User.Read"))
    }
}
