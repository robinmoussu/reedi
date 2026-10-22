#!/bin/sh
set -eu

prefix="${PREFIX:-/usr/local}"

cargo build --release
sha256sum --check reedi.sha256
install -m 755 target/release/reedi "$prefix/bin/reedi"
echo "reedi installed in $prefix/bin"