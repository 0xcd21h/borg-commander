#!/bin/sh
set -eu

tag="$1"
crate="$(awk -F'"' '/^version[[:space:]]*=/ { print $2; exit }' Cargo.toml)"
if [ "v${crate}" != "${tag}" ]; then
    echo "tag ${tag} does not match the crate version ${crate} in Cargo.toml" >&2
    exit 1
fi
echo "releasing ${crate}"
