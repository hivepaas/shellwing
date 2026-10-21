#!/bin/bash
# Makes the test data of crates/update with the signing tool itself: a key of
# each algorithm (test keys, thrown away: no release is signed with them), and
# release.json signed with them, as scripts/release_sign.sh signs a release.
#
#   crates/update/testdata/make.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tool="$here/../../../tools/releasesign"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
(cd "$tool" && go build -o "$tmp/releasesign" .)
"$tmp/releasesign" keygen -alg ed25519 -key-id test-ed -dir "$tmp"
"$tmp/releasesign" keygen -alg ml-dsa-65 -key-id test-ml -dir "$tmp"
"$tmp/releasesign" sign -key "$tmp/test-ed.key" -key "$tmp/test-ml.key" \
  -in "$here/release.json" -out "$here/release.signed.json"
cp "$tmp/test-ed.pub.pem" "$tmp/test-ml.pub.pem" "$here/"
