#!/bin/sh
# Packages borg-relay for a Linux target, beside borg's own archive.
set -eu

version="$1"
target="$2"
out="$3"
name="borg-relay-${version}-${target}"
work="$(mktemp -d)"

mkdir -p "${work}/${name}" "${out}"
cp "target/${target}/release/borg-relay" LICENSE-MIT LICENSE-APACHE "${work}/${name}/"
tar -C "${work}" -czf "${out}/${name}.tar.gz" "${name}"
(cd "${out}" && shasum -a 256 "${name}.tar.gz" > "${name}.tar.gz.sha256")
rm -rf "${work}"
echo "packaged ${out}/${name}.tar.gz"
