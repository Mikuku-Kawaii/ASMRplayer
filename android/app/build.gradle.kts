import java.util.UUID
import groovy.json.JsonSlurper

plugins {
    id("com.android.application")
    kotlin("android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("com.google.devtools.ksp")
}
android {
    namespace = "com.asmrplayer.android"
    compileSdk = 36
    defaultConfig {
        applicationId = "com.asmrplayer.android"
        val microsoft = providers.environmentVariable("ASMR_ONEDRIVE_CLIENT_ID").orNull.orEmpty().trim()
            .ifEmpty { rootProject.file("../config/onedrive-oauth-client-id.txt").readText().trim() }
            .lowercase()
        require(microsoft.isEmpty() || UUID.fromString(microsoft).toString().equals(microsoft, true))
        val msScheme = if (microsoft.isEmpty()) "com.asmrplayer.onedrive.unconfigured" else "msal$microsoft"
        buildConfigField("String", "ONEDRIVE_CLIENT_ID", "\"$microsoft\"")
        buildConfigField("String", "ONEDRIVE_REDIRECT_URI", "\"$msScheme://auth\"")
        manifestPlaceholders["oneDriveScheme"] = msScheme
        manifestPlaceholders["appAuthRedirectScheme"] = msScheme
        val githubClient = rootProject.file("../config/github-oauth-client-id.txt").readText().trim()
        buildConfigField("String", "GITHUB_CLIENT_ID", "\"$githubClient\"")
        minSdk = 23
        targetSdk = 36
        versionCode = providers.gradleProperty("androidVersionCode").orElse("3").get().toInt()
        versionName = (JsonSlurper().parse(rootProject.file("../package.json")) as Map<*, *>)["version"] as String
        val updater = ((JsonSlurper().parse(rootProject.file("../src-tauri/tauri.conf.json")) as Map<*, *>)["plugins"] as Map<*, *>)["updater"] as Map<*, *>
        buildConfigField("String", "UPDATE_PUBLIC_KEY", "\"${updater["pubkey"]}\"")
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        manifestPlaceholders["fixtureAuthority"] = providers.gradleProperty("androidFixtureAuthority").orElse("com.asmrplayer.android.test.fixture").get()
    }
    buildFeatures { compose = true; buildConfig = true }
    signingConfigs {
        if (System.getenv("ANDROID_KEYSTORE_FILE") != null) create("distribution") {
            storeFile = file(System.getenv("ANDROID_KEYSTORE_FILE"))
            storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("ANDROID_KEY_ALIAS")
            keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
        }
    }
    // Opt-in separate sandbox for instrumented tests on a connected phone.
    buildTypes.getByName("debug") {
        if (providers.gradleProperty("androidUiTestBuild").orNull == "true") applicationIdSuffix = providers.gradleProperty("androidUiTestSuffix").orElse(".uitest").get()
    }
    buildTypes.getByName("release") { signingConfig = signingConfigs.findByName("distribution") }
    sourceSets.getByName("androidTest").assets.srcDir("schemas")
    sourceSets.getByName("androidTest").assets.srcDir("../../tests/fixtures/audio")

    compileOptions {
        isCoreLibraryDesugaringEnabled = true
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}
ksp { arg("room.schemaLocation", "$projectDir/schemas") }
val supabaseAssets = tasks.register<Exec>("bundleSupabaseSdk") {
    workingDir(rootProject.projectDir.parentFile)
    inputs.files(listOf("src/lib/supabaseSync.ts", "config/supabase.json", "scripts/build-supabase-android.mjs",
        "pnpm-lock.yaml").map { workingDir.resolve(it) })
    val output = layout.buildDirectory.dir("generated/supabaseAssets").get().asFile
    outputs.file(output.resolve("supabase-sync.js"))
    commandLine("node", "scripts/build-supabase-android.mjs", output.absolutePath)
}
android.sourceSets.getByName("main").assets.srcDir(layout.buildDirectory.dir("generated/supabaseAssets"))
tasks.named("preBuild") { dependsOn(supabaseAssets) }
dependencies {
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
    implementation("net.openid:appauth:0.11.1")
    implementation("org.bouncycastle:bcprov-jdk15to18:1.86")
    implementation("androidx.work:work-runtime-ktx:2.10.1")
    implementation("androidx.room:room-runtime:2.8.4")
    implementation("androidx.room:room-ktx:2.8.4")
    ksp("androidx.room:room-compiler:2.8.4")
    implementation(project(":audio-core"))
    implementation(platform("androidx.compose:compose-bom:2025.11.01"))
    implementation("androidx.activity:activity-compose:1.11.0")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.10.0")
    implementation("androidx.documentfile:documentfile:1.0.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("androidx.media3:media3-exoplayer:1.8.0")
    implementation("androidx.media3:media3-session:1.8.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
    androidTestImplementation(platform("androidx.compose:compose-bom:2025.11.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.7.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
}
