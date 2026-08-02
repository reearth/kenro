import type { KenroWasm } from "./core.js";

/**
 * Register every kenro function on a wa-sqlite database handle.
 *
 * @param sqlite3 the object returned by `SQLite.Factory(module)`
 * @param db a database handle from `sqlite3.open_v2`
 * @param wasm the initialized kenro-wasm module
 */
export function registerKenro(sqlite3: object, db: number, wasm: KenroWasm): void;
