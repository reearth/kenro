#!/usr/bin/env bash
# Copy the wasm-pack output into vendor/ (the Workers Vitest pool cannot
# resolve a .wasm module from outside the project root). Prerequisite:
#   wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg -- --features full
set -euo pipefail
cd "$(dirname "$0")"

if [ ! -f ../js/pkg/kenro_wasm_bg.wasm ]; then
  echo "crates/kenro-wasm/js/pkg not built — run wasm-pack first (see README.md)" >&2
  exit 1
fi

mkdir -p vendor
cp ../js/pkg/kenro_wasm.js ../js/pkg/kenro_wasm.d.ts ../js/pkg/kenro_wasm_bg.wasm vendor/
echo "vendor/ updated from ../js/pkg" >&2
