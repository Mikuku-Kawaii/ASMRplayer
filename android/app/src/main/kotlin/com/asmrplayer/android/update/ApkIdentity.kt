package com.asmrplayer.android.update

import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.os.Build
import java.io.File

internal class ApkIdentity(private val context: Context) {
    @Suppress("DEPRECATION")
    private val flags = if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES
    @Suppress("DEPRECATION")
    fun installed(): PackageInfo = context.packageManager.getPackageInfo(context.packageName, flags)
    @Suppress("DEPRECATION")
    fun code(info: PackageInfo): Long = if (Build.VERSION.SDK_INT >= 28) info.longVersionCode else info.versionCode.toLong()
    @Suppress("DEPRECATION")
    private fun certificates(info: PackageInfo): Set<String> =
        (if (Build.VERSION.SDK_INT >= 28) info.signingInfo?.apkContentsSigners else info.signatures)
            .orEmpty().map { it.toCharsString() }.toSet()

    @Suppress("DEPRECATION")
    fun verify(file: File, release: UpdateRelease) {
        val archive = requireNotNull(context.packageManager.getPackageArchiveInfo(file.path, flags)) { "无法读取 APK" }
        val installed = installed()
        require(archive.packageName == context.packageName && archive.versionName == release.version && code(archive) == release.versionCode) { "APK 包名或版本不匹配" }
        require(UpdateManifest.eligible(release, requireNotNull(installed.versionName), code(installed), Build.VERSION.SDK_INT)) { "APK 不是兼容的新版本" }
        val certificates = certificates(archive)
        require(certificates.isNotEmpty() && certificates == certificates(installed)) { "APK 安装签名与当前应用不同" }
        val app = requireNotNull(archive.applicationInfo)
        if (Build.VERSION.SDK_INT >= 24) require(app.minSdkVersion == release.minSdk) { "APK 最低系统版本不匹配" }
        if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE == 0) {
            require(app.flags and ApplicationInfo.FLAG_DEBUGGABLE == 0) { "正式版本不能安装调试包" }
        }
    }
}
