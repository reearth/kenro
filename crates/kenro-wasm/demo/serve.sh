#!/usr/bin/env bash
# Assemble the demo into demo/dist (same layout the Pages workflow deploys)
# and serve it locally. Prerequisite:
#   wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg
set -euo pipefail
cd "$(dirname "$0")"

rm -rf dist
mkdir -p dist/pkg dist/adapters
cp index.html demo.mjs dist/
cp ../js/pkg/kenro_wasm.js ../js/pkg/kenro_wasm_bg.wasm dist/pkg/
cp ../js/src/*.mjs dist/adapters/

echo "serving http://localhost:${1:-8000}" >&2
python3 -m http.server -d dist "${1:-8000}"
