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
| does not (**Cloudflare D1, Durable Objects**) | index in SQL on columns kenro computes at write time, run the predicates in JS | [Without SQLite](#without-sqlite-prepared-and-the-spatial-indexes), then the [Cloudflare example](../crates/kenro-wasm/cloudflare/README.md) |

## Size

Measured on release builds (`wasm-pack build --target web --release`,
`wasm-opt -Oz`); enforced by a CI size gate (fails above 2.3 MB raw /
700 KB gzip). Three tiers are built and attached to every GitHub Release
(`kenro-wasm-minimal.tar.gz` / `kenro-wasm-standard.tar.gz` /
`kenro-wasm-full.tar.gz`); functions outside a tier register as stubs
naming the missing feature:

| tier | cargo flags | adds | raw | gzipped (wire) |
|---|---|---|---|---|
| minimal | `--no-default-features` | I/O, predicates, R-tree, accessors, measures, processing, affine, constructors, PostGIS-compat spellings | 595 KB | 232 KB |
| standard (default) | — | + `ST_Transform`, H3, GeoJSON, MVT (`ST_AsMVTGeom` clips with dedicated rectangle algorithms, so tiles cost almost nothing) | 793 KB | 314 KB |
| full | `--features full` | + overlay/`ST_MakeValid`/`ST_Buffer`/`ST_Split` (i_overlay's mesh is the single largest contributor), `ST_AsMVTGeom` gains PostGIS-grade validity repair, the `spheroid` measures pull geographiclib (~17 KB), and the two size-gated algorithms `ST_ConcaveHull` (+41 KB) and `ST_DelaunayTriangles` (+81 KB, pulling `spade`; `ST_TriangulatePolygon` rides along on the same crate), and `crs-full` — the whole EPSG registry, so a national or local grid transforms without a rebuild (+777 KB raw / +155 KB wire), `gml` (+31 KB raw / +13 KB wire, pulling `quick-xml` for reading), `text-encodings` (`ST_AsKML`/`ST_AsSVG`, no library of their own), and `voronoi` (+52 KB raw / +11 KB wire — it needs both `delaunay` for the triangulation and `overlay` to clip the cells) | 2197 KB | 669 KB |

The full tier carries `crs-full`, so `ST_Transform` reaches every EPSG code
in the registry — Japan's plane rectangular systems, the British National
Grid, state planes — rather than only kenro's curated table (WGS84, Web
Mercator, every UTM zone). That is 155 KB of the wire size, and the reason
the standard tier does not include it: a browser map that only ever touches
4326 and 3857 should not pay for the registry.

For comparison, DuckDB-WASM's spatial extension alone is ~23.5 MB
(~6.3 MB wire) — kenro is **11–43× smaller** raw, or **10–30× smaller**
over the wire, depending on the tier, at the cost of the GEOS/GDAL feature
classes kenro doesn't cover.

## Host support matrix

| | [@sqlite.org/sqlite-wasm] (primary) | [wa-sqlite] | [sql.js] |
|---|---|---|---|
| All 218 scalar functions | ✅ | ✅ | ⚠️ h3 family excluded |
| Aggregates (`ST_Union(geom)`, `ST_AsMVT(…)`, `ST_Extent(geom)`, `ST_3DExtent(geom)`) | ✅ xStep/xFinal keyed by `sqlite3_aggregate_context` (pass the `sqlite3` namespace as `registerKenro`'s 3rd argument) | ✅ finals matched FIFO in first-step order (the host exposes no aggregate context; verified empirically) | ✅ via `create_aggregate` through the registry shim |
| 64-bit H3 cell ids | ✅ BigInt | ✅ BigInt | ❌ no int64 path — the four `h3_*` functions register as **loud errors** (never silently-lossy doubles) |
| GeoPackage R-tree maintenance | ✅ incl. `trusted_schema=off` (UDFs registered innocuous) | ❌ **neither wa-sqlite build carries the rtree module** (sync and async, both SQLite 3.44.0) — measured, after this cell said ✅ for a long time with no test behind it | ❌ the stock sql.js build ships **without SQLite's R-tree module** |
| Arity overloads (`ST_GeomFromText/1,/2`, …) | ✅ | ✅ | ✅ via a registry shim (sql.js keys UDFs by name only; the adapter works around it — sql.js version pinned) |
| Error messages (`kenro: …`) | ✅ | ✅ (exceptions propagate to the statement caller) | ✅ via string-throw workaround (sql.js empties thrown `Error` objects) |
| Stub errors (helpful "not implemented") | ✅ variadic | ✅ variadic | ✅ registered per concrete arity |

All three hosts run the same CI suite in Node: every registered function
through SQL at least once, stub and NULL-strictness behavior, plus the full
golden-vector set (700+ vectors from PostGIS / the H3 reference library)
replayed against the raw wasm exports.

The SQLite versions differ more than the table suggests, and wa-sqlite is the
floor: **3.44.0**, against 3.50 for the official build and 3.49 for sql.js.
That matters for the row-splitting recipes in
[the function reference](scope.md#getting-n-rows-out), which need JSON1
(3.38+) and `unhex` (3.41+) — measured present on all three, but wa-sqlite is
the one to re-check if either recipe grows a newer dependency.

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

## Without SQLite: `Prepared` and the spatial indexes

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
available the language handles it (verified on Node 24+ and in workerd —
Node 22 rejects the `using` syntax at parse time, so the callback helpers
below are the portable form):

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

Two B-tree-indexable stand-ins for the R-tree that sql.js and D1/DO SQLite
lack. Both map a bounding box to integer cell ids stored in a side table, so a
window query becomes an index lookup instead of a half-open `minx <= ?` range
scan. Pure arithmetic, no wasm involved in either.

| | |
|---|---|
| **`kenro-wasm/quadtree`** | variable depth: each feature is filed at whatever cell size fits it. Nothing to tune, one row per feature, no cliff. Start here. |
| **`kenro-wasm/tiles`** | one fixed zoom for the whole dataset. Faster than the quadtree for windows near that zoom, much worse away from it. |

### `kenro-wasm/quadtree`

Each feature goes in the deepest quadtree cell that contains its bounding box.
Two such cells are always either nested or disjoint, so if a feature's box
overlaps a window, its cell is necessarily an ancestor or a descendant of one
of the window's cells. That makes the query two indexed shapes and no others:

```js
import { cellFilterSql, cellsForFeature } from "kenro-wasm/quadtree";

const cells = cellsForFeature(bbox);        // write side: one cell, one row
const { sql, params } = cellFilterSql(bbox); // query side: the whole filter
```

| | |
|---|---|
| `cellsForFeature(bbox, opts?)` → `number[]` | cells to store a feature under; one by default |
| `cellsForQuery(bbox, opts?)` → `{ancestors, ranges, wholeTable}` | `ancestors` are equality lookups, `ranges` are `[lo, hi]` id spans |
| `quadCover(bbox, opts?)` → `number[]` | the cover both sides build on |
| `cellFilterSql(bbox, opts?)` → `{sql, params, wholeTable}` | the filter as one statement, every value bound |
| `cellDepth(cell)` → `number` | the depth an id sits at; `0` is the world |
| `CELL_DEPTH` = `24`, `DEFAULT_QUERY_MAX_CELLS` = `16`, `DEFAULT_MAX_PARAMS` = `90` | |

A cell id is a Hilbert code shifted left and terminated by a single 1 bit that
records the depth — the S2 design. The sentinel is what keeps a depth-3 cell
from colliding with a depth-5 one, and it puts a cell's descendants in the ids
immediately around it, so they come out as one `BETWEEN`. Hilbert rather than
Z-order because neighbouring cells then tend to be neighbouring ids, which lets
adjacent ranges merge: measured at ~40% fewer range terms for the same rows.
Ids are 49 bits, so they stay ordinary JS numbers and SQLite stores them as
INTEGER.

**Nothing here has to match between the two sides.** Ids are always encoded at
`CELL_DEPTH`, and nesting makes the result complete for any combination of
`maxCells` and `maxDepth`, so the write side and the query side can be
configured independently — or by different people. `maxCells` only trades SQL
length against precision, and `maxDepth` only caps how fine a cover gets.

`cellFilterSql` emits one statement using `OR`, not a `UNION` per range,
because D1 and Durable Object SQLite cap a compound SELECT at five terms and
refuse at 100 bound variables. Over `maxParams` it coarsens the cover until it
fits, which widens the search rather than narrowing it — a tight budget costs
precision, never a hit.

There is no `OVERSIZED` bucket and nothing returns `null`: a feature too big
for a fine cell is simply filed at a coarse one, which is an ordinary cell that
happens to be an ancestor of a lot. A window covering the world reports
`wholeTable`, and the filter is dropped rather than applied to no purpose.

### `kenro-wasm/tiles`

The simple one, kept because a fixed grid tuned to the size you actually query
beats the quadtree in that band. It maps a bounding box to the Web Mercator
tile ids it covers at one zoom, so a window query becomes `WHERE cell IN (…)`.

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

### Which one

Candidate rows per window over 50,000 buildings, 500 line features and 20 large
polygons — fewer is better, bold is the best index for that row:

| window | true hits | `tiles` z10 | `tiles` z12 | `tiles` z14 | `quadtree` |
|---|---|---|---|---|---|
| 0.005° | 14 | 10,417 | 826 | **106** | 485 |
| 0.05° | 325 | 13,241 | 1,905 | **616** | 1,085 |
| 0.25° | 5,114 | 21,997 | **8,636** | 50,520 | 10,754 |
| 1° | 12,022 | **25,868** | 50,520 | 50,520 | 30,233 |

Every fixed zoom wins around what it was tuned for and then falls off a cliff:
past `maxCells` the cover stops being enumerable, `cellsForQuery` returns
`null`, and the query scans all 50,520 rows. The quadtree is never the fastest
column and never the cliff either — it degrades smoothly over three orders of
magnitude of window size with no zoom to choose, and stores one row per feature
(50,520 against z14's 61,050, since a feature spanning four tiles needs four
rows in the fixed grid).

So: `tiles` if your windows are all about one size and you will tune for it,
`quadtree` if they are not. A map that zooms is the obvious second case.

`node examples/index-comparison.mjs` in `crates/kenro-wasm/js` reproduces the
table; CI runs it with `--check`, which fails if either index moves away from
these numbers without the docs following.

Neither is an R-tree, and neither host will give you one: `USING rtree` answers
`SQLITE_AUTH` on both D1 and Durable Object SQLite. Building one in ordinary
tables is possible — both hosts run recursive CTEs, so the descent is one
statement — but an R-tree insert is read-decide-write, and neither host offers
an interactive transaction to do that in. The Cloudflare example's README works
through [why that was not the trade to make
first](../crates/kenro-wasm/cloudflare/README.md#why-not-build-a-real-r-tree-on-top).

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
   survivors. This is what [`Prepared` and the spatial indexes](#without-sqlite-prepared-and-the-spatial-indexes)
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
