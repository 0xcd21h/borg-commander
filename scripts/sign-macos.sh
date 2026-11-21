#!/bin/sh
set -eu

binary="$1"
entitlements="$(dirname "$0")/../packaging/macos/borg.entitlements"

codesign --force --sign - --options runtime --entitlements "${entitlements}" "${binary}"
codesign --verify --strict "${binary}"
codesign --display --verbose "${binary}" 2>&1 | grep -q 'flags=.*runtime' || {
    echo "${binary} is not signed with the hardened runtime" >&2
    exit 1
}

foreign="$(otool -L "${binary}" | tail -n +2 | awk '{print $1}' | grep -v -e '^/usr/lib/' -e '^/System/Library/' || true)"
if [ -n "${foreign}" ]; then
    echo "${binary} loads libraries from outside the system:" >&2
    echo "${foreign}" >&2
    exit 1
fi
echo "signed ${binary} with the hardened runtime; it loads only system libraries"
