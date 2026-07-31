# kenro（間縄）

**Spatial functions for SQLite in pure Rust** — works with rusqlite today; loadable extension and WASM builds are on the roadmap.

If you searched for *rusqlite spatial*, *SQLite spatial functions without SpatiaLite*, or *GeoPackage in pure Rust*: this is that crate.

kenro provides the 20% of SpatiaLite everyone actually uses, with zero C dependencies and one-call registration:

- **WKB / WKT / GeoPackage-blob I/O** — `ST_GeomFromText`, `ST_GeomFromWKB`, `ST_GeomFromGPB`, `ST_AsText`, `ST_AsBinary`, `ST_AsGPB`
- **Predicates** — `ST_Intersects`, `ST_Contains`, `ST_Within`, `ST_Distance`, `ST_DWithin` (DE-9IM via [georust/geo])
- **GeoPackage R-tree support** — `ST_MinX`, `ST_MaxX`, `ST_MinY`, `ST_MaxY`, `ST_IsEmpty`: the exact function set the GeoPackage spec's R-tree maintenance triggers require

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

"Geometry" values are GeoPackage blobs (they carry the SRID, and a value in a gpkg column is already valid storage). Every geometry-accepting function also auto-detects raw WKB input, so `ST_Within(p.geom, …)` works directly on a gpkg column.

All functions are **deterministic and pure** (no I/O, no clock, no randomness) and NULL-strict (NULL in → NULL out). Malformed input raises an explicit error prefixed `kenro:` — never a silent NULL.

### Not implemented — on purpose

Calling a known-but-unimplemented `ST_` function raises a helpful error instead of SQLite's `no such function`, e.g.:

```
kenro: ST_Buffer is not implemented in kenro. kenro deliberately excludes
GEOS-class operations; use SpatiaLite or DuckDB spatial for this.
```

- **Never**: `ST_Buffer`, `ST_Union`, `ST_Intersection`, `ST_Difference`, `ST_SymDifference` — GEOS-class computational geometry. Use SpatiaLite or DuckDB spatial; kenro stays small.
- **Planned (0.2)**: `ST_Transform` (proj4rs), `ST_AsGeoJSON` / `ST_GeomFromGeoJSON`, accessors (`ST_Area`, `ST_Length`, `ST_Centroid`, `ST_Envelope`, `ST_SRID`, `ST_X`, `ST_Y`, `ST_NumPoints`, `ST_IsValid`, `ST_Simplify`).
- Also out of scope: raster, network analysis, full PROJ grid transforms, and any claim of full SpatiaLite/PostGIS compatibility.

## PostGIS is the reference

Function names, signatures, and semantics follow PostGIS (SQL/MM `ST_` prefix). Predicates are validated against PostGIS-generated golden vectors committed in this repo (`tests/golden/predicates.jsonl`). Where kenro deviates, it does so **loudly and documentedly** — never a silently different result:

| Case | PostGIS | kenro |
|---|---|---|
| `ST_GeomFromText('POINT EMPTY')` | empty point | error (the underlying geometry model cannot represent it) |
| 3D/M geometries | supported | accepted as *input* to predicates and R-tree functions (2D result, same as PostGIS); output/constructor functions raise an error |
| `GeometryCollection` in predicates | `ST_Intersects` works; `ST_Contains`/`ST_Within` error | error for all predicates |
| Geometry with SRID vs geometry with unknown SRID (0) | error | proceeds (needed for `… AND ST_Within(gpkg_col, ST_GeomFromText(…))`) — mixed *known* SRIDs still error |
| bare WKB blob passed to a predicate | needs a cast | accepted (auto-detection) |

Divergent golden vectors carry a `kenro_expected` + `note` field in `predicates.jsonl`; that file is the source of truth for this table.

## Requirements & caveats

- SQLite ≥ 3.31 (for `SQLITE_INNOCUOUS`, which lets kenro functions run inside the GeoPackage R-tree triggers under `PRAGMA trusted_schema=off`).
- Loading kenro **and** SpatiaLite into the same connection: both register `ST_` names and SQLite keeps the last registration. Don't mix them (a registration-filter feature flag can be added if needed).
- `ST_Distance` is 2D cartesian in the geometry's coordinate system. For meters over lon/lat, reproject first (`ST_Transform` arrives in 0.2).

## Roadmap

1. ✅ Core: GeoPackage blobs, WKB/WKT, predicates, R-tree functions, rusqlite registration, PostGIS golden tests
2. `ST_Transform` (proj4rs; JGD2000/JGD2011/WGS84 accuracy measured and documented), H3 cell IDs, GeoJSON
3. `kenro-ext`: loadable extension (`.so`/`.dylib`/`.dll`) for Python / Node / sqlite3 CLI
4. `kenro-wasm`: sql.js / wa-sqlite builds, browser demo
5. v0.x releases on crates.io

## License

MIT OR Apache-2.0, at your option.

[georust/geo]: https://github.com/georust/geo
