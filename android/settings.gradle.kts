// The Android app (README.md): one module, app/, over the Rust library that
// scripts/build_android.sh builds.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        // rustls-platform-verifier's Kotlin half, which the Rust half calls
        // to check certificates with Android's trust store.
        maven("https://github.com/rustls/rustls-platform-verifier/raw/maven-archive/android-release-support/maven/")
    }
}

rootProject.name = "shellwing"
include(":app")
