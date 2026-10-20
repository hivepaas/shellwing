# Updates

shellwing tells its user when a newer version is out (*Settings › About*, a dot
on the Settings button, an entry of the palette). It looks once a day while
`[updates] check` is on (the default), and when asked (*Check now*). Nothing is
downloaded or installed until the user says so (see [Installing](#installing)).

## Channels

A version is **stable** (`v0.2.0`) or a **beta** (`v0.3.0-beta2`); the app's
channel is its own version's.

- On stable, the app is offered newer stable releases only, never a beta.
- On a beta, it is offered the newer of the newest beta and the newest stable: a
  release supersedes the betas before it (`v0.3.0` after `v0.3.0-beta2`), and from
  there the app is on stable.

## The list of releases

What the app reads is `release.signed.json` on the repository's `release` branch:

```
https://raw.githubusercontent.com/hivepaas/shellwing/release/release.signed.json
```

It is `release.json` (the readable copy, reviewed in the repository), base64,
with its hash and its signatures beside it in one file, so that a CDN caching
files one by one can never serve a new list with an old signature. `release.json`:

```json
{
  "channels": { "stable": "v0.2.0", "beta": "v0.3.0-beta2" },
  "releases": {
    "v0.2.0": {
      "releaseDate": "2026-11-02",
      "archives": {
        "linux_x86_64": { "file": "shellwing_0.2.0_linux_x86_64.AppImage", "sha256": "…" }
      }
    },
    "v0.3.0-beta2": { "releaseDate": "2026-11-20", "archives": {} }
  }
}
```

The app refuses a list that does not hold together: a version not written
`vX.Y.Z` or `vX.Y.Z-betaN`, a channel naming a release it does not list, a beta on
stable, a SHA-256 that is not one, a file not named as its platform's is
(below).

## Installing

What the app does with a release depends on how it was installed, as told from
where it runs:

| installed as | the app |
|---|---|
| an AppImage (`$APPIMAGE`) | downloads the release's AppImage and puts it in place of this one |
| `Shellwing.app` on macOS | downloads the `.app` (tarred), checks it with `codesign` (valid, signed by the team that signed this one), and swaps the two |
| by its installer on Windows (`…\AppData\Local\Programs\shellwing\`, `…\Program Files\shellwing\`) | downloads the installer and runs it, quiet (`/S`), once the app has quit |
| by Homebrew, Scoop, Flatpak, Snap, a system package, a store | says what updates it (`brew upgrade shellwing`, …) |
| by hand (a binary in a folder) | opens the release's page |

A release has one file for each platform, named so (`v0.2.0`):

| platform | file |
|---|---|
| `linux_x86_64`, `linux_aarch64` | `shellwing_0.2.0_linux_x86_64.AppImage` |
| `macos_aarch64`, `macos_x86_64` | `shellwing_0.2.0_macos_aarch64.app.tar.gz` |
| `windows_x86_64`, `windows_aarch64` | `shellwing_0.2.0_windows_x86_64-setup.exe` |

They are the release's assets on GitHub
(`https://github.com/hivepaas/shellwing/releases/download/v0.2.0/<file>`); the
list names each one's SHA-256, and the app keeps a download only when its hash is
that one. The signed list is read again, and checked again, before each download.

*Download and install* downloads the file to the cache folder
(`~/.cache/shellwing/updates/` and the like), and says the release is ready. It is
installed as the app quits: at *Restart now* (asked first while connections are
open, as quitting is), which starts the new one, or whenever the user quits. A
release downloaded by an earlier run is still ready at the next start; once the
app running is that version or newer, its file goes.

## Signing

The list is signed twice, with **Ed25519** (Ed25519ctx) and **ML-DSA-65**, under
the context `shellwing-release-v1`; the app requires both, by the keys built into
it (`crates/update/keys/`, whose README says how the keys are made and rotated).

The tool is `tools/releasesign/main.go`: one file, the Go standard library only.
It is never run from the working tree: `scripts/release_sign.sh` builds it from
the reviewed commit in `tools/releasesign/PIN`, with the network off, signs, and
then checks the result again with OpenSSL 3.5 or later (the payload byte for byte,
and each signature against the public key OpenSSL derives from the private key).

```sh
# On the signing machine, in a clone of the repository:
scripts/release_sign.sh /offline/2026-ed.key /offline/2026-ml.key
```

Moving `PIN` to a newer commit is how a change to the tool reaches the keys: only
after it is reviewed.

## Publishing a release's list

1. Add the release to `release.json` (and its channel), commit it.
2. Sign it (above); commit `release.signed.json` with it.
3. Move the `release` branch to that commit: from then on the apps see it.

Before the `release` branch exists the app finds no list, and says it is up to
date.

## Checking it

- `cd tools/releasesign && go test` checks the tool: what it signs verifies, one
  algorithm alone is refused, a key is never written over.
- `cargo test -p shellwing-update` checks the app's side against a list the tool
  signed (`crates/update/testdata/make.sh` makes it, with test keys thrown away).
