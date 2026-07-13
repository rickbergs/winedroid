#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:?informe o APK do SukiSU}"
OUTPUT="/tmp/winedroid-object-check"

cd "$ROOT"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cargo run -q -p winedroid-compiler --bin winedroid-aot -- \
  bootstrap-demo --output "$OUTPUT" --run

test "$("$OUTPUT")" = "42"

cargo run -q -p winedroid-compiler --bin winedroid-aot -- \
  sukisu-frontier "$APK" \
  --output-dir /tmp/winedroid-sukisu-bootstrap
