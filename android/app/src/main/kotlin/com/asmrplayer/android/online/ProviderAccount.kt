package com.asmrplayer.android.online

import android.content.Context
import com.asmrplayer.android.security.TokenCipher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

data class ProviderAccountState(val apiBase: String, val hasToken: Boolean, val cacheScope: String = "")

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
        check(prefs.edit().putString("base", base).commit())
        mutable.value = accountState(base)
    }
    suspend fun setToken(value: String?) = withContext(Dispatchers.IO) {
        val base = mutable.value.apiBase
        val token = value?.trim()?.takeIf { it.isNotEmpty() }
        require(token == null || (token.length <= 16_384 && token.none { it == '\r' || it == '\n' }))
        val editor = prefs.edit()
        if (token == null) editor.remove(tokenKey(base)) else {
            editor.putString(tokenKey(base), cipher.encrypt(token, base))
        }
        check(editor.putString("scope:" + tokenKey(base), java.util.UUID.randomUUID().toString()).commit())
        if (mutable.value.apiBase == base) mutable.value = accountState(base)
    }
    internal fun cacheScope(base: String): String = prefs.getString("scope:" + tokenKey(base), "").orEmpty()
    private fun accountState(base: String) = ProviderAccountState(base, prefs.contains(tokenKey(base)), cacheScope(base))

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
        private const val KEY_ALIAS = "asmr-online-token-v1"
    }
}
