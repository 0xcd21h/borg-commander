#!/bin/sh
set -eu

version="$1"
dist="$2"
template="packaging/homebrew/borg-commander.rb.in"

sha() {
    awk '{ print $1 }' "${dist}/borg-${version}-$1.tar.gz.sha256"
}

sed \
    -e "s/@VERSION@/${version}/g" \
    -e "s/@SHA_MAC_ARM@/$(sha aarch64-apple-darwin)/" \
    -e "s/@SHA_MAC_INTEL@/$(sha x86_64-apple-darwin)/" \
    -e "s/@SHA_LINUX_ARM@/$(sha aarch64-unknown-linux-musl)/" \
    -e "s/@SHA_LINUX_INTEL@/$(sha x86_64-unknown-linux-musl)/" \
    "${template}"
