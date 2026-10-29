#!/usr/bin/env bash
# Prints a Zig lib directory that compiles for iOS, for ZIG_LIB_DIR, or
# nothing when the Zig on PATH needs none.
#
#   scripts/zig_lib_ios.sh <dir>
#
# Zig 0.17.0's standard library does not compile for iOS
# (scripts/zig-0.17.0-ios.patch says why). For that version, <dir> becomes a
# copy of its lib directory with the fix applied, and its path is printed; the
# Zig installed is left alone. Any other version, or a lib directory that has
# the fix already (ZIG_LIB_DIR set by an earlier run), prints nothing.
set -euo pipefail

dir="${1:?usage: scripts/zig_lib_ios.sh <dir>}"
patch_file="$(cd "$(dirname "$0")" && pwd)/zig-0.17.0-ios.patch"
# The line the fix changes, as it reads once fixed.
fixed='if (is_darwin and process.can_spawn) if (t.spawnDarwin('

zig_env=$(zig env)
version=$(sed -n 's/^ *\.version = "\(.*\)",$/\1/p' <<<"$zig_env")
lib_dir=$(sed -n 's/^ *\.lib_dir = "\(.*\)",$/\1/p' <<<"$zig_env")
if [[ "$version" != 0.17.0 ]]; then
	exit 0
fi
if [[ -z "$lib_dir" ]]; then
	echo "zig env names no lib_dir" >&2
	exit 1
fi
if grep -qF "$fixed" "$lib_dir/std/Io/Threaded.zig"; then
	exit 0
fi

# Made once; the stamp says from which lib directory.
if [[ "$(cat "$dir/.patched-from" 2>/dev/null)" != "$lib_dir" ]]; then
	rm -rf "$dir"
	mkdir -p "$(dirname "$dir")"
	cp -R "$lib_dir" "$dir"
	# The patch's paths start at Zig's source root, lib/ included. Never
	# asking (-t), and saying nothing on stdout, which is for the path.
	patch -s -N -t -p2 -d "$dir" <"$patch_file" >&2
	if ! grep -qF "$fixed" "$dir/std/Io/Threaded.zig"; then
		echo "patching Zig's lib directory in $dir failed" >&2
		exit 1
	fi
	echo "$lib_dir" >"$dir/.patched-from"
fi
echo "$dir"
