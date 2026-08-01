# Spatial queries on Cloudflare D1 and Durable Object SQLite

A working Worker that gives D1 and Durable Object SQLite a spatial index and
PostGIS-style predicates, using [kenro-wasm](../README.md). Both backends run
the same plan from the same code (`src/spatial.mjs`) and the same test suite;
only the row plumbing differs.

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
everything left of the window. `src/tiles.mjs` instead tags each feature with
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

## Run it

```sh
# 1. build the wasm (once)
wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg -- --features full

# 2. from this directory
npm install
npm test        # runs the Worker + DO in workerd, real SQLite, real wasm
npm run dev     # http://localhost:8787
```

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

## Loading the wasm in a Worker

wasm-pack's `--target web` output fetches its `.wasm` relative to
`import.meta.url`, which no Worker can do. Workers hand you the compiled
module as an import instead, so `initSync` is the whole story — see
`src/kenro.mjs`. The module is well inside Worker size limits (standard tier
617 KB / 251 KB gzip, minimal 412 KB / 167 KB).

`sync-wasm.sh` copies `../js/pkg` into `vendor/` because the Workers Vitest
pool cannot resolve a `.wasm` module from outside the project root. In your
own project, `import wasm from "kenro-wasm/pkg/kenro_wasm_bg.wasm"` off the
published package works directly.

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
