#!/usr/bin/env bash
# Make the station bundle for a radio host without apt or with a different
# OpenSSL version. The bundle holds static musl binaries of
# musicindex-live-publisher and liquidsoap-now-playing, the two unit templates,
# the reference slot text and the AzuraCast runbook.
#
# The binaries link OpenSSL statically. Thus they need no libssl and no glibc
# version on the host. The CA certificates come from the host, for example
# /etc/ssl/certs/ca-certificates.crt.
#
# Requirements on the build computer: the Rust target x86_64-unknown-linux-musl,
# musl-gcc, make, perl, and the crate openssl-src in the Cargo registry.
#
# Usage: scripts/make-station-bundle.sh
# The bundle goes in target/station-bundle-REVISION/.
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo"

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    echo "The working tree has changes. Commit them first, so the binaries have a clean revision." >&2
    exit 1
fi
revision=$(git rev-parse --short HEAD)

openssl_src=$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -maxdepth 2 -type d -name 'openssl-src-300.*' | sort -V | tail -n 1)
if [ -z "$openssl_src" ]; then
    echo "No openssl-src crate is in the Cargo registry. Run 'cargo fetch' with the vendored OpenSSL source first." >&2
    exit 1
fi

work="$repo/target/station-bundle-openssl"
prefix="$work/prefix"
if [ ! -f "$prefix/lib/libssl.a" ]; then
    rm -rf "$work"
    mkdir -p "$work"
    cp -r "$openssl_src/openssl" "$work/src"
    (
        cd "$work/src"
        # The crate holds a reduced OpenSSL source. These options are the
        # options that the crate needs to configure it.
        CC="musl-gcc -fPIE" ./Configure linux-x86_64 no-shared no-module no-legacy \
            no-tests no-docs no-engine no-async no-afalgeng no-dso no-ui-console \
            -DOPENSSL_NO_SECURE_MEMORY --prefix="$prefix" --libdir=lib --openssldir=/etc/ssl
        make -j"$(nproc)" build_libs
        make install_dev
    )
fi

OPENSSL_DIR="$prefix" OPENSSL_STATIC=1 CC_x86_64_unknown_linux_musl=musl-gcc \
    cargo build --release --locked --target x86_64-unknown-linux-musl \
    -p musicindex-live-publisher -p liquidsoap-now-playing \
    --bin musicindex-live-publisher --bin liquidsoap-now-playing

out="$repo/target/station-bundle-$revision"
rm -rf "$out"
mkdir -p "$out"
release="$repo/target/x86_64-unknown-linux-musl/release"
install -m 0755 "$release/musicindex-live-publisher" "$release/liquidsoap-now-playing" "$out/"
install -m 0644 systemd/musicindex-live-publisher@.service systemd/liquidsoap-now-playing@.service "$out/"
install -m 0644 liquidsoap/musicindex-state.liq docs/runbooks/liquidsoap-now-playing-azuracast.md "$out/"
(cd "$out" && sha256sum -- * > SHA256SUMS)

"$out/musicindex-live-publisher" --version
echo "The bundle is in $out"
