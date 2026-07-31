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

import {
  i64UnsupportedUdf,
  loadManifest,
  makeAggregate,
  makeUdf,
  stubUdf,
} from "./core.mjs";

/**
 * Locate sql.js's internal name → function-table-pointer registry. Its
 * property name is minified in dist builds (e.g. `Sa` in 1.14), so it is
 * detected by planting a sentinel registration and finding the object that
 * contains it. Without the registry, arity overloads would free live
 * function-table slots — better to fail loudly at registration time.
 */
function findUdfRegistry(db) {
  const sentinel = "kenro_registry_probe";
  db.create_function(sentinel, () => null);
  const prop = Object.getOwnPropertyNames(db).find((p) => {
    const value = db[p];
    return (
      value !== null &&
      typeof value === "object" &&
      Object.prototype.hasOwnProperty.call(value, sentinel)
    );
  });
  if (prop === undefined) {
    throw new Error(
      "kenro: cannot locate sql.js's UDF registry in this build — arity " +
        "overloads would corrupt the function table. Pin sql.js ^1.14 or " +
        "use @sqlite.org/sqlite-wasm / wa-sqlite.",
    );
  }
  return db[prop];
}

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
  const registry = findUdfRegistry(db);
  let counter = 0;
  const register = (name, fn, arity) => {
    // sql.js derives the SQL arity from Function.prototype.length.
    Object.defineProperty(fn, "length", { value: arity });
    if (registry[name] !== undefined) {
      registry[`${name}/kenro:${counter++}`] = registry[name];
      delete registry[name];
    }
    db.create_function(name, fn);
  };
  for (const entry of manifest.functions) {
    const udf = entry.uses_i64
      ? i64UnsupportedUdf(entry, "sql.js")
      : makeUdf(entry, wasm);
    register(entry.sql_name, stringThrows(udf), entry.args.length);
  }
  for (const entry of manifest.aggregates ?? []) {
    // sql.js stores aggregate wrappers under BOTH `name` and
    // `name__finalize` registry keys — rename both before re-registering.
    for (const key of [entry.sql_name, `${entry.sql_name}__finalize`]) {
      if (registry[key] !== undefined) {
        registry[`${key}/kenro:${counter++}`] = registry[key];
        delete registry[key];
      }
    }
    if (entry.uses_i64) {
      const loud = stringThrows(i64UnsupportedUdf(entry, "sql.js"));
      Object.defineProperty(loud, "length", { value: entry.args.length });
      db.create_function(entry.sql_name, loud);
      continue;
    }
    const aggregate = makeAggregate(entry, wasm);
    const step = stringThrows((state, ...args) => {
      aggregate.step(state, args);
      return state;
    });
    // sql.js derives the aggregate's SQL arity from step.length - 1.
    Object.defineProperty(step, "length", { value: entry.args.length + 1 });
    db.create_aggregate(entry.sql_name, {
      init: () => aggregate.start(),
      step,
      finalize: stringThrows((state) => aggregate.finish(state)),
    });
  }
  for (const stub of manifest.stubs) {
    // No variadic registration in sql.js: register each concrete arity.
    for (const arity of stub.arities) {
      register(stub.name, stringThrows(stubUdf(stub)), arity);
    }
  }
}
