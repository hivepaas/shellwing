plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// The app's version is the workspace's, in Cargo.toml: one for every platform
// (docs/releasing.md).
val workspaceVersion: String = rootProject.file("../Cargo.toml").readLines()
    .dropWhile { it.trim() != "[workspace.package]" }
    .firstNotNullOf { Regex("""^version = "(.+)"""").find(it)?.groupValues?.get(1) }

/** 0.3.0-beta1 is 30001 and 0.3.0 is 30099: a beta comes before its release. */
fun versionCodeOf(version: String): Int {
    val parts = Regex("""^(\d+)\.(\d+)\.(\d+)(?:-beta(\d+))?$""").matchEntire(version)
        ?: error("Cargo.toml's version $version is not X.Y.Z or X.Y.Z-betaN")
    val (major, minor, patch, beta) = parts.destructured
    return major.toInt() * 1_000_000 + minor.toInt() * 10_000 + patch.toInt() * 100 + (beta.toIntOrNull() ?: 99)
}

// Release builds are signed when the environment holds the key: the Release
// workflow sets these from the repository's secrets (docs/releasing.md ›
// Signing for Android). Without it they come out unsigned.
val keystore: String? = System.getenv("SHELLWING_KEYSTORE")

android {
    namespace = "com.hivepaas.shellwing"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.hivepaas.shellwing"
        // scripts/build_android.sh builds the Rust library for the same.
        minSdk = 28
        targetSdk = 36
        versionCode = versionCodeOf(workspaceVersion)
        versionName = workspaceVersion
        // What libghostty-vt builds for: phones and the emulator.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    signingConfigs {
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("SHELLWING_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("SHELLWING_KEY_ALIAS")
                keyPassword = System.getenv("SHELLWING_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            if (keystore != null) signingConfig = signingConfigs.getByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    // The Kotlin UniFFI writes from the Rust library (scripts/build_android.sh).
    sourceSets["main"].kotlin.directories += "build/generated/uniffi"
}

// The Rust half of rustls-platform-verifier calls its Kotlin half by name:
// the two must be the same release.
val checkVerifierVersion by tasks.registering {
    val lock = rootProject.layout.projectDirectory.file("../Cargo.lock")
    val wanted = libs.versions.rustlsPlatformVerifier
    inputs.file(lock)
    doLast {
        val lines = lock.asFile.readLines()
        val at = lines.indexOfFirst { it.trim() == "name = \"rustls-platform-verifier-android\"" }
        val version = lines.getOrNull(at + 1)?.substringAfter('"')?.substringBefore('"')
        check(version == wanted.get()) {
            "Cargo.lock has rustls-platform-verifier-android $version, gradle/libs.versions.toml ${wanted.get()}"
        }
    }
}
tasks.named("preBuild") { dependsOn(checkVerifierVersion) }

dependencies {
    val bom = platform(libs.compose.bom)
    implementation(bom)
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)
    implementation(libs.activity.compose)
    implementation(libs.lifecycle.runtime.compose)
    implementation(libs.lifecycle.process)
    implementation(libs.biometric)
    implementation(libs.fragment)
    implementation(libs.coroutines.android)
    implementation(libs.jna) { artifact { type = "aar" } }
    implementation(libs.rustls.platform.verifier)
}
