package com.asmrplayer.android.update

import org.bouncycastle.crypto.digests.Blake2bDigest
import org.bouncycastle.crypto.params.Ed25519PublicKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer
import org.bouncycastle.util.encoders.Base64
import java.io.File
import java.security.MessageDigest

/** Tauri's base64-wrapped Minisign ED signature, including its signed version comment. */
internal object UpdaterSignature {
    fun verify(file: File, encodedSignature: String, encodedKey: String, version: String, sha256: String, size: Long) {
        require(file.length() == size && size in 1..UpdateManifest.MAX_APK_BYTES) { "APK 长度不匹配" }
        val keyLines = String(Base64.decode(encodedKey.trim()), Charsets.UTF_8).trim().lines()
        val lines = String(Base64.decode(encodedSignature.trim()), Charsets.UTF_8).trim().lines()
        require(keyLines.size == 2 && lines.size == 4 && lines[2].startsWith("trusted comment: "))
        val key = Base64.decode(keyLines[1]); val packet = Base64.decode(lines[1])
        require(key.size == 42 && String(key, 0, 2, Charsets.US_ASCII) == "Ed")
        // Tauri's current signer emits ED. Streaming the prehash avoids buffering an APK in RAM.
        require(packet.size == 74 && String(packet, 0, 2, Charsets.US_ASCII) == "ED") { "需要预哈希 Minisign 签名" }
        require(key.copyOfRange(2, 10).contentEquals(packet.copyOfRange(2, 10))) { "更新签名密钥不匹配" }
        val publicKey = Ed25519PublicKeyParameters(key, 10)
        val signature = packet.copyOfRange(10, 74)
        fun verifyBytes(bytes: ByteArray, sig: ByteArray) {
            val verifier = Ed25519Signer(); verifier.init(false, publicKey)
            verifier.update(bytes, 0, bytes.size)
            require(verifier.verifySignature(sig)) { "更新签名校验失败" }
        }
        val comment = lines[2].removePrefix("trusted comment: ")
        verifyBytes(signature + comment.toByteArray(Charsets.UTF_8), Base64.decode(lines[3]))
        require(comment.split('\t').filter { it.startsWith("version:") } == listOf("version:$version")) { "签名版本不匹配" }
        val digest = Blake2bDigest(512); val hash = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { stream ->
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val count = stream.read(buffer); if (count < 0) break
                digest.update(buffer, 0, count); hash.update(buffer, 0, count)
            }
        }
        require(hash.digest().joinToString("") { "%02x".format(it) } == sha256) { "APK 哈希校验失败" }
        val prehash = ByteArray(64); digest.doFinal(prehash, 0); verifyBytes(prehash, signature)
    }
}
