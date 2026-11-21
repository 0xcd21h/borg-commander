#!/bin/sh
set -eu

cask="$1"
tap="$2"
tag="$3"
work="$(mktemp -d)"

git clone --depth 1 "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/${tap}.git" "${work}"
mkdir -p "${work}/Casks"
cp "${cask}" "${work}/Casks/borg-commander.rb"
printf '{\n  "borg-commander": "borg-commander"\n}\n' > "${work}/tap_migrations.json"
cd "${work}"
git config user.name "borg-commander release"
git config user.email "releases@users.noreply.github.com"
git rm --quiet --ignore-unmatch Formula/borg-commander.rb
git add Casks/borg-commander.rb tap_migrations.json
if git diff --cached --quiet; then
    echo "borg-commander ${tag} is already published"
    exit 0
fi
git commit -m "borg-commander ${tag}"
git push origin HEAD
