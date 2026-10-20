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
with its signatures beside it in one file, as HivePaaS and its CLI publish theirs.
`release.json`:

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

The list is signed with **HivePaaS's release keys**, offline, by HivePaaS's
`make release-sign`, under the context `shellwing-release-v1`. The app requires a
valid ed25519 (Ed25519ctx) and a valid ML-DSA-65 signature by the keys built in
(`crates/update/keys/`); a signature for another context (HivePaaS's release, its
CLI's) does not verify here.

```sh
# In the hivepaas repository, on the signing machine:
make release-sign KEYS="/offline/2026_ed.key /offline/2026_ml.key" CONTEXT=shellwing \
  IN=../../../rust/shellwing/release.json OUT=../../../rust/shellwing/release.signed.json
```

HivePaaS's tool takes `CONTEXT=release` and `CONTEXT=cli` today; `shellwing` is to
be added to it (and its pin moved) before the first release.

To publish: commit `release.json` and `release.signed.json`, check the envelope
(`releasesign verify`), and move the `release` branch to that commit. Before the
branch exists the app finds no list, and says it is up to date.

## Checking it

- `cargo test -p shellwing-update` checks the format against a list Go signed
  (`crates/update/testdata/gen.go` makes it, with test keys of its own).
- `cargo test -p shellwing-update -- --ignored` reads the HivePaaS CLI's list from
  GitHub, signed with the same keys under its context: it opens under that one,
  and never as shellwing's.
