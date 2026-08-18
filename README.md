# kenro

[![CI](https://github.com/reearth/kenro/actions/workflows/ci.yml/badge.svg)](https://github.com/reearth/kenro/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/kenro)](https://crates.io/crates/kenro)
[![docs.rs](https://img.shields.io/docsrs/kenro)](https://docs.rs/kenro)
[![npm](https://img.shields.io/npm/v/kenro-wasm)](https://www.npmjs.com/package/kenro-wasm)
[![Go Reference](https://pkg.go.dev/badge/github.com/reearth/kenro/go.svg)](https://pkg.go.dev/github.com/reearth/kenro/go)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)

> **Status: v0 — usable, but unstable. Use at your own risk.** The API and behaviour can change in any release. Production use is not recommended, and mission-critical systems least of all. As the license already says: no warranty of any kind.

**SpatiaLite-style spatial SQL for SQLite, in pure Rust** — PostGIS-compatible `ST_` functions that work with rusqlite, as a loadable extension (Python / Node / Bun / Deno / Go / Ruby / C / sqlite3 CLI), in pure Go with no cgo (modernc.org/sqlite + wazero), in containers and serverless (Cloud Run / Lambda / Cloudflare Workers, D1 and Durable Objects), and in the browser (sql.js / wa-sqlite / official SQLite WASM).

If you searched for *rusqlite spatial*, *SQLite spatial functions without SpatiaLite*, *SpatiaLite alternative in Rust*, *GeoPackage in pure Rust*, or *spatial queries on Cloudflare D1 / Durable Objects*: this is that crate.

**kenro is a spatial SQL engine for SQLite** covering the PostGIS function surface you actually use — predicates, overlay, repair, buffering, reprojection, vector tiles, spatial aggregates, ~221 functions — in pure Rust, golden-tested against PostGIS itself, with zero C dependencies and one-call registration:

- **Geometry I/O** — WKT, WKB, GeoJSON, GML 2/3 and GeoPackage blobs in and out, MVT vector tiles, KML and SVG out; 3D and POLYHEDRALSURFACE/TIN columns are read, measured, affine-transformed, reprojected and flattened rather than silently lost — all first-class citizens
- **Predicates** — the full DE-9IM family: `ST_Intersects` / `ST_Contains` / `ST_Within` / `ST_Touches` / `ST_Crosses` / `ST_Overlaps` / `ST_Equals` / `ST_Covers` / `ST_Relate`, plus `ST_Distance` / `ST_DWithin` (via [georust/geo])
- **Overlay & repair** (`full` feature) — `ST_Intersection` / `ST_Union` (scalar *and* aggregate) / `ST_Difference` / `ST_SymDifference` / `ST_Buffer` / `ST_MakeValid` / `ST_Split` in pure Rust, with the differences vs GEOS quantified by golden tests
- **GeoPackage support** — the exact function set the spec's R-tree maintenance triggers require, plus the helper the (since-withdrawn) geometry-type triggers call, because files carrying them are still out there
- **CRS transform** — pure-Rust [proj4rs]: WGS84, Web Mercator and every UTM zone built in, the full EPSG registry behind a feature flag, with [measured accuracy](docs/accuracy.md)
- **3D** — a Z survives storage, transforms, reprojection and every derived geometry that can honestly keep it; `ST_3DDistance`/`ST_3DIntersects`/`ST_3DShortestLine` and the rest of the family core PostGIS has without SFCGAL, golden-tested against it
- **H3 cells** — mesh aggregation in `GROUP BY` ([h3-pg] naming)
- **Vector tiles** — `ST_AsMVTGeom` + the `ST_AsMVT` aggregate with a hand-rolled, dependency-free encoder
- **Routing** (`full` feature) — Dijkstra shortest paths and reachable sets over an edge table, as SQL aggregates (`kenro_dijkstra`, `kenro_dijkstra_cost`, `kenro_drivingdistance`), golden-tested against [pgRouting](https://pgrouting.org/); the query's `WHERE` clause is the edge query — see [Routing](docs/routing.md)
- **Accessors, measures, processing** — area, length, centroid, convex hull, line interpolation, simplification, affine transforms, …
- **Tiny** — the loadable extension is a single **~2 MB** file with zero dependencies, EPSG registry included, where mod_spatialite's GEOS/PROJ/proj.db chain is ~25 MB across 9 files (**~12× smaller**, measured); the wasm build starts at 595 KB (232 KB wire), and the everything-included tier is 2.2 MB (669 KB wire) against DuckDB-WASM spatial's ~23.5 MB. Two honest reasons: a [deliberately narrower scope](docs/scope.md#deliberately-out-of-scope) (no topology store, no XML machinery beyond geometry encodings, no spreadsheet import, no datum grids) *and* a statically-linked binary that only carries what you enable — a dynamic-library chain ships everything to everyone

The headline: **with kenro registered, a plain SQLite build maintains a GeoPackage spatial index correctly.** No SpatiaLite, no GDAL, no C toolchain.

> kenro = 間縄 (kennawa) × rope — two words for the same cord, twisted into one. Named after the measuring rope of historical Japanese land surveys: the tool for turning land into ledgers.

## Quickstart (Rust / rusqlite)

```toml
[dependencies]
kenro = { version = "0.3", features = ["rusqlite"] }  # add "full" for overlay/repair
```

```rust
let conn = rusqlite::Connection::open("parks.gpkg")?;
kenro::register(&conn)?;

// Query a GeoPackage with its R-tree index + a precise predicate refine:
let n: i64 = conn.query_row(
    "SELECT count(*) FROM parks p
     JOIN rtree_parks_geom r ON p.fid = r.id
     WHERE r.minx <= ?1 AND r.maxx >= ?2 AND r.miny <= ?3 AND r.maxy >= ?4
       AND ST_Within(ST_GeomFromGPB(p.geom), ST_GeomFromText(?5, 4326))",
    rusqlite::params![max_x, min_x, max_y, min_y, window_wkt],
    |r| r.get(0),
)?;
```

Inserts, updates and deletes through SQL keep the spatial index in sync via the standard GeoPackage triggers — including files written by GDAL/QGIS.

## Quickstart (every other platform — loadable extension)

kenro ships as a standard SQLite loadable extension — one prebuilt binary
per OS on the [releases page](https://github.com/reearth/kenro/releases)
(Linux x86_64/arm64, macOS universal, Windows), loadable from any language
whose SQLite driver exposes extension loading. (Not applicable in the
browser, on Cloudflare, or with pure-Go SQLite — see the three quickstarts
below.)

```sh
curl -fsSL https://github.com/reearth/kenro/releases/latest/download/kenro-ext-x86_64-unknown-linux-gnu.tar.gz | tar xz
# → libkenro_ext.so  (or build from source: cargo build -p kenro-ext --release)
```

```python
import sqlite3

con = sqlite3.connect("parks.gpkg")
con.enable_load_extension(True)
con.load_extension("./libkenro_ext")

print(con.execute(
    "SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1").fetchone())
```

**Copy-paste guides for every platform live in
[docs/quickstart.md](docs/quickstart.md)**: Python, Node.js, Bun, Deno,
Go, Ruby, C/C++, the sqlite3 CLI, containers (Cloud Run / Fly.io / ECS)
and AWS Lambda, with the per-platform gotchas (macOS system SQLite,
cross-compilation, …). JavaScript hosts — browser and Cloudflare — are
[docs/wasm.md](docs/wasm.md), where no extension binary is involved.

## Quickstart (browser — kenro-wasm)

Browser SQLite builds can't load native extensions, but they all accept
JS-level user-defined functions — so kenro's SQLite-free core compiles to
wasm — **572–2174 KB (224–661 KB wire) depending on the feature tier**
([sizes](docs/wasm.md#size)) — with one adapter per host:

```js
import sqlite3InitModule from "@sqlite.org/sqlite-wasm";
import initKenro, * as kenroWasm from "kenro-wasm";
import { registerKenro } from "kenro-wasm/sqlite-wasm";

await initKenro();
const sqlite3 = await sqlite3InitModule();
const db = new sqlite3.oo1.DB(":memory:");
registerKenro(db, kenroWasm, sqlite3);

db.selectValue("SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))"); // POINT(1 2)
```

Adapters for sql.js and wa-sqlite ship alongside; host support matrix,
measured sizes, and per-host limitations are in [docs/wasm.md](docs/wasm.md).

**Live demo: <https://reearth.github.io/kenro/>** — drag a GeoPackage in
and query it with spatial SQL, entirely client-side (source in
`crates/kenro-wasm/demo/`).

## Quickstart (Cloudflare — Workers, D1, Durable Objects)

Workers can't load a native extension, and **neither D1 nor Durable Object
SQLite accepts user-defined functions** — no `ST_` call can appear in their
SQL, and there is no R-tree module either. What works is a split: kenro
derives the bounding box and a tile cell at *write* time so plain SQL can
index them, then runs the exact predicate in JS on the survivors.

```js
import wasmModule from "kenro-wasm/pkg/kenro_wasm_bg.wasm";  // Workers hand you the Module
import * as kenro from "kenro-wasm";
import { cellsForQuery } from "kenro-wasm/tiles";            // the R-tree stand-in

kenro.initSync({ module: wasmModule });                      // once per isolate
```

A complete Worker doing this on both D1 and Durable Objects — schema,
migrations, and tests that run in workerd — is in
[`crates/kenro-wasm/cloudflare/`](crates/kenro-wasm/cloudflare/README.md).

## Quickstart (pure Go — no cgo)

[modernc.org/sqlite] is SQLite transpiled to Go, so it can't load a native
extension — but it can register Go functions. kenro's core therefore runs as
wasm inside [wazero], and both halves stay cgo-free: **spatial SQL in a
`CGO_ENABLED=0` static binary.**

```sh
go get github.com/reearth/kenro/go
```

```go
import (
    "database/sql"

    kenro "github.com/reearth/kenro/go"
    _ "modernc.org/sqlite"
)

kenro.Register() // once at start-up, before opening connections

db, _ := sql.Open("sqlite", "parks.gpkg")
db.QueryRow(`SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1`).Scan(&wkt)
```

The wasm module is committed to the repository, so `go get` needs no Rust
toolchain. GeoPackage R-tree triggers are maintained correctly here too
(modernc is built with `SQLITE_ENABLE_RTREE`).

**[`go/README.md`](go/README.md)** has the rest: runnable examples (storing
and querying geometries, the R-tree filter-then-refine query, dissolve with
the `ST_Union` aggregate, reprojection), the two modernc.org/sqlite
limitations worth knowing about, measured per-call costs, and how to embed a
smaller feature tier.

## Function reference

207 SQL functions across geometry I/O (WKT/WKB/GeoJSON/GML/KML/SVG/GeoPackage),
the full DE-9IM predicate family (`ST_Relate` included), measures, overlay &
buffer, splitting and merging, processing & affine transforms, hulls and
triangulation, Voronoi diagrams, grid generators, accessors, constructors, GeoPackage trigger helpers, 3D and
surface-collection pass-through, H3, and MVT vector tiles — plus four
aggregates (`ST_Union(geom)`, `ST_AsMVT(…)`, `ST_Extent(geom)`, `ST_3DExtent(geom)`).

**The full table — every function with its PostGIS / DuckDB Spatial /
SpatiaLite comparison, documented behavior differences, and a link to each
function's PostGIS page — lives in [docs/functions.md](docs/functions.md).**
Three topics have their own pages: **[3D geometry](docs/3d.md)** (what happens
to a Z, and surface collections), **[routing](docs/routing.md)** (the Dijkstra
aggregates, and how to build the edge table they need) and **[scope and
semantics](docs/scope.md)** (what kenro leaves out, and why).

All implemented functions are **deterministic and pure** (no I/O, no clock,
no randomness) and NULL-strict (NULL in → NULL out; aggregates skip NULL
rows, following PostGIS aggregate semantics). Malformed input raises an
explicit error prefixed `kenro:` — never a silent NULL. You also never get
SQLite's bare `no such function`: anything outside the build's feature set
registers as a stub naming the missing cargo feature (a default build
stubs the six overlay-family functions; `full` has none), and the one
deliberate exclusion, `ST_Collect`, errors with a pointer to the
`ST_Union` aggregate.

## Semantics: PostGIS is the reference

Function names, signatures, and semantics follow PostGIS (SQL/MM `ST_`
prefix), validated against PostGIS-generated golden vectors committed in
this repo (`tests/golden/*.jsonl` — 700+ vectors across nine suites; H3
vectors come from the reference C library, MVT tiles are cross-decoded by
two independent decoders). Where kenro deviates, it does so **loudly and
documentedly** — never a silently different result. The full list of
divergences (empty-geometry handling, GeometryCollection, SRID rules, the
overlay engine's areal-only results, …) is in
[docs/functions.md](docs/functions.md).

## Choosing kenro vs PostGIS vs DuckDB Spatial

Structural differences that matter more than any single function:

- **SRID model** — PostGIS geometries and kenro's GeoPackage blobs both
  carry their SRID; DuckDB's `GEOMETRY` does not, so CRS bookkeeping is the
  user's job there (and `always_xy` axis-order care is needed for EPSG:4326).
- **Where it runs / weight** — kenro lives *inside* SQLite: pure Rust, a
  single ~2 MB extension (SpatiaLite's GEOS/PROJ/proj.db chain is ~25 MB
  across 9 files), no C toolchain, deterministic. PostGIS is a server-side
  PostgreSQL extension. DuckDB spatial bundles GEOS + PROJ + GDAL (its
  WASM build is ~23.5 MB uncompressed, ~6.3 MB over the wire).
- **Division of labor** — kenro covers spatial SQL end-to-end inside your
  app's SQLite file: predicates, overlay/repair/buffer, R-tree maintenance,
  CRS transforms, MVT generation, Dijkstra routing, aggregates. Reach for
  PostGIS or DuckDB spatial when you need what kenro
  [deliberately leaves out](docs/scope.md#deliberately-out-of-scope) —
  raster, topology stores, file-format conversion,
  GeometryCollection-producing operations, datum-grid transforms. They
  compose rather than compete.

## Supported CRS

`ST_Transform` ships a built-in EPSG table (proj4rs carries no EPSG
database). kenro is region-neutral: the built-in codes are exactly the
globally-defined, algorithmically-derivable systems —

| Codes | System |
|---|---|
| 4326 | WGS84 geographic |
| 3857 | Web Mercator |
| 32601–32660 | WGS84 UTM zones 1N–60N |
| 32701–32760 | WGS84 UTM zones 1S–60S |

Every national and regional system is served the same way: `crs-full` adds
the whole `crs-definitions` registry as a fallback, and it is part of
**`full`** — so a full build transforms to EPSG:6677 or 27700 without a
rebuild (EPSG codes ≤ 65535 only). The default/standard tiers leave it out
and raise an error naming the code and the feature, because a build that
only touches 4326 and 3857 should not carry the tables. Accuracy against PROJ is
measured and documented in [docs/accuracy.md](docs/accuracy.md) — TL;DR:
nanometer-level projection math, but **no datum grids**: national datum
modernizations and earthquake-displacement models are not applied; use full
PROJ for survey-grade work.

## Cargo features

The **default (standard)** set — `transform` (proj4rs), `h3` (h3o),
`geojson`, `mvt` — covers most use cases: I/O, the whole predicate family,
GeoPackage triggers, measures/processing/affine, CRS transform, H3,
GeoJSON, and MVT vector tiles (tile clipping uses dedicated rectangle
algorithms, so MVT costs almost nothing).

**`full`** adds the features excluded from the default for size:
`overlay` (`ST_Intersection`/`ST_Union`/`ST_Difference`/`ST_SymDifference`/
`ST_Buffer`/`ST_MakeValid`/`ST_Split` — pulls the [i_overlay] mesh, the
largest single contributor to binary size) and `spheroid`
(`ST_DistanceSpheroid`/`ST_LengthSpheroid` — pulls geographiclib for a 0.1%
refinement over the always-available spherical `ST_DistanceSphere`), plus
`concave-hull` (+41 KB) and `delaunay` (+81 KB, pulling [spade]) — the two
functions whose algorithms cost more than any other single entry —
`gml` (GML 2/3 I/O, +13 KB for quick-xml), `routing` (the Dijkstra and
driving-distance aggregates — pure code, no dependency), `voronoi`
(`ST_VoronoiPolygons`/`ST_VoronoiLines`, +52 KB — it needs `delaunay` for the
triangulation and `overlay` to clip the cells, so the feature names both),
`text-encodings`
(`ST_AsKML`/`ST_AsSVG`; no XML library, but KML reprojects to WGS84 and so
needs `transform`), and
`crs-full`, the EPSG registry (+155 KB gzipped). With overlay present, `ST_AsMVTGeom`
also upgrades to PostGIS-grade validity repair (invalid input and
snap-induced self-intersections are made valid before tiling). In wasm terms: standard 770 KB
(306 KB gzip) vs full 2174 KB (661 KB gzip, the EPSG registry being most of
the difference); `--no-default-features` gives a 572 KB (224 KB gzip)
minimal build.

Building with any feature off keeps the corresponding SQL functions
registered as stubs that explain which feature is missing. `rusqlite` (off
by default) enables `kenro::register`. The prebuilt loadable extension
(`kenro-ext`) always ships full.

## Requirements & caveats

- SQLite ≥ 3.31 (for `SQLITE_INNOCUOUS`, which lets kenro functions run inside the GeoPackage R-tree triggers under `PRAGMA trusted_schema=off`); the loadable-extension path needs SQLite ≥ 3.34 (it fails with a clear version-mismatch message on older hosts).
- Loading kenro **and** SpatiaLite into the same connection: both register `ST_` names and SQLite keeps the last registration. Don't mix them (a registration-filter feature flag can be added if needed).
- `ST_Distance` is 2D cartesian in the geometry's coordinate system. For meters over lon/lat, reproject to the local UTM zone first (e.g. `ST_Transform(geom, 32654)` around Tokyo, `32633` around Berlin).

## License

MIT OR Apache-2.0, at your option.

[georust/geo]: https://github.com/georust/geo
[proj4rs]: https://github.com/3liz/proj4rs
[h3-pg]: https://github.com/zachasme/h3-pg
[modernc.org/sqlite]: https://pkg.go.dev/modernc.org/sqlite
[wazero]: https://wazero.io
[spade]: https://github.com/Stoeoef/spade
[i_overlay]: https://github.com/iShape-Rust/iOverlay
