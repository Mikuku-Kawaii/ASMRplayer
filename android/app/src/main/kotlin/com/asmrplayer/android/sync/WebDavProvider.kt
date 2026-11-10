package com.asmrplayer.android.sync

import android.content.Context
import android.util.Base64
import com.asmrplayer.android.security.TokenCipher
import java.io.ByteArrayOutputStream
import java.net.URI
import java.net.URL
import javax.net.ssl.HttpsURLConnection
import org.json.JSONObject

internal class WebDavProvider(
    context: Context,
    private val open: (URL) -> HttpsURLConnection = { it.openConnection() as HttpsURLConnection },
) : SyncProvider {
    private val prefs = context.getSharedPreferences("webdav-sync-account", Context.MODE_PRIVATE)
    private val cipher = TokenCipher("asmr-webdav-sync-v1")

    override fun getStatus() = BindingStatus(true, prefs.contains("account"))

    override fun login(input: LoginInput): String {
        val a = input as LoginInput.WebDav
        val base = URI(a.url.trim())
        require(
            base.scheme.equals("https", true) &&
                base.host != null &&
                base.userInfo == null &&
                base.query == null &&
                base.fragment == null
        ) {
            "请输入 HTTPS 文件夹地址，账号请单独填写"
        }
        require(a.username.isNotBlank() && ':' !in a.username && a.password.isNotEmpty()) {
            "请填写 WebDAV 用户名和密码"
        }
        val text =
            JSONObject()
                .put("file", base.toASCIIString().trimEnd('/') + "/asmr-player-sync-v1.json")
                .put("username", a.username)
                .put("password", a.password)
                .toString()
        downloadSyncData(text, "", SyncProtocol.empty())
        check(prefs.edit().putString("account", cipher.encrypt(text, "webdav")).commit()) {
            "无法保存 WebDAV 配置"
        }
        return text
    }

    override fun logout() {
        check(prefs.edit().clear().commit())
    }

    override fun accessToken() =
        cipher.decrypt(requireNotNull(prefs.getString("account", null)) { "请先绑定 WebDAV" }, "webdav")

    private fun connection(text: String, method: String): HttpsURLConnection {
        val a = JSONObject(text)
        return open(URL(a.getString("file"))).apply {
            instanceFollowRedirects = false
            connectTimeout = 15000
            readTimeout = 45000
            requestMethod = method
            val credentials = a.getString("username") + ":" + a.getString("password")
            setRequestProperty(
                "Authorization",
                "Basic " + Base64.encodeToString(credentials.toByteArray(), Base64.NO_WRAP),
            )
            setRequestProperty("Cache-Control", "no-cache")
        }
    }

    override fun downloadSyncData(token: String, device: String, merged: JSONObject): String? {
        val conn = connection(token, "GET")
        try {
            val code = conn.responseCode
            if (code == 404) return null
            check(code == 200) { "WebDAV 读取失败（HTTP $code）" }
            val tag = conn.getHeaderField("ETag")
            check(tag != null && tag.length >= 2 && tag.startsWith('"') && tag.endsWith('"')) {
                "WebDAV 服务需要为同步文件返回强 ETag"
            }
            val output = ByteArrayOutputStream()
            conn.inputStream.use { stream ->
                val buf = ByteArray(8192)
                while (true) {
                    val count = stream.read(buf)
                    if (count < 0) break
                    require(output.size() + count <= SyncProtocol.LIMIT) { "同步文件过大" }
                    output.write(buf, 0, count)
                }
            }
            SyncProtocol.merge(merged, JSONObject(output.toString("UTF-8")))
            return tag
        } finally {
            conn.disconnect()
        }
    }

    private fun put(token: String, merged: JSONObject, etag: String?): Boolean {
        val data = SyncProtocol.wire(merged).toString().toByteArray()
        require(data.size <= SyncProtocol.LIMIT) { "同步文件过大" }
        val conn = connection(token, "PUT")
        try {
            if (etag == null) conn.setRequestProperty("If-None-Match", "*")
            else conn.setRequestProperty("If-Match", etag)
            conn.setRequestProperty("Content-Type", "application/json")
            conn.doOutput = true
            conn.setFixedLengthStreamingMode(data.size)
            conn.outputStream.use { it.write(data) }
            val code = conn.responseCode
            if (code == 412) return false
            check(code in 200..299) { "WebDAV 写入失败（HTTP $code），请确认文件夹存在且可写" }
            return true
        } finally {
            conn.disconnect()
        }
    }

    override fun uploadSyncData(token: String, device: String, merged: JSONObject, own: String?) {
        check(put(token, merged, own)) { "远端资料已更新，请重试" }
    }

    override fun exchangeSyncData(token: String, device: String, merged: JSONObject) {
        repeat(3) {
            val rev = downloadSyncData(token, device, merged)
            if (put(token, merged, rev)) return
        }
        error("其他设备正在更新 WebDAV 文件，请稍后同步")
    }
}
