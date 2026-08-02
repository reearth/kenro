# kenro in JavaScript — browser and edge (kenro-wasm)

Browser SQLite builds are Emscripten C builds that cannot load a native
extension — but they all expose JS-level user-defined-function registration.
kenro-wasm is therefore kenro's SQLite-free pure core compiled to
`wasm32-unknown-unknown` (no SQLite inside), plus one small adapter per host
that wires the exports in as SQL functions. The function catalog is a single
machine-readable manifest shared with the rusqlite binding
(`kenro::functions::manifest`, consistency-tested in CI), so the hosts
cannot drift.

Two ways to use this package, and which one you get is decided by your host,
not by preference:

| your SQLite | what you do | start at |
|---|---|---|
| accepts JS user-defined functions (`@sqlite.org/sqlite-wasm`, sql.js, wa-sqlite) | register kenro and write ordinary spatial SQL | [Usage](#usage) |
| does not (**Cloudflare D1, Durable Objects**) | index in SQL on columns kenro computes at write time, run the predicates in JS | [Without SQLite](#without-sqlite-prepared-and-kenro-wasmtiles), then the [Cloudflare example](../crates/kenro-wasm/cloudflare/README.md) |

## Size

Measured on release builds (`wasm-pack build --target web --release`,
`wasm-opt -Oz`); enforced by a CI size gate (fails above 1.5 MB raw /
500 KB gzip). Three tiers are built and attached to every GitHub Release
(`kenro-wasm-minimal.tar.gz` / `kenro-wasm-standard.tar.gz` /
`kenro-wasm-full.tar.gz`); functions outside a tier register as stubs
naming the missing feature:

| tier | cargo flags | adds | raw | gzipped (wire) |
|---|---|---|---|---|
| minimal | `--no-default-features` | I/O, predicates, R-tree, accessors, measures, processing, affine, constructors, PostGIS-compat spellings | 419 KB | 168 KB |
| standard (default) | — | + `ST_Transform`, H3, GeoJSON, MVT (`ST_AsMVTGeom` clips with dedicated rectangle algorithms, so tiles cost almost nothing) | 619 KB | 250 KB |
| full | `--features full` | + overlay/`ST_MakeValid`/`ST_Buffer`, and `ST_AsMVTGeom` gains PostGIS-grade validity repair (i_overlay's mesh is the single largest contributor) | 965 KB | 359 KB |

For comparison, DuckDB-WASM's spatial extension alone is ~23.5 MB
(~6.3 MB wire) — kenro is **25–57× smaller** depending on the tier, at
the cost of the GEOS/GDAL feature classes kenro doesn't cover.

## Host support matrix

| | [@sqlite.org/sqlite-wasm] (primary) | [wa-sqlite] | [sql.js] |
|---|---|---|---|
| All ~80 scalar functions | ✅ | ✅ | ⚠️ h3 family excluded |
| Aggregates (`ST_Union(geom)`, `ST_AsMVT(…)`) | ✅ xStep/xFinal keyed by `sqlite3_aggregate_context` (pass the `sqlite3` namespace as `registerKenro`'s 3rd argument) | ✅ finals matched FIFO in first-step order (the host exposes no aggregate context; verified empirically) | ✅ via `create_aggregate` through the registry shim |
| 64-bit H3 cell ids | ✅ BigInt | ✅ BigInt | ❌ no int64 path — the four `h3_*` functions register as **loud errors** (never silently-lossy doubles) |
| GeoPackage R-tree maintenance | ✅ incl. `trusted_schema=off` (UDFs registered innocuous) | ✅ | ❌ the stock sql.js build ships **without SQLite's R-tree module** |
| Arity overloads (`ST_GeomFromText/1,/2`, …) | ✅ | ✅ | ✅ via a registry shim (sql.js keys UDFs by name only; the adapter works around it — sql.js version pinned) |
| Error messages (`kenro: …`) | ✅ | ✅ (exceptions propagate to the statement caller) | ✅ via string-throw workaround (sql.js empties thrown `Error` objects) |
| Stub errors (helpful "not implemented") | ✅ variadic | ✅ variadic | ✅ registered per concrete arity |

All three hosts run the same CI suite in Node: every registered function
through SQL at least once, stub and NULL-strictness behavior, plus the full
golden-vector set (700+ vectors from PostGIS / the H3 reference library)
replayed against the raw wasm exports.

## Usage

Build (or take the npm package once published):

```sh
wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg -- --features full
```

```js
// Official SQLite WASM (recommended)
import sqlite3InitModule from "@sqlite.org/sqlite-wasm";
import initKenro, * as kenroWasm from "kenro-wasm";
import { registerKenro } from "kenro-wasm/sqlite-wasm";

await initKenro();
const sqlite3 = await sqlite3InitModule();
const db = new sqlite3.oo1.DB(":memory:");
registerKenro(db, kenroWasm, sqlite3); // sqlite3 namespace needed for aggregates

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
registerKenro(db, kenroWasm, sqlite3);
```

## Without SQLite: `Prepared` and `kenro-wasm/tiles`

The exports also stand alone, for hosts where no SQLite can hold kenro's
UDFs at all — Cloudflare D1 and Durable Object SQLite support neither
user-defined functions nor an R-tree. There the split is: SQL does an
indexed coarse filter over columns kenro computed at write time, and kenro
does the exact predicate in JS.

Two pieces exist for that shape (a full Worker using both:
[`crates/kenro-wasm/cloudflare/`](../crates/kenro-wasm/cloudflare/README.md)):

**`Prepared`** — a geometry decoded once and reused. Every blob function
decodes per call, because that is what a SQLite UDF receives; a JS host
scanning candidates against one fixed search window does not have to.

```js
const win = kenroWasm.Prepared.fromText(windowWkt, 4326);
try {
  for (const row of rows) {
    const g = kenroWasm.Prepared.fromBlob(row.geom);
    try {
      if (!g.stIntersects(win)) continue;
      hits.push(JSON.parse(g.stAsGeojson()));   // output, no second decode
    } finally { g.free(); }
  }
} finally { win.free(); }
```

Constructors `fromBlob` (internal, WKB or GeoPackage) / `fromText(wkt, srid)`.

| | |
|---|---|
| predicates | `stIntersects` `stContains` `stWithin` `stCovers` `stDistance` `stDwithin` |
| output | `stAsGeojson` `stAsGeojsonDigits` `stAsText` `stAsBinary` `stAsGpb` `stSrid` |
| reprojection | `stTransform(srid)` → **a new handle**, freed separately |

Each is the same code path as the blob function of that name (the `decoded`
submodules in the core), so answers and error wording are identical by
construction — the golden predicate, geojson and transform vectors are
replayed through both APIs and compared.

Measured over a 500-candidate loop, half of them hits:

| loop | saved |
|---|---|
| predicate only, simple window | **41%** |
| predicate only, 200-vertex window | 17% |
| predicate only, 5000-vertex window | 12% |
| predicate + GeoJSON output | 16% |
| predicate + reproject + GeoJSON | 22% |
| …with a 200-vertex window | 26% |

Only decoding is saved, so the gain shrinks as the relate itself grows to
dominate, and widens as more calls per row are chained onto one handle.

**A handle must be freed.** wasm-bindgen cannot collect it for you; a leaked
one keeps its geometry in the wasm heap for the life of the isolate. Freeing
*twice* traps with `null pointer passed to rust`, so the early-exit paths are
where this goes wrong.

`Symbol.dispose` is wired to `free`, so where explicit resource management is
available the language handles it (verified on Node and in workerd):

```js
using g = kenroWasm.Prepared.fromBlob(row.geom);   // freed at end of block
```

Everywhere else, `kenro-wasm/prepared` is the callback equivalent — plus the
idempotence the built-in dispose lacks (it *is* `free`, so mixing `using`
with a manual `free()` still traps):

| | |
|---|---|
| `freeOnce(handle)` | free at most once; a no-op on an already-freed handle, `null` or `undefined` |
| `withPrepared(handle, fn)` | own one handle for `fn`, free however it exits, return `fn`'s value |
| `withScope(fn)` | `fn(own)`; every `own(handle)` is freed on the way out, in reverse order — this is the one that covers a handle created mid-scope, like a reprojection |

```js
import { withScope } from "kenro-wasm/prepared";

const geojson = withScope((own) => {          // one scope per row, not per scan
  const g = own(kenroWasm.Prepared.fromBlob(row.geom));
  if (!g.stIntersects(win)) return null;
  return own(g.stTransform(3857)).stAsGeojson();
});
```

**`kenro-wasm/tiles`** — a B-tree-indexable stand-in for the R-tree that
sql.js and D1/DO SQLite lack. It maps a bounding box to Web Mercator tile
ids, so a window query becomes `WHERE cell IN (…)` instead of a half-open
`minx <= ?` range scan. Pure arithmetic, no wasm involved.

```js
import { cellsForFeature, cellsForQuery } from "kenro-wasm/tiles";

const cells = cellsForFeature(bbox);   // write side: store one row per cell
const search = cellsForQuery(bbox);    // query side: null = scan, don't filter
```

The asymmetry is the point: `cellsForFeature` files a too-large feature
under `OVERSIZED` (a permanent candidate), while `cellsForQuery` returns
`null` for a too-large *window*, meaning "drop the filter and scan". Reading
the second as the first returns only continent-sized features for a wide
query and silently drops the rest. TypeScript enforces that difference —
`cellsForQuery` is `number[] | null` and `cellsForFeature` is `number[]`.

A bounding box is `{minx, miny, maxx, maxy}` in WGS84 degrees; every function
takes an optional `{zoom = 8, maxCells = 64}`.

| | |
|---|---|
| `cellsForFeature(bbox, opts?)` → `number[]` | cells to store a feature under: its cover, or `[OVERSIZED]` |
| `cellsForQuery(bbox, opts?)` → `number[] \| null` | cells to search: the window's cover **plus `OVERSIZED`**, or `null` = too large, scan the table |
| `tileCover(bbox, opts?)` → `number[] \| null` | the raw cover the two build on; `null` = over `maxCells` |
| `bboxOverlaps(a, b)` → `boolean` | inclusive overlap — the cheap reject before any wasm call |
| `padBbox(bbox, d)` → `Bbox` | grow on every side, for an `ST_DWithin` search area |
| `OVERSIZED` = `-1` | the bucket for features too large to enumerate |
| `DEFAULT_ZOOM` = `8`, `DEFAULT_MAX_CELLS` = `64` | |

Cell ids are `y * 2**zoom + x` — a safe integer below zoom 26, so a plain
`INTEGER` column indexes them. Web Mercator's y grows southward, and
latitudes beyond ±85.05° are clamped rather than allowed to go infinite.

`zoom` is a per-dataset knob — city-scale parcels and a global coastline want
different values, so tune it per table rather than picking one for the whole
app. Pick it so that a typical query window covers a handful of cells and a
typical feature fits in one: too coarse and every query scans, too fine and
`maxCells` sends everything to `OVERSIZED`. `stats.refined` in the example's
query response is how you tell.

This `zoom` has nothing to do with the z of the tiles you serve. It only sets
the granularity of the index, and `ST_AsMVTGeom`/`ST_AsMVT` never look at it —
they take the z/x/y you hand them. Serving z14 is no reason to index at zoom
14; pick the index zoom from your query windows, the serving z from your map.

Pass the same `zoom` and `maxCells` to `cellsForFeature` and `cellsForQuery`.
The two sides compute cell ids independently, so a mismatch means the query's
cells never meet the feature's, and rows go missing with no error. If you move
off the defaults, keep them in one constant per table and import it on both
sides.

## Cloudflare Workers, D1 and Durable Objects

Workers cannot load native extensions, and neither **D1** nor **Durable
Object SQLite** supports user-defined functions — their extension set is
FTS5, JSON and math, with no R-tree module either. So `ST_Intersects(...)`
cannot appear in their SQL at all. Three patterns that do work, in
increasing order of how much data they scale to:

1. **Process geometry in the Worker.** Store GeoPackage blobs in a column,
   `SELECT` them out, and call `stAsText` / `stIntersects` / `stTransform` …
   on the values in JS. Fine when the row set is already small.
2. **Index in SQL, refine in kenro** — the scalable version: derive a
   bounding box and tile cells with kenro at *write* time, let SQL filter on
   those with a plain B-tree index, then run the exact predicate in JS on the
   survivors. This is what [`Prepared` and `kenro-wasm/tiles`](#without-sqlite-prepared-and-kenro-wasmtiles)
   above are for. A complete Worker doing it on both backends — schema,
   migrations, and tests that run in workerd — is in
   [`crates/kenro-wasm/cloudflare/`](../crates/kenro-wasm/cloudflare/README.md).
3. **Run a full SQLite inside the Worker** with [sql.js] or [wa-sqlite] over
   bytes fetched from R2/KV — read-only analytics on a shipped
   `.gpkg`/`.sqlite` — and `registerKenro` as usual, the same adapters used
   in the browser. Then ordinary spatial SQL works, at the cost of loading
   the database into the isolate.

Loading the wasm is one synchronous call, because Wrangler hands a Worker the
compiled module as an import:

```js
import wasmModule from "kenro-wasm/pkg/kenro_wasm_bg.wasm";
import * as kenro from "kenro-wasm";

kenro.initSync({ module: wasmModule });   // once per isolate; no fetch, no await
```

(wasm-pack's `--target web` output would otherwise fetch its `.wasm` relative
to `import.meta.url`, which no Worker can do.)

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
