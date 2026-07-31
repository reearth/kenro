#!/usr/bin/env bash
# Build the wasm module the Go binding embeds.
#
# Target is wasm32-wasip1, not wasm32-unknown-unknown: proj4rs declares a
# `[target.wasm32-unknown-unknown.dependencies]` block on wasm-bindgen/js-sys,
# so an unknown-unknown build imports `__wbindgen_placeholder__` and cannot be
# instantiated by a non-JS host like wazero. wasip1 also links wasi-libc's
# math instead of compiling in the whole `libm` crate, which happens to be
# smaller — and it gives us panic messages on stderr.
#
# Usage: scripts/build-go-wasm.sh [feature-tier]   (default: full)
set -euo pipefail

cd "$(dirname "$0")/.."

TIER="${1:-full}"
OUT="go/internal/wasmbin/kenro.wasm"

case "$TIER" in
minimal) FEATURES=(--no-default-features) ;;
standard) FEATURES=() ;;
full) FEATURES=(--features full) ;;
*)
    echo "unknown tier: $TIER (expected minimal | standard | full)" >&2
    exit 2
    ;;
esac

rustup target add wasm32-wasip1 >/dev/null 2>&1 || true

echo "building kenro-abi ($TIER) for wasm32-wasip1"
cargo build -p kenro-abi --release --target wasm32-wasip1 "${FEATURES[@]}"

mkdir -p "$(dirname "$OUT")"
BUILT="target/wasm32-wasip1/release/kenro_abi.wasm"

if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -Oz \
        --enable-bulk-memory \
        --enable-nontrapping-float-to-int \
        --enable-sign-ext \
        --enable-mutable-globals \
        "$BUILT" -o "$OUT"
else
    echo "wasm-opt not found — shipping the unoptimized module" >&2
    cp "$BUILT" "$OUT"
fi

printf 'wrote %s (%s bytes, tier=%s)\n' "$OUT" "$(wc -c <"$OUT" | tr -d ' ')" "$TIER"
