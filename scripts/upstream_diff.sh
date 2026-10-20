#!/bin/sh
# What shellwing changed in a crate it took in: the crate as crates.io serves
# it, against ours (see each UPSTREAM.md).
#
#   scripts/upstream_diff.sh gpui-pre-linux 0.3.9 vendor/gpui-pre-linux
#   scripts/upstream_diff.sh zorite-editor 0.11.2 crates/note-editor
set -eu
name=$1
version=$2
ours=$3
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sSfL "https://static.crates.io/crates/$name/$name-$version.crate" | tar -xz -C "$tmp"
diff -ru "$tmp/$name-$version/src" "$ours/src" || true
