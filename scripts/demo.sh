#!/bin/sh
set -eu

out="${1:-docs/demo.gif}"
cargo build --release -p borg-app
cargo run --release -p borg-testkit --example demo -- target/release/borg "$out"
