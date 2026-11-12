package com.asmrplayer.android.update

import org.json.JSONObject
import java.net.URI

data class UpdateRelease(
    val version: String, val versionCode: Long, val minSdk: Int, val url: String,
    val signature: String, val sha256: String, val size: Long, val notes: String, val publishedAt: String,
)

internal object UpdateManifest {
    const val ENDPOINT = "https://github.com/Mikuku-Kawaii/ASMRplayer/releases/latest/download/latest-android.json"
    const val MAX_APK_BYTES = 512L * 1024 * 1024
    private val stable = Regex("(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)")

    fun compareVersions(left: String, right: String): Int {
        fun parts(value: String): List<Long> {
            require(stable.matches(value)) { "只接受稳定版本" }
            return value.split('.').map { it.toLong() }
        }
        return parts(left).zip(parts(right)).firstOrNull { it.first != it.second }
            ?.let { it.first.compareTo(it.second) } ?: 0
    }

    fun parse(raw: String): UpdateRelease? {
        val root = JSONObject(raw)
        require(!root.optBoolean("draft") && !root.optBoolean("prerelease")) { "不是正式发布" }
        val version = root.getString("version")
        compareVersions(version, version)
        // A Windows-only release remains a valid shared manifest.
        val entry = root.getJSONObject("platforms").optJSONObject("android-universal") ?: return null
        val uri = URI(entry.getString("url"))
        require(uri.scheme == "https" && uri.host == "github.com" && uri.port == -1 &&
            uri.rawUserInfo == null && uri.rawQuery == null && uri.rawFragment == null &&
            uri.rawPath.startsWith("/Mikuku-Kawaii/ASMRplayer/releases/download/v$version/") &&
            uri.rawPath.substringAfterLast('/').matches(Regex("[A-Za-z0-9._-]+\\.apk"))) { "无效的更新下载地址" }
        val size = entry.getLong("size")
        val code = entry.getLong("versionCode")
        val sdk = entry.getInt("minSdk")
        require(size in 1..MAX_APK_BYTES && code in 1..2_100_000_000 && sdk >= 23)
        val hash = entry.getString("sha256")
        require(hash.matches(Regex("[a-f0-9]{64}"))) { "无效的 APK 哈希" }
        val signature = entry.getString("signature")
        require(signature.length in 1..16_384)
        return UpdateRelease(version, code, sdk, uri.toString(), signature, hash, size,
            root.optString("notes"), root.optString("pub_date"))
    }

    fun eligible(release: UpdateRelease, installedVersion: String, installedCode: Long, sdk: Int): Boolean =
        compareVersions(release.version, installedVersion) > 0 && release.versionCode > installedCode && release.minSdk <= sdk
}
