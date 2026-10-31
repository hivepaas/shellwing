# Releasing

A release is a tag, the files CI builds for it, and an entry in the signed list
of releases the app reads ([updates.md](updates.md)). Until the list names it, no
app is told of it.

## What a release ships

| Platform | Files | Made by |
| --- | --- | --- |
| Linux x86_64, aarch64 | `shellwing_<v>_linux_<arch>.AppImage`, `.tar.gz`, `shellwing_<v>_<amd64\|arm64>.deb` | `scripts/package_linux.sh` |
| macOS aarch64, x86_64 | `shellwing_<v>_macos_<arch>.dmg`, `.app.tar.gz` | `scripts/package_macos.sh` |
| Windows x86_64 | `shellwing_<v>_windows_x86_64-setup.exe`, `.zip` | `scripts/package_windows.sh` |
| Android (arm64-v8a, x86_64), rehearsals only for now | `shellwing_<v>_android.apk` | `scripts/build_android.sh`, then `./gradlew assembleRelease` in `android/` |

and `SHA256SUMS`. The updater installs the AppImage, the `.app.tar.gz` and the
installer; the others are for people.

- **Linux** builds on Ubuntu 22.04, so the packages run on glibc 2.35 and newer.
  The binary links only libc and xkbcommon; Wayland, X11 and Vulkan are loaded
  when it starts. The `.deb` puts it in `/opt/shellwing` (linked from
  `/usr/bin`): under `/usr` the updater would take it for a distribution's own
  package and leave it to apt.
- **macOS**: `shellwing.app`, signed with the Developer ID and notarized when the
  repository holds one ([Signing for macOS](#signing-for-macos)), else signed ad
  hoc: then Gatekeeper asks the first time it opens (*Open Anyway* in *System
  Settings › Privacy & Security*; Control-click › Open on macOS 14 and older).
  The updater accepts a new `.app` whose signature verifies, by the team that
  signed the running one (any, while that one is signed by no team).
- **Windows**: an NSIS installer, for the user, into
  `%LOCALAPPDATA%\Programs\shellwing` (where the updater looks for it), with a
  Start menu entry and an uninstaller. It is not signed: SmartScreen asks the
  first time (More info › Run anyway).
- **Android**: one APK, signed with the release key
  ([Signing for Android](#signing-for-android)); its version is `Cargo.toml`'s
  (`versionCode` *major·10⁶ + minor·10⁴ + patch·100*, plus the beta's number or
  99, so a beta comes before its release). It is installed by hand (Android asks
  to allow installs from the browser or file manager it came from), and
  installed again over itself to update: the updater does not run on phones.

`make package` makes them for the computer it runs on, into `dist/`.

## Signing for macOS

With a Developer ID, the app opens without Gatekeeper's question, and Touch ID
can open the vault: its Keychain item needs an entitlement only a signed app
with a provisioning profile has. Once:

1. **Apple's side** (developer.apple.com, an Apple Developer Program account):
   - a **Developer ID Application** certificate, made from a signing request
     (Keychain Access › Certificate Assistant), installed in the login
     keychain, and exported with its private key as a `.p12`;
   - an **App ID** `com.hivepaas.shellwing` (the `CFBundleIdentifier` in
     `packaging/macos/Info.plist`) and a **Developer ID provisioning profile**
     for it, which allows `keychain-access-groups`;
   - an **App Store Connect API key** for notarizing (Users and Access ›
     Integrations › Keys): its `.p8`, key ID and issuer ID.
2. **The repository's secrets** (Settings › Secrets and variables › Actions):

   | Secret | What |
   | --- | --- |
   | `MACOS_CERTIFICATE` | the `.p12`, as `base64 -i developer-id.p12` |
   | `MACOS_CERTIFICATE_PASSWORD` | the `.p12`'s password |
   | `MACOS_PROVISIONING_PROFILE` | the profile, as `base64 -i shellwing.provisionprofile` |
   | `APPLE_API_KEY` | the `.p8`'s text |
   | `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID` | the key's ID and the issuer ID |

The Release workflow then signs with a keychain of its own for the run (the
team is read from the certificate), notarizes the app and the disk image, and
staples their tickets. Without `MACOS_CERTIFICATE` it signs ad hoc, as before;
without the profile, Touch ID does not open the vault; without the API key,
the app is not notarized, and Gatekeeper refuses it downloaded.

**On a Mac**, with the certificate in the login keychain:

```sh
MACOS_SIGN_IDENTITY="Developer ID Application: Name (TEAMID)" \
MACOS_PROVISIONING_PROFILE=shellwing.provisionprofile \
APPLE_API_KEY_PATH=AuthKey_XXXX.p8 APPLE_API_KEY_ID=XXXX APPLE_API_ISSUER_ID=… \
scripts/package_macos.sh
```

The private key stays in the keychain and in the repository's secrets: never
in the repository, nor anywhere else.

## Signing for Android

Android installs an update over an app only when both are signed with the same
key: the release key is made once and kept for good. Lost, every user has to
uninstall shellwing (and its vault) to install the next one.

1. **The key**, once, with the JDK's `keytool`, on the signing machine:

   ```sh
   keytool -genkeypair -v -keystore shellwing-release.jks -alias shellwing \
     -keyalg RSA -keysize 4096 -validity 10000
   ```

   Keep `shellwing-release.jks` and its passwords with the other signing keys,
   and a copy somewhere else.
2. **The repository's secrets** (Settings › Secrets and variables › Actions):

   | Secret | Value |
   | --- | --- |
   | `ANDROID_KEYSTORE` | the keystore, as `base64 -w0 shellwing-release.jks` |
   | `ANDROID_KEYSTORE_PASSWORD` | the keystore's password |
   | `ANDROID_KEY_ALIAS` | `shellwing` |
   | `ANDROID_KEY_PASSWORD` | the key's password |

**Tags do not build the APK yet.** The release key has not been made, so the
Android job runs in rehearsals only, unsigned. Once these secrets are set, take
the job's `if` out of `release.yml` and put `android` back in the draft release's
`needs`; the job then refuses a tag without them. To sign one by hand, `scripts/build_android.sh`, then in `android/`:

```sh
SHELLWING_KEYSTORE=/path/to/shellwing-release.jks SHELLWING_KEYSTORE_PASSWORD=… \
SHELLWING_KEY_ALIAS=shellwing SHELLWING_KEY_PASSWORD=… ./gradlew assembleRelease
```

## Making one

1. **The version.** `version` under `[workspace.package]` in `Cargo.toml` is
   the release's (`0.2.0`, or `0.3.0-beta1`). Write its notes in
   `docs/releases/v<version>.md`. Merge both into `main`.
2. **A rehearsal** (optional): *Actions › Release › Run workflow* on `main`
   builds and packages everything as the tag will, and keeps the files as the
   run's artifacts. A pull request that changes the packaging is rehearsed so
   too.
3. **The tag.** `git tag -a v0.2.0 -m "shellwing 0.2.0" <commit> && git push origin v0.2.0`,
   on `main`'s commit with the version and the notes. The Release
   workflow checks that the tag is `Cargo.toml`'s version, builds the five
   desktop platforms, and makes a **draft** release with the files and
   `SHA256SUMS`.
4. **Check the draft**: install it on each system ([Checking a
   release](#checking-a-release)), and publish it.
5. **The list.** Add the release to `release.json` with the SHA-256 of the
   AppImages, the `.app.tar.gz` and the installer under `archives`, and of the
   `.deb` and `.rpm` files under `packages` (never `archives`: [updates.md](updates.md#the-list-of-releases)),
   all from `SHA256SUMS`, and merge it. Sign what is on `main`: pull first, as the tool signs the `release.json` it
   finds. Commit `release.signed.json`; `cargo test -p shellwing-update` fails if it
   does not carry `release.json` byte for byte. Then move the `release` branch
   ([updates.md › Publishing a release's list](updates.md#publishing-a-releases-list)).
   From then on the apps already out there offer it, within a few minutes: GitHub
   caches the file.

A release that went wrong is not deleted once the list names it: the next one
replaces it.

## Checking a release

CI tests the code, not the packages on a real desktop. Before a release, on a
rehearsal's artifacts or the draft's files, what only a real system shows:

**Windows** (a VM will do; it has a GPU driver good enough):

- The installer: `Get-AuthenticodeSignature` says what is expected, the file's
  version is the release's, and it adds a Start menu entry and an uninstall entry.
- From the desktop session, not over SSH: connect to a host with `ProxyCommand`
  set to `ssh.exe -W %h:%p …`. The shell comes up and **no console window shows**.
  Closing the app leaves no `ssh.exe` behind.
- A `Match … exec` that holds and one that does not.
- The uninstaller leaves no program folder and no uninstall entry.
- SmartScreen shows only for a file really downloaded and opened by hand.

**macOS** (a VM without Metal cannot run the app, which exits with no message; it
can check the signature):

- `spctl -a -t exec -vv shellwing.app` says `source=Notarized Developer ID`, the
  same with `-t open --context context:primary-signature` for the `.dmg`, and
  `xcrun stapler validate` passes on both.
- `LSMinimumSystemVersion` and `CFBundleShortVersionString` are right.

On a real Mac:

- Drag the app from the disk image to Applications and open it from the Finder:
  one question, *Open*, and no trip to System Settings.
- A `ProxyCommand` whose program is only in `/opt/homebrew/bin`: an app opened
  from the Finder does not have that on `PATH`.
- Touch ID opens the vault.

**Linux:**

- The AppImage runs (`--help`), the `.deb` names the version and installs under
  `/opt/shellwing`.
- X11 forwarding with `DISPLAY=:0`, on the desktop's X server. GNOME under Wayland
  has no SECURITY extension, so *Untrusted* says why it cannot, and *Trusted* works.
