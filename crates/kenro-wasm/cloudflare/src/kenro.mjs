// kenro-wasm inside a Cloudflare Worker.
//
// wasm-pack's `--target web` output fetches its .wasm relative to
// import.meta.url, which no Worker can do. Workers instead hand you the
// compiled WebAssembly.Module as an import, so `initSync` is the whole
// story — one synchronous init per isolate, no top-level await, no network.
// Imported from `vendor/`, not `../../js/pkg/`: the Workers Vitest pool
// cannot resolve a .wasm module from outside the project root, so
// `npm run sync-wasm` copies the wasm-pack output in first.
import wasmModule from "../vendor/kenro_wasm_bg.wasm";
import * as wasm from "../vendor/kenro_wasm.js";

let ready = false;

/** The kenro-wasm exports, initialized on first use. */
export function kenro() {
  if (!ready) {
    wasm.initSync({ module: wasmModule });
    ready = true;
  }
  return wasm;
}
