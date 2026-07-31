// Adapter for wa-sqlite: the most faithful host API — true variadic
// registration, DETERMINISTIC and INNOCUOUS flags, exact value types,
// BigInt int64. UDF exceptions propagate to the statement caller with
// their message intact (verified against wa-sqlite 1.x), so no result_error
// plumbing is needed.

import { loadManifest, makeUdf, stubUdf } from "./core.mjs";

// Stable SQLite ABI constants (kept literal so this adapter has no runtime
// dependency on the wa-sqlite package itself).
const SQLITE_UTF8 = 1;
const SQLITE_DETERMINISTIC = 0x800;
const SQLITE_INNOCUOUS = 0x200000;

/**
 * Register every kenro function on a wa-sqlite database handle.
 *
 * @param {object} sqlite3 - the object returned by `SQLite.Factory(module)`
 * @param {number} db - a database handle from `sqlite3.open_v2`
 * @param {object} wasm - the initialized kenro-wasm module
 */
export function registerKenro(sqlite3, db, wasm) {
  const manifest = loadManifest(wasm);
  const flags = SQLITE_UTF8 | SQLITE_DETERMINISTIC | SQLITE_INNOCUOUS;
  // `values` is a Uint32Array of value pointers — TypedArray.map would
  // coerce the extracted JS values back to numbers, hence Array.from.
  const wrap = (fn) => (context, values) => {
    sqlite3.result(context, fn(...Array.from(values, (v) => sqlite3.value(v))));
  };
  for (const entry of manifest.functions) {
    sqlite3.create_function(
      db,
      entry.sql_name,
      entry.args.length,
      flags,
      0,
      wrap(makeUdf(entry, wasm)),
    );
  }
  for (const stub of manifest.stubs) {
    sqlite3.create_function(db, stub.name, -1, flags, 0, wrap(stubUdf(stub)));
  }
}
