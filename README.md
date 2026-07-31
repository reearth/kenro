# kenro（間縄）

**Spatial functions for SQLite in pure Rust** — works with rusqlite today; loadable extension and WASM builds are on the roadmap.

If you searched for *rusqlite spatial*, *SQLite spatial functions without SpatiaLite*, or *GeoPackage in pure Rust*: this is that crate.

kenro provides the 20% of SpatiaLite everyone actually uses, with zero C dependencies and one-call registration:

- **WKB / WKT / GeoJSON / GeoPackage-blob I/O** — `ST_GeomFromText`, `ST_GeomFromWKB`, `ST_GeomFromGPB`, `ST_GeomFromGeoJSON`, `ST_AsText`, `ST_AsBinary`, `ST_AsGPB`, `ST_AsGeoJSON`
- **Predicates** — `ST_Intersects`, `ST_Contains`, `ST_Within`, `ST_Distance`, `ST_DWithin` (DE-9IM via [georust/geo])
- **GeoPackage R-tree support** — `ST_MinX`, `ST_MaxX`, `ST_MinY`, `ST_MaxY`, `ST_IsEmpty`: the exact function set the GeoPackage spec's R-tree maintenance triggers require
- **CRS transform** — `ST_Transform` / `ST_SetSRID` / `ST_SRID` via pure-Rust [proj4rs], with first-class support for the Japanese national systems (JGD2000/JGD2011, plane rectangular I–XIX) and [measured accuracy](docs/accuracy.md)
- **H3 cells** — `h3_latlng_to_cell` & friends (h3-pg naming) for mesh aggregation in `GROUP BY`
- **Accessors** — `ST_Area`, `ST_Length`, `ST_Centroid`, `ST_Envelope`, `ST_X`, `ST_Y`, `ST_NumPoints`, `ST_IsValid`, `ST_Simplify`

That last set is the headline: **with kenro registered, a plain SQLite build maintains a GeoPackage spatial index correctly.** No SpatiaLite, no GDAL, no C toolchain.

> 間縄 (kenro): the measuring rope used in historical Japanese land surveys — the tool for turning land into ledgers.

## Quickstart

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

## Functions

| Function | Returns | Notes |
|---|---|---|
| `ST_GeomFromText(wkt [, srid])` | geometry | PostGIS signature; `POINT EMPTY` is rejected (see diffs) |
| `ST_GeomFromWKB(wkb [, srid])` | geometry | Accepts ISO WKB and EWKB; explicit srid wins |
| `ST_GeomFromGPB(gpb)` | geometry | Validates and normalizes a GeoPackage blob |
| `ST_AsText(geom)` | TEXT | WKT |
| `ST_AsBinary(geom)` | BLOB | ISO WKB, little-endian, no SRID (as in PostGIS) |
| `ST_AsGPB(geom)` | BLOB | Storage-grade GeoPackage blob (envelope included) — use for writing gpkg columns |
| `ST_Intersects(a, b)` / `ST_Contains(a, b)` / `ST_Within(a, b)` | 0/1 | DE-9IM |
| `ST_Distance(a, b)` | REAL | 2D cartesian; NULL for empty inputs |
| `ST_DWithin(a, b, d)` | 0/1 | `distance <= d` |
| `ST_MinX/ST_MaxX/ST_MinY/ST_MaxY(geom)` | REAL | GeoPackage R-tree contract; NULL for empty |
| `ST_IsEmpty(geom)` | 0/1 | GeoPackage R-tree contract |
| `ST_Transform(geom, to_srid)` | BLOB | Source CRS = the geometry's SRID (PostGIS-exact 2-arg form); see [Supported CRS](#supported-crs) |
| `ST_SetSRID(geom, srid)` / `ST_SRID(geom)` | BLOB / INT | Relabel (no reprojection) / read; 0 = unknown |
| `ST_AsGeoJSON(geom [, maxdecimaldigits])` | TEXT | Default 9 digits; byte-identical to PostGIS output (golden-tested) |
| `ST_GeomFromGeoJSON(text)` | BLOB | SRID 4326 per RFC 7946 (PostGIS ≥ 3.0 behavior) |
| `h3_latlng_to_cell(geom, res)` | INT | H3 cell of a lon/lat POINT ([h3-pg] naming); pairs with `h3_cell_to_parent(cell, res)`, `h3_cell_to_string(cell)`, `h3_string_to_cell(text)` |
| `ST_Area` / `ST_Length(geom)` | REAL | Planar; polygons have length 0 (as in PostGIS) |
| `ST_Centroid` / `ST_Envelope(geom)` | BLOB | Envelope degenerates to POINT/LINESTRING like PostGIS |
| `ST_X` / `ST_Y(geom)` | REAL | POINT only (error otherwise, as in PostGIS) |
| `ST_NumPoints(geom)` | INT | LINESTRING only, NULL otherwise (PostGIS semantics — distinct from `ST_NPoints`) |
| `ST_IsValid(geom)` | 0/1 | georust validation (see diff table) |
| `ST_Simplify(geom, tol)` | BLOB | Ramer-Douglas-Peucker |

"Geometry" values are GeoPackage blobs (they carry the SRID, and a value in a gpkg column is already valid storage). Every geometry-accepting function also auto-detects raw WKB input, so `ST_Within(p.geom, …)` works directly on a gpkg column.

All functions are **deterministic and pure** (no I/O, no clock, no randomness) and NULL-strict (NULL in → NULL out). Malformed input raises an explicit error prefixed `kenro:` — never a silent NULL.

### Not implemented — on purpose

Calling a known-but-unimplemented `ST_` function raises a helpful error instead of SQLite's `no such function`, e.g.:

```
kenro: ST_Buffer is not implemented in kenro. kenro deliberately excludes
GEOS-class operations; use SpatiaLite or DuckDB spatial for this.
```

- **Never**: `ST_Buffer`, `ST_Union`, `ST_Intersection`, `ST_Difference`, `ST_SymDifference` — GEOS-class computational geometry. Use SpatiaLite or DuckDB spatial; kenro stays small.
- **Planned**: `ST_NPoints`, `ST_Perimeter`, `ST_AsMVT` / `ST_AsMVTGeom` (geozero has an MVT writer, so tile generation is on the table — but PostGIS's ST_AsMVT is an aggregate, which needs aggregate-function support first).
- Also out of scope: raster, network analysis, full PROJ grid transforms, and any claim of full SpatiaLite/PostGIS compatibility.

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

## PostGIS is the reference

Function names, signatures, and semantics follow PostGIS (SQL/MM `ST_` prefix). Predicates are validated against PostGIS-generated golden vectors committed in this repo (`tests/golden/predicates.jsonl`). Where kenro deviates, it does so **loudly and documentedly** — never a silently different result:

| Case | PostGIS | kenro |
|---|---|---|
| `ST_GeomFromText('POINT EMPTY')` | empty point | error (the underlying geometry model cannot represent it) |
| 3D/M geometries | supported | accepted as *input* to predicates and R-tree functions (2D result, same as PostGIS); output/constructor functions raise an error — incl. `ST_GeomFromGeoJSON` with 3D positions |
| `GeometryCollection` in predicates | `ST_Intersects` works; `ST_Contains`/`ST_Within` error | error for all predicates |
| Geometry with SRID vs geometry with unknown SRID (0) | error | proceeds (needed for `… AND ST_Within(gpkg_col, ST_GeomFromText(…))`) — mixed *known* SRIDs still error |
| bare WKB blob passed to a predicate | needs a cast | accepted (auto-detection) |
| `ST_Transform` to an EPSG outside the curated table | works (full PROJ database) | error naming the code (see [Supported CRS](#supported-crs)) |
| `ST_IsValid` interior-connectivity check | full GEOS validation | georust validation — the split-interior case is not detected (everything else, incl. ring self-intersection and hole placement, is) |

Divergent golden vectors carry a `kenro_expected` + `note` field in `predicates.jsonl`; that file is the source of truth for this table.

## Comparison: kenro vs PostGIS vs DuckDB Spatial

The three tools occupy different niches — this table is for choosing the
right one, not for claiming parity. Verified against PostGIS 3.5 and DuckDB
1.4.0 + spatial (July 2026); function presence checked against the official
docs and a live DuckDB session.

| Function | kenro | PostGIS | DuckDB Spatial | Notes |
|---|---|---|---|---|
| `ST_GeomFromText` / `ST_AsText` | ✅ | ✅ | ✅ | DuckDB's constructor takes no srid argument (see SRID row below) |
| `ST_GeomFromWKB` | ✅ | ✅ | ✅ | |
| `ST_AsBinary` | ✅ | ✅ | ❌ — named `ST_AsWKB` | PostGIS conversely has no `ST_AsWKB` |
| `ST_GeomFromGPB` / `ST_AsGPB` (GeoPackage blobs) | ✅ | ❌ | ❌ | kenro operates on gpkg geometry BLOBs in SQL and maintains the gpkg R-tree; DuckDB imports gpkg **files** via GDAL `ST_Read`; PostGIS needs ogr2ogr |
| `ST_Intersects` / `ST_Contains` / `ST_Within` / `ST_Distance` / `ST_DWithin` | ✅ | ✅ | ✅ | |
| bbox accessors | `ST_MinX` … | `ST_XMin` … | `ST_XMin` … | Three-way naming split: kenro uses the GeoPackage spec's trigger names (Annex F.3), the other two use `ST_XMin` |
| `ST_IsEmpty` | ✅ | ✅ | ✅ | |
| `ST_Transform` | ✅ `(geom, to_srid)` — source = embedded SRID; curated EPSG table ([accuracy](docs/accuracy.md)) | ✅ 4 overloads, full PROJ database | ✅ but `(geom, source_crs, target_crs [, always_xy])` — source must be spelled out every call | DuckDB's `GEOMETRY` carries **no SRID at all**, so its signature cannot match PostGIS; kenro matches PostGIS |
| `ST_SetSRID` / `ST_SRID` | ✅ | ✅ | ❌ | Consequence of DuckDB's SRID-less geometry type |
| `ST_AsGeoJSON` | ✅ byte-identical to PostGIS (golden-tested) | ✅ | ✅ returns a JSON fragment | |
| `ST_GeomFromGeoJSON` | ✅ (2D only) | ✅ (keeps Z) | ✅ | |
| H3 cells | ✅ built-in (`h3` feature) | via [h3-pg] extension | via community `h3` extension | `h3_latlng_to_cell` is common to all three; cell→string differs: kenro `h3_cell_to_string`, h3-pg casts the `h3index` type, DuckDB `h3_h3_to_string` |
| `ST_Area` / `ST_Length` / `ST_Centroid` / `ST_Envelope` / `ST_X` / `ST_Y` / `ST_IsValid` | ✅ | ✅ | ✅ | All three: polygons have `ST_Length` 0. DuckDB additionally has scalar `ST_Extent` returning a `BOX_2D` |
| `ST_NumPoints` | ✅ LINESTRING-only, NULL otherwise | same (as implemented; its docs lag) | ⚠️ synonym of `ST_NPoints` — counts **all** vertices of any type | Same name, different answer between DuckDB and PostGIS/kenro — kenro follows PostGIS |
| `ST_NPoints` | stub (planned) | ✅ | ✅ | |
| `ST_Simplify` | ✅ RDP, collapse allowed | ✅ + `preserveCollapsed` arg | ✅ no third arg; also `ST_SimplifyPreserveTopology` | |
| `ST_Buffer` / `ST_Union` / `ST_Intersection` / `ST_Difference` | ❌ deliberate (GEOS-class) | ✅ | ✅ | kenro's stubs point you to the other two |
| `ST_SymDifference` | ❌ deliberate | ✅ | ❌ not implemented | |

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

## Requirements & caveats

- SQLite ≥ 3.31 (for `SQLITE_INNOCUOUS`, which lets kenro functions run inside the GeoPackage R-tree triggers under `PRAGMA trusted_schema=off`).
- Loading kenro **and** SpatiaLite into the same connection: both register `ST_` names and SQLite keeps the last registration. Don't mix them (a registration-filter feature flag can be added if needed).
- `ST_Distance` is 2D cartesian in the geometry's coordinate system. For meters over lon/lat, reproject first (e.g. `ST_Transform(geom, 6677)` for the Tokyo area).

## Roadmap

1. ✅ Core: GeoPackage blobs, WKB/WKT, predicates, R-tree functions, rusqlite registration, PostGIS golden tests
2. ✅ `ST_Transform` (proj4rs; JGD2000/JGD2011/WGS84 accuracy [measured and documented](docs/accuracy.md)), H3 cell IDs, GeoJSON, accessors
3. `kenro-ext`: loadable extension (`.so`/`.dylib`/`.dll`) for Python / Node / sqlite3 CLI
4. `kenro-wasm`: sql.js / wa-sqlite builds, browser demo
5. v0.x releases on crates.io

## License

MIT OR Apache-2.0, at your option.

[georust/geo]: https://github.com/georust/geo
[proj4rs]: https://github.com/3liz/proj4rs
[h3-pg]: https://github.com/zachasme/h3-pg
