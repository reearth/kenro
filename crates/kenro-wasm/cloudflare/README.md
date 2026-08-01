# Spatial queries on Cloudflare Durable Object SQLite

A working Worker that gives Durable Object SQLite a spatial index and
PostGIS-style predicates, using [kenro-wasm](../README.md).

Neither D1 nor Durable Object SQLite supports user-defined functions or
loadable extensions — the supported extension set is FTS5, JSON and the math
functions, and there is no R-tree module. So `ST_Intersects(...)` can never
appear in the SQL itself. What *can* happen is a split:

| stage | where | what |
|---|---|---|
| coarse filter | SQL | tile cells (`WHERE cell IN (…)`), then bounding boxes |
| exact predicate | kenro-wasm, in JS | `ST_Intersects` / `ST_Within` / `ST_DWithin` … |
| output | kenro-wasm, in JS | `ST_Transform`, `ST_AsGeoJSON` |

The trick that makes the SQL half indexable is that **kenro runs at write
time too**: `load()` parses each geometry once and derives the bounding box
and the tile cover before the row reaches SQLite, so the columns SQL needs
are plain REALs and INTEGERs with plain B-tree indexes on them.

A Durable Object is the better host for this than D1: `sql.exec` is
synchronous and the wasm lives in the same isolate, so refining a candidate
is a function call rather than a network round trip. One DO per
region/tile/tenant shards naturally — each holds its own SQLite and its own
copy of the wasm.

## The R-tree stand-in

Bounding-box columns alone index badly: `minx <= ? AND maxx >= ? …` gives a
B-tree only a half-open range to work with, so it degrades to a scan of
everything left of the window. `src/tiles.mjs` instead tags each feature with
the Web Mercator tiles its bbox covers at a fixed zoom, in a side table keyed
on the tile id — a window query becomes an equality lookup, which an index
serves exactly.

Features whose cover is too large to enumerate (a country outline) are filed
under a single `OVERSIZED` cell and scanned on every query. That bounds the
work without ever making the index lossy: correctness never depends on the
cover being complete, only performance does. `stats` in every query response
reports how many rows reached the predicate — if that number tracks the table
size, the zoom is wrong for the data.

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

Routes: `POST /load`, `POST /query`, `GET /stats`, `POST /clear`, all taking
`?shard=<name>` to pick the Durable Object.

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

## Doing the same on D1

The shape carries over, with the round trip moved to the outside:

```js
const { results } = await env.DB.prepare(
  `SELECT f.id, f.geom, f.props FROM features f
    WHERE f.id IN (SELECT id FROM feature_cells WHERE cell IN (${qs}) OR cell = -1)`,
).bind(...cells).all();

const wasm = kenro();
const window = wasm.stGeomFromText(wkt);
const hits = results.filter((r) => wasm.stIntersects(toBytes(r.geom), window));
```

Two things change:

- **Keep the candidate set small.** Every candidate crosses the network as a
  geometry blob and is billed as a row read, so the tile zoom matters much
  more than it does in a DO.
- **BLOBs come back as arrays**, not `Uint8Array` — wrap them before handing
  them to kenro.

Aggregates (`ST_Union`, `ST_AsMVT`) can't join a `GROUP BY` on either
backend; fetch the rows and fold them in JS.

## What this is not

A transparent wrapper. Making plain SQL containing `ST_` calls work would
need a SQL rewriter in front of the driver — a real project. This example is
the pragmatic version: an explicit query API over the same two-stage plan.
