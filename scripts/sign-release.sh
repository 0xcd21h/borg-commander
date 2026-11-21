#!/bin/sh
# Signs every checksum file in a folder with the Ed25519 release key in RELEASE_SIGNING_KEY
# (a PEM private key). Each signature is the 64 raw bytes, saved as <checksum file>.sig.
set -eu

dist="$1"
source="crates/borg-update/src/signature.rs"

if [ -z "${RELEASE_SIGNING_KEY:-}" ]; then
    echo "RELEASE_SIGNING_KEY is not set, so the release cannot be signed" >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
(umask 077 && printf '%s\n' "${RELEASE_SIGNING_KEY}" > "${work}/key.pem")
openssl pkey -in "${work}/key.pem" -pubout -out "${work}/public.pem"

public="$(openssl pkey -pubin -in "${work}/public.pem" -outform DER | tail -c 32 | od -An -v -tx1 | tr -d ' \n')"
built_in="$(grep -o '"[0-9a-fA-F]\{64\}"' "${source}" | tr -d '"' | tr 'A-F' 'a-f' || true)"
if ! printf '%s\n' "${built_in}" | grep -qx "${public}"; then
    echo "RELEASE_PUBLIC_KEYS in ${source} does not hold the public half of the signing key" >&2
    echo "  built in:    ${built_in:-(none)}" >&2
    echo "  signing key: ${public}" >&2
    exit 1
fi

signed=0
for checksum in "${dist}"/*.sha256; do
    [ -f "${checksum}" ] || continue
    openssl pkeyutl -sign -inkey "${work}/key.pem" -rawin -in "${checksum}" -out "${checksum}.sig"
    openssl pkeyutl -verify -pubin -inkey "${work}/public.pem" -rawin -in "${checksum}" -sigfile "${checksum}.sig" > /dev/null
    signed=$((signed + 1))
done

if [ "${signed}" -eq 0 ]; then
    echo "no checksum files in ${dist}" >&2
    exit 1
fi
echo "signed ${signed} checksum files"
