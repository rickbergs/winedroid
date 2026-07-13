#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:?informe o APK do SukiSU}"
OUTPUT="/tmp/winedroid-sukisu-recursive-check.elf"
SOURCE="/tmp/winedroid-sukisu-recursive-check.c"
LOG="/tmp/winedroid-sukisu-recursive-check.log"

cd "$ROOT"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cargo run -q -p winedroid-compiler --bin winedroid-sukisu-recursive -- \
  "$APK" \
  --output "$OUTPUT" \
  --emit-c "$SOURCE" \
  --max-depth 3 \
  --max-methods 32 \
  --run 2>&1 | tee "$LOG"

file "$OUTPUT"
readelf -h "$OUTPUT" | sed -n '1,22p'
file "$OUTPUT" | grep -Eq 'ELF 64-bit.*x86-64'
grep -Fq 'WineDroid: SukiSU recursive lifecycle completed' "$LOG"

LINKED="$(sed -n 's/^Métodos internos ligados: //p' "$LOG" | tail -n 1)"
test -n "$LINKED"
test "$LINKED" -gt 4
