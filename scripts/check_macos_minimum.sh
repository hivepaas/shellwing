#!/usr/bin/env bash
# Checks that libghostty-vt, as a macOS build linked it, was compiled for
# MACOSX_DEPLOYMENT_TARGET or an older macOS: the app says it runs there,
# and code built for a newer macOS may call what an older one lacks. Zig
# builds for the build machine's macOS unless told otherwise.
#
#   scripts/check_macos_minimum.sh [profile]    the build's profile, release by default
set -euo pipefail

cd "$(dirname "$0")/.."
target_version="${MACOSX_DEPLOYMENT_TARGET:?MACOSX_DEPLOYMENT_TARGET is not set}"
profile="${1:-release}"

shopt -s nullglob
libs=(target/"$profile"/build/libghostty-vt-sys-*/out/ghostty-install/lib/libghostty-vt.a)
if [[ ${#libs[@]} -eq 0 ]]; then
	echo "no libghostty-vt.a under target/$profile: build first" >&2
	exit 1
fi

# Whether dotted version $1 is newer than $2.
newer() {
	local IFS=.
	local -a a=($1) b=($2)
	local i x y
	for i in 0 1 2; do
		x=${a[i]:-0}
		y=${b[i]:-0}
		if ((x > y)); then return 0; fi
		if ((x < y)); then return 1; fi
	done
	return 1
}

# Each object's minimum: LC_BUILD_VERSION's minos, or an older object's
# LC_VERSION_MIN_MACOSX.
mins=$(otool -l "${libs[@]}" | awk '
	/cmd LC_BUILD_VERSION/ { build = 1; next }
	build && $1 == "minos" { print $2; build = 0 }
	/cmd LC_VERSION_MIN_MACOSX/ { min = 1; next }
	min && $1 == "version" { print $2; min = 0 }
')
if [[ -z "$mins" ]]; then
	echo "found no macOS version in ${libs[*]}" >&2
	exit 1
fi

echo "libghostty-vt's objects need macOS (count, version):"
sort <<<"$mins" | uniq -c
bad=0
for v in $(sort -u <<<"$mins"); do
	if newer "$v" "$target_version"; then
		echo "::error::libghostty-vt needs macOS $v, newer than MACOSX_DEPLOYMENT_TARGET $target_version" >&2
		bad=1
	fi
done
exit $bad
