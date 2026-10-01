#!/bin/sh
# Builds the three .deb packages (cadrat-common, cadrat-tool, cadratd) as a
# TEST BUILD.
#
# The version gets a "~test" suffix, which sorts before the plain release
# version, so a later release upgrades it. A test build has not passed the
# hardware checks (docs/hardware-test.md) and is not published as a release.
# Releases are built with packaging/build-release.sh on Ubuntu 22.04 (spec
# implementation §7).
#
# Usage: packaging/build-deb.sh [TEST_NUMBER]
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
. packaging/debs.sh

test_number=${1:-1}
version=$(cargo metadata --format-version 1 --no-deps \
    | sed -n 's/.*"name":"cadrat-tool","version":"\([^"]*\)".*/\1/p')
commit=$(git rev-parse --short=7 HEAD 2>/dev/null || echo unknown)
deb_version="${version}~test${test_number}+g${commit}"
# Uncommitted changes make the commit label wrong; say so in the version.
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    deb_version="${deb_version}.dirty"
fi

CADRAT_VERSION="$deb_version (test build)"
export CADRAT_VERSION
make_debs "$deb_version"

echo "TEST BUILD: cadrat ${deb_version} (not hardware-verified, not a release)"
