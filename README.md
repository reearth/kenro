# kenro

**SpatiaLite-style spatial SQL for SQLite, in pure Rust** — PostGIS-compatible `ST_` functions that work with rusqlite, as a loadable extension (Python / Node / Bun / Deno / Go / Ruby / C / sqlite3 CLI), in containers and serverless (Cloud Run / Lambda / Workers), and in the browser (sql.js / wa-sqlite / official SQLite WASM).

If you searched for *rusqlite spatial*, *SQLite spatial functions without SpatiaLite*, *SpatiaLite alternative in Rust*, or *GeoPackage in pure Rust*: this is that crate.

**kenro is a full spatial SQL engine for SQLite**: the PostGIS function surface — predicates, overlay, repair, buffering, reprojection, vector tiles, spatial aggregates, ~80 functions — in pure Rust, golden-tested against PostGIS itself, with zero C dependencies and one-call registration:

- **Geometry I/O** — WKT, WKB, GeoJSON and GeoPackage blobs in and out, MVT vector tiles out — all first-class citizens
- **Predicates** — the full DE-9IM family: `ST_Intersects` / `ST_Contains` / `ST_Within` / `ST_Touches` / `ST_Crosses` / `ST_Overlaps` / `ST_Equals` / `ST_Covers` / `ST_Relate`, plus `ST_Distance` / `ST_DWithin` (via [georust/geo])
- **Overlay & repair** (`full` feature) — `ST_Intersection` / `ST_Union` (scalar *and* aggregate) / `ST_Difference` / `ST_SymDifference` / `ST_Buffer` / `ST_MakeValid` in pure Rust, with the differences vs GEOS quantified by golden tests
- **GeoPackage support** — the exact function set the spec's R-tree (F.3) and geometry-type (F.4) maintenance triggers require
- **CRS transform** — pure-Rust [proj4rs]: WGS84, Web Mercator and every UTM zone built in, the full EPSG registry behind a feature flag, with [measured accuracy](docs/accuracy.md)
- **H3 cells** — mesh aggregation in `GROUP BY` ([h3-pg] naming)
- **Vector tiles** — `ST_AsMVTGeom` + the `ST_AsMVT` aggregate with a hand-rolled, dependency-free encoder
- **Accessors, measures, processing** — area, length, centroid, convex hull, line interpolation, simplification, affine transforms, …
- **Tiny** — the loadable extension is a single **1.1 MB** file with zero dependencies, where mod_spatialite's GEOS/PROJ/proj.db chain is ~25 MB across 9 files (**~23× smaller**, measured); the wasm build starts at 412 KB (167 KB wire), 25–57× smaller than DuckDB-WASM spatial. Two honest reasons: a [deliberately narrower scope](docs/functions.md#deliberately-out-of-scope) (no topology, GML/XML, spreadsheet import, datum grids) *and* a statically-linked binary that only carries what you enable — a dynamic-library chain ships everything to everyone

The headline: **with kenro registered, a plain SQLite build maintains a GeoPackage spatial index correctly.** No SpatiaLite, no GDAL, no C toolchain.

> kenro = 間縄 (kennawa) × rope — two words for the same cord, twisted into one. Named after the measuring rope of historical Japanese land surveys: the tool for turning land into ledgers.

## Quickstart (Rust / rusqlite)

```toml
[dependencies]
kenro = { version = "0.1", features = ["rusqlite"] }  # add "full" for overlay/repair
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
whose SQLite driver exposes extension loading:

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
Go, Ruby, C/C++, the sqlite3 CLI, containers (Cloud Run / Fly.io / ECS),
AWS Lambda, and Cloudflare Workers — including cross-compilation and
per-platform gotchas (macOS system SQLite, D1's no-UDF limitation, …).

## Quickstart (browser — kenro-wasm)

Browser SQLite builds can't load native extensions, but they all accept
JS-level user-defined functions — so kenro's SQLite-free core compiles to
wasm — **412–946 KB (167–353 KB wire) depending on the feature tier**
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

## Function reference

~80 SQL functions across geometry I/O (WKT/WKB/GeoJSON/GeoPackage), the
full DE-9IM predicate family (`ST_Relate` included), measures, overlay &
buffer, processing & affine transforms, accessors, constructors, GeoPackage
trigger helpers, H3, and MVT vector tiles — plus two aggregates
(`ST_Union(geom)`, `ST_AsMVT(…)`).

**The full table — every function with its PostGIS / DuckDB Spatial
comparison and documented behavior differences — lives in
[docs/functions.md](docs/functions.md).**

All implemented functions are **deterministic and pure** (no I/O, no clock,
no randomness) and NULL-strict (NULL in → NULL out; aggregates skip NULL
rows, following PostGIS aggregate semantics). Malformed input raises an
explicit error prefixed `kenro:` — never a silent NULL. Functions kenro
knows about but does not implement register as stubs whose error explains
what to use instead.

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
  single 1.1 MB extension (SpatiaLite's GEOS/PROJ/proj.db chain is ~25 MB
  across 9 files), no C toolchain, deterministic. PostGIS is a server-side
  PostgreSQL extension. DuckDB spatial bundles GEOS + PROJ + GDAL (its
  WASM build is ~23.5 MB uncompressed, ~6.3 MB over the wire).
- **Division of labor** — kenro covers spatial SQL end-to-end inside your
  app's SQLite file: predicates, overlay/repair/buffer, R-tree maintenance,
  CRS transforms, MVT generation, aggregates. Reach for PostGIS or DuckDB
  spatial when you need what kenro
  [deliberately leaves out](docs/functions.md#deliberately-out-of-scope) —
  raster, topology/networks, file-format conversion,
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

Every national and regional system is served the same way: the `crs-full`
cargo feature adds the full `crs-definitions` registry as a fallback
(megabytes of tables, EPSG codes ≤ 65535 only); without it, an unknown code
raises an error naming the code and the feature. Accuracy against PROJ is
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

**`full`** adds the one feature excluded from the default for size:
`overlay` (`ST_Intersection`/`ST_Union`/`ST_Difference`/`ST_SymDifference`/
`ST_Buffer`/`ST_MakeValid` — pulls the [i_overlay] mesh, the largest
single contributor to binary size). With overlay present, `ST_AsMVTGeom`
also upgrades to PostGIS-grade validity repair (invalid input and
snap-induced self-intersections are made valid before tiling). In wasm terms: standard 617 KB
(251 KB gzip) vs full 946 KB (353 KB gzip); `--no-default-features`
gives a 412 KB (167 KB gzip) minimal build.

Building with any feature off keeps the corresponding SQL functions
registered as stubs that explain which feature is missing. `rusqlite` (off
by default) enables `kenro::register`. The prebuilt loadable extension
(`kenro-ext`) always ships full.

## Requirements & caveats

- SQLite ≥ 3.31 (for `SQLITE_INNOCUOUS`, which lets kenro functions run inside the GeoPackage R-tree triggers under `PRAGMA trusted_schema=off`); the loadable-extension path needs SQLite ≥ 3.34 (it fails with a clear version-mismatch message on older hosts).
- Loading kenro **and** SpatiaLite into the same connection: both register `ST_` names and SQLite keeps the last registration. Don't mix them (a registration-filter feature flag can be added if needed).
- `ST_Distance` is 2D cartesian in the geometry's coordinate system. For meters over lon/lat, reproject to the local UTM zone first (e.g. `ST_Transform(geom, 32654)` around Tokyo, `32633` around Berlin).

## Roadmap

1. ✅ Core: GeoPackage blobs, WKB/WKT, predicates, R-tree functions, rusqlite registration, PostGIS golden tests
2. ✅ `ST_Transform` (proj4rs; accuracy [measured and documented](docs/accuracy.md)), H3 cell IDs, GeoJSON, accessors
3. ✅ `kenro-ext`: loadable extension (`.so`/`.dylib`/`.dll`) for Python / Node / sqlite3 CLI
4. ✅ `kenro-wasm`: browser builds (official SQLite WASM / sql.js / wa-sqlite, [details](docs/wasm.md)) + drag-and-drop GeoPackage demo
5. ✅ v0.3: full predicate family + `ST_Relate`, measures/processing/affine, pure-Rust overlay & `ST_Buffer`, SQL aggregates (`ST_Union`), MVT (`ST_AsMVTGeom` + `ST_AsMVT`), `GPKG_IsAssignable`
6. ✅ Release pipeline: prebuilt extension binaries (Linux x86_64/arm64, macOS universal, Windows) + wasm bundle on every `v*` tag
7. ✅ Public repository + [live demo on GitHub Pages](https://reearth.github.io/kenro/)
8. ✅ v0.1.0 on [crates.io](https://crates.io/crates/kenro) + [npm as `kenro-wasm`](https://www.npmjs.com/package/kenro-wasm)

## License

MIT OR Apache-2.0, at your option.

[georust/geo]: https://github.com/georust/geo
[proj4rs]: https://github.com/3liz/proj4rs
[h3-pg]: https://github.com/zachasme/h3-pg
[i_overlay]: https://github.com/iShape-Rust/iOverlay
