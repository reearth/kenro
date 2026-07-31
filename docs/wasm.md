# kenro in the browser (kenro-wasm)

Browser SQLite builds are Emscripten C builds that cannot load a native
extension — but they all expose JS-level user-defined-function registration.
kenro-wasm is therefore kenro's SQLite-free pure core compiled to
`wasm32-unknown-unknown` (no SQLite inside), plus one small adapter per host
that wires the exports in as SQL functions. The function catalog is a single
machine-readable manifest shared with the rusqlite binding
(`kenro::functions::manifest`, consistency-tested in CI), so the hosts
cannot drift.

## Size

Measured on the release build (`wasm-pack build --target web --release`,
`wasm-opt -Oz`); enforced by a CI size gate (fails above 1.5 MB raw /
500 KB gzip):

| artifact | size |
|---|---|
| `kenro_wasm_bg.wasm` | 485 KB |
| gzipped (wire size) | 203 KB |

For comparison, DuckDB-WASM's spatial extension alone is ~23.5 MB
(~6.3 MB wire) — kenro is roughly **30× smaller**, at the cost of the
GEOS/GDAL feature classes kenro deliberately excludes.

## Host support matrix

| | [@sqlite.org/sqlite-wasm] (primary) | [wa-sqlite] | [sql.js] |
|---|---|---|---|
| All 37 functions | ✅ | ✅ | ⚠️ h3 family excluded |
| 64-bit H3 cell ids | ✅ BigInt | ✅ BigInt | ❌ no int64 path — the four `h3_*` functions register as **loud errors** (never silently-lossy doubles) |
| GeoPackage R-tree maintenance | ✅ incl. `trusted_schema=off` (UDFs registered innocuous) | ✅ | ❌ the stock sql.js build ships **without SQLite's R-tree module** |
| Arity overloads (`ST_GeomFromText/1,/2`, …) | ✅ | ✅ | ✅ via a registry shim (sql.js keys UDFs by name only; the adapter works around it — sql.js version pinned) |
| Error messages (`kenro: …`) | ✅ | ✅ (exceptions propagate to the statement caller) | ✅ via string-throw workaround (sql.js empties thrown `Error` objects) |
| Stub errors (helpful "not implemented") | ✅ variadic | ✅ variadic | ✅ registered per concrete arity |

All three hosts run the same CI suite in Node: every registered function
through SQL at least once, stub and NULL-strictness behavior, plus the full
golden-vector set (270 vectors from PostGIS / the H3 reference library)
replayed against the raw wasm exports.

## Usage

Build (or take the npm package once published):

```sh
wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg
```

```js
// Official SQLite WASM (recommended)
import sqlite3InitModule from "@sqlite.org/sqlite-wasm";
import initKenro, * as kenroWasm from "kenro-wasm";
import { registerKenro } from "kenro-wasm/sqlite-wasm";

await initKenro();
const sqlite3 = await sqlite3InitModule();
const db = new sqlite3.oo1.DB(":memory:");
registerKenro(db, kenroWasm);

db.selectValue("SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))"); // POINT(1 2)
```

```js
// sql.js
import { registerKenro } from "kenro-wasm/sqljs";
registerKenro(db, kenroWasm);

// wa-sqlite
import { registerKenro } from "kenro-wasm/wa-sqlite";
registerKenro(sqlite3, db, kenroWasm);
```

To open a GeoPackage file in the browser, deserialize its bytes into an
in-memory database (see `crates/kenro-wasm/demo/` for the full drag-and-drop
flow):

```js
const bytes = new Uint8Array(await file.arrayBuffer());
const p = sqlite3.wasm.allocFromTypedArray(bytes);
const db = new sqlite3.oo1.DB();
sqlite3.capi.sqlite3_deserialize(
  db.pointer, "main", p, bytes.length, bytes.length,
  sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE,
);
registerKenro(db, kenroWasm);
```

## Semantics

Identical to native kenro: the adapters reproduce the rusqlite binding's
value mapping (NULL-strict, deterministic + innocuous, kenro-worded type
errors), and the golden suites are the proof. Two caveats beyond the host
matrix above:

- A Rust panic aborts the wasm instance (there is no unwinding on this
  target). The core is property-tested panic-free; treat a trap as a bug
  and report it.
- Hosts whose UDF APIs erase SQLite's INTEGER/REAL distinction cannot
  reproduce every "expected INTEGER, got REAL" error exactly; integral
  checks recover most of it.

[@sqlite.org/sqlite-wasm]: https://www.npmjs.com/package/@sqlite.org/sqlite-wasm
[wa-sqlite]: https://github.com/rhashimoto/wa-sqlite
[sql.js]: https://github.com/sql-js/sql.js
