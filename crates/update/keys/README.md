# Release signing keys

The public keys shellwing believes its list of releases (`release.signed.json`)
from. They are **HivePaaS's release keys**, copied from
`hivepaas/hivepaas_app/pkg/releasesig/releasekeys/`: one key ceremony, one offline
machine, for HivePaaS, its CLI and shellwing. Each signs under a context of its
own (`shellwing-release-v1` here), so a list signed for one is never taken for
another's.

The file name without `.pub.pem` is the key id `release.signed.json` refers to;
`src/lib.rs` (`trusted_keys`) names each file. The list must carry a valid
**ed25519** (Ed25519ctx) and a valid **ML-DSA-65** signature, each by a key here.

## Rotating a key

An app keeps trusting the keys it was built with, so, as for HivePaaS:

1. Copy the new key's `.pub.pem` here beside the old one, add it to
   `trusted_keys`, and release.
2. Sign with both old and new keys for as long as apps built before (1) are about.
3. Then drop the old key here, and stop signing with it.
