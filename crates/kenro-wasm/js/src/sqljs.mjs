// Adapter for sql.js. Three documented host limitations are worked around
// here:
//
// 1. `db.create_function` keys its registry by name only; registering a
//    second arity would free the first wrapper's function-table slot while
//    SQLite still points at it. The shim renames the existing registry key
//    before re-registering — `db.close()` cleans up via `Object.values()`,
//    so key names are opaque to sql.js. (Pin sql.js ^1.13; the smoke test
//    is the tripwire for internal changes.)
// 2. sql.js has no int64 path (everything becomes a double), so the h3_*
//    functions register as loud errors instead of silently corrupting cell
//    ids > 2^53.
// 3. sql.js drops the message of thrown Error objects (its UDF wrapper
//    passes the Error where a string is expected, yielding an empty SQL
//    error). Throwing a string primitive survives — so every UDF is wrapped
//    to rethrow `String(message)`.
//
// Note also that the stock sql.js build ships without SQLite's R-tree
// module, so GeoPackage spatial-index maintenance is not possible on this
// host regardless of kenro (documented in docs/wasm.md).

import { i64UnsupportedUdf, loadManifest, makeUdf, stubUdf } from "./core.mjs";

/** sql.js keeps string throws but empties Error throws — rethrow strings. */
function stringThrows(fn) {
  return (...args) => {
    try {
      return fn(...args);
    } catch (e) {
      throw String(e?.message ?? e);
    }
  };
}

/**
 * Register every kenro function on a sql.js `Database`.
 *
 * @param {object} db - a sql.js Database instance
 * @param {object} wasm - the initialized kenro-wasm module
 */
export function registerKenro(db, wasm) {
  const manifest = loadManifest(wasm);
  const register = (name, fn, arity) => {
    // sql.js derives the SQL arity from Function.prototype.length.
    Object.defineProperty(fn, "length", { value: arity });
    if (db.functions && db.functions[name] !== undefined) {
      db.functions[`${name}/kenro:${arity}`] = db.functions[name];
      delete db.functions[name];
    }
    db.create_function(name, fn);
  };
  for (const entry of manifest.functions) {
    const udf = entry.uses_i64
      ? i64UnsupportedUdf(entry, "sql.js")
      : makeUdf(entry, wasm);
    register(entry.sql_name, stringThrows(udf), entry.args.length);
  }
  for (const stub of manifest.stubs) {
    // No variadic registration in sql.js: register each concrete arity.
    for (const arity of stub.arities) {
      register(stub.name, stringThrows(stubUdf(stub)), arity);
    }
  }
}
