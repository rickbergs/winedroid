#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:?informe o APK do SukiSU}"
OUTPUT="/tmp/winedroid-sukisu-generic-abi.elf"
SOURCE="/tmp/winedroid-sukisu-generic-abi.c"
LOG="/tmp/winedroid-sukisu-generic-abi.log"

cd "$ROOT"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cargo run -q -p winedroid-compiler --bin winedroid-sukisu-recursive -- \
  "$APK" \
  --output "$OUTPUT" \
  --emit-c "$SOURCE" \
  --max-depth 3 \
  --max-methods 96 \
  --run 2>&1 | tee "$LOG"

file "$OUTPUT" | grep -Eq 'ELF 64-bit.*x86-64'
grep -Fq 'WineDroid: SukiSU recursive lifecycle completed' "$LOG"
! grep -Fq 'ABI atual não cobre' "$LOG"

LINKED="$(sed -n 's/^Métodos internos ligados: //p' "$LOG" | tail -n 1)"
test -n "$LINKED"
test "$LINKED" -gt 23

grep -Fq 'uint32_t argc, const wd_value *args' "$SOURCE"
grep -Fq 'argc=%u' "$SOURCE"
