import type { KenroWasm } from "./core.js";

/**
 * Register every kenro function on an `sqlite3.oo1.DB`.
 *
 * The host types are deliberately structural: `@sqlite.org/sqlite-wasm` ships
 * its own, and requiring them here would make this package depend on them.
 *
 * @param db an `sqlite3.oo1.DB` instance
 * @param wasm the initialized kenro-wasm module
 * @param sqlite3 the sqlite3 namespace — **required for the aggregates**,
 *   whose per-group state is keyed by `sqlite3_aggregate_context`. Scalar
 *   functions register without it.
 */
export function registerKenro(db: object, wasm: KenroWasm, sqlite3?: object): void;
