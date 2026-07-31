# kenro（間縄）

**Spatial functions for SQLite in pure Rust** — works with rusqlite and as a loadable extension (Python / Node / sqlite3 CLI) today; a WASM build is on the roadmap.

If you searched for *rusqlite spatial*, *SQLite spatial functions without SpatiaLite*, or *GeoPackage in pure Rust*: this is that crate.

kenro provides the 20% of SpatiaLite everyone actually uses, with zero C dependencies and one-call registration:

- **Geometry I/O** — WKT, WKB, GeoJSON, and GeoPackage blobs as first-class citizens
- **Predicates** — DE-9IM `ST_Intersects` / `ST_Contains` / `ST_Within`, plus `ST_Distance` / `ST_DWithin` (via [georust/geo])
- **GeoPackage R-tree support** — the exact function set the GeoPackage spec's R-tree maintenance triggers require
- **CRS transform** — pure-Rust [proj4rs] with first-class support for the Japanese national systems (JGD2000/JGD2011, plane rectangular I–XIX) and [measured accuracy](docs/accuracy.md)
- **H3 cells** — mesh aggregation in `GROUP BY` ([h3-pg] naming)
- **Accessors** — area, length, centroid, envelope, validity, simplification, …

The headline: **with kenro registered, a plain SQLite build maintains a GeoPackage spatial index correctly.** No SpatiaLite, no GDAL, no C toolchain.

> 間縄 (kenro): the measuring rope used in historical Japanese land surveys — the tool for turning land into ledgers.

## Quickstart (Rust / rusqlite)

```toml
[dependencies]
kenro = { version = "0.1", features = ["rusqlite"] }
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

## Quickstart (Python / Node / sqlite3 CLI — loadable extension)

kenro also builds as a standard SQLite loadable extension. Until binary
releases exist, build it yourself (any OS, no C toolchain or SQLite dev
files needed):

```sh
cargo build -p kenro-ext --release
# → target/release/libkenro_ext.so (Linux) / libkenro_ext.dylib (macOS)
#   / target/release/kenro_ext.dll (Windows)
```

The host SQLite must be ≥ 3.34. Loading is per-connection; all kenro
functions become available on the loading connection.

### Python (stdlib `sqlite3`)

```python
import sqlite3

con = sqlite3.connect("parks.gpkg")
con.enable_load_extension(True)
con.load_extension("./target/release/libkenro_ext")  # suffix optional
con.enable_load_extension(False)

print(con.execute(
    "SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1").fetchone())
```

macOS note: python.org installers ship a `sqlite3` module without
`enable_load_extension`; Homebrew Python has it
(`hasattr(con, "enable_load_extension")` tells you which you have).

### Node

```js
// better-sqlite3
const Database = require("better-sqlite3");
const db = new Database("parks.gpkg");
db.loadExtension("./target/release/libkenro_ext");

// or the built-in node:sqlite
const { DatabaseSync } = require("node:sqlite");
const db2 = new DatabaseSync("parks.gpkg", { allowExtension: true });
db2.enableLoadExtension(true);
db2.loadExtension("./target/release/libkenro_ext");
```

### sqlite3 CLI

```
$ sqlite3 parks.gpkg
sqlite> .load ./target/release/libkenro_ext
sqlite> SELECT ST_AsGeoJSON(ST_Transform(ST_GeomFromGPB(geom), 4326)) FROM parks LIMIT 1;
```

macOS: the system `/usr/bin/sqlite3` is compiled without extension loading —
use `brew install sqlite` and run `$(brew --prefix sqlite)/bin/sqlite3`.

Renamed copies load too (e.g. `libkenro.so`): the binary exports
`sqlite3_extension_init`, `sqlite3_kenroext_init` and `sqlite3_kenro_init`.

## Function reference

Every SQL function kenro registers, with its support status in PostGIS and
DuckDB Spatial for comparison (columns verified against PostGIS 3.5 and a
live DuckDB 1.4.0 + spatial session, July 2026). ✅ = present with the same
name and compatible semantics; deviations are spelled out.

"Geometry" values in and out of kenro functions are GeoPackage blobs — they
carry the SRID, and a value in a gpkg column is already valid storage. Every
geometry-accepting function also auto-detects raw WKB input, so
`ST_Within(p.geom, …)` works directly on a gpkg column.

| Function | Returns | PostGIS | DuckDB Spatial | Notes |
|---|---|---|---|---|
| **Geometry I/O** | | | | |
| `ST_GeomFromText(wkt [, srid])` | geometry | ✅ | ⚠️ no srid arg | kenro rejects `POINT EMPTY` (geometry model limit); DuckDB's geometry is SRID-less |
| `ST_GeomFromWKB(wkb [, srid])` | geometry | ✅ | ⚠️ no srid arg | Accepts ISO WKB and EWKB; an explicit srid overrides an embedded one (PostGIS behavior) |
| `ST_GeomFromGPB(gpb)` | geometry | ❌ | ❌ | kenro-only: validates + normalizes a GeoPackage blob. DuckDB imports gpkg **files** via GDAL `ST_Read`; PostGIS needs ogr2ogr |
| `ST_GeomFromGeoJSON(text)` | geometry | ✅ keeps Z | ✅ | SRID 4326 per RFC 7946 (PostGIS ≥ 3.0); kenro is 2D-only and errors on 3D rather than dropping Z |
| `ST_AsText(geom)` | TEXT | ✅ | ✅ | kenro's formatting is byte-identical to PostGIS (golden-tested) |
| `ST_AsBinary(geom)` | BLOB | ✅ | ❌ named `ST_AsWKB` | ISO WKB, little-endian, SRID dropped (as in PostGIS); PostGIS conversely has no `ST_AsWKB` |
| `ST_AsGPB(geom)` | BLOB | ❌ | ❌ | kenro-only: storage-grade GeoPackage blob (envelope included) — use for writing gpkg columns |
| `ST_AsGeoJSON(geom [, maxdecimaldigits])` | TEXT | ✅ | ✅ JSON fragment | Default 9 digits; kenro's output is byte-identical to PostGIS (golden-tested) |
| **SRID & CRS transform** | | | | |
| `ST_SRID(geom)` | INT | ✅ | ❌ | 0 = unknown. DuckDB's `GEOMETRY` carries no SRID at all — CRS bookkeeping is manual there |
| `ST_SetSRID(geom, srid)` | geometry | ✅ | ❌ | Relabel only, no reprojection |
| `ST_Transform(geom, to_srid)` | geometry | ✅ 4 overloads, full PROJ | ⚠️ `(geom, source_crs, target_crs [, always_xy])` | kenro: PostGIS-exact 2-arg form, source = embedded SRID, curated EPSG table (see [Supported CRS](#supported-crs), [accuracy](docs/accuracy.md)). DuckDB must be told the source CRS on every call |
| **Predicates & measures** | | | | |
| `ST_Intersects(a, b)` | 0/1 | ✅ | ✅ | DE-9IM. kenro errors on GeometryCollection operands (PostGIS's `ST_Intersects` accepts them; its `ST_Contains`/`ST_Within` also error) |
| `ST_Contains(a, b)` | 0/1 | ✅ | ✅ | Boundary semantics golden-tested against PostGIS |
| `ST_Within(a, b)` | 0/1 | ✅ | ✅ | `ST_Within(a,b) = ST_Contains(b,a)`, property-tested |
| `ST_Distance(a, b)` | REAL | ✅ | ✅ | 2D cartesian; NULL for empty inputs |
| `ST_DWithin(a, b, d)` | 0/1 | ✅ | ✅ | `distance <= d`; negative tolerance errors (matches PostGIS) |
| **GeoPackage R-tree** | | | | |
| `ST_MinX` / `ST_MaxX` / `ST_MinY` / `ST_MaxY` | REAL | ⚠️ named `ST_XMin` … | ⚠️ named `ST_XMin` … | kenro uses the GeoPackage spec's trigger names (Annex F.3) — required verbatim for gpkg index maintenance; the other two spell it `ST_XMin` |
| `ST_IsEmpty(geom)` | 0/1 | ✅ | ✅ | gpkg R-tree contract; NULL on NULL |
| **H3 cells** (`h3` feature) | | | | |
| `h3_latlng_to_cell(geom, res)` | INT | via [h3-pg] ext | via community `h3` ext | Same name in all three ecosystems; POINT in lon/lat only |
| `h3_cell_to_parent(cell, res)` | INT | via h3-pg | via `h3` ext | For coarser `GROUP BY` |
| `h3_cell_to_string(cell)` / `h3_string_to_cell(text)` | TEXT / INT | ⚠️ h3-pg casts its `h3index` type | ⚠️ DuckDB: `h3_h3_to_string` | Hex-string conversion names differ per ecosystem; kenro uses the H3 v4 canonical verbs |
| **Accessors** | | | | |
| `ST_Area(geom)` | REAL | ✅ | ✅ | Planar; 0 for non-areal or empty |
| `ST_Length(geom)` | REAL | ✅ | ✅ | Linear geometries only — polygons return 0 in all three |
| `ST_Centroid(geom)` | geometry | ✅ | ✅ | Empty input → `POINT EMPTY` |
| `ST_Envelope(geom)` | geometry | ✅ | ✅ | Degenerates to POINT/LINESTRING exactly like PostGIS. DuckDB also has scalar `ST_Extent` → `BOX_2D` |
| `ST_X(geom)` / `ST_Y(geom)` | REAL | ✅ | ✅ | POINT only, error otherwise (as in PostGIS); `POINT EMPTY` → NULL |
| `ST_NumPoints(geom)` | INT / NULL | ✅ | ⚠️ synonym of `ST_NPoints` | LINESTRING-only, NULL for other types (PostGIS as implemented; its docs lag). **Same name, different answer in DuckDB** (counts all vertices of any type) — kenro follows PostGIS |
| `ST_IsValid(geom)` | 0/1 | ✅ GEOS | ✅ GEOS | georust validation: everything incl. ring self-intersection and hole placement, except the split-interior case (documented gap) |
| `ST_Simplify(geom, tol)` | geometry | ✅ + `preserveCollapsed` arg | ✅ 2-arg; also `ST_SimplifyPreserveTopology` | Ramer-Douglas-Peucker, collapse allowed (= PostGIS 2-arg form) |
| **Stubs — planned** (call = helpful error) | | | | |
| `ST_NPoints` | stub | ✅ | ✅ | Counts all vertices; stubbed separately from `ST_NumPoints` to keep the semantics split visible |
| `ST_Perimeter` | stub | ✅ | ✅ | |
| `ST_AsMVT` / `ST_AsMVTGeom` | stub | ✅ (aggregate) | ✅ | geozero has an MVT writer, but PostGIS's `ST_AsMVT` is an aggregate — kenro needs aggregate-function support first |
| **Stubs — deliberately excluded** (GEOS-class) | | | | |
| `ST_Buffer` / `ST_Union` / `ST_Intersection` / `ST_Difference` | stub | ✅ | ✅ | Computational geometry needs GEOS's muscle; kenro's error message points you to SpatiaLite / DuckDB spatial |
| `ST_SymDifference` | stub | ✅ | ❌ | Not in DuckDB spatial either |

All implemented functions are **deterministic and pure** (no I/O, no clock,
no randomness) and NULL-strict (NULL in → NULL out). Malformed input raises
an explicit error prefixed `kenro:` — never a silent NULL.

Calling a stub raises a helpful error instead of SQLite's
`no such function`:

```
kenro: ST_Buffer is not implemented in kenro. kenro deliberately excludes
GEOS-class operations; use SpatiaLite or DuckDB spatial for this.
```

Also out of scope: raster, network analysis, full PROJ grid transforms, and
any claim of full SpatiaLite/PostGIS compatibility.

## Semantics: PostGIS is the reference

Function names, signatures, and semantics follow PostGIS (SQL/MM `ST_`
prefix). Results are validated against PostGIS-generated golden vectors
committed in this repo (`tests/golden/*.jsonl` — 270+ vectors across
predicates, transforms, GeoJSON, and accessors; H3 vectors come from the
reference C library). Where kenro deviates, it does so **loudly and
documentedly** — never a silently different result. The cross-cutting
divergences:

- **`POINT EMPTY`** cannot be constructed from WKT/GeoJSON (the underlying
  geometry model cannot represent it) — reading one from a GeoPackage/WKB
  blob works, and `ST_AsText` prints `POINT EMPTY` like PostGIS.
- **3D/M geometries** are accepted as *input* to predicates and R-tree
  functions (2D result, same as PostGIS); output and constructor functions
  raise an error rather than silently dropping Z/M.
- **GeometryCollection** operands error in all predicates (PostGIS accepts
  them in `ST_Intersects` only).
- **SRID leniency**: a geometry with a known SRID can meet one with unknown
  SRID (0) in a predicate — needed for
  `ST_Within(gpkg_col, ST_GeomFromText(…))`. Mixed *known* SRIDs still
  error, like PostGIS.
- **Bare WKB blobs** are accepted anywhere a geometry is expected
  (auto-detection; PostGIS would require a cast).

Divergent golden vectors carry a `kenro_expected` + `note` field in the
vector files; those files are the source of truth for this list and the
table above.

## Choosing kenro vs PostGIS vs DuckDB Spatial

Structural differences that matter more than any single function:

- **SRID model** — PostGIS geometries and kenro's GeoPackage blobs both
  carry their SRID; DuckDB's `GEOMETRY` does not, so CRS bookkeeping is the
  user's job there (and `always_xy` axis-order care is needed for EPSG:4326).
- **Where it runs / weight** — kenro lives *inside* SQLite: pure Rust,
  KB–MB scale, no C toolchain, deterministic. PostGIS is a server-side
  PostgreSQL extension. DuckDB spatial bundles GEOS + PROJ + GDAL (its
  WASM build is ~23.5 MB uncompressed, ~6.3 MB over the wire).
- **Division of labor** — heavyweight analytics, overlays and format
  conversion belong to DuckDB spatial or PostGIS; predicates, R-tree
  maintenance and CRS transforms *inside your app's SQLite file* are
  kenro's seat. They compose rather than compete.

## Supported CRS

`ST_Transform` uses a curated EPSG table (proj4rs carries no EPSG database):

| Codes | System |
|---|---|
| 4326 | WGS84 geographic |
| 3857 | Web Mercator |
| 4612 / 6668 | JGD2000 / JGD2011 geographic |
| 2443–2461 | JGD2000 plane rectangular zones I–XIX |
| 6669–6687 | JGD2011 plane rectangular zones I–XIX |
| 32651–32656 | WGS84 UTM zones 51N–56N |

Anything else raises an error naming the code. The `crs-full` cargo feature
adds the full `crs-definitions` registry as a fallback (megabytes of tables,
EPSG codes ≤ 65535 only). Accuracy against PROJ is measured and documented
in [docs/accuracy.md](docs/accuracy.md) — TL;DR: nanometer-level for
projection math, 0.1 mm-level for the JGD Helmert pairs, but **no datum
grids**: real-world JGD2000↔JGD2011 displacement is not modeled; use full
PROJ for survey-grade work.

## Cargo features

Everything is on by default: `transform` (proj4rs), `h3` (h3o), `geojson`.
Building with a feature off keeps the corresponding SQL functions registered
as stubs that explain which feature is missing. `rusqlite` (off by default)
enables `kenro::register`.

## Requirements & caveats

- SQLite ≥ 3.31 (for `SQLITE_INNOCUOUS`, which lets kenro functions run inside the GeoPackage R-tree triggers under `PRAGMA trusted_schema=off`); the loadable-extension path needs SQLite ≥ 3.34 (it fails with a clear version-mismatch message on older hosts).
- Loading kenro **and** SpatiaLite into the same connection: both register `ST_` names and SQLite keeps the last registration. Don't mix them (a registration-filter feature flag can be added if needed).
- `ST_Distance` is 2D cartesian in the geometry's coordinate system. For meters over lon/lat, reproject first (e.g. `ST_Transform(geom, 6677)` for the Tokyo area).

## Roadmap

1. ✅ Core: GeoPackage blobs, WKB/WKT, predicates, R-tree functions, rusqlite registration, PostGIS golden tests
2. ✅ `ST_Transform` (proj4rs; JGD2000/JGD2011/WGS84 accuracy [measured and documented](docs/accuracy.md)), H3 cell IDs, GeoJSON, accessors
3. ✅ `kenro-ext`: loadable extension (`.so`/`.dylib`/`.dll`) for Python / Node / sqlite3 CLI
4. `kenro-wasm`: sql.js / wa-sqlite builds, browser demo
5. v0.x releases on crates.io

## License

MIT OR Apache-2.0, at your option.

[georust/geo]: https://github.com/georust/geo
[proj4rs]: https://github.com/3liz/proj4rs
[h3-pg]: https://github.com/zachasme/h3-pg
