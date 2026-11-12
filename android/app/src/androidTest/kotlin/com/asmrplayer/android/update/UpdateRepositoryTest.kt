package com.asmrplayer.android.update

import android.content.Context
import android.content.ContextWrapper
import androidx.core.content.FileProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.BuildConfig
import com.asmrplayer.android.online.HttpTransport
import com.asmrplayer.android.online.OnlineFixtureServer
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.bouncycastle.crypto.digests.Blake2bDigest
import org.bouncycastle.crypto.params.Ed25519PrivateKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer
import org.bouncycastle.util.encoders.Base64
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.security.MessageDigest
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class UpdateRepositoryTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private class Fixture : UpdateRemote {
        val bytes = ByteArray(100_000) { (it % 251).toByte() }
        val version = BuildConfig.VERSION_NAME.substringBeforeLast('.') + "." +
            (BuildConfig.VERSION_NAME.substringAfterLast('.').toLong() + 1)
        private val key = Ed25519PrivateKeyParameters(ByteArray(32) { (it + 7).toByte() }, 0)
        private val id = ByteArray(8) { it.toByte() }
        private fun sign(bytes: ByteArray) = Ed25519Signer().run { init(true, key); update(bytes, 0, bytes.size); generateSignature() }
        val publicKey = Base64.toBase64String(("untrusted comment: fixture\n" + Base64.toBase64String("Ed".toByteArray() + id + key.generatePublicKey().encoded) + "\n").toByteArray())
        val json: String
        var tamper = false
        var suspended = false
        var checks = 0
        init {
            val digest = Blake2bDigest(512); digest.update(bytes, 0, bytes.size)
            val hash = ByteArray(64); digest.doFinal(hash, 0)
            val sig = sign(hash); val comment = "timestamp:1\tversion:$version"
            val signature = Base64.toBase64String(("untrusted comment: fixture\n" + Base64.toBase64String("ED".toByteArray() + id + sig) +
                "\ntrusted comment: $comment\n" + Base64.toBase64String(sign(sig + comment.toByteArray())) + "\n").toByteArray())
            json = JSONObject().put("version", version).put("platforms", JSONObject().put("android-universal", JSONObject()
                .put("url", "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v$version/fixture.apk")
                .put("versionCode", BuildConfig.VERSION_CODE + 1).put("minSdk", 23).put("signature", signature).put("size", bytes.size)
                .put("sha256", MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }))).toString()
        }
        override suspend fun manifest(): String { checks++; return json }
        override suspend fun download(release: UpdateRelease, destination: File, progress: (Long) -> Unit) {
            destination.writeBytes(bytes.copyOf().apply { if (tamper) this[1] = 0 })
            progress(bytes.size.toLong())
            if (suspended) awaitCancellation()
        }
    }
    private fun isolated(): Context {
        val id = UUID.randomUUID().toString()
        return object : ContextWrapper(context) {
            override fun getFilesDir(): File = File(context.cacheDir, "update-test-$id").apply { mkdirs() }
            override fun getSharedPreferences(name: String, mode: Int) = context.getSharedPreferences("$id-$name", mode)
        }
    }
    @Test fun signedDownloadRestoresAndTamperedCacheIsDiscarded() = runBlocking {
        val isolated = isolated(); val remote = Fixture(); var identities = 0
        fun repository() = UpdateRepository(isolated, remote, remote.publicKey) { _, _ -> identities++ }
        try {
            val first = repository()
            assertEquals(UpdatePhase.AVAILABLE, first.check().phase)
            assertTrue(first.download()); assertEquals(UpdatePhase.READY, first.state.value.phase)
            val restored = repository()
            assertEquals(UpdatePhase.READY, restored.check().phase)
            assertEquals(1, remote.checks); assertEquals(2, identities)
            File(isolated.filesDir, "updates/ready/update.apk").appendText("tampered")
            val rejected = repository()
            assertEquals(UpdatePhase.AVAILABLE, rejected.check().phase)
            assertFalse(File(isolated.filesDir, "updates/ready/update.apk").exists())
        } finally { isolated.filesDir.deleteRecursively() }
    }
    @Test fun tamperedDownloadNeverReachesApkVerifierAndCanRetry() = runBlocking {
        val isolated = isolated(); val remote = Fixture().apply { tamper = true }; var verified = false
        try {
            val repository = UpdateRepository(isolated, remote, remote.publicKey) { _, _ -> verified = true }
            repository.check(); assertFalse(repository.download())
            assertEquals(UpdatePhase.ERROR, repository.state.value.phase); assertFalse(verified)
            assertFalse(File(isolated.filesDir, "updates/download.part").exists())
            remote.tamper = false; assertTrue(repository.download()); assertTrue(verified)
        } finally { isolated.filesDir.deleteRecursively() }
    }
    @Test fun cancellationRemovesPartialAndSkippedVersionStillAllowsManualCheck() = runBlocking {
        val isolated = isolated(); val remote = Fixture().apply { suspended = true }
        try {
            val repository = UpdateRepository(isolated, remote, remote.publicKey) { _, _ -> }
            repository.check(); repository.skipVersion(remote.version)
            assertEquals(UpdatePhase.UP_TO_DATE, repository.check(manual = false).phase)
            assertEquals(UpdatePhase.AVAILABLE, repository.check().phase)
            val job = launch(Dispatchers.IO) { repository.download() }
            withTimeout(5_000) { repository.state.first { it.phase == UpdatePhase.DOWNLOADING && it.downloadedBytes > 0 } }
            job.cancelAndJoin()
            assertEquals(UpdatePhase.AVAILABLE, repository.state.value.phase)
            assertFalse(File(isolated.filesDir, "updates/download.part").exists())
        } finally { isolated.filesDir.deleteRecursively() }
    }
    @Test fun apkIdentityRejectsInvalidAndSameVersionAndProviderIsNarrow() {
        val installed = ApkIdentity(context)
        val release = UpdateManifest.parse(Fixture().json)!!
        assertThrows(IllegalArgumentException::class.java) { installed.verify(File(context.applicationInfo.sourceDir), release) }
        val invalid = File(context.cacheDir, "invalid-update.apk").apply { writeText("not an APK") }
        try {
            assertThrows(IllegalArgumentException::class.java) { installed.verify(invalid, release) }
            assertThrows(IllegalArgumentException::class.java) { FileProvider.getUriForFile(context, "${context.packageName}.updates", invalid) }
            val ready = File(context.filesDir, "updates/ready/update.apk")
            assertEquals("content", FileProvider.getUriForFile(context, "${context.packageName}.updates", ready).scheme)
        } finally { invalid.delete() }
    }
    @Test fun httpTransferChecksLengthAndStatus() = runBlocking {
        OnlineFixtureServer().use { server ->
            val remote = HttpUpdateRemote(HttpTransport(true))
            val release = UpdateManifest.parse(Fixture().json)!!.copy(url = "${server.base}/media", size = server.bytes.size.toLong())
            val destination = File(context.cacheDir, "update-transfer.part")
            try {
                var progress = 0L
                remote.download(release, destination) { progress = it }
                assertArrayEquals(server.bytes, destination.readBytes()); assertEquals(release.size, progress)
                try { remote.download(release.copy(size = 1), destination) {}; fail("length mismatch accepted") } catch (_: IllegalArgumentException) { }
                server.mode = "unauthorized"
                try { remote.download(release, destination) {}; fail("HTTP error accepted") } catch (_: IllegalArgumentException) { }
            } finally { destination.delete() }
        }
    }
}
