# Function reference

Every SQL function kenro registers, with its support status in PostGIS and
DuckDB Spatial for comparison (columns verified against PostGIS 3.5 and a
live DuckDB 1.4.0 + spatial session, July 2026). ✅ = present with the same
name and compatible semantics; deviations are spelled out.

Functions marked with the `overlay` feature need a `full` build (default
builds register them as stubs naming the feature); everything else,
including MVT, is in the default set (see
[Cargo features](../README.md#cargo-features)).

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
| `ST_Transform(geom, to_srid)` | geometry | ✅ 4 overloads, full PROJ | ⚠️ `(geom, source_crs, target_crs [, always_xy])` | kenro: PostGIS-exact 2-arg form, source = embedded SRID, curated EPSG table (see [Supported CRS](../README.md#supported-crs), [accuracy](accuracy.md)). DuckDB must be told the source CRS on every call |
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
| `ST_MakeValid(geom)` | geometry | ✅ + params arg | ✅ | GEOS *structure*-method semantics: bowties split, stray holes become polygons, zero-area parts drop — areal results only, where PostGIS's default linework method can return collections with lines. Points/lines pass through unchanged. Property-tested: output always validates and is idempotent |
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
| `ST_AsMVTGeom(geom, bounds [, extent [, buffer [, clip]]])` | geometry / NULL | ✅ | ✅ | World → integer tile coordinates (Y down); clipped-away input → NULL. `bounds` is any geometry (its envelope is used). ±1 pixel vs PostGIS at tile edges (kenro clips before grid snapping, PostGIS after). **`full` builds add PostGIS-grade validity repair**: invalid input and snap-induced self-intersections are made valid (golden-tested); standard builds clip exactly but pass invalid rings through |
| `ST_AsMVT(geom [, name [, extent [, props_json]]])` **aggregate** | BLOB | ⚠️ record-based signature | ⚠️ | **Deliberate signature divergence**: SQLite has no record type, so properties come from `json_object(...)` instead of row columns. A PostGIS-style call fails loudly at the type level. Layer name/extent must be constant per group |
| **Stubs** (call = helpful error) | | | | |
| `ST_Collect` | stub | ✅ | ✅ | kenro never produces GeometryCollection values; use the `ST_Union` aggregate for areal dissolve, or collect rows on the application side |

All implemented functions are **deterministic and pure** (no I/O, no clock,
no randomness) and NULL-strict (NULL in → NULL out; aggregates skip NULL
rows instead, following PostGIS aggregate semantics). Malformed input raises
an explicit error prefixed `kenro:` — never a silent NULL.

Calling a stub raises a helpful error instead of SQLite's
`no such function`:

```
kenro: ST_Collect is not implemented in kenro. kenro never produces
GeometryCollection values; for areal dissolve use the ST_Union aggregate,
otherwise collect rows on the application side.
```

## Deliberately out of scope

- **Raster** — kenro is vector-only.
- **Topology / network analysis** — no `ST_Node`/`ST_Polygonize`, no
  routing.
- **File-format conversion** — kenro operates on geometry *values*
  (WKT/WKB/GeoJSON/GeoPackage blobs), not files; reading shapefiles or
  writing whole GeoPackages is GDAL/ogr2ogr territory (DuckDB's `ST_Read`).
- **GeometryCollection-producing operations** — kenro never returns a
  GeometryCollection; operations that would (mixed-dimension overlays,
  `ST_Collect`) error loudly instead.
- **Datum-grid transforms** — `ST_Transform` is gridless projection math
  ([accuracy](accuracy.md)); survey-grade work needs full PROJ.
- Any claim of full SpatiaLite/PostGIS compatibility.

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

[georust/geo]: https://github.com/georust/geo
[h3-pg]: https://github.com/zachasme/h3-pg
[i_overlay]: https://github.com/iShape-Rust/iOverlay
