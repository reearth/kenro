# Spatial queries on Cloudflare D1 and Durable Object SQLite

A working Worker that gives D1 and Durable Object SQLite a spatial index and
PostGIS-style predicates, using [kenro-wasm](../README.md) — whose API
reference for the two pieces this leans on, the `Prepared` handle and
`kenro-wasm/tiles`, is
[docs/wasm.md](../../../docs/wasm.md#without-sqlite-prepared-and-the-spatial-indexes).
Both backends run the same plan from the same code (`src/spatial.mjs`) and the
same test suite; only the row plumbing differs.

Neither D1 nor Durable Object SQLite supports user-defined functions or
loadable extensions — the supported extension set is FTS5, JSON and the math
functions, and there is no R-tree module. So `ST_Intersects(...)` can never
appear in the SQL itself. What *can* happen is a split:

| stage | where | what |
|---|---|---|
| coarse filter | SQL | tile cells (`WHERE cell IN (…)`), then bounding boxes |
| exact predicate | kenro-wasm, in JS | `ST_Intersects` / `ST_Within` / `ST_DWithin` …, over a `Prepared` window decoded once per scan |
| output | kenro-wasm, in JS | `ST_Transform`, `ST_AsGeoJSON` |

The trick that makes the SQL half indexable is that **kenro runs at write
time too**: `load()` parses each geometry once and derives the bounding box
and the tile cover before the row reaches SQLite, so the columns SQL needs
are plain REALs and INTEGERs with plain B-tree indexes on them.

## D1 or a Durable Object

The plan is identical; the economics are not.

| | Durable Object | D1 |
|---|---|---|
| refine loop | `sql.exec` is synchronous and the wasm is in the same isolate → a function call per candidate | every candidate crosses the network as a geometry blob and is billed as a row read |
| schema | DDL in the constructor | `migrations/0001_init.sql`, via `wrangler d1 migrations apply` |
| BLOB reads | `ArrayBuffer` | plain `number[]` — wrap before handing to kenro |
| writes | `transactionSync` | `batch()`, atomic |
| scaling | one DO per region/tile/tenant, each with its own SQLite and its own wasm, no coordination | one shared database; the coarse filter matters much more |

A DO is the better host when the refine step is hot, because a bad tile zoom
costs you CPU there and money on D1.

## The R-tree stand-in

Bounding-box columns alone index badly: `minx <= ? AND maxx >= ? …` gives a
B-tree only a half-open range to work with, so it degrades to a scan of
everything left of the window. `kenro-wasm/quadtree` instead files each
feature under the deepest quadtree cell that contains its bounding box, in a
side table keyed on the cell id — a small building lands in a small cell, a
prefecture in a large one, and a window query becomes an index lookup.

**The cell grid is internal to the index. Queries take an arbitrary
geometry** — any WKT, any size, aligned to nothing — and the cells are
derived from *its* bounding box at query time. Cells are how rows are found,
never what can be asked for.

What makes that complete is that two quadtree cells are always either nested
or disjoint. If a feature's box overlaps the window, the feature's cell is
necessarily an ancestor or a descendant of one of the window's cells — never
off to the side. So the query is two indexed shapes and nothing else:

- **ancestors** — the coarser cells containing the window, a handful of
  equality lookups. A country outline is filed at a shallow cell and is picked
  up here, by being an ancestor of everything inside it.
- **descendants** — one contiguous id range per cell the window is covered
  by, because a cell id is a Hilbert code with the depth marked in its low
  bit, so a cell's descendants are the ids immediately around it.

There is no bucket for features too large to file and no signal the two sides
have to read in opposite directions, because "too large" is not a case: a big
feature is just a shallow cell. `stats` reports `shallowestDepth`, the coarsest
cell in the table — the feature that stays a candidate for the widest range of
queries — and `refined` per query, the rows that reached the predicate.

### What this costs, honestly

A fixed grid tuned to the size you actually query is *faster* than this. Over
50,000 buildings, 500 line features and 20 large polygons, candidate rows per
window (fewer is better):

| window | true hits | `tiles` z10 | `tiles` z12 | `tiles` z14 | `quadtree` |
|---|---|---|---|---|---|
| 0.005° | 14 | 10,417 | 826 | **106** | 485 |
| 0.05° | 325 | 13,241 | 1,905 | **616** | 1,085 |
| 0.25° | 5,114 | 21,997 | **8,636** | 50,520 | 10,754 |
| 1° | 12,022 | **25,868** | 50,520 | 50,520 | 30,233 |

Every fixed zoom wins in a band around what it was tuned for, then falls off a
cliff: past `maxCells` the cover stops being enumerable and the query scans the
whole table. The quadtree is never the fastest column and never the cliff
either — it degrades smoothly across four orders of magnitude of window size,
with no zoom to choose. It also stores exactly one row per feature (50,520)
where z14 stores 61,050, because a feature spanning four tiles needs four rows
in the fixed grid and one cell here. `node examples/index-comparison.mjs` in
`crates/kenro-wasm/js` reproduces this table.

Pick `kenro-wasm/tiles` if your windows are all about one size and you are
willing to tune for it. Pick `kenro-wasm/quadtree` if they are not — a map that
zooms is the obvious case.

### Why not build a real R-tree on top

The obvious alternative is to stop approximating and keep an actual R-tree in
ordinary tables — a node row per box, children by parent id — descending it
from JS. Neither host offers the built-in module — `CREATE VIRTUAL TABLE …
USING rtree` answers `SQLITE_AUTH` on both — but nothing stops you implementing
one, and it is worth being clear that this was a choice rather than an
oversight. Each claim below is pinned by a test in `test/cell-ids.test.mjs`
that runs against the real hosts.

**Reading would work.** Both hosts support recursive CTEs, so the descent fits
in one statement — the overlap test at each level is plain `REAL` comparisons,
no user-defined function required. That half is not the problem.

**Writing is.** An R-tree insert is read-decide-write: descend to choose a
subtree, split the leaf if it is full, then propagate the enlarged boxes back
up. D1 has no interactive transaction — `BEGIN` and `SAVEPOINT` are rejected,
and `batch()` runs statements you committed to before you saw any results — so
you cannot branch on a node you just read without another round trip. Loading
*n* features costs round trips proportional to *n* × depth, against one batched
insert per feature here. Worse, two writers splitting the same interior node
interleave and corrupt the tree, and there is no lock to take. A Durable Object
escapes both problems (single-threaded, `transactionSync`), but it is also the
host that needs the help least, since its SQLite is local.

The cell index has no shared mutable structure at all. A row is `(cell, id)`,
computed by arithmetic from one feature's bounding box, independent of every
other row and idempotent on re-insert — which is why loading is a plain batch
and concurrent writers cannot corrupt anything.

**And the ceiling is the same.** An R-tree returns bounding-box candidates,
exactly like the cell index; the exact predicate still runs in kenro afterwards
either way. What it buys is splits that follow the data — tighter candidate
sets in dense areas, where a fixed grid line falls wherever it falls. That is a
real gain, and a bounded one: it changes the constant, not the shape. Set
against maintaining Guttman splits and rebalancing in JS, where every bug is a
silently missing row, it did not look like the trade to make first.

## End to end

Two tables. Nothing in them is spatial — which is the point, because that is
all D1 and Durable Object SQLite can index:

```sql
CREATE TABLE features (
  id    TEXT PRIMARY KEY,
  geom  BLOB NOT NULL,          -- GeoPackage blob; its header envelope makes ST_MinX free
  props TEXT NOT NULL,
  minx  REAL NOT NULL, miny REAL NOT NULL,
  maxx  REAL NOT NULL, maxy REAL NOT NULL
);
CREATE TABLE feature_cells (   -- one row per (cell, feature): the R-tree stand-in
  cell INTEGER NOT NULL,
  id   TEXT NOT NULL,
  PRIMARY KEY (cell, id)
) WITHOUT ROWID;
```

**Write.** kenro parses the geometry once and hands SQL the numbers it can
index. `cellsForFeature` returns the one cell the feature fits in — whatever
its size, and with nothing to configure:

```js
import * as kenro from "kenro-wasm";
import { cellsForFeature } from "kenro-wasm/quadtree";

function load(feature) {
  const id = String(feature.id);
  const geom = kenro.stAsGpb(kenro.stGeomFromGeojson(JSON.stringify(feature.geometry)));
  const bbox = {
    minx: kenro.stMinX(geom), miny: kenro.stMinY(geom),
    maxx: kenro.stMaxX(geom), maxy: kenro.stMaxY(geom),
  };

  sql.exec(
    `INSERT INTO features (id, geom, props, minx, miny, maxx, maxy)
     VALUES (?, ?, ?, ?, ?, ?, ?)`,
    id, geom, JSON.stringify(feature.properties), bbox.minx, bbox.miny, bbox.maxx, bbox.maxy);

  for (const cell of cellsForFeature(bbox))
    sql.exec("INSERT INTO feature_cells (cell, id) VALUES (?, ?)", cell, id);
}
```

**Read.** `cellFilterSql` turns the window's bounding box into the whole
coarse filter — the ancestor lookups and the descendant ranges, with every
value bound. Then SQL narrows, and kenro decides:

```js
import { bboxOverlaps, cellFilterSql } from "kenro-wasm/quadtree";

function query(wkt) {
  const window = kenro.stGeomFromText(wkt);
  const search = {
    minx: kenro.stMinX(window), miny: kenro.stMinY(window),
    maxx: kenro.stMaxX(window), maxy: kenro.stMaxY(window),
  };
  const filter = cellFilterSql(search);
  const columns = "id, geom, props, minx, miny, maxx, maxy";

  //  stage 1 — indexed lookup in SQL
  const rows = (filter.wholeTable
    ? sql.exec(`SELECT ${columns} FROM features`)
    : sql.exec(
        `SELECT ${columns} FROM features WHERE id IN (${filter.sql})`,
        ...filter.params)
  ).toArray();

  const hits = [];
  using win = kenro.Prepared.fromBlob(window);      // decoded once for the scan
  for (const row of rows) {
    //  stage 2 — bounding boxes, from the columns SQL just returned
    if (!bboxOverlaps(search, row)) continue;
    //  stage 3 — the exact predicate, the only stage that touches geometry
    using g = kenro.Prepared.fromBlob(new Uint8Array(row.geom));
    if (g.stIntersects(win)) hits.push(JSON.parse(g.stAsGeojson()));
  }
  return hits;
}
```

`cellFilterSql` emits **one** statement — `cell IN (…) OR cell BETWEEN ? AND ?
OR …` — rather than a `UNION` per range, because D1 and Durable Object SQLite
allow only **five** terms in a compound SELECT and a cover of any useful size
runs straight past that. They also refuse at 100 bound variables, so the filter
coarsens its cover until it fits under `maxParams` (90 by default); that widens
the search and never narrows it, so a tight parameter budget costs precision,
never a hit. Both limits are pinned by tests in `test/cell-ids.test.mjs`
against the real runtime, along with the fact that a 49-bit cell id round-trips
through D1 as an exact INTEGER rather than a float.

That is the whole design. `src/spatial.mjs` is this with the predicate made
selectable, reprojection on output, and a candidate counter; the two backend
files differ only in how rows go in and come out — a Durable Object's
`sql.exec` is synchronous and returns BLOBs as `ArrayBuffer`, D1's is
awaited and returns them as `number[]`.

## Using this in your own Worker

Install the package — there is nothing to build:

```sh
npm install kenro-wasm
```

The one thing the snippets above leave out is where `kenro` comes from.
Wrangler hands a Worker the compiled `WebAssembly.Module` as an import, so
initialization is one synchronous call per isolate — no fetch, no top-level
await:

```js
import wasmModule from "kenro-wasm/pkg/kenro_wasm_bg.wasm";
import * as kenro from "kenro-wasm";

let ready = false;
function init() {
  if (!ready) { kenro.initSync({ module: wasmModule }); ready = true; }
  return kenro;
}
```

(wasm-pack's `--target web` output would otherwise fetch its `.wasm` relative
to `import.meta.url`, which no Worker can do.) The module is well inside
Worker size limits at every feature tier — the measured table lives in
[docs/wasm.md](../../../docs/wasm.md#size), next to the CI gate that enforces
it. Both subpaths ship TypeScript types.

From there, take what you need from this directory: `src/spatial.mjs` is the
plan above in ~150 lines, `src/spatial-do.mjs` and `src/spatial-d1.mjs` are
the two plumbings, `migrations/` is the D1 schema.

## Running this example from the repo

The example builds the wasm from the working tree instead of installing the
package, so it needs one extra step:

```sh
# 1. build the wasm (once)
wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg -- --features full

# 2. from this directory
npm install
npm test        # runs the Worker in workerd, real SQLite, real wasm
npm run dev     # http://localhost:8787
```

`sync-wasm.sh` then copies `../js/pkg` into `vendor/`, because the Workers
Vitest pool cannot resolve a `.wasm` module from outside the project root.
None of that applies to a project that installs `kenro-wasm` normally.

`npm test` runs in CI on every push (the `wasm` job, against the wasm built
in the same job): this example is where `Prepared` and `kenro-wasm/tiles`
meet a real host, so a regression in either shows up here.

## HTTP API

```sh
curl -X POST 'localhost:8787/load' --data-binary @parks.geojson
curl -X POST 'localhost:8787/query' -d '{
  "wkt": "POLYGON((139.6 35.6, 139.8 35.6, 139.8 35.75, 139.6 35.75, 139.6 35.6))",
  "predicate": "intersects",
  "srid": 3857
}'
```

Routes: `POST /load`, `POST /query`, `GET /stats`, `POST /clear`.

`?backend=do` (default) or `?backend=d1` picks the storage; `?shard=<name>`
picks the Durable Object and is ignored by D1. To use D1 for real, create the
database and apply the schema:

```sh
wrangler d1 create kenro-spatial     # put the id in wrangler.jsonc
wrangler d1 migrations apply kenro-spatial
```

Query fields: `wkt` (required), `predicate` (`intersects` | `within` |
`contains` | `dwithin`), `distance` (required for `dwithin`, in SRID units —
degrees for EPSG:4326), `srid` (reproject the output), `limit`.

## Layout

| file | |
|---|---|
| `src/spatial.mjs` | the plan — write-time derivation, the coarse-filter SQL, the refine loop. Backend-independent. |
| `src/spatial-do.mjs` | Durable Object plumbing |
| `src/spatial-d1.mjs` | D1 plumbing |
| `kenro-wasm/tiles` | the R-tree stand-in — published with the package, imported here by path |
| `src/kenro.mjs` | wasm init |
| `migrations/` | D1 schema (kept in step with the DO's by `test/schema.test.mjs`) |

## Limits

Aggregates (`ST_Union`, `ST_AsMVT`) can't join a `GROUP BY` on either backend
— fetch the rows and fold them in JS. Likewise anything that wants a geometry
in `ORDER BY` or a join condition: the SQL half only ever sees numbers.

## What this is not

A transparent wrapper. Making plain SQL containing `ST_` calls work would
need a SQL rewriter in front of the driver — a real project. This example is
the pragmatic version: an explicit query API over the same two-stage plan.
