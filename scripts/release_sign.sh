#!/bin/bash
#
# Signs shellwing's list of releases (release.json) with the offline release
# keys into release.signed.json, the file the app reads (see docs/updates.md).
#
#   scripts/release_sign.sh /offline/2026-ed.key /offline/2026-ml.key
#
# The signing tool is not run from the working tree: it is built from
# tools/releasesign/main.go as it is at the commit in tools/releasesign/PIN (a
# commit that was reviewed), outside any module, with the toolchain and module
# downloads off. A change to the tool on a branch does not reach the keys until
# it is reviewed and the pin moves; an import from outside the standard library
# fails the build rather than being fetched.
#
# The envelope is then checked again without the tool: its payload must be
# release.json byte for byte, and each signature must verify with openssl
# against the public key openssl derives from the private key itself. A tool
# that signed something else, or with another key, passes its own check but not
# this one.
#
# Environment: IN (default release.json), OUT (default release.signed.json),
# RELEASESIGN_SHA (default: the commit in tools/releasesign/PIN).

set -euo pipefail

CONTEXT="shellwing-release-v1" # must match tools/releasesign and crates/update
ROOT="$(git rev-parse --show-toplevel)"
IN="${IN:-$ROOT/release.json}"
OUT="${OUT:-$ROOT/release.signed.json}"
PIN_FILE="$ROOT/tools/releasesign/PIN"

fail() {
  echo "release_sign: $*" >&2
  exit 1
}

SHA="${RELEASESIGN_SHA:-$(tr -d '[:space:]' <"$PIN_FILE" 2>/dev/null || true)}"
[[ "$SHA" =~ ^[0-9a-f]{40}$ ]] ||
  fail "no reviewed commit to build the tool from: put its full sha in tools/releasesign/PIN (or RELEASESIGN_SHA)"
(($# > 0)) || fail "usage: scripts/release_sign.sh <key-id>.key ... (one key of each algorithm)"
for key in "$@"; do
  [[ -f "$key" && "$(basename "$key")" == *.key ]] || fail "$key: not a key file named <key-id>.key"
done
[[ -f "$IN" ]] || fail "$IN not found"
git -C "$ROOT" cat-file -e "${SHA}^{commit}" 2>/dev/null || fail "commit $SHA is not in this clone (git fetch first)"

# OpenSSL 3.5 or later has ML-DSA and Ed25519ctx; LibreSSL (macOS's openssl) has neither.
OPENSSL=""
for candidate in openssl /opt/homebrew/opt/openssl@3/bin/openssl /usr/local/opt/openssl@3/bin/openssl; do
  if command -v "$candidate" >/dev/null 2>&1 &&
    "$candidate" version 2>/dev/null | grep -Eq '^OpenSSL (3\.([5-9]|[1-9][0-9])|[4-9])\.'; then
    OPENSSL="$candidate"
    break
  fi
done
[[ -n "$OPENSSL" ]] || fail "OpenSSL 3.5 or later is needed for the check on the tool (brew install openssl@3)"

if ! git -C "$ROOT" diff --quiet HEAD -- "$IN" 2>/dev/null; then
  echo "WARNING: $IN has changes not committed: what is signed is the file, not the commit." >&2
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Building releasesign from:"
git -C "$ROOT" log -1 --format='  %H%n  %an, %ad%n  %s' "$SHA"
git -C "$ROOT" show "${SHA}:tools/releasesign/main.go" >"$TMP/main.go" ||
  fail "tools/releasesign/main.go is not at $SHA"
(cd "$TMP" && env GOTOOLCHAIN=local GOFLAGS= GOWORK=off GOPROXY=off go build -trimpath -o releasesign main.go) ||
  fail "building the tool failed (it must import the standard library only)"

ARGS=()
for key in "$@"; do ARGS+=(-key "$key"); done
"$TMP/releasesign" sign "${ARGS[@]}" -in "$IN" -out "$OUT"

echo "Checking it again with $("$OPENSSL" version | cut -d' ' -f1-2):"
# The tool writes the payload, and each signature, on a line of its own.
payload="$(sed -n 's/^ *"payload": *"\([^"]*\)",*$/\1/p' "$OUT")"
[[ -n "$payload" ]] || fail "$OUT has no payload"
printf '%s' "$payload" | "$OPENSSL" base64 -d -A >"$TMP/payload"
cmp -s "$TMP/payload" "$IN" || fail "$OUT does not carry $IN byte for byte: do not publish it"
echo "  payload: $IN, byte for byte"
for key in "$@"; do
  id="$(basename "$key" .key)"
  # Its first line, read whole: cut short, openssl dies of a broken pipe.
  type="$("$OPENSSL" pkey -in "$key" -noout -text)"
  type="${type%%$'\n'*}"
  case "$type" in
  ED25519*) opts=(-pkeyopt instance:Ed25519ctx -pkeyopt "context-string:$CONTEXT") ;;
  ML-DSA-65*) opts=(-pkeyopt "context-string:$CONTEXT") ;;
  *) fail "$key: a key of an unexpected type: $type" ;;
  esac
  sig="$(sed -n "s/.*{\"keyId\":\"${id}\",\"alg\":\"[^\"]*\",\"sig\":\"\([^\"]*\)\"}.*/\1/p" "$OUT")"
  [[ -n "$sig" ]] || fail "$OUT has no signature by $id"
  printf '%s' "$sig" | "$OPENSSL" base64 -d -A >"$TMP/$id.sig"
  "$OPENSSL" pkey -in "$key" -pubout -out "$TMP/$id.pub.pem"
  printf '  %s (%s): ' "$id" "${type%% *}"
  "$OPENSSL" pkeyutl -verify -pubin -inkey "$TMP/$id.pub.pem" -rawin -in "$TMP/payload" \
    -sigfile "$TMP/$id.sig" "${opts[@]}" || fail "openssl does not accept the signature by $id: do not publish $OUT"
done
echo "  sha256: $("$OPENSSL" dgst -sha256 -r "$IN" | cut -d' ' -f1)"
echo "Done. Commit $(basename "$OUT") with $(basename "$IN"), then move the release branch to it."
