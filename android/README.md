# shellwing for Android

A Kotlin and Jetpack Compose app over the same core as the desktop and the
iPhone: `crates/mobile` wraps `shellwing-core` (SSH, libghostty-vt,
hosts.toml, the vault) for Kotlin with [UniFFI](https://mozilla.github.io/uniffi-rs/),
as it does for Swift. The app draws the terminal's frames and sends it what is
typed; connecting, prompts (passwords, host keys) and the terminal itself are
the desktop's own code. The design is the canvas "shellwing Android".

## Building

Needs JDK 17 or newer, the Android SDK with an NDK (`ANDROID_HOME`, or
`~/Android/Sdk`), [cargo-ndk](https://github.com/bbqsrc/cargo-ndk), and Zig 0.16
on `PATH`:

```sh
cargo install cargo-ndk
make android                 # the Rust library, its Kotlin, and a debug APK
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

`make android` runs `scripts/build_android.sh` (the library for arm64-v8a and
x86_64, and the Kotlin UniFFI writes from it), then `./gradlew assembleDebug`
in `android/`. After changing `crates/mobile`, run it again. Android Studio
opens `android/` once the script has run.

What is generated, and not committed: `android/app/src/main/jniLibs/` and
`android/app/build/generated/uniffi/`. There is no 32-bit build:
libghostty-vt builds for 64-bit Android only.

## How it fits Android

| | |
| --- | --- |
| `ShellwingApp.kt` | The core's environment (a home, known_hosts, the CAs libgit2 checks against), then the app's state, for the process's life |
| `AppModel.kt`, `SessionModel.kt` | The core, the hosts, the open sessions; each session's frames and status, as the iOS app's AppModel |
| `SessionService.kt` | A foreground service while sessions are open: Android keeps the process, and so the connections, when shellwing is not on screen; its notification lists them |
| `KeystoreKeys.kt` | The vault's key, encrypted by a key in Android's keystore, in the no-backup folder: on this device only |
| `AppLock.kt` | Lock shellwing when away: the fingerprint, the face or the screen lock (BiometricPrompt) |
| `ui/TerminalView.kt` | The terminal: draws a frame's runs, takes the on-screen keyboard and a physical one, scrolls the history, pinch sizes the text, a long press selects (dragging takes more) and offers Copy |
| `ui/` | Hosts, a session with the keys a phone lacks, Sessions, the host editor, a session's files, the library, the vault, sync, Settings, the lock |

Coming back to the app, or a network change, checks each session
(`MobileSession.resumed`) as on iOS. `Native.init` hands the core the app's
context once, so certificates are checked with Android's trust store.

Every string goes through the desktop's catalogs (`I18n.kt`);
`scripts/i18n.py` scans the Kotlin as it does the Swift.

Sync offers WebDAV, S3 and Git, not a folder: Android gives an app no folder
of the user's to write in by its path.

A session's files (SFTP) go to and from the phone through Android's pickers:
the core reads and writes paths, the pickers hand content URIs, so each
transfer is copied through the app's cache, and a file provider hands those
copies to other apps to show or send.

On a tablet (600 dp wide or more) the tabs are a rail; from 840 dp wide and
480 dp tall, Hosts and Sessions show beside the session open, a tab for each
session above it (`ui/AppRoot.kt`). With a keyboard attached the keys row
hides unless the on-screen keyboard is up, and Ctrl+Shift+C and V copy and
paste.
