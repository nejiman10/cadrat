#!/bin/sh
# Builds the cadrat-tool .deb as a RELEASE BUILD (spec 04 §7).
#
# Run it on Ubuntu 22.04, the oldest supported LTS, so the binary needs no
# newer glibc than 2.35. docs/packaging.md shows how to prepare that
# environment in a container. Run it in a fresh clone: the script refuses
# an existing target/ (whose binaries may come from a newer system), other
# systems, an uncommitted tree and a binary that needs a newer glibc.
#
# Usage: packaging/build-release.sh
set -eu

# The oldest supported release and its glibc.
min_ubuntu=22.04
max_glibc=2.35

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

fail() {
    echo "build-release: $*" >&2
    exit 1
}

. /etc/os-release
[ "${ID:-}" = ubuntu ] && [ "${VERSION_ID:-}" = "$min_ubuntu" ] \
    || fail "run this on Ubuntu $min_ubuntu (found ${PRETTY_NAME:-unknown}); see docs/packaging.md"
[ ! -e target ] || fail "target/ exists; run this in a fresh clone"
[ -z "$(git status --porcelain)" ] || fail "the working tree has uncommitted changes"

version=$(cargo metadata --format-version 1 --no-deps \
    | sed -n 's/.*"name":"cadrat-tool","version":"\([^"]*\)".*/\1/p')

cargo run --locked --quiet -p xtask -- dist
# xz rather than zstd, which older dpkg cannot read.
# CADRAT_VERSION must not carry a test-build label into a release.
deb=$(env -u CADRAT_VERSION \
    cargo deb --locked -p cadrat-tool --deb-version "$version" --compress-type xz \
    --output target/debian/ | tail -n 1)

binary=target/release/cadrat-tool
needed=$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n 1)
[ "$(printf '%s\n%s\n' "$needed" "$max_glibc" | sort -V | tail -n 1)" = "$max_glibc" ] \
    || fail "$binary needs glibc $needed, newer than $max_glibc"

[ -f "$deb" ] || fail "cargo deb did not report the package it built"
ar t "$deb" | grep -qx 'data.tar.xz' || fail "$deb is not xz-compressed"

echo "RELEASE BUILD: cadrat-tool ${version} (commit $(git rev-parse --short=7 HEAD), glibc >= ${needed})"
sha256sum "$deb"
