package com.asmrplayer.android.sync

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.test.platform.app.InstrumentationRegistry
import net.openid.appauth.AppAuthConfiguration
import net.openid.appauth.AuthorizationResponse
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL

class OneDriveAuthorizationTest {
    @Test fun microsoftPkceAndEncryptedRefreshRotation() =
        verifyOAuth(StorageOAuth.oneDrive.copy(client = "5258babd-abf7-4d50-aa50-43c8a215e2dc"))
    private fun verifyOAuth(fixture: OAuthProfile) {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        assumeTrue(context.packageName.endsWith(".drivetest"))
        val bodies = mutableListOf<ByteArrayOutputStream>()
        val replies = ArrayDeque(listOf(
            JSONObject().put("scope", fixture.scope).put("refresh_token", "first-refresh"),
            JSONObject().put("refresh_token", "rotated-refresh"), JSONObject(),
            JSONObject().put("scope", fixture.scope + " email")
        ))
        val config = AppAuthConfiguration.Builder().setConnectionBuilder { uri ->
            check(uri.toString() == fixture.tokenUrl)
            val output = ByteArrayOutputStream().also(bodies::add)
            val reply = replies.removeFirst().put("access_token", "fixture-access").put("token_type", "Bearer")
            object : HttpURLConnection(URL(uri.toString())) {
                override fun connect() = Unit
                override fun disconnect() = Unit
                override fun usingProxy() = false
                override fun getOutputStream() = output
                override fun getInputStream() = reply.toString().byteInputStream()
                override fun getResponseCode() = 200
            }
        }.build()
        val session = StorageOAuth(context, config, fixture)
        session.clear()
        try {
            val request = session.request()
            val redirect = request.redirectUri
            val intent = Intent(Intent.ACTION_VIEW, redirect).setPackage(context.packageName).addCategory(Intent.CATEGORY_BROWSABLE)
            assertEquals("net.openid.appauth.RedirectUriReceiverActivity", intent.resolveActivity(context.packageManager)?.className)
            assertEquals(fixture.scope, request.scope)
            assertEquals("S256", request.codeVerifierChallengeMethod)
            assertNull(request.additionalParameters["access_type"])
            val builder = AuthorizationResponse.Builder(request).setAuthorizationCode("fixture-code")
            val invalid = builder.setState("wrong").build()
            assertTrue(runCatching { session.exchange(invalid) }.isFailure)
            assertTrue(bodies.isEmpty())
            assertEquals("fixture-access", session.exchange(builder.setState(request.state).build()))
            fun params(index: Int) = Uri.parse("https://example.invalid/?" + bodies[index].toString("UTF-8"))
            assertEquals(request.codeVerifier, params(0).getQueryParameter("code_verifier"))
            assertEquals(redirect.toString(), params(0).getQueryParameter("redirect_uri"))
            val prefs = context.getSharedPreferences(fixture.storage, Context.MODE_PRIVATE)
            assertFalse(prefs.all.values.any { it.toString().contains("first-refresh") || it.toString().contains("fixture-access") })
            val restored = StorageOAuth(context, config, fixture)
            assertTrue(restored.connected())
            assertEquals("fixture-access", restored.refresh())
            run {
                assertEquals(fixture.scope,params(0).getQueryParameter("scope"))
                assertEquals(fixture.scope,params(1).getQueryParameter("scope"))
            }
            assertEquals("first-refresh", params(1).getQueryParameter("refresh_token"))
            assertEquals("fixture-access", restored.refresh())
            assertEquals("rotated-refresh", params(2).getQueryParameter("refresh_token"))
            assertTrue(runCatching { restored.refresh() }.isFailure)
        } finally { session.clear() }
        assertFalse(session.connected())
    }
}
