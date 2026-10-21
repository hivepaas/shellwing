#!/bin/bash
#
# Packages a release build of shellwing for macOS into dist/: shellwing.app,
# signed ad hoc, tarred for the updater, and in a disk image for people (see
# docs/releasing.md).
#
#   cargo build --release -p shellwing
#   scripts/package_macos.sh
#
#   dist/shellwing_0.1.0_macos_aarch64.app.tar.gz
#   dist/shellwing_0.1.0_macos_aarch64.dmg
#
# The signature is ad hoc (no Developer ID yet): Gatekeeper asks the first time
# the app opens (Control-click > Open), and the updater accepts a new .app whose
# signature verifies, as it does for any app not signed by a team.
#
# Environment: BIN (default target/release/shellwing), ARCH (default the
# machine's: aarch64 or x86_64), DIST (default dist).

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
ARCH="${ARCH:-$(uname -m | sed 's/arm64/aarch64/')}"
BIN="${BIN:-target/release/shellwing}"
DIST="${DIST:-dist}"
WORK="target/package-macos"

[ -x "$BIN" ] || { echo "no build at $BIN: cargo build --release -p shellwing" >&2; exit 1; }
rm -rf "$WORK"
mkdir -p "$WORK" "$DIST"

## The icon, from the PNGs
ICONSET="$WORK/shellwing.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
	cp "packaging/icons/png/shellwing-$size.png" "$ICONSET/icon_${size}x${size}.png"
	cp "packaging/icons/png/shellwing-$((size * 2)).png" "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns -o "$WORK/shellwing.icns" "$ICONSET"

## shellwing.app
APP="$WORK/shellwing.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/shellwing"
strip -x "$APP/Contents/MacOS/shellwing"
cp "$WORK/shellwing.icns" "$APP/Contents/Resources/shellwing.icns"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --deep --strict "$APP"

## For the updater: the .app, tarred
tar -C "$WORK" -czf "$DIST/shellwing_${VERSION}_macos_${ARCH}.app.tar.gz" shellwing.app

## For people: a disk image with the app and a link to /Applications
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "shellwing $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov \
	"$DIST/shellwing_${VERSION}_macos_${ARCH}.dmg"

ls -l "$DIST"
