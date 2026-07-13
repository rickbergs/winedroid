#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT="/tmp/winedroid-native-check"
SOURCE="/tmp/winedroid-native-check.c"

cd "$ROOT"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p winedroid-compiler --bin winedroid-aot -- \
  demo --output "$OUTPUT" --emit-c "$SOURCE" --run

file "$OUTPUT"
readelf -h "$OUTPUT" | sed -n '1,22p'
test "$("$OUTPUT")" = "42"
