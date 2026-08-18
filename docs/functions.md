# Function reference

> **Related:** [3D geometry](3d.md) · [Scope and semantics](scope.md) ·
> [Routing](routing.md) · [Quickstart](quickstart.md) ·
> [Transform accuracy](accuracy.md) · [WebAssembly hosts](wasm.md)

Every SQL function kenro registers, with its support status in PostGIS, DuckDB
Spatial and SpatiaLite for comparison. **Function names link to their PostGIS
documentation page** — all 183 links verified live.

This file is the comparison table. The two topics that need more than a table
cell live next door:

| | |
|---|---|
| **[3D geometry](3d.md)** | What happens to a Z through storage, coordinate transforms, reprojection, derived geometries, interpolation and the `ST_3D*` metric family — plus surface collections (POLYHEDRALSURFACE / TIN / TRIANGLE). Every 3D function table is there, beside the semantics it needs |
| **[Routing](routing.md)** | The `kenro_dijkstra` family: shortest paths over an edge table, with pgRouting rather than PostGIS as the reference. Signatures, the trailing-`reverse_cost` divergence, the `json_each` recipe that turns a path into rows, and a `pgr_createTopology` replacement in plain SQL |
| **[Scope and semantics](scope.md)** | What kenro deliberately leaves out and why, how to get N rows out of a MULTI\* result, and what "PostGIS is the reference" means in practice |

**Reading the columns.** ✅ = present with the same name and compatible
semantics; ⚠️ = present with a difference, spelled out in Notes; ❌ = absent.
Verified against PostGIS 3.5, a live DuckDB 1.4.0 + spatial session, and a live
mod_spatialite 5.1 session, July–August 2026.

**Feature gates.** Functions marked `overlay`, `spheroid`, `concave-hull`,
`delaunay`, `gml`, `routing`, `text-encodings` or `voronoi` need a `full` build
— a default build registers them as stubs that name the missing feature. Everything else,
MVT included, is in the default set (see
[Cargo features](../README.md#cargo-features)).

**Geometry values** in and out of kenro functions are GeoPackage blobs: they
carry the SRID, and a value in a gpkg column is already valid storage. Every
geometry-accepting function also auto-detects raw WKB, so `ST_Within(p.geom, …)`
works directly on a gpkg column.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| **Geometry I/O** | | | | | |
| [`ST_GeomFromText(wkt [, srid])`](https://postgis.net/docs/ST_GeomFromText.html) | geometry | ✅ | ⚠️ no srid arg | ✅ | kenro rejects `POINT EMPTY` (geometry model limit); DuckDB's geometry is SRID-less |
| [`ST_GeomFromWKB(wkb [, srid])`](https://postgis.net/docs/ST_GeomFromWKB.html) | geometry | ✅ | ⚠️ no srid arg | ✅ | Accepts ISO WKB and EWKB; an explicit srid overrides an embedded one (PostGIS behavior) |
| `ST_GeomFromGPB(gpb)` | geometry | ❌ | ❌ | ⚠️ named `GeomFromGPB` | kenro-only: validates + normalizes a GeoPackage blob. DuckDB imports gpkg **files** via GDAL `ST_Read`; PostGIS needs ogr2ogr |
| [`ST_GeomFromGeoJSON(text)`](https://postgis.net/docs/ST_GeomFromGeoJSON.html) | geometry | ✅ keeps Z | ✅ | ⚠️ named `GeomFromGeoJSON` | SRID 4326 per RFC 7946 (PostGIS ≥ 3.0); kenro is 2D-only and errors on 3D rather than dropping Z |
| [`ST_AsText(geom)`](https://postgis.net/docs/ST_AsText.html) | TEXT | ✅ | ✅ | ✅ | Byte-identical to PostGIS across the golden suite. One rendering difference exists for values that are not exactly representable: kenro writes the shortest string that round-trips the double, PostGIS trims to 15 significant digits — so a coordinate of `1.2000000000000002` prints in full here and as `1.2` there. The doubles are the same |
| [`ST_AsBinary(geom)`](https://postgis.net/docs/ST_AsBinary.html) | BLOB | ✅ | ❌ named `ST_AsWKB` | ✅ | ISO WKB, little-endian, SRID dropped (as in PostGIS); PostGIS conversely has no `ST_AsWKB` |
| `ST_AsGPB(geom)` | BLOB | ❌ | ❌ | ⚠️ named `AsGPB` | kenro-only: storage-grade GeoPackage blob (envelope included) — use for writing gpkg columns |
| [`ST_AsGeoJSON(geom [, maxdecimaldigits])`](https://postgis.net/docs/ST_AsGeoJSON.html) | TEXT | ✅ | ✅ JSON fragment | ⚠️ named `AsGeoJSON` | Default 9 digits; kenro's output is byte-identical to PostGIS (golden-tested) |
| **SRID & CRS transform** | | | | | |
| [`ST_SRID(geom)`](https://postgis.net/docs/ST_SRID.html) | INT | ✅ | ❌ | ✅ | 0 = unknown. DuckDB's `GEOMETRY` carries no SRID at all — CRS bookkeeping is manual there |
| [`ST_SetSRID(geom, srid)`](https://postgis.net/docs/ST_SetSRID.html) | geometry | ✅ | ❌ | ⚠️ named `SetSRID` | Relabel only, no reprojection |
| [`ST_Transform(geom, to_srid)`](https://postgis.net/docs/ST_Transform.html) | geometry | ✅ 4 overloads, full PROJ | ⚠️ `(geom, source_crs, target_crs [, always_xy])` | ✅ | kenro: PostGIS-exact 2-arg form, source = embedded SRID, curated EPSG table (see [Supported CRS](../README.md#supported-crs), [accuracy](accuracy.md)). DuckDB must be told the source CRS on every call. **Z and surface collections ride through** — see [3D affine transforms](3d.md#3d-affine-transforms) |
| **Constructors** | | | | | |
| [`ST_MakePoint(x, y)`](https://postgis.net/docs/ST_MakePoint.html) | geometry | ✅ | ✅ | ⚠️ named `MakePoint` | 2D only |
| [`ST_Point(x, y [, srid])`](https://postgis.net/docs/ST_Point.html) | geometry | ✅ | ⚠️ no srid arg | ✅ | The srid form is PostGIS 3.2+ |
| [`ST_MakeEnvelope(xmin, ymin, xmax, ymax [, srid])`](https://postgis.net/docs/ST_MakeEnvelope.html) | geometry | ✅ | ⚠️ no srid arg | ⚠️ named `BuildMBR` | Degenerate extents still return a POLYGON, like PostGIS |
| **Predicates** | | | | | |
| [`ST_Intersects(a, b)`](https://postgis.net/docs/ST_Intersects.html) | 0/1 | ✅ | ✅ | ✅ | DE-9IM. kenro errors on GeometryCollection operands (PostGIS's `ST_Intersects` accepts them; its `ST_Contains`/`ST_Within` also error) |
| [`ST_Contains(a, b)`](https://postgis.net/docs/ST_Contains.html) | 0/1 | ✅ | ✅ | ✅ | Boundary semantics golden-tested against PostGIS |
| [`ST_Within(a, b)`](https://postgis.net/docs/ST_Within.html) | 0/1 | ✅ | ✅ | ✅ | `ST_Within(a,b) = ST_Contains(b,a)`, property-tested |
| [`ST_Disjoint(a, b)`](https://postgis.net/docs/ST_Disjoint.html) | 0/1 | ✅ | ✅ | ✅ | Empty operands → true (golden-arbitrated PostGIS behavior) |
| [`ST_Touches(a, b)`](https://postgis.net/docs/ST_Touches.html) / [`ST_Crosses(a, b)`](https://postgis.net/docs/ST_Crosses.html) / [`ST_Overlaps(a, b)`](https://postgis.net/docs/ST_Overlaps.html) | 0/1 | ✅ | ✅ | ✅ | |
| [`ST_Equals(a, b)`](https://postgis.net/docs/ST_Equals.html) | 0/1 | ✅ | ✅ | ✅ | Topological equality; both-empty → true |
| [`ST_Covers(a, b)`](https://postgis.net/docs/ST_Covers.html) / [`ST_CoveredBy(a, b)`](https://postgis.net/docs/ST_CoveredBy.html) | 0/1 | ✅ | ✅ | ✅ | The boundary-tolerant contains/within variants |
| [`ST_Relate(a, b)`](https://postgis.net/docs/ST_Relate.html) | TEXT | ✅ | ❌ | ✅ | The 9-character DE-9IM matrix |
| [`ST_Relate(a, b, pattern)`](https://postgis.net/docs/ST_Relate.html) | 0/1 | ✅ | ❌ | ✅ | Pattern matching with `*`/`T`/`F`/`0`/`1`/`2` |
| **Measures** | | | | | |
| [`ST_Distance(a, b)`](https://postgis.net/docs/ST_Distance.html) | REAL | ✅ | ✅ | ✅ | 2D cartesian; NULL for empty inputs |
| [`ST_DWithin(a, b, d)`](https://postgis.net/docs/ST_DWithin.html) | 0/1 | ✅ | ✅ | ⚠️ named `PtDistWithin` | `distance <= d`; negative tolerance errors (matches PostGIS) |
| [`ST_ClosestPoint(a, b)`](https://postgis.net/docs/ST_ClosestPoint.html) | geometry | ✅ any × any | ❌ (`ST_ShortestLine` instead) | ✅ | kenro: second operand must be a POINT (geo API limit) — anything else errors |
| [`ST_LineInterpolatePoint(line, fraction)`](https://postgis.net/docs/ST_LineInterpolatePoint.html) | geometry | ✅ | ✅ | ⚠️ `ST_Line_Interpolate_Point` | Fraction outside [0, 1] errors, like PostGIS |
| [`ST_LineLocatePoint(line, point)`](https://postgis.net/docs/ST_LineLocatePoint.html) | REAL | ✅ | ✅ | ⚠️ `ST_Line_Locate_Point` | |
| [`ST_HausdorffDistance(a, b)`](https://postgis.net/docs/ST_HausdorffDistance.html) | REAL | ✅ | ❌ | ✅ | kenro computes vertex-to-vertex distance (geo API), GEOS vertex-to-segment — equal on shared golden vectors, can differ on long sparse segments |
| [`ST_FrechetDistance(a, b)`](https://postgis.net/docs/ST_FrechetDistance.html) | REAL | ✅ + densify arg | ❌ | ✅ | kenro: LINESTRING × LINESTRING, 2-arg form only |
| [`ST_Azimuth(a, b)`](https://postgis.net/docs/ST_Azimuth.html) | REAL | ✅ | ✅ | ✅ | Radians clockwise from north; coincident points → NULL |
| **Overlay & buffer** (pure Rust — see [semantics](scope.md#semantics-postgis-is-the-reference)) | | | | | |
| [`ST_Intersection(a, b)`](https://postgis.net/docs/ST_Intersection.html) | geometry | ✅ | ✅ | ✅ | Areal results only: polygons that merely touch → empty, where GEOS returns the shared LINESTRING. line × line errors (needs noding) |
| [`ST_Difference(a, b)`](https://postgis.net/docs/ST_Difference.html) | geometry | ✅ | ✅ | ✅ | Same decision matrix; point operands are filtered exactly |
| [`ST_SymDifference(a, b)`](https://postgis.net/docs/ST_SymDifference.html) | geometry | ✅ | ❌ | ✅ | areal × areal and puntal × puntal; mixed dimensions error |
| [`ST_Union(a, b)`](https://postgis.net/docs/ST_Union.html) | geometry | ✅ | ✅ | ✅ | Scalar form; line unions error (noding) |
| `ST_Union(geom)` **aggregate** | geometry | ✅ | ⚠️ named `ST_Union_Agg` | ✅ | Dissolve in `GROUP BY`; NULL rows skipped, zero rows → NULL (PostGIS aggregate semantics) |
| [`ST_Buffer(geom, d [, opts])`](https://postgis.net/docs/ST_Buffer.html) | geometry | ✅ | ⚠️ 3rd arg differs | ⚠️ no style options | PostGIS-style options TEXT (`quad_segs= endcap= join= mitre_limit=`) or INTEGER quad_segs; `side=` unsupported. Negative distance erodes polygons. Golden-tested within 2% area of GEOS |
| [`ST_MakeValid(geom)`](https://postgis.net/docs/ST_MakeValid.html) | geometry | ✅ + params arg | ✅ | ✅ | GEOS *structure*-method semantics: bowties split, stray holes become polygons, zero-area parts drop — areal results only, where PostGIS's default linework method can return collections with lines. Points/lines pass through unchanged. Property-tested: output always validates and is idempotent |
| **Processing & affine** | | | | | |
| [`ST_ConvexHull(geom)`](https://postgis.net/docs/ST_ConvexHull.html) | geometry | ✅ | ✅ | ✅ | Degenerate hulls collapse to POINT/LINESTRING like PostGIS |
| [`ST_PointOnSurface(geom)`](https://postgis.net/docs/ST_PointOnSurface.html) | geometry | ✅ | ✅ | ✅ | Guaranteed interior; exact coordinates may differ from GEOS (documented in vectors) |
| [`ST_Simplify(geom, tol)`](https://postgis.net/docs/ST_Simplify.html) | geometry | ✅ + `preserveCollapsed` arg | ✅ 2-arg; also `ST_SimplifyPreserveTopology` | ✅ | Ramer-Douglas-Peucker, collapse allowed (= PostGIS 2-arg form) |
| [`ST_SimplifyVW(geom, tol)`](https://postgis.net/docs/ST_SimplifyVW.html) | geometry | ✅ | ❌ | ❌ | Visvalingam-Whyatt; tolerance is an area |
| [`ST_ChaikinSmoothing(geom [, iterations])`](https://postgis.net/docs/ST_ChaikinSmoothing.html) | geometry | ✅ | ❌ | ❌ | PostGIS variant (endpoints preserved on open lines); iterations capped at 5 |
| [`ST_RemoveRepeatedPoints(geom)`](https://postgis.net/docs/ST_RemoveRepeatedPoints.html) | geometry | ✅ + tolerance arg | ✅ | ❌ | kenro: exact duplicates only (no tolerance form) |
| [`ST_OrientedEnvelope(geom)`](https://postgis.net/docs/ST_OrientedEnvelope.html) | geometry | ✅ | ⚠️ named `ST_MinimumRotatedRectangle` | ✅ | Minimum rotated rectangle; equal-area alternates possible (rotation-normalized comparison in goldens) |
| [`ST_Rotate(geom, radians [, x0, y0])`](https://postgis.net/docs/ST_Rotate.html) | geometry | ✅ | ✅ | ⚠️ named `RotateCoords` | About the origin (or the given point) — PostGIS semantics, **not** geo's centroid default. Z/M and surface collections ride through — [3D affine transforms](3d.md#3d-affine-transforms) |
| [`ST_Translate(geom, dx, dy)`](https://postgis.net/docs/ST_Translate.html) | geometry | ✅ | ✅ | ✅ | Z/M and surface collections ride through — [3D affine transforms](3d.md#3d-affine-transforms) |
| [`ST_Scale(geom, xf, yf)`](https://postgis.net/docs/ST_Scale.html) | geometry | ✅ | ✅ | ⚠️ named `ScaleCoords` | About the origin, like PostGIS. Z is **not** scaled by this arity (measured); the 3-argument form is not implemented |
| **GeoPackage triggers** | | | | | |
| [`ST_MinX`](https://postgis.net/docs/ST_XMin.html) / [`ST_MaxX`](https://postgis.net/docs/ST_XMax.html) / [`ST_MinY`](https://postgis.net/docs/ST_YMin.html) / [`ST_MaxY`](https://postgis.net/docs/ST_YMax.html) | REAL | ⚠️ named `ST_XMin` … | ⚠️ named `ST_XMin` … | ✅ | kenro uses the GeoPackage spec's R-tree trigger names — required verbatim for gpkg index maintenance; the other two spell it `ST_XMin` |
| [`ST_IsEmpty(geom)`](https://postgis.net/docs/ST_IsEmpty.html) | 0/1 | ✅ | ✅ | ✅ | gpkg R-tree contract; NULL on NULL |
| `GPKG_IsAssignable(expected, actual)` | 0/1 | ❌ | ❌ | ✅ | The geometry-type-trigger helper. ⚠️ that extension was **removed from the GeoPackage standard in 2016** over interoperability concerns and now survives only in the 1.1.0 archive — kenro keeps the function because files carrying those triggers are still out there. Accepts both `'POINT'` and `'ST_Point'` spellings so the old DDL works with kenro's `ST_GeometryType` |
| **H3 cells** (`h3` feature) | | | | | |
| `h3_latlng_to_cell(geom, res)` | INT | via [h3-pg] ext | via community `h3` ext | ❌ | Same name in all three ecosystems; POINT in lon/lat only |
| `h3_cell_to_parent(cell, res)` | INT | via h3-pg | via `h3` ext | ❌ | For coarser `GROUP BY` |
| `h3_cell_to_string(cell)` / `h3_string_to_cell(text)` | TEXT / INT | ⚠️ h3-pg casts its `h3index` type | ⚠️ DuckDB: `h3_h3_to_string` | ❌ | Hex-string conversion names differ per ecosystem; kenro uses the H3 v4 canonical verbs |
| **Accessors** | | | | | |
| [`ST_Area(geom)`](https://postgis.net/docs/ST_Area.html) | REAL | ✅ | ✅ | ✅ | Planar; 0 for non-areal or empty |
| [`ST_Length(geom)`](https://postgis.net/docs/ST_Length.html) | REAL | ✅ | ✅ | ✅ | Linear geometries only — polygons return 0 in all three |
| [`ST_Perimeter(geom)`](https://postgis.net/docs/ST_Perimeter.html) | REAL | ✅ | ✅ | ✅ | Ring lengths; 0 for non-areal |
| [`ST_Centroid(geom)`](https://postgis.net/docs/ST_Centroid.html) | geometry | ✅ | ✅ | ✅ | Empty input → `POINT EMPTY` |
| [`ST_Envelope(geom)`](https://postgis.net/docs/ST_Envelope.html) | geometry | ✅ | ✅ | ✅ | Degenerates to POINT/LINESTRING exactly like PostGIS. DuckDB also has scalar `ST_Extent` → `BOX_2D` |
| [`ST_X(geom)`](https://postgis.net/docs/ST_X.html) / [`ST_Y(geom)`](https://postgis.net/docs/ST_Y.html) | REAL | ✅ | ✅ | ✅ | POINT only, error otherwise (as in PostGIS); `POINT EMPTY` → NULL |
| [`ST_NPoints(geom)`](https://postgis.net/docs/ST_NPoints.html) | INT | ✅ | ✅ | ✅ | All vertices of any type; empty → 0 |
| [`ST_NumPoints(geom)`](https://postgis.net/docs/ST_NumPoints.html) | INT / NULL | ✅ | ⚠️ synonym of `ST_NPoints` | ✅ | LINESTRING-only, NULL for other types (PostGIS as implemented; its docs lag). **Same name, different answer in DuckDB** (counts all vertices of any type) — kenro follows PostGIS |
| [`ST_GeometryType(geom)`](https://postgis.net/docs/ST_GeometryType.html) | TEXT | ✅ | ⚠️ returns `POINT` | ✅ | kenro returns the PostGIS spelling `ST_Point`; DuckDB returns bare `POINT` |
| [`ST_NumGeometries(geom)`](https://postgis.net/docs/ST_NumGeometries.html) | INT | ✅ | ✅ | ✅ | Single geometry → 1; empty → 0 |
| [`ST_GeometryN(geom, n)`](https://postgis.net/docs/ST_GeometryN.html) | geometry / NULL | ✅ | ❌ | ✅ | 1-based; out of range → NULL; GeometryCollection supported |
| [`ST_StartPoint(geom)`](https://postgis.net/docs/ST_StartPoint.html) / [`ST_EndPoint(geom)`](https://postgis.net/docs/ST_EndPoint.html) | geometry / NULL | ✅ | ✅ | ✅ | PostGIS 3.2 semantics: POINT → itself, MULTILINESTRING works, areal → NULL |
| [`ST_PointN(line, n)`](https://postgis.net/docs/ST_PointN.html) | geometry / NULL | ✅ | ✅ | ✅ | 1-based with negative indexing from the end |
| [`ST_Reverse(geom)`](https://postgis.net/docs/ST_Reverse.html) | geometry | ✅ | ✅ | ✅ | Member order of multi geometries preserved |
| [`ST_IsValid(geom)`](https://postgis.net/docs/ST_IsValid.html) | 0/1 | ✅ GEOS | ✅ GEOS | ✅ | georust validation: everything incl. ring self-intersection and hole placement, except the split-interior case (documented gap) |
| **Vector tiles (MVT)** | | | | | |
| [`ST_AsMVTGeom(geom, bounds [, extent [, buffer [, clip]]])`](https://postgis.net/docs/ST_AsMVTGeom.html) | geometry / NULL | ✅ | ✅ | ❌ | World → integer tile coordinates (Y down); clipped-away input → NULL. `bounds` is any geometry (its envelope is used). ±1 pixel vs PostGIS at tile edges (kenro clips before grid snapping, PostGIS after). **`full` builds add PostGIS-grade validity repair**: invalid input and snap-induced self-intersections are made valid (golden-tested); standard builds clip exactly but pass invalid rings through |
| `ST_AsMVT(geom [, name [, extent [, props_json]]])` **aggregate** | BLOB | ⚠️ record-based signature | ⚠️ | ❌ | **Deliberate signature divergence**: SQLite has no record type, so properties come from `json_object(...)` instead of row columns. A PostGIS-style call fails loudly at the type level. Layer name/extent must be constant per group |
| **Stubs** (call = helpful error) | | | | | |
| [`ST_Collect`](https://postgis.net/docs/ST_Collect.html) | stub | ✅ | ✅ | ✅ | kenro never produces GeometryCollection values; use the `ST_Union` aggregate for areal dissolve, or collect rows on the application side |

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

## PostGIS spellings and typed constructors

PostGIS reaches several of these functions by another name, and SQL written
against it should keep working. Aliases share the implementation — and the
wasm export — so they cost a registration and nothing else.

| Alias | Same as | Note |
|---|---|---|
| `ST_XMin` / `ST_XMax` / `ST_YMin` / `ST_YMax` | `ST_MinX` / `ST_MaxX` / `ST_MinY` / `ST_MaxY` | kenro's primary names are the GeoPackage R-tree trigger spellings, required verbatim for index maintenance |
| `ST_GeometryFromText(wkt [, srid])` | `ST_GeomFromText` | |
| `ST_GeomFromEWKB(bytes)` | `ST_GeomFromWKB` | kenro's WKB reader already accepts EWKB |
| `ST_SymmetricDifference(a, b)` | `ST_SymDifference` | `overlay` feature |
| `ST_Area2D` / `ST_Perimeter2D` / `ST_Length2D` | `ST_Area` / `ST_Perimeter` / `ST_Length` | kenro is 2D throughout, so these are the same function |

New in this group:

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_Force2D(geom)`](https://postgis.net/docs/ST_Force2D.html) | geometry | ✅ | ✅ | ❌ | Drops Z/M. kenro decodes 3D input but refuses to *encode* it rather than silently writing 2D — this is the explicit opt-in, and the only way a 3D GeoPackage column reaches `ST_AsText`/`ST_AsGeoJSON` |
| [`ST_AsEWKT(geom)`](https://postgis.net/docs/ST_AsEWKT.html) | TEXT | ✅ | ❌ | ✅ | `SRID=n;` prefix, omitted when the SRID is 0 (PostGIS behavior, verified live) |
| [`ST_GeomFromEWKT(text)`](https://postgis.net/docs/ST_GeomFromEWKT.html) | geometry | ✅ | ❌ | ✅ | Accepts the prefix or plain WKT |
| [`ST_AsEWKB(geom)`](https://postgis.net/docs/ST_AsEWKB.html) | BLOB | ✅ | ❌ | ✅ | ISO WKB with PostGIS's `0x20000000` SRID flag; plain WKB when the SRID is 0 |
| [`ST_AsHexEWKB(geom)`](https://postgis.net/docs/ST_AsEWKB.html) | TEXT | ✅ | ❌ | ✅ | Upper-case hex of the above; byte-identical to PostGIS 3.5 (golden-tested) |
| [`ST_PointFromText`](https://postgis.net/docs/ST_PointFromText.html) / [`ST_LineFromText`](https://postgis.net/docs/ST_LineFromText.html) / [`ST_LineStringFromText`](https://postgis.net/docs/ST_LineFromText.html) / [`ST_PolyFromText`](https://postgis.net/docs/ST_PolygonFromText.html) / [`ST_PolygonFromText`](https://postgis.net/docs/ST_PolygonFromText.html) / [`ST_MPointFromText`](https://postgis.net/docs/ST_MPointFromText.html) / [`ST_MLineFromText`](https://postgis.net/docs/ST_MLineFromText.html) / [`ST_MPolyFromText`](https://postgis.net/docs/ST_MPolyFromText.html) `(wkt [, srid])` | geometry / NULL | ✅ | ⚠️ partial | ✅ | Parse, then **NULL when the geometry is another type** — an error would be the intuitive choice, but PostGIS returns NULL and so does kenro |
| [`ST_PointFromWKB`](https://postgis.net/docs/ST_PointFromWKB.html) / [`ST_LineFromWKB`](https://postgis.net/docs/ST_LineFromWKB.html) / [`ST_PolyFromWKB`](https://postgis.net/docs/ST_GeomFromWKB.html) / [`ST_MPointFromWKB`](https://postgis.net/docs/ST_GeomFromWKB.html) / [`ST_MLineFromWKB`](https://postgis.net/docs/ST_GeomFromWKB.html) / [`ST_MPolyFromWKB`](https://postgis.net/docs/ST_GeomFromWKB.html) `(bytes [, srid])` | geometry / NULL | ✅ | ❌ | ✅ | Same contract over WKB/EWKB/GeoPackage input. kenro takes the srid on all six; stock PostGIS omits it from `ST_MultiLineFromWKB` alone, which looks like an oversight rather than a rule |

## Structural accessors and editing

Rings, boundaries, vertex surgery and coordinate-space tweaks. No new
dependencies — but several PostGIS conventions here are easy to guess wrong,
so each was read off a live PostGIS 3.5 and is golden-tested:

- **ring indexes are 1-based, vertex indexes 0-based** (`ST_InteriorRingN(g, 1)`
  but `ST_SetPoint(g, 0, p)`)
- a wrong-type argument gives **NULL, not an error** — except `ST_IsRing`,
  which raises
- `ST_Boundary` of a point is `POINT EMPTY`; of a closed line, `MULTIPOINT EMPTY`

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_ExteriorRing(polygon)`](https://postgis.net/docs/ST_ExteriorRing.html) | geometry / NULL | ✅ | ✅ | ✅ | NULL for any non-polygon |
| [`ST_InteriorRingN(polygon, n)`](https://postgis.net/docs/ST_InteriorRingN.html) | geometry / NULL | ✅ | ✅ | ✅ | 1-based; NULL when out of range |
| [`ST_NumInteriorRings(polygon)`](https://postgis.net/docs/ST_NumInteriorRings.html) / [`ST_NumInteriorRing`](https://postgis.net/docs/ST_NumInteriorRing.html) | INTEGER / NULL | ✅ | ✅ | ✅ | Both spellings, as in PostGIS |
| [`ST_NRings(geom)`](https://postgis.net/docs/ST_NRings.html) | INTEGER | ✅ | ❌ | ✅ | Exterior + interior, summed over a multipolygon |
| [`ST_Boundary(geom)`](https://postgis.net/docs/ST_Boundary.html) | geometry | ✅ | ❌ | ✅ | Polygon → its rings; open line → `MULTIPOINT` of the endpoints; the mod-2 rule applies to a multilinestring |
| [`ST_IsClosed(geom)`](https://postgis.net/docs/ST_IsClosed.html) | INTEGER | ✅ | ✅ | ✅ | Areal input is closed by definition |
| [`ST_IsRing(line)`](https://postgis.net/docs/ST_IsRing.html) | INTEGER | ✅ | ❌ | ✅ | Closed *and* simple. **Raises** on non-linear input — PostGIS's wording, kept |
| [`ST_AddPoint(line, point [, position])`](https://postgis.net/docs/ST_AddPoint.html) | geometry / NULL | ✅ | ❌ | ✅ | 0-based; default (or -1) appends |
| [`ST_SetPoint(line, index, point)`](https://postgis.net/docs/ST_SetPoint.html) | geometry / NULL | ✅ | ❌ | ✅ | 0-based |
| [`ST_RemovePoint(line, index)`](https://postgis.net/docs/ST_RemovePoint.html) | geometry / NULL | ✅ | ❌ | ✅ | 0-based |
| [`ST_MakeLine(a, b)`](https://postgis.net/docs/ST_MakeLine.html) | geometry | ✅ | ✅ | ✅ | Two-geometry form; points and lines concatenate. The aggregate form is not implemented |
| [`ST_MakePolygon(line)`](https://postgis.net/docs/ST_MakePolygon.html) | geometry | ✅ | ❌ | ✅ | Shell must be closed; the with-holes arity is not implemented |
| [`ST_Multi(geom)`](https://postgis.net/docs/ST_Multi.html) | geometry | ✅ | ❌ | ✅ | Already-multi input passes through |
| [`ST_SnapToGrid(geom, size)`](https://postgis.net/docs/ST_SnapToGrid.html) / `(geom, sizex, sizey)` | geometry | ✅ | ❌ | ✅ | Grid anchored at the origin; size 0 leaves that axis alone. The origin-offset arities are not implemented. ⚠️ **PostGIS also drops the vertices that collapse together** — `ST_SnapToGrid(LINESTRING(0 0,0.1 0.1,1 1,1.1 1.1), 1)` is `LINESTRING(0 0,1 1)` there against kenro's `LINESTRING(0 0,0 0,1 1,1 1)`, and a fully-collapsing polygon is `POLYGON EMPTY` against kenro's degenerate ring (measured). `ST_RemoveRepeatedPoints` after, for the vertex behavior. 2D only, and 3D input is an error rather than a silent flatten |
| [`ST_FlipCoordinates(geom)`](https://postgis.net/docs/ST_FlipCoordinates.html) | geometry | ✅ | ✅ | ✅ | The lat/lon-order fix. x and y only: Z/M stay put, and surface collections flip — [3D affine transforms](3d.md#3d-affine-transforms) |
| [`ST_ShiftLongitude(geom)`](https://postgis.net/docs/ST_ShiftLongitude.html) | geometry | ✅ | ❌ | ✅ | x from [-180,180) into [0,360). Z/M and surface collections ride through |
| [`ST_Expand(geom, units)`](https://postgis.net/docs/ST_Expand.html) | geometry / NULL | ✅ | ❌ | ✅ | ⚠️ returns a **POLYGON**: PostGIS returns its `box2d` type, which SQLite has no equivalent for |

## Dimension, validity and orientation

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_Dimension(geom)`](https://postgis.net/docs/ST_Dimension.html) | INTEGER | ✅ | ✅ | ✅ | 0/1/2 for puntal/lineal/areal |
| [`ST_CoordDim(geom)`](https://postgis.net/docs/ST_CoordDim.html) / [`ST_NDims(geom)`](https://postgis.net/docs/ST_NDims.html) | INTEGER | ✅ | ✅ | ✅ | Always 2 — kenro is 2D, and 3D input has Z/M dropped on decode (see `ST_Force2D`) |
| [`ST_IsValidReason(geom)`](https://postgis.net/docs/ST_IsValidReason.html) | TEXT | ✅ | ✅ | ✅ | `"Valid Geometry"` or a description. ⚠️ **wording is geo's, not PostGIS's**: PostGIS says `Self-intersection[1 1]` with the coordinate, geo names the ring and defect. A diagnostic, not a string to match on |
| [`ST_ForcePolygonCW(geom)`](https://postgis.net/docs/ST_ForcePolygonCW.html) / [`ST_ForceRHR(geom)`](https://postgis.net/docs/ST_ForceRHR.html) | geometry | ✅ | ❌ | ✅ | Exterior clockwise, interiors counter-clockwise; non-areal input passes through |
| [`ST_ForcePolygonCCW(geom)`](https://postgis.net/docs/ST_ForcePolygonCCW.html) | geometry | ✅ | ❌ | ✅ | The mirror |
| [`ST_IsPolygonCW(geom)`](https://postgis.net/docs/ST_IsPolygonCW.html) / [`ST_IsPolygonCCW(geom)`](https://postgis.net/docs/ST_IsPolygonCCW.html) | INTEGER | ✅ | ❌ | ✅ | Non-areal input is true |

## Line structure

Simplicity, merging and splitting. These were listed as out of scope on the
grounds that they need a noding engine; re-checking that against the crates
already in the tree, two thirds of it was wrong — `geo` has the segment-pair
sweep `ST_IsSimple` needs, merging never required noding in the first place
(GEOS's `LineMerger` doesn't node either), and `i_overlay` already carries
the slice `ST_Split` uses. `ST_Node` — noding an arbitrary line soup against
itself — really is the piece none of them needed, and stays out.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_IsSimple(geom)`](https://postgis.net/docs/ST_IsSimple.html) | INTEGER | ✅ | ❌ | ✅ | No anomalous self-intersection: curves may close on themselves and may meet each other, but only end to end. A closed ring is simple; the same ring with a tail on its closing vertex is not. Areal input is judged **ring by ring** — a bow-tie ring fails, while two overlapping MULTIPOLYGON members are still simple (PostGIS agrees) |
| [`ST_LineMerge(geom [, directed])`](https://postgis.net/docs/ST_LineMerge.html) | geometry | ✅ | ✅ | ✅ | Sews lines together at nodes where exactly two ends meet. A Y junction keeps all three arms, and two lines crossing in their interiors are **not** merged — there is no vertex there, and this function does not create one. `directed` honours the original directions instead of reversing parts to fit. ⚠️ Non-lineal input is an error, where PostGIS answers `GEOMETRYCOLLECTION EMPTY`; and the direction and start vertex of a chain assembled from reversed parts are arbitrary, so `ST_AsText` can read backwards from PostGIS's |
| [`ST_Split(input, blade)`](https://postgis.net/docs/ST_Split.html) | geometry | ✅ | ❌ | ✅ | `overlay` feature. Lineal input splits at the blade's points or crossings, areal input is sliced by a lineal blade. Holes survive the cut. A blade that misses returns the input unchanged. ⚠️ MULTILINESTRING or MULTIPOLYGON, not PostGIS's GEOMETRYCOLLECTION. Splitting a polygon by a point is an error, as in PostGIS |

## Linear referencing and distance geometry

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_Segmentize(geom, max_length)`](https://postgis.net/docs/ST_Segmentize.html) | geometry | ✅ | ❌ | ✅ | Splits each segment into **equal** parts, as PostGIS does: 10 units at a maximum of 4 gives three 3⅓ segments, not 4+4+2 |
| [`ST_LineSubstring(line, from, to)`](https://postgis.net/docs/ST_LineSubstring.html) | geometry / NULL | ✅ | ❌ | ✅ | Fractions in [0,1]; NULL for non-linear input, error for a bad range |
| [`ST_ShortestLine(a, b)`](https://postgis.net/docs/ST_ShortestLine.html) | geometry / NULL | ✅ | ✅ | ✅ | Searched vertex-against-segment both ways — exact when the geometries are disjoint; when they intersect the distance is 0 and PostGIS may pick a different, equally valid zero-length line |
| [`ST_LongestLine(a, b)`](https://postgis.net/docs/ST_LongestLine.html) | geometry / NULL | ✅ | ❌ | ✅ | Always attained at a vertex pair |
| [`ST_MaxDistance(a, b)`](https://postgis.net/docs/ST_MaxDistance.html) | REAL / NULL | ✅ | ❌ | ✅ | The length of `ST_LongestLine` |

## Measures on a sphere and an ellipsoid

kenro's `ST_Distance` and `ST_Length` are planar, so on EPSG:4326 data — the
common case for a GeoPackage — they answer in **degrees**. PostGIS users
reach for `geography` there; kenro has no geography type, so these functions
are the answer instead.

`ST_DistanceSphere` is in every build. The **ellipsoidal** pair needs the
`spheroid` feature (in `full`), because geographiclib costs ~17 KB of wasm
for a 0.1% refinement over the sphere; without it they register as stubs
naming the feature.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_DistanceSphere(a, b)`](https://postgis.net/docs/ST_DistanceSphere.html) | REAL | ✅ | ❌ | ✅ | Great-circle metres, radius 6 371 008.7714 m — the same radius PostGIS uses (golden-tested against it). **POINT arguments only**; PostGIS takes any pair |
| [`ST_DistanceSpheroid(a, b [, spheroid])`](https://postgis.net/docs/ST_Distance_Spheroid.html) | REAL | ✅ | ❌ | ✅ | `spheroid` feature. WGS84 by default; the third argument takes PostGIS's `SPHEROID["name",a,1/f]` text. POINT arguments only |
| [`ST_LengthSpheroid(geom, spheroid)`](https://postgis.net/docs/ST_Length_Spheroid.html) / [`ST_Length2DSpheroid`](https://postgis.net/docs/ST_Length_Spheroid.html) | REAL | ✅ | ❌ | ✅ | `spheroid` feature. Geodesic length in metres; PostGIS has no one-argument form, so neither does kenro |
| [`ST_Project(point, distance, azimuth)`](https://postgis.net/docs/ST_Project.html) | geometry | ✅ | ❌ | ❌ | ⚠️ **planar**, matching PostGIS's *geometry* overload (verified live). PostGIS's geodesic behavior belongs to its `geography` overload — transform to a projected CRS first |

## Enclosing circle and areal operations

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_MinimumBoundingRadius(geom)`](https://postgis.net/docs/ST_MinimumBoundingRadius.html) | REAL / NULL | ✅ | ❌ | ✅ | ⚠️ PostGIS returns a `(center, radius)` record; SQLite has no record type, so kenro returns the radius. The centre is `ST_Centroid(ST_MinimumBoundingCircle(geom))` |
| [`ST_MinimumBoundingCircle(geom [, segs_per_quarter])`](https://postgis.net/docs/ST_MinimumBoundingCircle.html) | geometry / NULL | ✅ | ❌ | ✅ | Welzl, run deterministically (no shuffle) so SQL is reproducible; 48 segments per quarter by default, as in PostGIS |
| [`ST_UnaryUnion(geom)`](https://postgis.net/docs/ST_UnaryUnion.html) | geometry | ✅ | ❌ | ✅ | `overlay` feature. Dissolves a multipolygon's overlapping members into one areal result; non-areal input passes through |
| [`ST_ClipByBox2D(geom, box)`](https://postgis.net/docs/ST_ClipByBox2D.html) | geometry | ✅ | ❌ | ❌ | `overlay` feature. ⚠️ takes **any geometry** and uses its envelope (PostGIS takes a `box2d`) — pass `ST_MakeEnvelope(...)`. Unlike PostGIS, which documents that it may return an invalid geometry, this goes through the overlay engine, so the result is valid |
| [`ST_Subdivide(geom, max_vertices)`](https://postgis.net/docs/ST_Subdivide.html) | geometry | ✅ | ❌ | ❌ | `overlay` feature. ⚠️ PostGIS returns **one row per part**; kenro has no set-returning functions, so this returns a MULTIPOLYGON — walk it with `ST_NumGeometries` / `ST_GeometryN`. Splits along the longer axis; `max_vertices` must be ≥ 5 |
| [`ST_SquareGrid(size, bounds)`](https://postgis.net/docs/ST_SquareGrid.html) | geometry | ✅ | ❌ | ⚠️ scalar, args reversed | The square tiling covering `bounds`, **anchored at the origin** — cell `(i,j)` is always `[i·size, (i+1)·size] × [j·size, (j+1)·size]`, so grids from different bounds line up. Byte-identical to PostGIS's cells and cell order (i-major, then j) across eleven measured size/bounds combinations. ⚠️ PostGIS yields one row per cell with `i`/`j` columns; kenro returns a **MULTIPOLYGON** and drops the indices — recover them as `ST_MinX(cell)/size`. ⚠️ **SpatiaLite's `ST_SquareGrid(geom, size)` takes its arguments the other way round**; kenro follows PostGIS, so pasted SpatiaLite SQL fails on the argument type instead of gridding the wrong thing. `size ≤ 0` is empty, not an error (PostGIS returns zero rows). Over 100,000 cells is an error, because kenro materialises what PostGIS streams |
| [`ST_HexagonGrid(size, bounds)`](https://postgis.net/docs/ST_HexagonGrid.html) | geometry | ✅ | ❌ | ⚠️ named `ST_HexagonalGrid`, different layout | As above, hexagonal. `size` is the **circumradius**: cell `(0,0)` is centred on the origin with vertices at `(±size, 0)`, flat-topped, `2·size` wide and `√3·size` tall; centres step `1.5·size` in x and `√3·size` in y with odd columns staggered up by half a row. All of that is measured — pointy-top, and `size` as the inradius or the width, are equally plausible and all wrong. ⚠️ The edge rule is **asymmetric**: a cell whose low edge sits exactly on the bounds' maximum is kept, one whose high edge sits exactly on the minimum is dropped. Same MULTIPOLYGON and cell-budget divergences as `ST_SquareGrid` |

## Hulls and triangulation

The two most expensive functions in the catalog by binary size — measured on
the wasm standard tier at **+41 KB** and **+81 KB**, against ~21 KB for a
whole group of ordinary functions. Each has its own feature, both are in
`full`, and a build without them registers stubs naming the feature.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_ConcaveHull(geom, target_percent)`](https://postgis.net/docs/ST_ConcaveHull.html) | geometry | ✅ | ✅ | ✅ | `concave-hull` feature. Keeps **PostGIS's argument contract** — the fraction of the convex hull's area to aim for, 1.0 being the convex hull — by searching `geo`'s differently-scaled "concavity" parameter for it, at a few hull computations per call. A value outside [0,1] is an error, so pasting geo's own concavity (~2) fails loudly instead of returning a very different shape. ⚠️ The hull family differs from GEOS's, so the vertices are not PostGIS's; what holds is the contract (never exceeds the convex hull, monotone in the target) |
| [`ST_DelaunayTriangles(geom)`](https://postgis.net/docs/ST_DelaunayTriangles.html) | geometry | ✅ | ✅ | ❌ | `delaunay` feature. ⚠️ returns a **MULTIPOLYGON** where PostGIS returns a GEOMETRYCOLLECTION — kenro never produces collections. The `tolerance` and `flags` arguments are not implemented; `geo`'s triangulator has no snapping tolerance, and the edge output is `ST_Boundary` of this |
| [`ST_TriangulatePolygon(geom)`](https://postgis.net/docs/ST_TriangulatePolygon.html) | geometry | ✅ | ❌ | ❌ | `delaunay` feature. The **constrained** triangulation: the triangles tile the polygon exactly, so holes and concavities stay uncovered — a square with a 2×2 hole triangulates to area 96, where `ST_DelaunayTriangles` spans the convex hull and gives 100. ⚠️ MULTIPOLYGON, not a GEOMETRYCOLLECTION; a triangulation is not unique, so the individual triangles are not GEOS's. Non-areal input is an error rather than PostGIS's empty collection |
| [`ST_VoronoiPolygons(geom [, tolerance [, extend_to]])`](https://postgis.net/docs/ST_VoronoiPolygons.html) | geometry | ✅ | ❌ | ❌ | `voronoi` feature. The dual of the triangulation above: one cell per input vertex. Uniquely in this family, PostGIS's version is **not** set-returning either, so kenro uses the real name. The clip box is PostGIS's, measured rather than assumed: the sites' envelope padded by **max(width, height) on all four sides** — so a 10×2 input pads its *height* by 10 — and `extend_to` is **unioned** with that box, envelope only, so one smaller than the default changes nothing. (`geo`'s own `Padded` mode documents itself as matching PostGIS at 50%; it does not.) ⚠️ MULTIPOLYGON, not a GEOMETRYCOLLECTION. 2D even for 3D input, as in PostGIS — a cell corner is a circumcentre, never an input vertex, so there is no Z to carry. **Collinear sites are an error** where PostGIS returns degenerate cells; use `ST_VoronoiLines` |
| [`ST_VoronoiLines(geom [, tolerance [, extend_to]])`](https://postgis.net/docs/ST_VoronoiLines.html) | geometry | ✅ | ❌ | ❌ | `voronoi` feature. The cell boundaries — the one function in the pair with **no return-type divergence**, since PostGIS returns a MULTILINESTRING here too. Also works on collinear sites, where the polygons cannot |

## Predicates, transforms and accessors reachable without a dependency

The tail of the PostGIS surface kenro can implement with what it already
carries — several of these exist *because* an earlier group landed
(`ST_DFullyWithin` is `ST_MaxDistance` with a comparison; `ST_ContainsProperly`
is the DE-9IM pattern `T**FF*FF*` read by `ST_RelateMatch`).

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_ContainsProperly(a, b)`](https://postgis.net/docs/ST_ContainsProperly.html) | INTEGER | ✅ | ❌ | ✅ | b in a's interior, touching neither boundary nor exterior — a polygon does not properly contain its own corner |
| [`ST_DFullyWithin(a, b, d)`](https://postgis.net/docs/ST_DFullyWithin.html) | INTEGER | ✅ | ❌ | ✅ | *Every* part within `d`, i.e. the maximum distance is at most `d` |
| [`ST_RelateMatch(matrix, pattern)`](https://postgis.net/docs/ST_RelateMatch.html) | INTEGER | ✅ | ❌ | ✅ | The DE-9IM pattern language: `T` non-empty, `F` empty, `*` anything, `0`/`1`/`2` exact |
| [`ST_Affine(geom, a, b, d, e, xoff, yoff)`](https://postgis.net/docs/ST_Affine.html) | geometry | ✅ | ❌ | ✅ | `x' = a·x + b·y + xoff`, `y' = d·x + e·y + yoff`. **Z and M ride through untouched**, as in PostGIS, and surface collections transform — see [3D affine transforms](3d.md#3d-affine-transforms) |
| [`ST_Affine(geom, a,b,c, d,e,f, g,h,i, xoff,yoff,zoff)`](https://postgis.net/docs/ST_Affine.html) | geometry | ✅ | ❌ | ❌ | The 3D form. A 2D geometry stays 2D — see [3D affine transforms](3d.md#3d-affine-transforms) |
| [`ST_TransScale(geom, dx, dy, xfactor, yfactor)`](https://postgis.net/docs/ST_TransScale.html) | geometry | ✅ | ❌ | ✅ | Translate **then** scale — PostGIS's order, which is easy to invert |
| [`ST_ReducePrecision(geom, gridsize)`](https://postgis.net/docs/ST_ReducePrecision.html) | geometry | ✅ | ❌ | ❌ | ⚠️ rounds only; PostGIS also repairs the result. Follow with `ST_MakeValid` if you need that |
| [`ST_Angle(p1, p2, p3 [, p4])`](https://postgis.net/docs/ST_Angle.html) | REAL / NULL | ✅ | ❌ | ❌ | ⚠️ **clockwise**, in [0, 2π) — `ST_Angle((0 0),(1 0),(0 0),(0 1))` is 270°, not 90°. POINT arguments only; the linestring form is not implemented |
| [`ST_LineInterpolatePoints(line, fraction)`](https://postgis.net/docs/ST_LineInterpolatePoints.html) | geometry / NULL | ✅ | ❌ | ✅ | A point at every multiple of `fraction`, the far end included |
| [`ST_Points(geom)`](https://postgis.net/docs/ST_Points.html) | geometry | ✅ | ✅ | ❌ | Every vertex as a MULTIPOINT, duplicates and all — a ring's closing vertex appears twice, as in PostGIS |
| [`ST_BoundingDiagonal(geom)`](https://postgis.net/docs/ST_BoundingDiagonal.html) | geometry / NULL | ✅ | ❌ | ❌ | The bounding box's lower-left → upper-right line |
| [`ST_OrderingEquals(a, b)`](https://postgis.net/docs/ST_OrderingEquals.html) | INTEGER | ✅ | ❌ | ✅ | Same geometry *and* same vertex order, unlike the topological `ST_Equals` |
| [`ST_GeoHash(geom [, maxchars])`](https://postgis.net/docs/ST_GeoHash.html) | TEXT / NULL | ✅ | ❌ | ✅ | 20 characters by default. An extended geometry keeps only the prefix its bbox corners agree on (PostGIS's behavior); non-lon/lat input is an error |
| `ST_Extent(geom)` **aggregate** | geometry / NULL | ✅ | ✅ | ✅ | ⚠️ returns a **POLYGON**: PostGIS returns its `box2d` type, which SQLite has no equivalent for. NULL rows are skipped; an all-NULL group is NULL |
| `ST_3DExtent(geom)` **aggregate** | TEXT / NULL | ✅ | ❌ | ❌ | ⚠️ returns **text** — `BOX3D(minx miny minz,maxx maxy maxz)` — see [3D affine transforms](3d.md#3d-affine-transforms) for why, and what to use instead |

## GML 2/3 I/O

`gml` feature (in `full`). Reading pulls `quick-xml`; writing is hand-rolled
like the WKT and GeoJSON emitters, so the pair costs **+31 KB raw / +13 KB
gzipped** measured on the full tier. SpatiaLite reaches for libxml2 here
because it validates against a schema and stores XmlBLOBs (`XB_*`); kenro
does neither, and a pull parser is all that is left.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_AsGML([version, ] geom [, maxdecimaldigits])`](https://postgis.net/docs/ST_AsGML.html) | TEXT | ✅ | ❌ | ✅ | Byte-identical to PostGIS for the shapes kenro supports, golden-tested — including GML 3's habit of writing a `Curve` with segments where a `LineString` would do, and `MultiCurve`/`MultiSurface` for the multis. Version defaults to 2 and precision to 15, as in PostGIS. The `options`, `nprefix` and `id` arguments are not implemented: always the `gml:` prefix, never an id |
| [`ST_GeomFromGML(text [, srid])`](https://postgis.net/docs/ST_GeomFromGML.html) / [`ST_GMLToSQL`](https://postgis.net/docs/ST_GMLToSQL.html) | geometry | ✅ | ❌ | ✅ | Structural, not schema-driven: elements are matched by **local name**, so any namespace prefix works and unknown elements are ignored — which is what lets a CityGML fragment be read without carrying the schema. `srsName` is read from either `EPSG:6697` or the `urn:ogc:def:crs:EPSG::6697` form. `srsDimension="3"` sets the coordinate stride, and the Z is dropped like everywhere else in kenro |

## KML and SVG output

Both are `text-encodings`. Neither uses an XML library — the only text that
is not a number is `ST_AsKML`'s namespace prefix, and that is validated as an
XML name rather than escaped, so a prefix that would break the document is an
error instead of a literally-named element. The feature exists because of the
first row's reprojection, not because of a parser.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_AsKML(geom [, maxdecimaldigits [, nprefix]])`](https://postgis.net/docs/ST_AsKML.html) | TEXT | ✅ | ❌ | ⚠️ named `AsKml` | **Reprojects to WGS84**, and errors on SRID 0 — KML is defined in lon/lat and PostGIS transforms rather than labelling, unlike `ST_AsGML` next door. The reprojection is kenro's gridless one ([accuracy](accuracy.md)). Rings keep their closing vertex; all three multi types become `MultiGeometry`; an empty geometry gives an empty string; a GeometryCollection is an error, as in PostGIS. ⚠️ 3D input is an error where PostGIS writes a third ordinate — `ST_Force2D` first |
| [`ST_AsSVG(geom [, rel [, maxdecimaldigits]])`](https://postgis.net/docs/ST_AsSVG.html) | TEXT | ✅ | ❌ | ⚠️ named `AsSvg` | A path fragment, not a document. **Y is negated** (SVG's axis points down), so `POINT(1 2)` is `cx="1" cy="-2"`. `rel = 1` switches to relative commands (`l`, `z`) **and renames the point attributes to `x`/`y`** — an undocumented PostGIS behaviour kenro matches. Rings drop their closing vertex in favour of `Z`. Relative deltas are taken on full-precision coordinates and rounded after. ⚠️ 3D input is an error |

## The tail

Alternative spellings and the small functions that had simply never been
written. Aliases share their original's implementation and wasm export.

| Alias | Same as |
|---|---|
| `ST_RotateZ(geom, radians)` | `ST_Rotate` — kenro is 2D, so Z *is* the rotation axis |
| `ST_MultiPointFromText` / `ST_MultiLineStringFromText` / `ST_MultiPolygonFromText` | `ST_MPointFromText` / `ST_MLineFromText` / `ST_MPolyFromText` |
| `ST_PolygonFromWKB` / `ST_LineStringFromWKB` / `ST_MultiPointFromWKB` / `ST_MultiLineFromWKB` / `ST_MultiPolyFromWKB` | the `ST_*FromWKB` typed constructors, srid form included |
| `ST_Box2dFromGeoHash` | `ST_GeomFromGeoHash` |

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| [`ST_Polygon(line, srid)`](https://postgis.net/docs/ST_Polygon.html) | geometry | ✅ | ✅ | ✅ | `ST_MakePolygon` with an SRID |
| [`ST_LineFromMultiPoint(multipoint)`](https://postgis.net/docs/ST_LineFromMultiPoint.html) | geometry / NULL | ✅ | ❌ | ✅ | The points in order |
| [`ST_LineExtend(line, forward [, backward])`](https://postgis.net/docs/ST_LineExtend.html) | geometry / NULL | ✅ | ❌ | ❌ | Extends along the end segments; the original vertices survive |
| [`ST_PointInsideCircle(point, cx, cy, radius)`](https://postgis.net/docs/ST_PointInsideCircle.html) | INTEGER | ✅ | ❌ | ✅ | Planar |
| [`ST_WrapX(geom, wrap, move)`](https://postgis.net/docs/ST_WrapX.html) | geometry | ✅ | ❌ | ❌ | ⚠️ kenro shifts **whole vertices**; PostGIS cuts segments that span the meridian. `ST_Segmentize` first if that matters. Z/M ride through; a surface collection is an error, as in PostGIS |
| [`ST_MakeBox2D(low, high)`](https://postgis.net/docs/ST_MakeBox2D.html) | geometry | ✅ | ❌ | ✅ | ⚠️ POLYGON, not `box2d` (as with `ST_Extent`, `ST_Expand`) |
| [`ST_GeomFromGeoHash(hash [, precision])`](https://postgis.net/docs/ST_GeomFromGeoHash.html) | geometry | ✅ | ❌ | ✅ | The cell as a POLYGON, SRID 4326 |
| [`ST_PointFromGeoHash(hash [, precision])`](https://postgis.net/docs/ST_PointFromGeoHash.html) | geometry | ✅ | ❌ | ✅ | Its centre; round-trips with `ST_GeoHash` |
| [`ST_GeometricMedian(geom [, tolerance])`](https://postgis.net/docs/ST_GeometricMedian.html) | geometry / NULL | ✅ | ❌ | ❌ | Weiszfeld iteration over the vertices |
| [`ST_LineCrossingDirection(a, b)`](https://postgis.net/docs/ST_LineCrossingDirection.html) | INTEGER | ✅ | ❌ | ✅ | PostGIS's codes: 0 none, ±1 single, ±2 multiple same-side, ±3 multiple ending that way |
| [`ST_Summary(geom)`](https://postgis.net/docs/ST_Summary.html) | TEXT | ✅ | ❌ | ✅ | ⚠️ PostGIS prints a tree with byte offsets; kenro keeps the leading token (`Point[S]`) and adds its vertex count |
| [`ST_MemSize(geom)`](https://postgis.net/docs/ST_MemSize.html) | INTEGER | ✅ | ❌ | ✅ | ⚠️ the length of the GeoPackage blob kenro would store — the number that means something for a SQLite column, not PostGIS's in-memory size |
| [`ST_Normalize(geom)`](https://postgis.net/docs/ST_Normalize.html) | geometry | ✅ | ❌ | ❌ | Rings oriented, parts ordered by bounding box. ⚠️ PostGIS orders by its own internal comparison, so the two agree on orientation but not always on part order |

## Routing — see [Routing](routing.md)

kenro-only, and the one family whose reference is **pgRouting** rather than
PostGIS, which has no routing at all. Both are aggregates over an edge table:
one input row per edge, the query's `WHERE` clause playing the part of
pgRouting's SQL-string argument. Needs the `routing` feature (in `full`).

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `kenro_dijkstra(id, source, target, cost, start_vid, end_vid [, reverse_cost])` | TEXT / NULL | ❌ (pgRouting `pgr_dijkstra`) | ❌ | ⚠️ VirtualRouting, a virtual table | Aggregate. The `pgr_dijkstra` row shape as a JSON array — `json_each` turns it into rows. ⚠️ `reverse_cost` is the **trailing** argument, not an edge-query column; ids are i32 |
| `kenro_dijkstra_cost(source, target, cost, start_vid, end_vid [, reverse_cost])` | REAL / NULL | ❌ (pgRouting `pgr_dijkstraCost`) | ❌ | ⚠️ VirtualRouting | Aggregate. The total cost only, without materializing the path |

Directed graph; a negative `cost` closes that direction, `reverse_cost` is the
`target → source` cost. Zero rows, no path, a missing endpoint and
`start_vid = end_vid` are all NULL — pgRouting returns the empty set for each,
and the golden vectors pin it. Full semantics in [Routing](routing.md).

## 3D — see [3D geometry](3d.md)

Every 3D function table lives there rather than here: pass-through accessors
(`ST_HasZ`, `ST_ZMin`, `ST_NDims`, …), surface collections (`ST_NumPatches`,
`ST_PatchN`, `ST_IsClosed`), the affine transforms, `ST_Force3D` /
`ST_MakePoint(x, y, z)`, and the nine `ST_3D*` metric functions.

They are there because each carries measured caveats — which functions keep a
Z, which refuse, where PostGIS contradicts itself — and a table cell cannot hold
them. Splitting the rows from the reasoning would leave both halves
misleading.
