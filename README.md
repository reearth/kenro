# kenro（間縄）

**Spatial functions for SQLite in pure Rust** — works with rusqlite, as a loadable extension (Python / Node / sqlite3 CLI), and in the browser (sql.js / wa-sqlite / official SQLite WASM).

If you searched for *rusqlite spatial*, *SQLite spatial functions without SpatiaLite*, or *GeoPackage in pure Rust*: this is that crate.

kenro provides the 20% of SpatiaLite everyone actually uses, with zero C dependencies and one-call registration:

- **Geometry I/O** — WKT, WKB, GeoJSON, and GeoPackage blobs as first-class citizens
- **Predicates** — the full DE-9IM family: `ST_Intersects` / `ST_Contains` / `ST_Within` / `ST_Touches` / `ST_Crosses` / `ST_Overlaps` / `ST_Equals` / `ST_Covers` / `ST_Relate`, plus `ST_Distance` / `ST_DWithin` (via [georust/geo])
- **Overlay & buffer** — `ST_Intersection` / `ST_Union` (scalar *and* aggregate) / `ST_Difference` / `ST_SymDifference` / `ST_Buffer` in pure Rust, with the differences vs GEOS quantified by golden tests
- **GeoPackage support** — the exact function set the spec's R-tree (F.3) and geometry-type (F.4) maintenance triggers require
- **CRS transform** — pure-Rust [proj4rs]: WGS84, Web Mercator and every UTM zone built in, the full EPSG registry behind a feature flag, with [measured accuracy](docs/accuracy.md)
- **H3 cells** — mesh aggregation in `GROUP BY` ([h3-pg] naming)
- **Vector tiles** — `ST_AsMVTGeom` + the `ST_AsMVT` aggregate with a hand-rolled, dependency-free encoder
- **Accessors, measures, processing** — area, length, centroid, convex hull, line interpolation, simplification, affine transforms, …

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

## Quickstart (browser — kenro-wasm)

Browser SQLite builds can't load native extensions, but they all accept
JS-level user-defined functions — so kenro's SQLite-free core compiles to a
**~950 KB wasm (~350 KB wire)** module with one adapter per host:

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
A drag-a-GeoPackage-and-query demo lives in `crates/kenro-wasm/demo/`
(`./serve.sh` after building; deploys to GitHub Pages once the repo is
public).

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
| **Constructors** | | | | |
| `ST_MakePoint(x, y)` | geometry | ✅ | ✅ | 2D only |
| `ST_Point(x, y [, srid])` | geometry | ✅ | ⚠️ no srid arg | The srid form is PostGIS 3.2+ |
| `ST_MakeEnvelope(xmin, ymin, xmax, ymax [, srid])` | geometry | ✅ | ⚠️ no srid arg | Degenerate extents still return a POLYGON, like PostGIS |
| **Predicates** | | | | |
| `ST_Intersects(a, b)` | 0/1 | ✅ | ✅ | DE-9IM. kenro errors on GeometryCollection operands (PostGIS's `ST_Intersects` accepts them; its `ST_Contains`/`ST_Within` also error) |
| `ST_Contains(a, b)` | 0/1 | ✅ | ✅ | Boundary semantics golden-tested against PostGIS |
| `ST_Within(a, b)` | 0/1 | ✅ | ✅ | `ST_Within(a,b) = ST_Contains(b,a)`, property-tested |
| `ST_Disjoint(a, b)` | 0/1 | ✅ | ✅ | Empty operands → true (golden-arbitrated PostGIS behavior) |
| `ST_Touches(a, b)` / `ST_Crosses(a, b)` / `ST_Overlaps(a, b)` | 0/1 | ✅ | ✅ | |
| `ST_Equals(a, b)` | 0/1 | ✅ | ✅ | Topological equality; both-empty → true |
| `ST_Covers(a, b)` / `ST_CoveredBy(a, b)` | 0/1 | ✅ | ✅ | The boundary-tolerant contains/within variants |
| `ST_Relate(a, b)` | TEXT | ✅ | ❌ | The 9-character DE-9IM matrix |
| `ST_Relate(a, b, pattern)` | 0/1 | ✅ | ❌ | Pattern matching with `*`/`T`/`F`/`0`/`1`/`2` |
| **Measures** | | | | |
| `ST_Distance(a, b)` | REAL | ✅ | ✅ | 2D cartesian; NULL for empty inputs |
| `ST_DWithin(a, b, d)` | 0/1 | ✅ | ✅ | `distance <= d`; negative tolerance errors (matches PostGIS) |
| `ST_ClosestPoint(a, b)` | geometry | ✅ any × any | ❌ (`ST_ShortestLine` instead) | kenro: second operand must be a POINT (geo API limit) — anything else errors |
| `ST_LineInterpolatePoint(line, fraction)` | geometry | ✅ | ✅ | Fraction outside [0, 1] errors, like PostGIS |
| `ST_LineLocatePoint(line, point)` | REAL | ✅ | ✅ | |
| `ST_HausdorffDistance(a, b)` | REAL | ✅ | ❌ | kenro computes vertex-to-vertex distance (geo API), GEOS vertex-to-segment — equal on shared golden vectors, can differ on long sparse segments |
| `ST_FrechetDistance(a, b)` | REAL | ✅ + densify arg | ❌ | kenro: LINESTRING × LINESTRING, 2-arg form only |
| `ST_Azimuth(a, b)` | REAL | ✅ | ✅ | Radians clockwise from north; coincident points → NULL |
| **Overlay & buffer** (pure Rust — see [semantics](#semantics-postgis-is-the-reference)) | | | | |
| `ST_Intersection(a, b)` | geometry | ✅ | ✅ | Areal results only: polygons that merely touch → empty, where GEOS returns the shared LINESTRING. line × line errors (needs noding) |
| `ST_Difference(a, b)` | geometry | ✅ | ✅ | Same decision matrix; point operands are filtered exactly |
| `ST_SymDifference(a, b)` | geometry | ✅ | ❌ | areal × areal and puntal × puntal; mixed dimensions error |
| `ST_Union(a, b)` | geometry | ✅ | ✅ | Scalar form; line unions error (noding) |
| `ST_Union(geom)` **aggregate** | geometry | ✅ | ⚠️ named `ST_Union_Agg` | Dissolve in `GROUP BY`; NULL rows skipped, zero rows → NULL (PostGIS aggregate semantics) |
| `ST_Buffer(geom, d [, opts])` | geometry | ✅ | ⚠️ 3rd arg differs | PostGIS-style options TEXT (`quad_segs= endcap= join= mitre_limit=`) or INTEGER quad_segs; `side=` unsupported. Negative distance erodes polygons. Golden-tested within 2% area of GEOS |
| **Processing & affine** | | | | |
| `ST_ConvexHull(geom)` | geometry | ✅ | ✅ | Degenerate hulls collapse to POINT/LINESTRING like PostGIS |
| `ST_PointOnSurface(geom)` | geometry | ✅ | ✅ | Guaranteed interior; exact coordinates may differ from GEOS (documented in vectors) |
| `ST_Simplify(geom, tol)` | geometry | ✅ + `preserveCollapsed` arg | ✅ 2-arg; also `ST_SimplifyPreserveTopology` | Ramer-Douglas-Peucker, collapse allowed (= PostGIS 2-arg form) |
| `ST_SimplifyVW(geom, tol)` | geometry | ✅ | ❌ | Visvalingam-Whyatt; tolerance is an area |
| `ST_ChaikinSmoothing(geom [, iterations])` | geometry | ✅ | ❌ | PostGIS variant (endpoints preserved on open lines); iterations capped at 5 |
| `ST_RemoveRepeatedPoints(geom)` | geometry | ✅ + tolerance arg | ✅ | kenro: exact duplicates only (no tolerance form) |
| `ST_OrientedEnvelope(geom)` | geometry | ✅ | ⚠️ named `ST_MinimumRotatedRectangle` | Minimum rotated rectangle; equal-area alternates possible (rotation-normalized comparison in goldens) |
| `ST_Rotate(geom, radians [, x0, y0])` | geometry | ✅ | ✅ | About the origin (or the given point) — PostGIS semantics, **not** geo's centroid default |
| `ST_Translate(geom, dx, dy)` | geometry | ✅ | ✅ | |
| `ST_Scale(geom, xf, yf)` | geometry | ✅ | ✅ | About the origin, like PostGIS |
| **GeoPackage triggers** | | | | |
| `ST_MinX` / `ST_MaxX` / `ST_MinY` / `ST_MaxY` | REAL | ⚠️ named `ST_XMin` … | ⚠️ named `ST_XMin` … | kenro uses the GeoPackage spec's trigger names (Annex F.3) — required verbatim for gpkg index maintenance; the other two spell it `ST_XMin` |
| `ST_IsEmpty(geom)` | 0/1 | ✅ | ✅ | gpkg R-tree contract; NULL on NULL |
| `GPKG_IsAssignable(expected, actual)` | 0/1 | ❌ | ❌ | kenro-only: the geometry-type-trigger helper (Annex F.4); accepts both `'POINT'` and `'ST_Point'` spellings so the spec DDL works with kenro's `ST_GeometryType` |
| **H3 cells** (`h3` feature) | | | | |
| `h3_latlng_to_cell(geom, res)` | INT | via [h3-pg] ext | via community `h3` ext | Same name in all three ecosystems; POINT in lon/lat only |
| `h3_cell_to_parent(cell, res)` | INT | via h3-pg | via `h3` ext | For coarser `GROUP BY` |
| `h3_cell_to_string(cell)` / `h3_string_to_cell(text)` | TEXT / INT | ⚠️ h3-pg casts its `h3index` type | ⚠️ DuckDB: `h3_h3_to_string` | Hex-string conversion names differ per ecosystem; kenro uses the H3 v4 canonical verbs |
| **Accessors** | | | | |
| `ST_Area(geom)` | REAL | ✅ | ✅ | Planar; 0 for non-areal or empty |
| `ST_Length(geom)` | REAL | ✅ | ✅ | Linear geometries only — polygons return 0 in all three |
| `ST_Perimeter(geom)` | REAL | ✅ | ✅ | Ring lengths; 0 for non-areal |
| `ST_Centroid(geom)` | geometry | ✅ | ✅ | Empty input → `POINT EMPTY` |
| `ST_Envelope(geom)` | geometry | ✅ | ✅ | Degenerates to POINT/LINESTRING exactly like PostGIS. DuckDB also has scalar `ST_Extent` → `BOX_2D` |
| `ST_X(geom)` / `ST_Y(geom)` | REAL | ✅ | ✅ | POINT only, error otherwise (as in PostGIS); `POINT EMPTY` → NULL |
| `ST_NPoints(geom)` | INT | ✅ | ✅ | All vertices of any type; empty → 0 |
| `ST_NumPoints(geom)` | INT / NULL | ✅ | ⚠️ synonym of `ST_NPoints` | LINESTRING-only, NULL for other types (PostGIS as implemented; its docs lag). **Same name, different answer in DuckDB** (counts all vertices of any type) — kenro follows PostGIS |
| `ST_GeometryType(geom)` | TEXT | ✅ | ⚠️ returns `POINT` | kenro returns the PostGIS spelling `ST_Point`; DuckDB returns bare `POINT` |
| `ST_NumGeometries(geom)` | INT | ✅ | ✅ | Single geometry → 1; empty → 0 |
| `ST_GeometryN(geom, n)` | geometry / NULL | ✅ | ❌ | 1-based; out of range → NULL; GeometryCollection supported |
| `ST_StartPoint(geom)` / `ST_EndPoint(geom)` | geometry / NULL | ✅ | ✅ | PostGIS 3.2 semantics: POINT → itself, MULTILINESTRING works, areal → NULL |
| `ST_PointN(line, n)` | geometry / NULL | ✅ | ✅ | 1-based with negative indexing from the end |
| `ST_Reverse(geom)` | geometry | ✅ | ✅ | Member order of multi geometries preserved |
| `ST_IsValid(geom)` | 0/1 | ✅ GEOS | ✅ GEOS | georust validation: everything incl. ring self-intersection and hole placement, except the split-interior case (documented gap) |
| **Vector tiles (MVT)** | | | | |
| `ST_AsMVTGeom(geom, bounds [, extent [, buffer [, clip]]])` | geometry / NULL | ✅ | ✅ | World → integer tile coordinates (Y down); clipped-away input → NULL. `bounds` is any geometry (its envelope is used). ±1 pixel vs PostGIS at tile edges (kenro clips before grid snapping, PostGIS after) |
| `ST_AsMVT(geom [, name [, extent [, props_json]]])` **aggregate** | BLOB | ⚠️ record-based signature | ⚠️ | **Deliberate signature divergence**: SQLite has no record type, so properties come from `json_object(...)` instead of row columns. A PostGIS-style call fails loudly at the type level. Layer name/extent must be constant per group |
| **Stubs — planned** (call = helpful error) | | | | |
| `ST_MakeValid` | stub | ✅ | ✅ | geo's polygon repair is triangulation-based and structurally different from GEOS — wiring it up would produce confusingly different geometry |

All implemented functions are **deterministic and pure** (no I/O, no clock,
no randomness) and NULL-strict (NULL in → NULL out; aggregates skip NULL
rows instead, following PostGIS aggregate semantics). Malformed input raises
an explicit error prefixed `kenro:` — never a silent NULL.

Calling a stub raises a helpful error instead of SQLite's
`no such function`:

```
kenro: ST_MakeValid is not implemented in kenro. geo has no GEOS-equivalent
MakeValid (its polygon repair is triangulation-based and structurally
different); validate with ST_IsValid and repair in PostGIS for now.
```

Also out of scope: raster, network analysis, full PROJ grid transforms, and
any claim of full SpatiaLite/PostGIS compatibility.

## Semantics: PostGIS is the reference

Function names, signatures, and semantics follow PostGIS (SQL/MM `ST_`
prefix). Results are validated against PostGIS-generated golden vectors
committed in this repo (`tests/golden/*.jsonl` — 700+ vectors across nine
suites: predicates, transforms, GeoJSON, accessors, processing, overlay,
buffer, and MVT; H3 vectors come from the reference C library, MVT tiles
are cross-decoded by two independent decoders). Where kenro deviates, it
does so **loudly and documentedly** — never a silently different result.
The cross-cutting divergences:

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

And for the overlay family specifically (pure Rust via geo's BooleanOps on
[i_overlay], which — like geo itself — does not aim for bit-level GEOS
compatibility):

- **Areal results only**: two polygons that merely touch along an edge
  intersect to an *empty polygon* in kenro, where GEOS returns the shared
  `LINESTRING`. Point operands are filtered exactly (no divergence).
- **Unsupported combinations error instead of guessing**: line × line
  overlays (which need noding) and mixed-dimension combinations (which
  produce GeometryCollections in PostGIS) raise a `kenro:` error naming the
  operand classes — never a wrong-looking answer.
- **Vertex chains differ**: i_overlay snaps to an internal integer grid
  (~1e-8 relative), so overlay results agree with GEOS by
  symmetric-difference area (golden bound 1e-6 ratio), not vertex-by-vertex;
  `ST_Buffer` agrees within 2% area.

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
- **Division of labor** — heavyweight analytics, exotic GEOS operations and
  format conversion belong to DuckDB spatial or PostGIS; predicates, R-tree
  maintenance, CRS transforms, everyday overlays/buffers and MVT generation
  *inside your app's SQLite file* are kenro's seat. They compose rather
  than compete.

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

Everything is on by default: `transform` (proj4rs), `h3` (h3o), `geojson`.
Building with a feature off keeps the corresponding SQL functions registered
as stubs that explain which feature is missing. `rusqlite` (off by default)
enables `kenro::register`.

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
6. v0.x releases on crates.io (+ npm for kenro-wasm, GitHub Pages for the demo)

## License

MIT OR Apache-2.0, at your option.

[georust/geo]: https://github.com/georust/geo
[proj4rs]: https://github.com/3liz/proj4rs
[h3-pg]: https://github.com/zachasme/h3-pg
[i_overlay]: https://github.com/iShape-Rust/iOverlay
