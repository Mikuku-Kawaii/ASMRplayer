package com.asmrplayer.android.sync

import android.os.SystemClock
import com.asmrplayer.android.BuildConfig
import java.net.URL
import java.net.URLEncoder
import javax.net.ssl.HttpsURLConnection
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONObject

internal object GitHubAuthorization {
    private const val tokenUrl = "https://github.com/login/oauth/access_token"
    const val browserUrl = "https://github.com/login/device"

    class Challenge(
        val userCode: String,
        val deviceCode: String,
        val expires: Long,
        val interval: Long,
    )

    private fun post(url: String, fields: Map<String, String>): JSONObject {
        val conn = URL(url).openConnection() as HttpsURLConnection
        try {
            conn.instanceFollowRedirects = false
            conn.connectTimeout = 15000
            conn.readTimeout = 30000
            conn.requestMethod = "POST"
            conn.doOutput = true
            conn.setRequestProperty("Accept", "application/json")
            conn.setRequestProperty("Content-Type", "application/x-www-form-urlencoded")
            val params = fields + ("client_id" to BuildConfig.GITHUB_CLIENT_ID)
            val body =
                params.entries
                    .joinToString("&") { (k, v) -> k + "=" + URLEncoder.encode(v, "UTF-8") }
                    .toByteArray()
            conn.setFixedLengthStreamingMode(body.size)
            conn.outputStream.use { it.write(body) }
            check(conn.responseCode in 200..299) { "GitHub 授权服务返回 HTTP ${conn.responseCode}" }
            return JSONObject(conn.inputStream.bufferedReader().use { it.readText() })
        } finally {
            conn.disconnect()
        }
    }

    fun begin(): Challenge {
        val v = post("https://github.com/login/device/code", mapOf("scope" to "gist"))
        return Challenge(
            v.getString("user_code"),
            v.getString("device_code"),
            SystemClock.elapsedRealtime() + 1000 * v.optLong("expires_in", 900).coerceIn(1, 1800),
            v.optLong("interval", 5).coerceIn(1, 60),
        )
    }

    private fun grant(v: JSONObject, old: String?): String {
        check(!v.has("scope") || v.getString("scope") == "gist") { "GitHub 返回的权限与 gist 不一致" }
        val access = v.optString("access_token")
        check(access.isNotEmpty()) { "GitHub 未返回访问令牌，请重新登录" }
        return JSONObject()
            .put("access", access)
            .put("refresh", v.optString("refresh_token").ifEmpty { old })
            .put(
                "expires",
                if (v.has("expires_in")) System.currentTimeMillis() / 1000 + v.getLong("expires_in")
                else Long.MAX_VALUE,
            )
            .toString()
    }

    suspend fun awaitGrant(challenge: Challenge): String {
        var interval = challenge.interval
        while (SystemClock.elapsedRealtime() < challenge.expires) {
            delay(interval * 1000)
            val v =
                withContext(Dispatchers.IO) {
                    post(
                        tokenUrl,
                        mapOf(
                            "device_code" to challenge.deviceCode,
                            "grant_type" to "urn:ietf:params:oauth:grant-type:device_code",
                        ),
                    )
                }
            when (v.optString("error")) {
                "" -> return grant(v, null)
                "authorization_pending" -> Unit
                "slow_down" -> interval += 5
                "access_denied" -> error("GitHub 授权已取消")
                else -> error("GitHub 授权未完成，请重新登录")
            }
        }
        error("GitHub 授权码已过期，请重新登录")
    }

    fun access(stored: String, save: (String) -> Unit): String {
        if (!stored.startsWith("{")) return stored
        val old = JSONObject(stored)
        if (old.getLong("expires") > System.currentTimeMillis() / 1000 + 60)
            return old.getString("access")
        val refresh = old.optString("refresh")
        check(refresh.isNotEmpty()) { "GitHub 登录已过期，请重新登录" }
        val next =
            grant(
                post(tokenUrl, mapOf("grant_type" to "refresh_token", "refresh_token" to refresh)),
                refresh,
            )
        save(next)
        return JSONObject(next).getString("access")
    }
}
