package com.asmrplayer.android.sync

import android.content.Context
import com.asmrplayer.android.security.TokenCipher
import java.io.ByteArrayOutputStream
import java.net.URL
import javax.net.ssl.HttpsURLConnection
import net.openid.appauth.AuthorizationRequest
import net.openid.appauth.AuthorizationResponse
import org.json.JSONArray
import org.json.JSONObject

enum class Storage(val title: String) {
    GITHUB("GitHub Gist"),
    ONEDRIVE("OneDrive"),
    WEBDAV("WebDAV"),
    SUPABASE("Supabase"),
}

internal data class BindingStatus(val configured: Boolean, val connected: Boolean)

internal sealed interface LoginInput {
    data class Email(val email:String,val password:String,val signup:Boolean=false):LoginInput
    data class WebDav(val url:String,val username:String,val password:String):LoginInput
    data class Browser(val response: AuthorizationResponse) : LoginInput

    data class GitHubGrant(val encoded:String):LoginInput
    data class Token(val token: String) : LoginInput
}

internal interface SyncProvider {
    fun login(input: LoginInput): String

    fun logout()

    fun getStatus(): BindingStatus

    fun accessToken(): String

    fun exchangeSyncData(token:String, device:String, merged:JSONObject) {
        val own=downloadSyncData(token,device,merged)
        uploadSyncData(token,device,merged,own)
    }
    fun authorizationRequest(): AuthorizationRequest? = null

    fun downloadSyncData(token: String, device: String, merged: JSONObject): String?

    fun uploadSyncData(token: String, device: String, merged: JSONObject, own: String?)
}

internal abstract class OAuthSyncProvider(context: Context, private val profile: OAuthProfile) :
    SyncProvider {
    private val auth = StorageOAuth(context, profile = profile)

    override fun getStatus() = BindingStatus(profile.client.isNotEmpty(), auth.connected())

    override fun authorizationRequest() = auth.request()

    override fun login(input: LoginInput) = auth.exchange((input as LoginInput.Browser).response)

    override fun logout() = auth.clear()

    override fun accessToken() = auth.refresh()
}

private const val GISTS = "https://api.github.com/gists"
private const val GRAPH = "https://graph.microsoft.com/v1.0/me/drive/special/approot"
private const val DESCRIPTION = "ASMR Player sync v1"

private fun filename(device: String) = "asmr-sync-v1-$device.json"

private val snapshot =
    Regex(
        "asmr-sync-v1-[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\\.json"
    )

internal interface StorageTransport {
    fun request(
        url: String,
        token: String? = null,
        method: String = "GET",
        body: String? = null,
        limit: Int = SyncProtocol.LIMIT * 2,
    ): String
}

internal object StorageHttp : StorageTransport {
    override fun request(
        url: String,
        token: String?,
        method: String,
        body: String?,
        limit: Int,
    ): String {
        val address = URL(url)
        require(address.protocol == "https" && address.userInfo == null)
        val conn = address.openConnection() as HttpsURLConnection
        try {
            conn.instanceFollowRedirects = false
            conn.connectTimeout = 15_000
            conn.readTimeout = 45_000
            conn.requestMethod = method
            conn.setRequestProperty("User-Agent", "ASMR-Player-Sync")
            if (token != null) conn.setRequestProperty("Authorization", "Bearer $token")
            if (body != null) {
                val bytes = body.toByteArray()
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.setFixedLengthStreamingMode(bytes.size)
                conn.outputStream.use { it.write(bytes) }
            }
            val status = conn.responseCode
            check(status in 200..299) { "同步存储返回 HTTP $status" }
            val output = ByteArrayOutputStream()
            conn.inputStream.use { input ->
                val buf = ByteArray(8192)
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    require(output.size() + n <= limit) { "同步响应超过大小上限" }
                    output.write(buf, 0, n)
                }
            }
            return output.toString("UTF-8")
        } finally {
            conn.disconnect()
        }
    }
}

private fun payload(doc: JSONObject): String =
    SyncProtocol.wire(doc).toString().also { require(it.toByteArray().size <= SyncProtocol.LIMIT) { "同步资料超过大小上限" } }

internal class GitHubGistProvider(
    context: Context,
    private val http: StorageTransport = StorageHttp,
) : SyncProvider {
    private val prefs = context.getSharedPreferences("github-gist-account", Context.MODE_PRIVATE)
    private val cipher = TokenCipher("asmr-github-gist-token-v1")

    override fun getStatus() = BindingStatus(true, prefs.contains("token"))

    override fun login(input: LoginInput): String {
        if(input is LoginInput.GitHubGrant){
            save(input.encoded)
            return GitHubAuthorization.access(input.encoded,::save)
        }
        val t = (input as LoginInput.Token).token.trim()
        require(t.isNotEmpty()) { "请输入具有 Gist 读写权限的 GitHub 令牌" }
        http.request("https://api.github.com/user", t)
        save(t)
        return t
    }
    private fun save(t:String){
        check(prefs.edit().putString("token", cipher.encrypt(t, GISTS)).commit()) {
            "无法保存 GitHub 凭据"
        }
    }

    override fun logout() {
        check(prefs.edit().clear().commit())
    }

    override fun accessToken() =
        GitHubAuthorization.access(cipher.decrypt(requireNotNull(prefs.getString("token", null)) { "请先绑定 GitHub" }, GISTS), ::save)

    override fun downloadSyncData(token: String, device: String, merged: JSONObject): String? {
        var own: String? = null
        var page = 1
        do {
            val items = JSONArray(http.request("$GISTS?per_page=100&page=$page", token))
            for (i in 0 until items.length()) {
                val item = items.getJSONObject(i)
                if (item.getBoolean("public") || item.optString("description") != DESCRIPTION)
                    continue
                val id = item.getString("id")
                require(id.matches(Regex("[0-9a-fA-F]+")))
                val detail = JSONObject(http.request("$GISTS/$id", token))
                if (detail.getBoolean("public") || detail.optString("description") != DESCRIPTION)
                    continue
                val files = detail.getJSONObject("files")
                for (name in files.keys()) {
                    if (!snapshot.matches(name)) continue
                    val file = files.getJSONObject(name)
                    val content =
                        if (file.optBoolean("truncated")) {
                            val raw = URL(file.getString("raw_url"))
                            require(raw.host == "gist.githubusercontent.com" && raw.port == -1)
                            http.request(raw.toString(), limit = SyncProtocol.LIMIT)
                        } else file.getString("content")
                    require(content.toByteArray().size <= SyncProtocol.LIMIT)
                    SyncProtocol.merge(merged, JSONObject(content))
                    if (name == filename(device)) own = id
                }
            }
            page++
        } while (items.length() == 100)
        return own
    }

    override fun uploadSyncData(token: String, device: String, merged: JSONObject, own: String?) {
        val content = payload(merged)
        require(content.toByteArray().size <= 10_000_000) { "快照超过 Gist API 可下载的 10 MB，请改用其他存储" }
        val data =
            JSONObject()
                .put(
                    "files",
                    JSONObject().put(filename(device), JSONObject().put("content", content)),
                )
        if (own == null)
            http.request(
                GISTS,
                token,
                "POST",
                data.put("public", false).put("description", DESCRIPTION).toString(),
            )
        else http.request("$GISTS/$own", token, "PATCH", data.toString())
    }
}

internal class OneDriveProvider(
    context: Context,
    private val http: StorageTransport = StorageHttp,
) : OAuthSyncProvider(context, StorageOAuth.oneDrive) {
    override fun downloadSyncData(token: String, device: String, merged: JSONObject): String? {
        http.request(GRAPH, token)
        var page: String? = "$GRAPH/children"
        while (page != null) {
            val url = URL(page)
            require(url.host == "graph.microsoft.com" && url.port == -1)
            val data = JSONObject(http.request(page, token))
            val files = data.getJSONArray("value")
            for (i in 0 until files.length()) {
                val file = files.getJSONObject(i)
                if (!snapshot.matches(file.getString("name")) || !file.has("file")) continue
                val raw =
                    http.request(
                        file.getString("@microsoft.graph.downloadUrl"),
                        limit = SyncProtocol.LIMIT,
                    )
                SyncProtocol.merge(merged, JSONObject(raw))
            }
            page = data.optString("@odata.nextLink").takeIf { it.isNotEmpty() }
        }
        return null
    }

    override fun uploadSyncData(token: String, device: String, merged: JSONObject, own: String?) {
        http.request("$GRAPH:/${filename(device)}:/content", token, "PUT", payload(merged))
    }
}
