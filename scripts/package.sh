#!/bin/sh
set -eu

version="$1"
target="$2"
out="$3"
name="borg-${version}-${target}"
work="$(mktemp -d)"

mkdir -p "${work}/${name}" "${out}"
cp "target/${target}/release/borg" README.md LICENSE-MIT LICENSE-APACHE "${work}/${name}/"
cp -R fonts "${work}/${name}/fonts"
cp docs/SYNTAX-LICENSES.md "${work}/${name}/"
tar -C "${work}" -czf "${out}/${name}.tar.gz" "${name}"
(cd "${out}" && shasum -a 256 "${name}.tar.gz" > "${name}.tar.gz.sha256")
rm -rf "${work}"
echo "packaged ${out}/${name}.tar.gz"
