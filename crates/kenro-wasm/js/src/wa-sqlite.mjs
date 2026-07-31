// Adapter for wa-sqlite: the most faithful host API — true variadic
// registration, DETERMINISTIC and INNOCUOUS flags, exact value types,
// BigInt int64. UDF exceptions propagate to the statement caller with
// their message intact (verified against wa-sqlite 1.x), so no result_error
// plumbing is needed.

import { loadManifest, makeAggregate, makeUdf, stubUdf } from "./core.mjs";

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
  for (const entry of manifest.aggregates ?? []) {
    const aggregate = makeAggregate(entry, wasm);
    // Per-instance state keyed by the xStep context pointer, which is stable
    // across rows and groups. xFinal, however, arrives with a DIFFERENT
    // pointer (SQLite finalizes through a stack-allocated context) and
    // wa-sqlite does not expose sqlite3_aggregate_context — so finals are
    // matched FIFO: SQLite finalizes aggregates in the same order their
    // first xStep ran (memory-cell order), verified empirically on 1.x.
    const states = new Map();
    sqlite3.create_function(
      db,
      entry.sql_name,
      entry.args.length,
      flags,
      0,
      null,
      (context, values) => {
        if (!states.has(context)) states.set(context, aggregate.start());
        aggregate.step(
          states.get(context),
          Array.from(values, (v) => sqlite3.value(v)),
        );
      },
      (context) => {
        let acc;
        if (states.has(context)) {
          acc = states.get(context);
          states.delete(context);
        } else {
          const oldest = states.keys().next();
          if (oldest.done) {
            acc = aggregate.start(); // zero rows stepped
          } else {
            acc = states.get(oldest.value);
            states.delete(oldest.value);
          }
        }
        sqlite3.result(context, aggregate.finish(acc));
      },
    );
  }
  for (const stub of manifest.stubs) {
    sqlite3.create_function(db, stub.name, -1, flags, 0, wrap(stubUdf(stub)));
  }
}
