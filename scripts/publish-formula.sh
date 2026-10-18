#!/bin/sh
set -eu

formula="$1"
tap="$2"
tag="$3"
work="$(mktemp -d)"

git clone --depth 1 "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/${tap}.git" "${work}"
mkdir -p "${work}/Formula"
cp "${formula}" "${work}/Formula/borg-commander.rb"
cd "${work}"
git config user.name "borg-commander release"
git config user.email "releases@users.noreply.github.com"
git add Formula/borg-commander.rb
git commit -m "borg-commander ${tag}"
git push origin HEAD
