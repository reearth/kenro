# Spatial queries on Cloudflare D1 and Durable Object SQLite

A working Worker that gives D1 and Durable Object SQLite a spatial index and
PostGIS-style predicates, using [kenro-wasm](../README.md) — whose API
reference for the two pieces this leans on, the `Prepared` handle and
`kenro-wasm/tiles`, is
[docs/wasm.md](../../../docs/wasm.md#without-sqlite-prepared-and-kenro-wasmtiles).
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
everything left of the window. `kenro-wasm/tiles` instead tags each feature with
the Web Mercator tiles its bbox covers at a fixed zoom, in a side table keyed
on the tile id — a window query becomes an equality lookup, which an index
serves exactly.

**The tile grid is internal to the index. Queries take an arbitrary
geometry** — any WKT, any size, aligned to nothing — and the cells are
derived from *its* bounding box at query time. Tiles are how rows are found,
never what can be asked for.

Two cases keep that true, and they read the same "cover too large to
enumerate" signal in opposite directions:

- **A feature too big to tile** (a country outline) is filed under a single
  `OVERSIZED` cell and is a candidate for every query.
- **A window too big to tile** drops the cell filter entirely and scans the
  table. Falling back to the `OVERSIZED` bucket here would be the subtle bug:
  that bucket holds only the continent-sized features, so a large window
  would return a handful of them and silently miss everything else.

So the index is never lossy — correctness never depends on a cover being
complete, only performance does. `stats` in every query response reports how
many rows reached the predicate; if that number tracks the table size, the
zoom is wrong for the data.

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
CREATE TABLE feature_cells (   -- one row per (tile, feature): the R-tree stand-in
  cell INTEGER NOT NULL,
  id   TEXT NOT NULL,
  PRIMARY KEY (cell, id)
) WITHOUT ROWID;
```

**Write.** kenro parses the geometry once and hands SQL the numbers it can
index. `cellsForFeature` returns one cell per tile the feature touches — or
`[OVERSIZED]` when that would be too many, which keeps the index complete
without exploding:

```js
import * as kenro from "kenro-wasm";
import { cellsForFeature } from "kenro-wasm/tiles";

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

**Read.** `cellsForQuery` turns the window's bounding box into the cell list
to look up — or `null`, meaning the window is too large to enumerate, so drop
the filter and scan. Then SQL narrows, and kenro decides:

```js
import { bboxOverlaps, cellsForQuery } from "kenro-wasm/tiles";

function query(wkt) {
  const window = kenro.stGeomFromText(wkt);
  const search = {
    minx: kenro.stMinX(window), miny: kenro.stMinY(window),
    maxx: kenro.stMaxX(window), maxy: kenro.stMaxY(window),
  };
  const cells = cellsForQuery(search);
  const columns = "id, geom, props, minx, miny, maxx, maxy";

  //  stage 1 — indexed lookup in SQL
  const rows = (cells === null
    ? sql.exec(`SELECT ${columns} FROM features`)
    : sql.exec(
        `SELECT ${columns} FROM features
          WHERE id IN (SELECT id FROM feature_cells
                        WHERE cell IN (${cells.map(() => "?").join(", ")}))`,
        ...cells)
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
