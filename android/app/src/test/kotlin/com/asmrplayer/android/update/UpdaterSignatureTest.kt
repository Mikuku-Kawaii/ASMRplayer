package com.asmrplayer.android.update

import org.bouncycastle.crypto.digests.Blake2bDigest
import org.bouncycastle.crypto.params.Ed25519PrivateKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer
import org.bouncycastle.util.encoders.Base64
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.security.MessageDigest

class UpdaterSignatureTest {
    private val key = Ed25519PrivateKeyParameters(ByteArray(32) { (it + 1).toByte() }, 0)
    private val id = ByteArray(8) { it.toByte() }
    private fun sign(bytes: ByteArray): ByteArray = Ed25519Signer().run {
        init(true, key); update(bytes, 0, bytes.size); generateSignature()
    }
    private val publicKey = Base64.toBase64String(("untrusted comment: fixture\n" +
        Base64.toBase64String("Ed".toByteArray() + id + key.generatePublicKey().encoded) + "\n").toByteArray())
    private fun signature(bytes: ByteArray, comment: String = "timestamp:1\tversion:1.4.0"): String {
        val digest = Blake2bDigest(512); digest.update(bytes, 0, bytes.size)
        val prehash = ByteArray(64); digest.doFinal(prehash, 0)
        val sig = sign(prehash)
        return Base64.toBase64String(("untrusted comment: fixture\n" + Base64.toBase64String("ED".toByteArray() + id + sig) +
            "\ntrusted comment: $comment\n" + Base64.toBase64String(sign(sig + comment.toByteArray())) + "\n").toByteArray())
    }
    private fun hash(bytes: ByteArray) = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
    private fun withFile(block: (File, ByteArray) -> Unit) {
        val bytes = ByteArray(200_000) { (it % 251).toByte() }
        val file = File.createTempFile("update", ".apk")
        try { file.writeBytes(bytes); block(file, bytes) } finally { file.delete() }
    }
    @Test fun verifiesStreamingSignedArtifactAndTrustedVersion() = withFile { file, bytes ->
        UpdaterSignature.verify(file, signature(bytes), publicKey, "1.4.0", hash(bytes), bytes.size.toLong())
    }
    @Test fun rejectsPayloadTamperingEvenWithReplacedManifestHash() = withFile { file, bytes ->
        val signature = signature(bytes); bytes[5] = 99; file.writeBytes(bytes)
        assertThrows(IllegalArgumentException::class.java) { UpdaterSignature.verify(file, signature, publicKey, "1.4.0", hash(bytes), bytes.size.toLong()) }
    }
    @Test fun rejectsWrongAndDuplicateSignedVersions() = withFile { file, bytes ->
        for (comment in listOf("version:1.3.0", "version:1.4.0\tversion:1.4.0", "timestamp:1")) {
            assertThrows(IllegalArgumentException::class.java) { UpdaterSignature.verify(file, signature(bytes, comment), publicKey, "1.4.0", hash(bytes), bytes.size.toLong()) }
        }
    }
    @Test fun rejectsForgedTrustedCommentAndTruncation() = withFile { file, bytes ->
        val forged = Base64.toBase64String(String(Base64.decode(signature(bytes))).replace("version:1.4.0", "version:9.9.9").toByteArray())
        assertThrows(IllegalArgumentException::class.java) { UpdaterSignature.verify(file, forged, publicKey, "9.9.9", hash(bytes), bytes.size.toLong()) }
        assertThrows(IllegalArgumentException::class.java) { UpdaterSignature.verify(file, signature(bytes), publicKey, "1.4.0", hash(bytes), bytes.size.toLong() + 1) }
    }
    @Test fun numericStableVersionOrdering() {
        assertTrue(UpdateManifest.compareVersions("1.10.0", "1.9.9") > 0)
        assertEquals(0, UpdateManifest.compareVersions("1.3.0", "1.3.0"))
        assertThrows(IllegalArgumentException::class.java) { UpdateManifest.compareVersions("1.4.0-beta", "1.3.0") }
    }
    @Test fun verifiesActualTauriSignerVersionBoundFixture() {
        val lines = requireNotNull(javaClass.getResourceAsStream("/update-signature.txt")).bufferedReader().use { it.readLines() }
        val bytes = Base64.decode(lines[2]); val file = File.createTempFile("tauri-update", ".apk")
        try {
            file.writeBytes(bytes)
            UpdaterSignature.verify(file, lines[1], lines[0], "1.4.0", hash(bytes), bytes.size.toLong())
        } finally { file.delete() }
    }
}
