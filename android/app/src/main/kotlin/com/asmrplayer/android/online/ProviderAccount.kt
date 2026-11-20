package com.asmrplayer.android.online

import android.content.Context
import com.asmrplayer.android.security.TokenCipher
import com.asmrplayer.android.BuildConfig
import kotlinx.coroutines.CancellationException
import org.json.JSONObject
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

data class ProviderAccountState(val apiBase: String, val hasToken: Boolean, val cacheScope: String = "", val accountName: String? = null)

/** Only encrypted token blobs are stored. Plaintext is never returned in the observable state. */
class ProviderAccount(context: Context, private val transport: HttpTransport) {
    private val cipher = TokenCipher(KEY_ALIAS)
    private val prefs = context.getSharedPreferences("online-account", Context.MODE_PRIVATE)
    private val mutable = MutableStateFlow(ProviderAccountState(prefs.getString("base", DEFAULT_BASE)!!, false))
    val state = mutable.asStateFlow()
    init { mutable.value = accountState(mutable.value.apiBase) }
    suspend fun setApiBase(value: String) = withContext(Dispatchers.IO) {
        val uri = transport.validate(value.trim().trimEnd('/'))
        require(uri.path.isNullOrEmpty() && uri.query == null)
        val base = uri.toString()
        synchronized(prefs) {
            check(prefs.edit().putString("base", base).commit())
            mutable.value = accountState(base)
        }
    }
    suspend fun setToken(value: String?) = saveCredential(state.value, value, null)

    suspend fun login(base: String, name: String, password: String) {
        val expected = state.value
        if (expected.apiBase != base) throw OnlineException(OnlineError.SOURCE_CHANGED, "站点已切换，请重新登录。")
        if (!allowsLogin(base)) throw OnlineException(OnlineError.AUTH_REQUIRED, "请选择 asmr.one 官方镜像后登录。")
        require(name.isNotBlank() && password.isNotEmpty())
        val raw = try {
            transport.text("$base/api/auth/me", mapOf("Accept" to "application/json, text/plain, */*",
                "Origin" to "https://asmr.one", "Referer" to "https://asmr.one/"),
                JSONObject().put("name", name.trim()).put("password", password).toString(), followRedirects = false)
        } catch (e: CancellationException) { throw e }
        catch (e: HttpStatusException) {
            throw when (e.status) {
                400, 401, 403 -> OnlineException(OnlineError.AUTH_REQUIRED, "账号名或密码未通过验证。")
                429 -> OnlineException(OnlineError.RATE_LIMITED, "登录尝试过于频繁，请稍后重试。")
                else -> OnlineException(OnlineError.NETWORK, "站点暂时无法完成登录，请稍后重试。")
            }
        } catch (_: java.io.IOException) { throw OnlineException(OnlineError.NETWORK, "暂时无法连接 asmr.one，请稍后重试。") }
        val result = try { JSONObject(raw) } catch (_: org.json.JSONException) { null }
        val user = result?.optJSONObject("user")
        val token = result?.opt("token") as? String
        if (user == null || token.isNullOrBlank()) throw OnlineException(OnlineError.SCHEMA_CHANGED, "无法读取站点登录结果。")
        if (!user.optBoolean("loggedIn") || user.optString("name") == "guest") throw OnlineException(OnlineError.AUTH_REQUIRED, "账号名或密码未通过验证。")
        saveCredential(expected, token, (user.opt("name") as? String)?.takeIf { it.isNotBlank() } ?: name.trim())
    }

    private suspend fun saveCredential(expected: ProviderAccountState, value: String?, name: String?) = withContext(Dispatchers.IO) {
        val base = expected.apiBase
        val token = value?.trim()?.takeIf { it.isNotEmpty() }
        require(token == null || (token.length <= 16_384 && token.none { it == '\r' || it == '\n' }))
        synchronized(prefs) {
            if (mutable.value != expected) throw OnlineException(OnlineError.SOURCE_CHANGED, "站点或账号已切换，请重试。")
            val key = tokenKey(base)
            val editor = prefs.edit().putString("name:$key", name)
            if (token == null) editor.remove(key) else editor.putString(key, cipher.encrypt(token, base))
            check(editor.putString("scope:$key", java.util.UUID.randomUUID().toString()).commit())
            mutable.value = accountState(base)
        }
    }
    internal fun cacheScope(base: String): String = prefs.getString("scope:" + tokenKey(base), "").orEmpty()
    internal suspend fun guestRecommenderUuid(base: String): String = withContext(Dispatchers.IO) {
        val key = "guest-recommender:$base"
        synchronized(prefs) {
            prefs.getString(key, null) ?: java.util.UUID.randomUUID().toString().also { id ->
                check(prefs.edit().putString(key, id).commit())
            }
        }
    }

    private fun accountState(base: String) = ProviderAccountState(base, prefs.contains(tokenKey(base)), cacheScope(base), prefs.getString("name:" + tokenKey(base), null))

    internal fun headers(base: String): Map<String, String> {
        val text = prefs.getString(tokenKey(base), null) ?: return emptyMap()
        try {
            return mapOf("Authorization" to "Bearer ${cipher.decrypt(text, base)}")
        } catch (_: Exception) { throw OnlineException(OnlineError.AUTH_REQUIRED, "凭据无法读取，请重新设置 Token") }
    }
    private fun tokenKey(base: String) = "token:" + com.asmrplayer.android.library.LibraryIndex.id(base)
    companion object {
        const val DEFAULT_BASE = "https://api.asmr-200.com"
        val OFFICIAL_BASES=listOf(DEFAULT_BASE,"https://api.asmr.one","https://api.asmr-300.com","https://api.asmr-100.com")
        fun allowsLogin(base: String) = base in OFFICIAL_BASES || (BuildConfig.DEBUG && BuildConfig.APPLICATION_ID.endsWith(".uitest") &&
            base.matches(Regex("http://127\\.0\\.0\\.1:[0-9]+")))
        private const val KEY_ALIAS = "asmr-online-token-v1"
    }
}
