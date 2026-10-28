plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "com.hivepaas.shellwing"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.hivepaas.shellwing"
        // scripts/build_android.sh builds the Rust library for the same.
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        // What libghostty-vt builds for: phones and the emulator.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
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
