#!/bin/sh
set -eu

repo="0xcd21h/borg-commander"
dest="${BORG_INSTALL_DIR:-${HOME}/.local/bin}"

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target="aarch64-apple-darwin" ;;
    Darwin-x86_64) target="x86_64-apple-darwin" ;;
    Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-musl" ;;
    Linux-x86_64) target="x86_64-unknown-linux-musl" ;;
    *) echo "no prebuilt borg for $(uname -s) $(uname -m); try: cargo install --locked --git https://github.com/${repo} borg-app" >&2; exit 1 ;;
esac

tag="$(curl -fsSL "https://api.github.com/repos/${repo}/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
if [ -z "${tag}" ]; then
    echo "could not find the latest release of ${repo}" >&2
    exit 1
fi

version="${tag#v}"
name="borg-${version}-${target}"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

echo "Downloading borg ${version} for ${target}…"
curl -fsSL -o "${work}/${name}.tar.gz" "https://github.com/${repo}/releases/download/${tag}/${name}.tar.gz"
curl -fsSL -o "${work}/${name}.tar.gz.sha256" "https://github.com/${repo}/releases/download/${tag}/${name}.tar.gz.sha256"
# Linux ships sha256sum and macOS ships shasum; both read the same checksum file.
if command -v sha256sum >/dev/null 2>&1; then
    verify="sha256sum -c"
elif command -v shasum >/dev/null 2>&1; then
    verify="shasum -a 256 -c"
else
    echo "cannot verify the download: install sha256sum (coreutils) or shasum (perl)" >&2
    exit 1
fi
(cd "${work}" && ${verify} "${name}.tar.gz.sha256" >/dev/null)

tar -xzf "${work}/${name}.tar.gz" -C "${work}"
mkdir -p "${dest}"
install -m 0755 "${work}/${name}/borg" "${dest}/borg"
echo "Installed borg ${version} to ${dest}/borg"
if [ -d "${work}/${name}/fonts" ]; then
    data="${BORG_DATA_DIR:-${XDG_DATA_HOME:-${HOME}/.local/share}/borg-commander}"
    mkdir -p "${data}"
    cp -R "${work}/${name}/fonts" "${data}/"
    echo "Optional Borg Mono fonts saved to ${data}/fonts; install and select them in your terminal."
fi
case ":${PATH}:" in
    *":${dest}:"*) ;;
    *) echo "Add ${dest} to your PATH to run borg." ;;
esac
