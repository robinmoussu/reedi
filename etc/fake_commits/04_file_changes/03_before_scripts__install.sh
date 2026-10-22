#!/bin/sh
set -eu

prefix="${PREFIX:-/usr/local}"

make build
install -m 755 target/release/reedi "$prefix/bin/reedi"
echo "reedi installed in $prefix/bin"