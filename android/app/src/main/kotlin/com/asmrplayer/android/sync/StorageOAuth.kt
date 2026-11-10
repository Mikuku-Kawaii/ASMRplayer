package com.asmrplayer.android.sync

import android.content.Context
import android.net.Uri
import com.asmrplayer.android.BuildConfig
import com.asmrplayer.android.security.TokenCipher
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import net.openid.appauth.*

internal data class OAuthProfile(
    val client: String,
    val redirect: String,
    val authUrl: String,
    val tokenUrl: String,
    val scope: String,
    val storage: String,
    val alias: String,
) {
    fun acceptsScope(value: String?): Boolean {
        if (value == null) return true
        val grants =
            value
                .split(" ")
                .filter { it.isNotBlank() }
                .map { it.removePrefix("https://graph.microsoft.com/").lowercase() }
                .toSet()
        return "files.readwrite.appfolder" in grants &&
            grants.all { it in setOf("files.readwrite.appfolder", "offline_access") }
    }
}

internal class StorageOAuth(
    private val context: Context,
    private val tokenConfig: AppAuthConfiguration = AppAuthConfiguration.DEFAULT,
    private val profile: OAuthProfile = oneDrive,
) {
    private val prefs = context.getSharedPreferences(profile.storage, Context.MODE_PRIVATE)
    private val cipher = TokenCipher(profile.alias)

    fun connected() =
        profile.client.isNotBlank() &&
            prefs.contains("refresh") &&
            prefs.getString("client", null) == profile.client

    fun clear() {
        check(prefs.edit().clear().commit()) { "无法清除账号登录凭据" }
    }

    fun exchange(response: AuthorizationResponse): String {
        val request = response.request
        check(
            profile.client.isNotBlank() &&
                request.clientId == profile.client &&
                request.redirectUri.toString() == profile.redirect &&
                request.configuration.tokenEndpoint == endpoints.tokenEndpoint &&
                request.scope == profile.scope &&
                request.codeVerifierChallengeMethod == "S256" &&
                !request.codeVerifier.isNullOrBlank() &&
                !request.state.isNullOrBlank() &&
                request.state == response.state
        ) {
            "账号登录回调无效，请重新登录"
        }
        val exchange =
            TokenRequest.Builder(endpoints, profile.client)
                .setGrantType(GrantTypeValues.AUTHORIZATION_CODE)
                .setScope(profile.scope)
                .setAuthorizationCode(response.authorizationCode)
                .setCodeVerifier(request.codeVerifier)
                .setRedirectUri(request.redirectUri)
                .build()
        val tokens =
            try {
                perform(exchange)
            } catch (_: AuthorizationException) {
                error("账号登录未完成，请检查客户端注册后重新登录")
            }
        val refresh =
            requireNotNull(tokens.refreshToken?.takeIf { it.isNotEmpty() }) { "未获得离线授权，请重新登录" }
        val access = accessToken(tokens)
        save(refresh)
        return access
    }

    fun refresh(): String {
        check(connected()) { "请先绑定当前存储" }
        val refresh =
            try {
                cipher.decrypt(requireNotNull(prefs.getString("refresh", null)), profile.client)
            } catch (_: Exception) {
                clear()
                error("账号登录凭据无法读取，请重新登录")
            }
        val request =
            TokenRequest.Builder(endpoints, profile.client)
                .setGrantType(GrantTypeValues.REFRESH_TOKEN)
                .setRefreshToken(refresh)
                .setScope(profile.scope)
                .build()
        val tokens =
            try {
                perform(request)
            } catch (e: AuthorizationException) {
                if (e.error == "invalid_grant") {
                    clear()
                    error("账号授权已过期，请重新登录")
                }
                error("无法刷新账号授权，请检查网络后重试")
            }
        val access = accessToken(tokens)
        tokens.refreshToken?.let(::save)
        return access
    }

    private fun accessToken(response: TokenResponse): String {
        check(profile.acceptsScope(response.scope)) { "授权服务未返回指定的应用文件夹权限" }
        return requireNotNull(response.accessToken?.takeIf { it.isNotEmpty() }) { "授权服务未返回访问令牌" }
    }

    private fun save(refresh: String) {
        check(
            prefs
                .edit()
                .putString("refresh", cipher.encrypt(refresh, profile.client))
                .putString("client", profile.client)
                .remove("email")
                .commit()
        ) {
            "无法保存账号登录凭据"
        }
    }

    // SyncRepository serializes exchanges on an IO thread; AppAuth delivers its callback on the
    // main thread.
    private fun perform(request: TokenRequest): TokenResponse {
        val service = AuthorizationService(context, tokenConfig)
        try {
            val done = CountDownLatch(1)
            var tokens: TokenResponse? = null
            var failure: AuthorizationException? = null
            service.performTokenRequest(request) { result, error ->
                tokens = result
                failure = error
                done.countDown()
            }
            check(done.await(60, TimeUnit.SECONDS)) { "账号授权请求超时" }
            failure?.let { throw it }
            return checkNotNull(tokens) { "账号授权未成功" }
        } finally {
            service.dispose()
        }
    }

    private val endpoints =
        AuthorizationServiceConfiguration(Uri.parse(profile.authUrl), Uri.parse(profile.tokenUrl))

    fun request(): AuthorizationRequest {
        check(profile.client.isNotBlank()) { "此版本尚未启用此存储的登录" }
        val builder =
            AuthorizationRequest.Builder(
                    endpoints,
                    profile.client,
                    ResponseTypeValues.CODE,
                    Uri.parse(profile.redirect),
                )
                .setScope(profile.scope)
                .setPrompt("select_account")
        return builder.build()
    }

    companion object {
        val oneDrive =
            OAuthProfile(
                BuildConfig.ONEDRIVE_CLIENT_ID,
                BuildConfig.ONEDRIVE_REDIRECT_URI,
                "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                "Files.ReadWrite.AppFolder offline_access",
                "onedrive-account",
                "asmr-onedrive-refresh-v1",
            )

        fun browser(context: Context) =
            AuthorizationService(
                context,
                AppAuthConfiguration.Builder().setBrowserMatcher { it.useCustomTab }.build(),
            )
    }
}
