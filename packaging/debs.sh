# Shared by build-deb.sh and build-release.sh (sourced, not run): builds the
# three packages of one version (spec implementation §7).
#
# make_debs VERSION writes cadrat-common, cadrat-tool and cadratd to
# target/debian/ and prints their paths. The caller sets CADRAT_VERSION (the
# version --version shows) or leaves it unset.

# The crate that carries each package's [package.metadata.deb].
deb_crates="cadrat-hold-open cadrat-tool cadratd"

make_debs() {
    version=$1
    cargo run --locked --quiet -p xtask -- dist >&2
    # cadratctl has no package of its own: the cadratd package carries it.
    cargo build --locked --release \
        -p cadrat-hold-open -p cadrat-tool -p cadratd -p cadratctl >&2
    for crate in $deb_crates; do
        # xz rather than zstd, which older dpkg cannot read.
        deb=$(cargo deb --locked --no-build -p "$crate" --deb-version "$version" \
            --compress-type xz --output target/debian/ | tail -n 1)
        [ -f "$deb" ] || { echo "cargo deb did not report the package it built" >&2; return 1; }
        pin_version "$deb" "$version"
        echo "$deb"
    done
}

# cargo-deb has no variable for the package version, so the dependencies on
# cadrat-common say "(= @VERSION@)" in Cargo.toml; this writes the version in.
pin_version() {
    dir=$(mktemp -d)
    dpkg-deb -R "$1" "$dir"
    if grep -q '@VERSION@' "$dir/DEBIAN/control"; then
        sed -i "s/@VERSION@/$2/g" "$dir/DEBIAN/control"
        dpkg-deb --root-owner-group -Zxz -b "$dir" "$1" >/dev/null
    fi
    rm -rf "$dir"
}
