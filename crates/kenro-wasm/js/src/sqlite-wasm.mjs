// Adapter for the official SQLite WASM build (@sqlite.org/sqlite-wasm).
// The primary host: full arity support, deterministic + innocuous flags
// (innocuous is what lets kenro functions run inside GeoPackage R-tree
// triggers under `PRAGMA trusted_schema=off`), Uint8Array blobs both ways,
// BigInt int64, and JS exceptions surfacing as SQL errors.

import { i64UnsupportedUdf, loadManifest, makeUdf, stubUdf } from "./core.mjs";

/**
 * Register every kenro function on an `sqlite3.oo1.DB`.
 *
 * @param {object} db - an sqlite3.oo1.DB instance
 * @param {object} wasm - the initialized kenro-wasm module
 */
export function registerKenro(db, wasm) {
  const manifest = loadManifest(wasm);
  const bigIntOk = typeof db.selectValue === "function"; // canonical builds are BigInt-enabled
  for (const entry of manifest.functions) {
    const udf =
      entry.uses_i64 && !bigIntOk
        ? i64UnsupportedUdf(entry, "this SQLite WASM build")
        : makeUdf(entry, wasm);
    // oo1 callback signature: (pCtx, ...args) — the context pointer is not
    // a SQL argument.
    db.createFunction(entry.sql_name, (_ctx, ...args) => udf(...args), {
      arity: entry.args.length,
      deterministic: true,
      innocuous: true,
    });
  }
  for (const stub of manifest.stubs) {
    const udf = stubUdf(stub);
    db.createFunction(stub.name, (_ctx, ..._args) => udf(), {
      arity: -1,
      deterministic: true,
      innocuous: true,
    });
  }
}
