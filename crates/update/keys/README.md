# Release signing keys

The public keys shellwing believes its list of releases (`release.signed.json`)
from. Every `<key-id>.pub.pem` here is built into the app (`build.rs`); its key
id is its file name. The list must carry a valid **Ed25519** (Ed25519ctx) and a
valid **ML-DSA-65** signature, each by a key here, under the context
`shellwing-release-v1`.

A build with no key here cannot tell of updates: it does not look, and
*Settings › About* says so.

## Making the keys

On the offline signing machine, with the tool built as `scripts/release_sign.sh`
builds it (from `tools/releasesign/main.go` at a reviewed commit):

```sh
releasesign keygen -alg ed25519   -key-id 2026-ed -dir /offline
releasesign keygen -alg ml-dsa-65 -key-id 2026-ml -dir /offline
```

Copy only the two `.pub.pem` files here. The `.key` files never leave that
machine; back them up as you would any secret that cannot be made again: an app
trusts the keys it was built with, and a lost key is replaced only by shipping
apps that trust another.

## Rotating a key

1. Make the new key, copy its `.pub.pem` here beside the old one, and release.
2. Sign with both the old and the new key for as long as apps built before (1)
   are about.
3. Then take the old key out of here, and stop signing with it.
