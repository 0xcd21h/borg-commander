#!/bin/sh
# Installs what the process watcher's eBPF programs are built with on Linux: the nightly
# compiler that crates/borg-app/build.rs names, with the source of the core library, and the
# bpf-linker release below, checked against its SHA-256 before it runs.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
toolchain="$(sed -n 's/^ *const TOOLCHAIN: &str = "\(.*\)";/\1/p' "${root}/crates/borg-app/build.rs")"
linker_version="0.12.0"

if [ "$(uname -s)" != "Linux" ]; then
    echo "the eBPF programs are built on Linux only" >&2
    exit 1
fi
case "$(uname -m)" in
    x86_64) arch="x86_64"; digest="6789717c8cb6a2a5d920f3b6bfac2bd7efc00f6228ecd149108a99699a75c679" ;;
    aarch64 | arm64) arch="aarch64"; digest="5e201e69dafac7af98f9788f1c68edd5ccc8fa0805093f899bf02bd2947b3935" ;;
    *)
        echo "bpf-linker publishes no build for $(uname -m); run: cargo install bpf-linker --version ${linker_version} --locked" >&2
        exit 1
        ;;
esac
if [ -z "${toolchain}" ]; then
    echo "found no TOOLCHAIN in crates/borg-app/build.rs" >&2
    exit 1
fi

rustup toolchain install "${toolchain}" --profile minimal --component rust-src

name="bpf-linker-${arch}-unknown-linux-musl.tar.zst"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
curl -sSfL --proto '=https' -o "${work}/${name}" \
    "https://github.com/aya-rs/bpf-linker/releases/download/v${linker_version}/${name}"
echo "${digest}  ${work}/${name}" | sha256sum -c -
mkdir "${work}/unpacked"
zstd -dcq "${work}/${name}" | tar -xf - -C "${work}/unpacked" bpf-linker
bin="${CARGO_HOME:-${HOME}/.cargo}/bin"
mkdir -p "${bin}"
install -m 0755 "${work}/unpacked/bpf-linker" "${bin}/bpf-linker"
"${bin}/bpf-linker" --version
