#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:-${HOME}/Downloads/instagram.apk}"
TARGET='Lcom/facebook/perf/background/BackgroundStartupDetector;->access$getBackgroundedCount$cp()I'
OUTPUT='/tmp/winedroid-instagram-static-field'
SOURCE='/tmp/winedroid-instagram-static-field.c'

cd "$ROOT"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cargo run -q -p winedroid-compiler --bin winedroid-aot -- \
  compile-apk "$APK" \
  --method "$TARGET" \
  --default-static-int 42 \
  --output "$OUTPUT" \
  --emit-c "$SOURCE" \
  --run

file "$OUTPUT"
test "$("$OUTPUT")" = '42'
