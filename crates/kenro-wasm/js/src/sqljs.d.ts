import type { KenroWasm } from "./core.js";

/**
 * Register every kenro function on a sql.js `Database`.
 *
 * sql.js has no R-tree module and no 64-bit integers: the h3 functions
 * register as bodies that throw rather than silently truncating cell ids.
 *
 * @param db a sql.js `Database` instance
 * @param wasm the initialized kenro-wasm module
 */
export function registerKenro(db: object, wasm: KenroWasm): void;
