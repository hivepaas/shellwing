#!/bin/bash
#
# Packages a release build of shellwing for Linux into dist/: the AppImage the
# updater replaces itself with, a .deb, and a tar.gz of the binary (see
# docs/releasing.md).
#
#   cargo build --release -p shellwing
#   scripts/package_linux.sh
#
#   dist/shellwing_0.1.0_linux_x86_64.AppImage
#   dist/shellwing_0.1.0_linux_x86_64.tar.gz
#   dist/shellwing_0.1.0_amd64.deb
#
# appimagetool and the AppImage runtime are downloaded at the versions pinned
# below and checked against their SHA-256 before they run.
#
# Environment: BIN (default target/release/shellwing), ARCH (default the
# machine's: x86_64 or aarch64), DIST (default dist).

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
ARCH="${ARCH:-$(uname -m)}"
BIN="${BIN:-target/release/shellwing}"
DIST="${DIST:-dist}"
WORK="target/package-linux"

case "$ARCH" in
x86_64) DEB_ARCH=amd64 ;;
aarch64) DEB_ARCH=arm64 ;;
*) echo "no Linux package for $ARCH" >&2; exit 1 ;;
esac

# appimagetool 1.9.1 and the type2 runtime of 2025-11-08.
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-$ARCH.AppImage"
RUNTIME_URL="https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-$ARCH"
case "$ARCH" in
x86_64)
	APPIMAGETOOL_SHA=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
	RUNTIME_SHA=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
	;;
aarch64)
	APPIMAGETOOL_SHA=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
	RUNTIME_SHA=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444
	;;
esac

[ -x "$BIN" ] || { echo "no build at $BIN: cargo build --release -p shellwing" >&2; exit 1; }
rm -rf "$WORK"
mkdir -p "$WORK" "$DIST"

# What every package holds, under a prefix: the binary (stripped), the
# launcher and the icons.
install_tree() {
	local prefix="$1"
	install -Dm755 "$BIN" "$prefix/bin/shellwing"
	strip "$prefix/bin/shellwing"
	install -Dm644 packaging/linux/shellwing.desktop "$prefix/share/applications/shellwing.desktop"
	install -Dm644 packaging/icons/shellwing.svg "$prefix/share/icons/hicolor/scalable/apps/shellwing.svg"
	for size in 16 32 48 64 128 256 512; do
		install -Dm644 "packaging/icons/png/shellwing-$size.png" \
			"$prefix/share/icons/hicolor/${size}x${size}/apps/shellwing.png"
	done
}

# A download, kept only when its SHA-256 is the one expected.
fetch() {
	local url="$1" sha="$2" out="$3"
	curl -fsSL --retry 3 -o "$out" "$url"
	echo "$sha  $out" | sha256sum -c --quiet - || { echo "$url: not the file expected" >&2; exit 1; }
}

## The AppImage
APPDIR="$WORK/shellwing.AppDir"
install_tree "$APPDIR/usr"
cp packaging/linux/shellwing.desktop "$APPDIR/shellwing.desktop"
cp packaging/icons/png/shellwing-256.png "$APPDIR/shellwing.png"
ln -s usr/share/icons/hicolor/256x256/apps/shellwing.png "$APPDIR/.DirIcon"
cat >"$APPDIR/AppRun" <<'EOF'
#!/bin/sh
exec "$(dirname "$(readlink -f "$0")")/usr/bin/shellwing" "$@"
EOF
chmod 755 "$APPDIR/AppRun"

fetch "$APPIMAGETOOL_URL" "$APPIMAGETOOL_SHA" "$WORK/appimagetool"
fetch "$RUNTIME_URL" "$RUNTIME_SHA" "$WORK/runtime"
chmod 755 "$WORK/appimagetool"
APPIMAGE="$DIST/shellwing_${VERSION}_linux_${ARCH}.AppImage"
# Extracted rather than mounted: CI machines have no FUSE.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" VERSION="$VERSION" \
	"$WORK/appimagetool" --no-appstream --runtime-file "$WORK/runtime" "$APPDIR" "$APPIMAGE"

## The tar.gz: the binary, its launcher and icons, as /usr/local would have them
NAME="shellwing-${VERSION}-linux-${ARCH}"
install_tree "$WORK/$NAME"
cp LICENSE README.md "$WORK/$NAME/"
tar -C "$WORK" --owner=0 --group=0 -czf "$DIST/shellwing_${VERSION}_linux_${ARCH}.tar.gz" "$NAME"

## The .deb
# The binary in /opt, linked from /usr/bin: the updater takes one under /usr for
# a distribution's package, updated by its package manager, while this one is
# updated by downloading the next .deb.
DEB="$WORK/deb"
install_tree "$DEB/usr"
mkdir -p "$DEB/opt/shellwing"
mv "$DEB/usr/bin/shellwing" "$DEB/opt/shellwing/shellwing"
ln -s /opt/shellwing/shellwing "$DEB/usr/bin/shellwing"
install -Dm644 LICENSE "$DEB/usr/share/doc/shellwing/copyright"
# The libraries it links, as the system names their packages.
mkdir -p "$WORK/shlibs/debian"
printf 'Source: shellwing\n\nPackage: shellwing\nArchitecture: any\n' >"$WORK/shlibs/debian/control"
DEPENDS="$(cd "$WORK/shlibs" && dpkg-shlibdeps -O "$ROOT/$DEB/opt/shellwing/shellwing" 2>/dev/null | sed -n 's/^shlibs:Depends=//p')"
mkdir -p "$DEB/DEBIAN"
cat >"$DEB/DEBIAN/control" <<EOF
Package: shellwing
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: HivePaaS <support@hivepaas.com>
Installed-Size: $(du -sk --exclude=DEBIAN "$DEB" | cut -f1)
Depends: $DEPENDS
Recommends: libwayland-client0, libxcb1, libx11-xcb1, libvulkan1, mesa-vulkan-drivers
Section: net
Priority: optional
Homepage: https://shellwing.app
Description: Fast, native SSH client
 Tabs, split panes, SFTP, port forwarding and an encrypted vault that syncs
 between devices, in a GPU-rendered terminal built with Rust and Zig.
EOF
chmod -R go-w "$DEB"
dpkg-deb --root-owner-group --build "$DEB" "$DIST/shellwing_${VERSION}_${DEB_ARCH}.deb"

ls -l "$DIST"
