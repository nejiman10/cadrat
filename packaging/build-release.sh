#!/bin/sh
# Builds the three .deb packages (cadrat-common, cadrat-tool, cadratd) as a
# RELEASE BUILD (spec implementation §7).
#
# Run it on Ubuntu 22.04, the oldest supported LTS, so the binaries need no
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
. packaging/debs.sh

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

# CADRAT_VERSION must not carry a test-build label into a release.
unset CADRAT_VERSION
debs=$(make_debs "$version")

needed=0
for binary in cadrat-hold-open cadrat-tool cadratd cadratctl; do
    binary=target/release/$binary
    glibc=$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n 1)
    [ "$(printf '%s\n%s\n' "$glibc" "$max_glibc" | sort -V | tail -n 1)" = "$max_glibc" ] \
        || fail "$binary needs glibc $glibc, newer than $max_glibc"
    needed=$(printf '%s\n%s\n' "$glibc" "$needed" | sort -V | tail -n 1)
done

for deb in $debs; do
    ar t "$deb" | grep -qx 'data.tar.xz' || fail "$deb is not xz-compressed"
done

echo "RELEASE BUILD: cadrat ${version} (commit $(git rev-parse --short=7 HEAD), glibc >= ${needed})"
# shellcheck disable=SC2086
sha256sum $debs
