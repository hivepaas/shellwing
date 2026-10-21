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

and `SHA256SUMS`. The updater installs the AppImage, the `.app.tar.gz` and the
installer; the others are for people.

- **Linux** builds on Ubuntu 22.04, so the packages run on glibc 2.35 and newer.
  The binary links only libc and xkbcommon; Wayland, X11 and Vulkan are loaded
  when it starts. The `.deb` puts it in `/opt/shellwing` (linked from
  `/usr/bin`): under `/usr` the updater would take it for a distribution's own
  package and leave it to apt.
- **macOS**: `shellwing.app`, signed ad hoc. With no Developer ID, Gatekeeper
  asks the first time it opens (*Open Anyway* in *System Settings › Privacy &
  Security*; Control-click › Open on macOS 14 and older). The updater accepts a new `.app`
  whose signature verifies, as it does while the app is signed by no team.
- **Windows**: an NSIS installer, for the user, into
  `%LOCALAPPDATA%\Programs\shellwing` (where the updater looks for it), with a
  Start menu entry and an uninstaller. It is not signed: SmartScreen asks the
  first time (More info › Run anyway).

`make package` makes them for the computer it runs on, into `dist/`.

## Making one

1. **The version.** `version` under `[workspace.package]` in `Cargo.toml` is
   the release's (`0.2.0`, or `0.3.0-beta1`). Write its notes in
   `docs/releases/v<version>.md`. Merge both into `main`.
2. **A rehearsal** (optional): *Actions › Release › Run workflow* on `main`
   builds and packages everything as the tag will, and keeps the files as the
   run's artifacts. A pull request that changes the packaging is rehearsed so
   too.
3. **The tag.** `git tag -s v0.2.0 && git push origin v0.2.0`. The Release
   workflow checks that the tag is `Cargo.toml`'s version, builds the five
   platforms, and makes a **draft** release with the files and `SHA256SUMS`.
4. **Check the draft**: install it on each system, and publish it.
5. **The list.** Add the release to `release.json` with the SHA-256 of the
   AppImages, the `.app.tar.gz` and the installer (from `SHA256SUMS`), sign it
   on the signing machine, and move the `release` branch
   ([updates.md › Publishing a release's list](updates.md#publishing-a-releases-list)).
   From then on the apps already out there offer it.

A release that went wrong is not deleted once the list names it: the next one
replaces it.
