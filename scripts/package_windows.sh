#!/bin/bash
#
# Packages a release build of shellwing for Windows into dist/: the installer
# the updater runs quietly over the installed app, and a zip of the program to
# run from a folder (see docs/releasing.md). Run from Git Bash or MSYS2, with
# NSIS 3 installed (or makensis on PATH; on Linux the nsis package's works too).
#
#   cargo build --release -p shellwing
#   scripts/package_windows.sh
#
#   dist/shellwing_0.1.0_windows_x86_64-setup.exe
#   dist/shellwing_0.1.0_windows_x86_64.zip
#
# Neither is signed (no Authenticode certificate yet): SmartScreen asks the
# first time the installer runs (More info > Run anyway).
#
# Environment: EXE (default target/release/shellwing.exe), ARCH (default
# x86_64), DIST (default dist), MAKENSIS (default: on PATH, else where NSIS
# installs itself).

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
ARCH="${ARCH:-x86_64}"
EXE="$(realpath "${EXE:-target/release/shellwing.exe}")"
DIST="${DIST:-dist}"
WORK="target/package-windows"

if [ -z "${MAKENSIS:-}" ]; then
	MAKENSIS="$(command -v makensis || true)"
	for dir in "/c/Program Files (x86)/NSIS" "/c/Program Files/NSIS"; do
		[ -n "$MAKENSIS" ] || { [ -x "$dir/makensis.exe" ] && MAKENSIS="$dir/makensis.exe"; } || true
	done
fi
[ -n "$MAKENSIS" ] || { echo "no makensis: install NSIS 3 (choco install nsis)" >&2; exit 1; }
[ -f "$EXE" ] || { echo "no build at $EXE: cargo build --release -p shellwing" >&2; exit 1; }
rm -rf "$WORK"
mkdir -p "$WORK" "$DIST"

# A path as the tool takes it: Windows' own under MSYS, as is elsewhere.
native() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi; }

## The installer
SETUP="$DIST/shellwing_${VERSION}_windows_${ARCH}-setup.exe"
FILEVERSION="${VERSION%%-*}.0"
# The paths are Windows' already: MSYS is not to rewrite them.
MSYS2_ARG_CONV_EXCL='*' "$MAKENSIS" -V3 \
	"-DVERSION=$VERSION" "-DFILEVERSION=$FILEVERSION" \
	"-DEXE=$(native "$EXE")" "-DOUT=$(native "$ROOT/$SETUP")" \
	"$(native "$ROOT/packaging/windows/installer.nsi")"

## The zip: the program and its license, in a folder of their own
NAME="shellwing-${VERSION}-windows-${ARCH}"
mkdir -p "$WORK/$NAME"
cp "$EXE" "$WORK/$NAME/shellwing.exe"
cp LICENSE "$WORK/$NAME/LICENSE.txt"
ZIP="$ROOT/$DIST/shellwing_${VERSION}_windows_${ARCH}.zip"
rm -f "$ZIP"
if command -v 7z >/dev/null; then
	(cd "$WORK" && 7z a -tzip -bso0 "$(native "$ZIP")" "$NAME")
else
	(cd "$WORK" && zip -qr "$ZIP" "$NAME")
fi

ls -l "$DIST"
