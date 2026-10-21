#!/bin/bash
#
# Tries the updater on this machine, as a user sees it (Linux, as an AppImage):
# a release v99.0.0, whose AppImage is this very build, in a list signed with
# keys made for the try (thrown away), served from 127.0.0.1.
#
#   scripts/update_try.sh
#
# shellwing is built with the try's keys and addresses (at build time only:
# SHELLWING_RELEASE_KEYS, SHELLWING_UPDATE_LIST, SHELLWING_UPDATE_FILES; a build
# without them has the release keys and GitHub's addresses), and run as the
# AppImage $TRY_DIR/shellwing.AppImage. In Settings › About: Check now, Download
# and install, then Restart now (or quit): the AppImage is replaced and, after a
# Restart, started again.
#
# It runs with your settings: what the updater remembers (state/updates.json) is
# put back as it was when you press Enter at the end. Building again without
# the try's variables gives the usual build back.
#
# Environment: TRY_DIR (default: a new temporary folder), PORT (default 8737),
# FEATURES (default: --no-default-features --features wayland).

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
DIR="${TRY_DIR:-$(mktemp -d)}"
PORT="${PORT:-8737}"
read -r -a FEATURES <<<"${FEATURES:---no-default-features --features wayland}"
VERSION=v99.0.0
export PATH="$HOME/.cargo/bin:$PATH"

case "$(uname -m)" in
  x86_64) PLATFORM=linux_x86_64 ;;
  aarch64) PLATFORM=linux_aarch64 ;;
  *) echo "update_try: no release is made for $(uname -m)" >&2; exit 1 ;;
esac
[[ "$(uname -s)" == Linux ]] || { echo "update_try: Linux only" >&2; exit 1; }

CONFIG="${SHELLWING_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/shellwing}"
STATE="$CONFIG/state/updates.json"
mkdir -p "$DIR/keys" "$DIR/pub" "$DIR/www/$VERSION" "$DIR/cache"
[[ -f "$STATE" ]] && cp "$STATE" "$DIR/updates.json.before"

server=""
cleanup() {
  [[ -n "$server" ]] && kill "$server" 2>/dev/null || true
}
trap cleanup EXIT

echo "== keys for the try, in $DIR/keys"
(cd "$ROOT/tools/releasesign" && go build -o "$DIR/releasesign" .)
"$DIR/releasesign" keygen -alg ed25519 -key-id try-ed -dir "$DIR/keys" >/dev/null
"$DIR/releasesign" keygen -alg ml-dsa-65 -key-id try-ml -dir "$DIR/keys" >/dev/null
cp "$DIR/keys/"*.pub.pem "$DIR/pub/"

echo "== shellwing, built with them"
SHELLWING_RELEASE_KEYS="$DIR/pub" \
  SHELLWING_UPDATE_LIST="http://127.0.0.1:$PORT/release.signed.json" \
  SHELLWING_UPDATE_FILES="http://127.0.0.1:$PORT" \
  cargo build -p shellwing "${FEATURES[@]}" --manifest-path "$ROOT/Cargo.toml"
BIN="$ROOT/target/debug/shellwing"

echo "== the release $VERSION, and its signed list"
FILE="shellwing_${VERSION#v}_${PLATFORM}.AppImage"
# Stripped, and told apart from the one installed by a line at its end (which
# the system's loader does not read).
strip -o "$DIR/www/$VERSION/$FILE" "$BIN"
echo "shellwing $VERSION, for the try" >>"$DIR/www/$VERSION/$FILE"
SHA="$(sha256sum "$DIR/www/$VERSION/$FILE" | cut -d' ' -f1)"
cat >"$DIR/release.json" <<JSON
{
  "channels": { "stable": "$VERSION" },
  "releases": {
    "$VERSION": {
      "releaseDate": "$(date +%F)",
      "archives": { "$PLATFORM": { "file": "$FILE", "sha256": "$SHA" } }
    }
  }
}
JSON
"$DIR/releasesign" sign -key "$DIR/keys/try-ed.key" -key "$DIR/keys/try-ml.key" \
  -in "$DIR/release.json" -out "$DIR/www/release.signed.json"

python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$DIR/www" >"$DIR/server.log" 2>&1 &
server=$!

echo "== shellwing, as $DIR/shellwing.AppImage"
strip -o "$DIR/shellwing.AppImage" "$BIN"
APPIMAGE="$DIR/shellwing.AppImage" XDG_CACHE_HOME="$DIR/cache" "$DIR/shellwing.AppImage" || true

echo
echo "The AppImage is now $(cmp -s "$DIR/shellwing.AppImage" "$DIR/www/$VERSION/$FILE" && echo "the release's" || echo "still the one built")."
read -r -p "Press Enter when done trying (an app started again keeps running; quit it first): " _
if [[ -f "$DIR/updates.json.before" ]]; then
  cp "$DIR/updates.json.before" "$STATE"
else
  rm -f "$STATE"
fi
echo "Put back $STATE. The try's files are in $DIR."
