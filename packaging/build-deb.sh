#!/bin/sh
# Builds the cadrat-tool .deb as a TEST BUILD.
#
# The version gets a "~test" suffix, which sorts before the plain release
# version, so a later release upgrades it. A test build has not passed the
# Phase 1 hardware checks (docs/hardware-test.md) and is not published as a
# release. Official releases are built on the oldest supported Ubuntu LTS
# (spec 04 §7), which this script does not check.
#
# Usage: packaging/build-deb.sh [TEST_NUMBER]
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

test_number=${1:-1}
version=$(cargo metadata --format-version 1 --no-deps \
    | sed -n 's/.*"name":"cadrat-tool","version":"\([^"]*\)".*/\1/p')
commit=$(git rev-parse --short=7 HEAD 2>/dev/null || echo unknown)
deb_version="${version}~test${test_number}+g${commit}"
# Uncommitted changes make the commit label wrong; say so in the version.
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    deb_version="${deb_version}.dirty"
fi

cargo run --quiet -p xtask -- dist
CADRAT_VERSION="$deb_version (test build)" \
    cargo deb -p cadrat-tool --deb-version "$deb_version" --output target/debian/

echo "TEST BUILD: cadrat-tool ${deb_version} (not hardware-verified, not a release)"
