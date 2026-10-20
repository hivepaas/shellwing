# Updates

shellwing tells its user when a newer version is out (*Settings › About*, a dot
on the Settings button, an entry of the palette). It looks once a day while
`[updates] check` is on (the default), and when asked (*Check now*). Installing
what is offered is the user's, for now: the app opens the release's page, or, when
something else installed it (Homebrew, Scoop, Flatpak, Snap, a system package, a
store), says what updates it.

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
stable, a SHA-256 that is not one. `archives` is for the app installing a release
itself, later; nothing reads it yet.

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
