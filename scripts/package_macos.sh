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
# Signed ad hoc unless a Developer ID is given: then Gatekeeper asks the first
# time the app opens (Control-click > Open), and the updater accepts a new .app
# whose signature verifies, as it does for any app not signed by a team.
#
# Environment: BIN (default target/release/shellwing), ARCH (default the
# machine's: aarch64 or x86_64), DIST (default dist). To sign for people
# (docs/releasing.md):
#
#   MACOS_SIGN_IDENTITY         "Developer ID Application: Name (TEAMID)", in a
#                               keychain this user can use; none: ad hoc
#   MACOS_PROVISIONING_PROFILE  its Developer ID profile for the app, which
#                               allows the keychain entitlement that Touch ID
#                               or the Mac's password opens the vault with
#   APPLE_API_KEY_PATH, APPLE_API_KEY_ID, APPLE_API_ISSUER_ID
#                               an App Store Connect API key, to notarize

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

IDENTITY="${MACOS_SIGN_IDENTITY:-}"
if [ -z "$IDENTITY" ]; then
	codesign --force --sign - --timestamp=none "$APP"
else
	# The hardened runtime and a secure timestamp, as notarizing requires.
	SIGN=(codesign --force --sign "$IDENTITY" --options runtime --timestamp)
	if [ -n "${MACOS_PROVISIONING_PROFILE:-}" ]; then
		BUNDLE="$(plutil -extract CFBundleIdentifier raw "$APP/Contents/Info.plist")"
		TEAM="$(sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p' <<<"$IDENTITY")"
		[ -n "$TEAM" ] || { echo "no team id at the end of $IDENTITY" >&2; exit 1; }
		cp "$MACOS_PROVISIONING_PROFILE" "$APP/Contents/embedded.provisionprofile"
		sed -e "s/@TEAM@/$TEAM/g" -e "s/@BUNDLE@/$BUNDLE/g" \
			packaging/macos/shellwing.entitlements >"$WORK/shellwing.entitlements"
		SIGN+=(--entitlements "$WORK/shellwing.entitlements")
	else
		echo "warning: no MACOS_PROVISIONING_PROFILE: Touch ID will not open the vault" >&2
	fi
	"${SIGN[@]}" "$APP"
fi
codesign --verify --deep --strict "$APP"

# Apple checks what it is given and keeps a ticket for it, stapled here so
# Gatekeeper lets it open offline too.
notarize() {
	local file="$1" result status id
	result="$(xcrun notarytool submit "$file" --wait --output-format json \
		--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID")"
	status="$(plutil -extract status raw - <<<"$result" 2>/dev/null || true)"
	if [ "$status" != "Accepted" ]; then
		echo "notarizing $file: ${status:-no answer}" >&2
		id="$(plutil -extract id raw - <<<"$result" 2>/dev/null || true)"
		[ -z "$id" ] || xcrun notarytool log "$id" \
			--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID" >&2
		exit 1
	fi
}
# Stapling fetches the ticket from Apple's CloudKit, and a dropped connection
# there is no reason to lose a build that Apple has already accepted.
staple() {
	local try
	for try in 1 2 3 4 5; do
		if xcrun stapler staple "$1"; then
			return 0
		fi
		echo "stapling $1 failed (try $try of 5)" >&2
		[ "$try" -lt 5 ] && sleep 30
	done
	return 1
}
NOTARIZE=""
if [ -n "$IDENTITY" ] && [ -n "${APPLE_API_KEY_PATH:-}" ]; then
	NOTARIZE=1
elif [ -n "$IDENTITY" ]; then
	echo "warning: not notarized (no APPLE_API_KEY_PATH): Gatekeeper refuses it downloaded" >&2
fi

## Notarized, the app first: the disk image and the updater's tarball carry
## it with its ticket.
if [ -n "$NOTARIZE" ]; then
	ditto -c -k --keepParent "$APP" "$WORK/shellwing.zip"
	notarize "$WORK/shellwing.zip"
	staple "$APP"
	spctl --assess --type execute --verbose=2 "$APP"
fi

## For people: a disk image with the app and a link to /Applications
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$DIST/shellwing_${VERSION}_macos_${ARCH}.dmg"
hdiutil create -volname "shellwing $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG"

## The disk image signed, and notarized too.
if [ -n "$IDENTITY" ]; then
	codesign --force --sign "$IDENTITY" --timestamp "$DMG"
fi
if [ -n "$NOTARIZE" ]; then
	notarize "$DMG"
	staple "$DMG"
fi

## For the updater: the .app, tarred
tar -C "$WORK" -czf "$DIST/shellwing_${VERSION}_macos_${ARCH}.app.tar.gz" shellwing.app

ls -l "$DIST"
