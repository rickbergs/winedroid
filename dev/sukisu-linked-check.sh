#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:?informe o APK do SukiSU}"
OUTPUT="/tmp/winedroid-sukisu-linked-check"
SOURCE="/tmp/winedroid-sukisu-linked-check.c"
LOG="/tmp/winedroid-sukisu-linked-check.log"

cd "$ROOT"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cargo run -q -p winedroid-compiler --bin winedroid-sukisu-link -- \
  "$APK" \
  --output "$OUTPUT" \
  --emit-c "$SOURCE" \
  --run 2>&1 | tee "$LOG"

file "$OUTPUT"
readelf -h "$OUTPUT" | sed -n '1,22p'

file "$OUTPUT" | grep -Eq 'ELF 64-bit.*x86-64'
grep -Fq 'Métodos ligados: 4' "$LOG"
grep -Fq 'WineDroid: SukiSU linked lifecycle completed' "$LOG"
test "$(grep -c '^int main(void)' "$SOURCE")" -eq 1
