# Function reference

Every SQL function kenro registers, with its support status in PostGIS,
DuckDB Spatial and SpatiaLite for comparison (columns verified against
PostGIS 3.5, a live DuckDB 1.4.0 + spatial session, and a live
mod_spatialite 5.1 session, July–August 2026). ✅ = present with the same
name and compatible semantics; deviations are spelled out.

Functions marked with the `overlay`, `spheroid`, `concave-hull`,
`delaunay`, `gml` or `text-encodings` feature need a `full` build (default builds register them as stubs naming the feature); everything
else, including MVT, is in the default set (see
[Cargo features](../README.md#cargo-features)).

"Geometry" values in and out of kenro functions are GeoPackage blobs — they
carry the SRID, and a value in a gpkg column is already valid storage. Every
geometry-accepting function also auto-detects raw WKB input, so
`ST_Within(p.geom, …)` works directly on a gpkg column.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| **Geometry I/O** | | | | | |
| `ST_GeomFromText(wkt [, srid])` | geometry | ✅ | ⚠️ no srid arg | ✅ | kenro rejects `POINT EMPTY` (geometry model limit); DuckDB's geometry is SRID-less |
| `ST_GeomFromWKB(wkb [, srid])` | geometry | ✅ | ⚠️ no srid arg | ✅ | Accepts ISO WKB and EWKB; an explicit srid overrides an embedded one (PostGIS behavior) |
| `ST_GeomFromGPB(gpb)` | geometry | ❌ | ❌ | ⚠️ named `GeomFromGPB` | kenro-only: validates + normalizes a GeoPackage blob. DuckDB imports gpkg **files** via GDAL `ST_Read`; PostGIS needs ogr2ogr |
| `ST_GeomFromGeoJSON(text)` | geometry | ✅ keeps Z | ✅ | ⚠️ named `GeomFromGeoJSON` | SRID 4326 per RFC 7946 (PostGIS ≥ 3.0); kenro is 2D-only and errors on 3D rather than dropping Z |
| `ST_AsText(geom)` | TEXT | ✅ | ✅ | ✅ | Byte-identical to PostGIS across the golden suite. One rendering difference exists for values that are not exactly representable: kenro writes the shortest string that round-trips the double, PostGIS trims to 15 significant digits — so a coordinate of `1.2000000000000002` prints in full here and as `1.2` there. The doubles are the same |
| `ST_AsBinary(geom)` | BLOB | ✅ | ❌ named `ST_AsWKB` | ✅ | ISO WKB, little-endian, SRID dropped (as in PostGIS); PostGIS conversely has no `ST_AsWKB` |
| `ST_AsGPB(geom)` | BLOB | ❌ | ❌ | ⚠️ named `AsGPB` | kenro-only: storage-grade GeoPackage blob (envelope included) — use for writing gpkg columns |
| `ST_AsGeoJSON(geom [, maxdecimaldigits])` | TEXT | ✅ | ✅ JSON fragment | ⚠️ named `AsGeoJSON` | Default 9 digits; kenro's output is byte-identical to PostGIS (golden-tested) |
| **SRID & CRS transform** | | | | | |
| `ST_SRID(geom)` | INT | ✅ | ❌ | ✅ | 0 = unknown. DuckDB's `GEOMETRY` carries no SRID at all — CRS bookkeeping is manual there |
| `ST_SetSRID(geom, srid)` | geometry | ✅ | ❌ | ⚠️ named `SetSRID` | Relabel only, no reprojection |
| `ST_Transform(geom, to_srid)` | geometry | ✅ 4 overloads, full PROJ | ⚠️ `(geom, source_crs, target_crs [, always_xy])` | ✅ | kenro: PostGIS-exact 2-arg form, source = embedded SRID, curated EPSG table (see [Supported CRS](../README.md#supported-crs), [accuracy](accuracy.md)). DuckDB must be told the source CRS on every call. **Z and surface collections ride through** — see [3D affine transforms](#3d-affine-transforms) |
| **Constructors** | | | | | |
| `ST_MakePoint(x, y)` | geometry | ✅ | ✅ | ⚠️ named `MakePoint` | 2D only |
| `ST_Point(x, y [, srid])` | geometry | ✅ | ⚠️ no srid arg | ✅ | The srid form is PostGIS 3.2+ |
| `ST_MakeEnvelope(xmin, ymin, xmax, ymax [, srid])` | geometry | ✅ | ⚠️ no srid arg | ⚠️ named `BuildMBR` | Degenerate extents still return a POLYGON, like PostGIS |
| **Predicates** | | | | | |
| `ST_Intersects(a, b)` | 0/1 | ✅ | ✅ | ✅ | DE-9IM. kenro errors on GeometryCollection operands (PostGIS's `ST_Intersects` accepts them; its `ST_Contains`/`ST_Within` also error) |
| `ST_Contains(a, b)` | 0/1 | ✅ | ✅ | ✅ | Boundary semantics golden-tested against PostGIS |
| `ST_Within(a, b)` | 0/1 | ✅ | ✅ | ✅ | `ST_Within(a,b) = ST_Contains(b,a)`, property-tested |
| `ST_Disjoint(a, b)` | 0/1 | ✅ | ✅ | ✅ | Empty operands → true (golden-arbitrated PostGIS behavior) |
| `ST_Touches(a, b)` / `ST_Crosses(a, b)` / `ST_Overlaps(a, b)` | 0/1 | ✅ | ✅ | ✅ | |
| `ST_Equals(a, b)` | 0/1 | ✅ | ✅ | ✅ | Topological equality; both-empty → true |
| `ST_Covers(a, b)` / `ST_CoveredBy(a, b)` | 0/1 | ✅ | ✅ | ✅ | The boundary-tolerant contains/within variants |
| `ST_Relate(a, b)` | TEXT | ✅ | ❌ | ✅ | The 9-character DE-9IM matrix |
| `ST_Relate(a, b, pattern)` | 0/1 | ✅ | ❌ | ✅ | Pattern matching with `*`/`T`/`F`/`0`/`1`/`2` |
| **Measures** | | | | | |
| `ST_Distance(a, b)` | REAL | ✅ | ✅ | ✅ | 2D cartesian; NULL for empty inputs |
| `ST_DWithin(a, b, d)` | 0/1 | ✅ | ✅ | ⚠️ named `PtDistWithin` | `distance <= d`; negative tolerance errors (matches PostGIS) |
| `ST_ClosestPoint(a, b)` | geometry | ✅ any × any | ❌ (`ST_ShortestLine` instead) | ✅ | kenro: second operand must be a POINT (geo API limit) — anything else errors |
| `ST_LineInterpolatePoint(line, fraction)` | geometry | ✅ | ✅ | ⚠️ `ST_Line_Interpolate_Point` | Fraction outside [0, 1] errors, like PostGIS |
| `ST_LineLocatePoint(line, point)` | REAL | ✅ | ✅ | ⚠️ `ST_Line_Locate_Point` | |
| `ST_HausdorffDistance(a, b)` | REAL | ✅ | ❌ | ✅ | kenro computes vertex-to-vertex distance (geo API), GEOS vertex-to-segment — equal on shared golden vectors, can differ on long sparse segments |
| `ST_FrechetDistance(a, b)` | REAL | ✅ + densify arg | ❌ | ✅ | kenro: LINESTRING × LINESTRING, 2-arg form only |
| `ST_Azimuth(a, b)` | REAL | ✅ | ✅ | ✅ | Radians clockwise from north; coincident points → NULL |
| **Overlay & buffer** (pure Rust — see [semantics](#semantics-postgis-is-the-reference)) | | | | | |
| `ST_Intersection(a, b)` | geometry | ✅ | ✅ | ✅ | Areal results only: polygons that merely touch → empty, where GEOS returns the shared LINESTRING. line × line errors (needs noding) |
| `ST_Difference(a, b)` | geometry | ✅ | ✅ | ✅ | Same decision matrix; point operands are filtered exactly |
| `ST_SymDifference(a, b)` | geometry | ✅ | ❌ | ✅ | areal × areal and puntal × puntal; mixed dimensions error |
| `ST_Union(a, b)` | geometry | ✅ | ✅ | ✅ | Scalar form; line unions error (noding) |
| `ST_Union(geom)` **aggregate** | geometry | ✅ | ⚠️ named `ST_Union_Agg` | ✅ | Dissolve in `GROUP BY`; NULL rows skipped, zero rows → NULL (PostGIS aggregate semantics) |
| `ST_Buffer(geom, d [, opts])` | geometry | ✅ | ⚠️ 3rd arg differs | ⚠️ no style options | PostGIS-style options TEXT (`quad_segs= endcap= join= mitre_limit=`) or INTEGER quad_segs; `side=` unsupported. Negative distance erodes polygons. Golden-tested within 2% area of GEOS |
| `ST_MakeValid(geom)` | geometry | ✅ + params arg | ✅ | ✅ | GEOS *structure*-method semantics: bowties split, stray holes become polygons, zero-area parts drop — areal results only, where PostGIS's default linework method can return collections with lines. Points/lines pass through unchanged. Property-tested: output always validates and is idempotent |
| **Processing & affine** | | | | | |
| `ST_ConvexHull(geom)` | geometry | ✅ | ✅ | ✅ | Degenerate hulls collapse to POINT/LINESTRING like PostGIS |
| `ST_PointOnSurface(geom)` | geometry | ✅ | ✅ | ✅ | Guaranteed interior; exact coordinates may differ from GEOS (documented in vectors) |
| `ST_Simplify(geom, tol)` | geometry | ✅ + `preserveCollapsed` arg | ✅ 2-arg; also `ST_SimplifyPreserveTopology` | ✅ | Ramer-Douglas-Peucker, collapse allowed (= PostGIS 2-arg form) |
| `ST_SimplifyVW(geom, tol)` | geometry | ✅ | ❌ | ❌ | Visvalingam-Whyatt; tolerance is an area |
| `ST_ChaikinSmoothing(geom [, iterations])` | geometry | ✅ | ❌ | ❌ | PostGIS variant (endpoints preserved on open lines); iterations capped at 5 |
| `ST_RemoveRepeatedPoints(geom)` | geometry | ✅ + tolerance arg | ✅ | ❌ | kenro: exact duplicates only (no tolerance form) |
| `ST_OrientedEnvelope(geom)` | geometry | ✅ | ⚠️ named `ST_MinimumRotatedRectangle` | ✅ | Minimum rotated rectangle; equal-area alternates possible (rotation-normalized comparison in goldens) |
| `ST_Rotate(geom, radians [, x0, y0])` | geometry | ✅ | ✅ | ⚠️ named `RotateCoords` | About the origin (or the given point) — PostGIS semantics, **not** geo's centroid default. Z/M and surface collections ride through — [3D affine transforms](#3d-affine-transforms) |
| `ST_Translate(geom, dx, dy)` | geometry | ✅ | ✅ | ✅ | Z/M and surface collections ride through — [3D affine transforms](#3d-affine-transforms) |
| `ST_Scale(geom, xf, yf)` | geometry | ✅ | ✅ | ⚠️ named `ScaleCoords` | About the origin, like PostGIS. Z is **not** scaled by this arity (measured); the 3-argument form is not implemented |
| **GeoPackage triggers** | | | | | |
| `ST_MinX` / `ST_MaxX` / `ST_MinY` / `ST_MaxY` | REAL | ⚠️ named `ST_XMin` … | ⚠️ named `ST_XMin` … | ✅ | kenro uses the GeoPackage spec's R-tree trigger names — required verbatim for gpkg index maintenance; the other two spell it `ST_XMin` |
| `ST_IsEmpty(geom)` | 0/1 | ✅ | ✅ | ✅ | gpkg R-tree contract; NULL on NULL |
| `GPKG_IsAssignable(expected, actual)` | 0/1 | ❌ | ❌ | ✅ | The geometry-type-trigger helper. ⚠️ that extension was **removed from the GeoPackage standard in 2016** over interoperability concerns and now survives only in the 1.1.0 archive — kenro keeps the function because files carrying those triggers are still out there. Accepts both `'POINT'` and `'ST_Point'` spellings so the old DDL works with kenro's `ST_GeometryType` |
| **H3 cells** (`h3` feature) | | | | | |
| `h3_latlng_to_cell(geom, res)` | INT | via [h3-pg] ext | via community `h3` ext | ❌ | Same name in all three ecosystems; POINT in lon/lat only |
| `h3_cell_to_parent(cell, res)` | INT | via h3-pg | via `h3` ext | ❌ | For coarser `GROUP BY` |
| `h3_cell_to_string(cell)` / `h3_string_to_cell(text)` | TEXT / INT | ⚠️ h3-pg casts its `h3index` type | ⚠️ DuckDB: `h3_h3_to_string` | ❌ | Hex-string conversion names differ per ecosystem; kenro uses the H3 v4 canonical verbs |
| **Accessors** | | | | | |
| `ST_Area(geom)` | REAL | ✅ | ✅ | ✅ | Planar; 0 for non-areal or empty |
| `ST_Length(geom)` | REAL | ✅ | ✅ | ✅ | Linear geometries only — polygons return 0 in all three |
| `ST_Perimeter(geom)` | REAL | ✅ | ✅ | ✅ | Ring lengths; 0 for non-areal |
| `ST_Centroid(geom)` | geometry | ✅ | ✅ | ✅ | Empty input → `POINT EMPTY` |
| `ST_Envelope(geom)` | geometry | ✅ | ✅ | ✅ | Degenerates to POINT/LINESTRING exactly like PostGIS. DuckDB also has scalar `ST_Extent` → `BOX_2D` |
| `ST_X(geom)` / `ST_Y(geom)` | REAL | ✅ | ✅ | ✅ | POINT only, error otherwise (as in PostGIS); `POINT EMPTY` → NULL |
| `ST_NPoints(geom)` | INT | ✅ | ✅ | ✅ | All vertices of any type; empty → 0 |
| `ST_NumPoints(geom)` | INT / NULL | ✅ | ⚠️ synonym of `ST_NPoints` | ✅ | LINESTRING-only, NULL for other types (PostGIS as implemented; its docs lag). **Same name, different answer in DuckDB** (counts all vertices of any type) — kenro follows PostGIS |
| `ST_GeometryType(geom)` | TEXT | ✅ | ⚠️ returns `POINT` | ✅ | kenro returns the PostGIS spelling `ST_Point`; DuckDB returns bare `POINT` |
| `ST_NumGeometries(geom)` | INT | ✅ | ✅ | ✅ | Single geometry → 1; empty → 0 |
| `ST_GeometryN(geom, n)` | geometry / NULL | ✅ | ❌ | ✅ | 1-based; out of range → NULL; GeometryCollection supported |
| `ST_StartPoint(geom)` / `ST_EndPoint(geom)` | geometry / NULL | ✅ | ✅ | ✅ | PostGIS 3.2 semantics: POINT → itself, MULTILINESTRING works, areal → NULL |
| `ST_PointN(line, n)` | geometry / NULL | ✅ | ✅ | ✅ | 1-based with negative indexing from the end |
| `ST_Reverse(geom)` | geometry | ✅ | ✅ | ✅ | Member order of multi geometries preserved |
| `ST_IsValid(geom)` | 0/1 | ✅ GEOS | ✅ GEOS | ✅ | georust validation: everything incl. ring self-intersection and hole placement, except the split-interior case (documented gap) |
| **Vector tiles (MVT)** | | | | | |
| `ST_AsMVTGeom(geom, bounds [, extent [, buffer [, clip]]])` | geometry / NULL | ✅ | ✅ | ❌ | World → integer tile coordinates (Y down); clipped-away input → NULL. `bounds` is any geometry (its envelope is used). ±1 pixel vs PostGIS at tile edges (kenro clips before grid snapping, PostGIS after). **`full` builds add PostGIS-grade validity repair**: invalid input and snap-induced self-intersections are made valid (golden-tested); standard builds clip exactly but pass invalid rings through |
| `ST_AsMVT(geom [, name [, extent [, props_json]]])` **aggregate** | BLOB | ⚠️ record-based signature | ⚠️ | ❌ | **Deliberate signature divergence**: SQLite has no record type, so properties come from `json_object(...)` instead of row columns. A PostGIS-style call fails loudly at the type level. Layer name/extent must be constant per group |
| **Stubs** (call = helpful error) | | | | | |
| `ST_Collect` | stub | ✅ | ✅ | ✅ | kenro never produces GeometryCollection values; use the `ST_Union` aggregate for areal dissolve, or collect rows on the application side |

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
| `ST_Force2D(geom)` | geometry | ✅ | ✅ | ❌ | Drops Z/M. kenro decodes 3D input but refuses to *encode* it rather than silently writing 2D — this is the explicit opt-in, and the only way a 3D GeoPackage column reaches `ST_AsText`/`ST_AsGeoJSON` |
| `ST_AsEWKT(geom)` | TEXT | ✅ | ❌ | ✅ | `SRID=n;` prefix, omitted when the SRID is 0 (PostGIS behavior, verified live) |
| `ST_GeomFromEWKT(text)` | geometry | ✅ | ❌ | ✅ | Accepts the prefix or plain WKT |
| `ST_AsEWKB(geom)` | BLOB | ✅ | ❌ | ✅ | ISO WKB with PostGIS's `0x20000000` SRID flag; plain WKB when the SRID is 0 |
| `ST_AsHexEWKB(geom)` | TEXT | ✅ | ❌ | ✅ | Upper-case hex of the above; byte-identical to PostGIS 3.5 (golden-tested) |
| `ST_PointFromText` / `ST_LineFromText` / `ST_LineStringFromText` / `ST_PolyFromText` / `ST_PolygonFromText` / `ST_MPointFromText` / `ST_MLineFromText` / `ST_MPolyFromText` `(wkt [, srid])` | geometry / NULL | ✅ | ⚠️ partial | ✅ | Parse, then **NULL when the geometry is another type** — an error would be the intuitive choice, but PostGIS returns NULL and so does kenro |
| `ST_PointFromWKB` / `ST_LineFromWKB` / `ST_PolyFromWKB` / `ST_MPointFromWKB` / `ST_MLineFromWKB` / `ST_MPolyFromWKB` `(bytes [, srid])` | geometry / NULL | ✅ | ❌ | ✅ | Same contract over WKB/EWKB/GeoPackage input. kenro takes the srid on all six; stock PostGIS omits it from `ST_MultiLineFromWKB` alone, which looks like an oversight rather than a rule |

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
| `ST_ExteriorRing(polygon)` | geometry / NULL | ✅ | ✅ | ✅ | NULL for any non-polygon |
| `ST_InteriorRingN(polygon, n)` | geometry / NULL | ✅ | ✅ | ✅ | 1-based; NULL when out of range |
| `ST_NumInteriorRings(polygon)` / `ST_NumInteriorRing` | INTEGER / NULL | ✅ | ✅ | ✅ | Both spellings, as in PostGIS |
| `ST_NRings(geom)` | INTEGER | ✅ | ❌ | ✅ | Exterior + interior, summed over a multipolygon |
| `ST_Boundary(geom)` | geometry | ✅ | ❌ | ✅ | Polygon → its rings; open line → `MULTIPOINT` of the endpoints; the mod-2 rule applies to a multilinestring |
| `ST_IsClosed(geom)` | INTEGER | ✅ | ✅ | ✅ | Areal input is closed by definition |
| `ST_IsRing(line)` | INTEGER | ✅ | ❌ | ✅ | Closed *and* simple. **Raises** on non-linear input — PostGIS's wording, kept |
| `ST_AddPoint(line, point [, position])` | geometry / NULL | ✅ | ❌ | ✅ | 0-based; default (or -1) appends |
| `ST_SetPoint(line, index, point)` | geometry / NULL | ✅ | ❌ | ✅ | 0-based |
| `ST_RemovePoint(line, index)` | geometry / NULL | ✅ | ❌ | ✅ | 0-based |
| `ST_MakeLine(a, b)` | geometry | ✅ | ✅ | ✅ | Two-geometry form; points and lines concatenate. The aggregate form is not implemented |
| `ST_MakePolygon(line)` | geometry | ✅ | ❌ | ✅ | Shell must be closed; the with-holes arity is not implemented |
| `ST_Multi(geom)` | geometry | ✅ | ❌ | ✅ | Already-multi input passes through |
| `ST_SnapToGrid(geom, size)` / `(geom, sizex, sizey)` | geometry | ✅ | ❌ | ✅ | Grid anchored at the origin; size 0 leaves that axis alone. The origin-offset arities are not implemented. ⚠️ **PostGIS also drops the vertices that collapse together** — `ST_SnapToGrid(LINESTRING(0 0,0.1 0.1,1 1,1.1 1.1), 1)` is `LINESTRING(0 0,1 1)` there against kenro's `LINESTRING(0 0,0 0,1 1,1 1)`, and a fully-collapsing polygon is `POLYGON EMPTY` against kenro's degenerate ring (measured). `ST_RemoveRepeatedPoints` after, for the vertex behavior. 2D only, and 3D input is an error rather than a silent flatten |
| `ST_FlipCoordinates(geom)` | geometry | ✅ | ✅ | ✅ | The lat/lon-order fix. x and y only: Z/M stay put, and surface collections flip — [3D affine transforms](#3d-affine-transforms) |
| `ST_ShiftLongitude(geom)` | geometry | ✅ | ❌ | ✅ | x from [-180,180) into [0,360). Z/M and surface collections ride through |
| `ST_Expand(geom, units)` | geometry / NULL | ✅ | ❌ | ✅ | ⚠️ returns a **POLYGON**: PostGIS returns its `box2d` type, which SQLite has no equivalent for |

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
| `ST_DistanceSphere(a, b)` | REAL | ✅ | ❌ | ✅ | Great-circle metres, radius 6 371 008.7714 m — the same radius PostGIS uses (golden-tested against it). **POINT arguments only**; PostGIS takes any pair |
| `ST_DistanceSpheroid(a, b [, spheroid])` | REAL | ✅ | ❌ | ✅ | `spheroid` feature. WGS84 by default; the third argument takes PostGIS's `SPHEROID["name",a,1/f]` text. POINT arguments only |
| `ST_LengthSpheroid(geom, spheroid)` / `ST_Length2DSpheroid` | REAL | ✅ | ❌ | ✅ | `spheroid` feature. Geodesic length in metres; PostGIS has no one-argument form, so neither does kenro |
| `ST_Project(point, distance, azimuth)` | geometry | ✅ | ❌ | ❌ | ⚠️ **planar**, matching PostGIS's *geometry* overload (verified live). PostGIS's geodesic behavior belongs to its `geography` overload — transform to a projected CRS first |

## Dimension, validity and orientation

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_Dimension(geom)` | INTEGER | ✅ | ✅ | ✅ | 0/1/2 for puntal/lineal/areal |
| `ST_CoordDim(geom)` / `ST_NDims(geom)` | INTEGER | ✅ | ✅ | ✅ | Always 2 — kenro is 2D, and 3D input has Z/M dropped on decode (see `ST_Force2D`) |
| `ST_IsValidReason(geom)` | TEXT | ✅ | ✅ | ✅ | `"Valid Geometry"` or a description. ⚠️ **wording is geo's, not PostGIS's**: PostGIS says `Self-intersection[1 1]` with the coordinate, geo names the ring and defect. A diagnostic, not a string to match on |
| `ST_ForcePolygonCW(geom)` / `ST_ForceRHR(geom)` | geometry | ✅ | ❌ | ✅ | Exterior clockwise, interiors counter-clockwise; non-areal input passes through |
| `ST_ForcePolygonCCW(geom)` | geometry | ✅ | ❌ | ✅ | The mirror |
| `ST_IsPolygonCW(geom)` / `ST_IsPolygonCCW(geom)` | INTEGER | ✅ | ❌ | ✅ | Non-areal input is true |

## Linear referencing and distance geometry

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_Segmentize(geom, max_length)` | geometry | ✅ | ❌ | ✅ | Splits each segment into **equal** parts, as PostGIS does: 10 units at a maximum of 4 gives three 3⅓ segments, not 4+4+2 |
| `ST_LineSubstring(line, from, to)` | geometry / NULL | ✅ | ❌ | ✅ | Fractions in [0,1]; NULL for non-linear input, error for a bad range |
| `ST_ShortestLine(a, b)` | geometry / NULL | ✅ | ✅ | ✅ | Searched vertex-against-segment both ways — exact when the geometries are disjoint; when they intersect the distance is 0 and PostGIS may pick a different, equally valid zero-length line |
| `ST_LongestLine(a, b)` | geometry / NULL | ✅ | ❌ | ✅ | Always attained at a vertex pair |
| `ST_MaxDistance(a, b)` | REAL / NULL | ✅ | ❌ | ✅ | The length of `ST_LongestLine` |

## Enclosing circle and areal operations

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_MinimumBoundingRadius(geom)` | REAL / NULL | ✅ | ❌ | ✅ | ⚠️ PostGIS returns a `(center, radius)` record; SQLite has no record type, so kenro returns the radius. The centre is `ST_Centroid(ST_MinimumBoundingCircle(geom))` |
| `ST_MinimumBoundingCircle(geom [, segs_per_quarter])` | geometry / NULL | ✅ | ❌ | ✅ | Welzl, run deterministically (no shuffle) so SQL is reproducible; 48 segments per quarter by default, as in PostGIS |
| `ST_UnaryUnion(geom)` | geometry | ✅ | ❌ | ✅ | `overlay` feature. Dissolves a multipolygon's overlapping members into one areal result; non-areal input passes through |
| `ST_ClipByBox2D(geom, box)` | geometry | ✅ | ❌ | ❌ | `overlay` feature. ⚠️ takes **any geometry** and uses its envelope (PostGIS takes a `box2d`) — pass `ST_MakeEnvelope(...)`. Unlike PostGIS, which documents that it may return an invalid geometry, this goes through the overlay engine, so the result is valid |
| `ST_Subdivide(geom, max_vertices)` | geometry | ✅ | ❌ | ❌ | `overlay` feature. ⚠️ PostGIS returns **one row per part**; kenro has no set-returning functions, so this returns a MULTIPOLYGON — walk it with `ST_NumGeometries` / `ST_GeometryN`. Splits along the longer axis; `max_vertices` must be ≥ 5 |

## Predicates, transforms and accessors reachable without a dependency

The tail of the PostGIS surface kenro can implement with what it already
carries — several of these exist *because* an earlier group landed
(`ST_DFullyWithin` is `ST_MaxDistance` with a comparison; `ST_ContainsProperly`
is the DE-9IM pattern `T**FF*FF*` read by `ST_RelateMatch`).

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_ContainsProperly(a, b)` | INTEGER | ✅ | ❌ | ✅ | b in a's interior, touching neither boundary nor exterior — a polygon does not properly contain its own corner |
| `ST_DFullyWithin(a, b, d)` | INTEGER | ✅ | ❌ | ✅ | *Every* part within `d`, i.e. the maximum distance is at most `d` |
| `ST_RelateMatch(matrix, pattern)` | INTEGER | ✅ | ❌ | ✅ | The DE-9IM pattern language: `T` non-empty, `F` empty, `*` anything, `0`/`1`/`2` exact |
| `ST_Affine(geom, a, b, d, e, xoff, yoff)` | geometry | ✅ | ❌ | ✅ | `x' = a·x + b·y + xoff`, `y' = d·x + e·y + yoff`. **Z and M ride through untouched**, as in PostGIS, and surface collections transform — see [3D affine transforms](#3d-affine-transforms) |
| `ST_Affine(geom, a,b,c, d,e,f, g,h,i, xoff,yoff,zoff)` | geometry | ✅ | ❌ | ❌ | The 3D form. A 2D geometry stays 2D — see [3D affine transforms](#3d-affine-transforms) |
| `ST_TransScale(geom, dx, dy, xfactor, yfactor)` | geometry | ✅ | ❌ | ✅ | Translate **then** scale — PostGIS's order, which is easy to invert |
| `ST_ReducePrecision(geom, gridsize)` | geometry | ✅ | ❌ | ❌ | ⚠️ rounds only; PostGIS also repairs the result. Follow with `ST_MakeValid` if you need that |
| `ST_Angle(p1, p2, p3 [, p4])` | REAL / NULL | ✅ | ❌ | ❌ | ⚠️ **clockwise**, in [0, 2π) — `ST_Angle((0 0),(1 0),(0 0),(0 1))` is 270°, not 90°. POINT arguments only; the linestring form is not implemented |
| `ST_LineInterpolatePoints(line, fraction)` | geometry / NULL | ✅ | ❌ | ✅ | A point at every multiple of `fraction`, the far end included |
| `ST_Points(geom)` | geometry | ✅ | ✅ | ❌ | Every vertex as a MULTIPOINT, duplicates and all — a ring's closing vertex appears twice, as in PostGIS |
| `ST_BoundingDiagonal(geom)` | geometry / NULL | ✅ | ❌ | ❌ | The bounding box's lower-left → upper-right line |
| `ST_OrderingEquals(a, b)` | INTEGER | ✅ | ❌ | ✅ | Same geometry *and* same vertex order, unlike the topological `ST_Equals` |
| `ST_GeoHash(geom [, maxchars])` | TEXT / NULL | ✅ | ❌ | ✅ | 20 characters by default. An extended geometry keeps only the prefix its bbox corners agree on (PostGIS's behavior); non-lon/lat input is an error |
| `ST_Extent(geom)` **aggregate** | geometry / NULL | ✅ | ✅ | ✅ | ⚠️ returns a **POLYGON**: PostGIS returns its `box2d` type, which SQLite has no equivalent for. NULL rows are skipped; an all-NULL group is NULL |
| `ST_3DExtent(geom)` **aggregate** | TEXT / NULL | ✅ | ❌ | ❌ | ⚠️ returns **text** — `BOX3D(minx miny minz,maxx maxy maxz)` — see [3D affine transforms](#3d-affine-transforms) for why, and what to use instead |

## 3D affine transforms

kenro computes in 2D, but a coordinate transform does not need a geometry
model: it needs each coordinate, once. So `ST_Affine` does not go through the
2D value every other function decodes into — it rewrites the coordinates
**in the encoding**, which means Z survives, `POLYHEDRALSURFACE` transforms,
and *placing a CityGML building into the world* works without kenro becoming
a 3D engine.

Three properties, each measured against PostGIS 3.5:

- **Z rides through the 2D form.** `ST_Affine(POINT Z (1 2 3), 2,0,0,2, 10,20)`
  is `POINT(12 24 3)` — PostGIS leaves the Z alone, and so does kenro.
  (Earlier kenro versions raised an error here instead.)
- **The 3D form cannot raise dimensionality.** On 2D input, `z` is taken as 0
  for the `x'`/`y'` rows and the `z'` row is discarded:
  `ST_Affine(POINT(1 2), 1,2,3, 4,5,6, 7,8,9, 10,20,30)` is `POINT(15 34)`,
  not a 3D point. Producing 3D from 2D is `ST_Force3D`'s and
  `ST_MakePoint(x, y, z)`'s job, and neither is implemented — they need a
  geometry model kenro does not have, where this does not.
- **M is never mistaken for Z.** ISO dimension code 2 is XYM: three
  ordinates, none of them a height.
  `ST_Affine(POINT M (1 2 99), …, zoff := 30)` is `POINTM(11 22 99)`.

The 3D matrix is the upper 3×4 of a 4×4, row-major:

```text
x' = a·x + b·y + c·z + xoff
y' = d·x + e·y + f·z + yoff
z' = g·x + h·y + i·z + zoff
```

### Which functions take this path

Every function whose whole job is to move coordinates, and no others:

| On the encoding-level path | 2D only, deliberately |
|---|---|
| `ST_Affine` (both arities) | `ST_SnapToGrid` |
| `ST_Translate`, `ST_Scale` | `ST_ReducePrecision` |
| `ST_Rotate`, `ST_RotateZ` | |
| `ST_TransScale` | |
| `ST_FlipCoordinates` | |
| `ST_ShiftLongitude`, `ST_WrapX` | |
| `ST_Transform` | |

The split is not about effort — it is what PostGIS does. Measured on 3.5, the
left column all preserve Z and (except `ST_WrapX`) all accept a surface
collection. The right column **is not coordinate-wise there**: PostGIS drops
the vertices that collapse onto each other, so
`ST_SnapToGrid(LINESTRING(0 0,0.1 0.1,1 1,1.1 1.1), 1)` is `LINESTRING(0 0,1 1)`
and a fully-collapsing polygon is `POLYGON EMPTY`. kenro only rounds. Rewriting
coordinates in place would have handed those two 3D support while deepening
that divergence, so they keep to the 2D value — and now **raise an error on 3D
input instead of silently dropping the Z**, which is what they used to do.

`ST_WrapX` is on the left column but refuses surface collections, matching
PostGIS's own "Wrapping of PolyhedralSurface geometries is unsupported".

## Derived geometries and the Z

A coordinate transform is not the only thing that can keep a height. Anything
whose output coordinates *came from* its input's — `ST_Reverse`,
`ST_ExteriorRing`, `ST_Simplify`, `ST_ConvexHull` — could carry the Z along too,
and PostGIS does. kenro now does as well, and the rule is decided **per call
from the data** rather than from a list of function names:

1. No input carried a Z → nothing changes, and nothing costs.
2. Every coordinate of the result was a vertex of some input → the result is
   written with those heights.
3. Some coordinate was **invented** — a segment midpoint, an intersection, a
   buffer arc — and there is no honest Z for it → **error**, naming
   `ST_Force2D`.

That third case is the point. PostGIS interpolates a Z there; kenro cannot, and
the alternative it used to take was to return a 2D geometry without saying so.
`UPDATE buildings SET geom = ST_AsGPB(ST_Simplify(ST_GeomFromGPB(geom), 0.1))`
would flatten a whole table in silence. It now either keeps the heights or
refuses.

Deciding from the data also gets cases a hand-written list would have got
wrong: `ST_ConvexHull`'s output vertices *are* input vertices, so its Z
survives, while `ST_Segmentize`'s midpoints cannot — and the same
`ST_Intersection` call refuses or succeeds depending on whether the operands
actually cross.

| | |
|---|---|
| **Keeps the Z** (output reuses input vertices) | `ST_StartPoint` `ST_EndPoint` `ST_PointN` `ST_GeometryN` `ST_ExteriorRing` `ST_InteriorRingN` `ST_Boundary` `ST_Multi` `ST_Normalize` `ST_Reverse` `ST_ForcePolygonCW` `ST_ForcePolygonCCW` `ST_ForceRHR` `ST_AddPoint` `ST_SetPoint` `ST_RemovePoint` `ST_MakePolygon` `ST_MakeLine` `ST_LineFromMultiPoint` `ST_Points` `ST_RemoveRepeatedPoints` `ST_Simplify` `ST_SimplifyVW` `ST_ConvexHull` `ST_ConcaveHull` `ST_DelaunayTriangles` `ST_TriangulatePolygon` `ST_LineMerge` `ST_UnaryUnion` `ST_Subdivide` `ST_Split` `ST_MakeValid` `ST_Intersection` `ST_Difference` `ST_SymDifference` `ST_Union` — *when* the operation happens not to invent a vertex |
| **Refuses on 3D input** (the Z would have to be invented) | the same overlay and interpolation functions when they do: `ST_Segmentize` `ST_ChaikinSmoothing` `ST_LineSubstring` `ST_LineInterpolatePoint` `ST_LineInterpolatePoints` `ST_LineExtend` `ST_GeometricMedian`, the `ST_Union` **aggregate**, and any crossing overlay |
| **2D on purpose** (PostGIS answers 2D too — measured) | `ST_Centroid` `ST_PointOnSurface` `ST_Envelope` `ST_OrientedEnvelope` `ST_MinimumBoundingCircle` `ST_ClosestPoint` `ST_ShortestLine` `ST_LongestLine` `ST_Buffer` `ST_ClipByBox2D` `ST_AsMVTGeom` `ST_Force2D` |
| **2D on purpose** (a bounding box, whose corners are not input vertices) | `ST_Expand` `ST_BoundingDiagonal` `ST_MakeBox2D` `ST_Extent` |

Two details worth knowing:

- **A bounding box never borrows a neighbour's Z.** `ST_BoundingDiagonal` of a
  polygon whose (10 10) corner sits at z = 3 ends at that x and y but needs the
  box's zmax, 4. Answering 3 would be confidently wrong, so every box-shaped
  result stays 2D. ⚠️ PostGIS returns 3D for `ST_Expand` and
  `ST_BoundingDiagonal`; kenro's answer is 2D there.
- **Two heights at one plan position are ambiguous.** A vertical wall gives the
  same (x, y) two Z values, and there is no way to pick. Those coordinates
  count as "no honest Z", so the function refuses rather than choosing.

`ST_Project` is the one exception that asserts a Z for a coordinate no input
occupied: sliding a point along the ground does not change its elevation, which
is what PostGIS does too.

### `ST_Transform`

Reprojection is coordinate-wise, so it takes the encoding-level path as well —
which matters more here than anywhere else, because reprojecting is the
operation a 3D city model needs most and it used to refuse 3D outright.
Measured on PostGIS 3.5:

- `ST_Transform(POINT Z (139.7 35.7 100), 32654)` moves x and y and returns
  `z = 100` untouched.
- A `POLYHEDRALSURFACE Z` comes back a `POLYHEDRALSURFACE`, patch structure and
  roof heights intact, rather than being rejected.

The Z is not merely carried past the projection: proj4rs takes `(x, y, z)` per
coordinate, so a datum shift routed through geocentric coordinates *reads* the
height, exactly as PROJ does. Same-datum pairs (`4326 → 32654` among them)
leave it exactly as it was — verified by moving the height and watching x and y
not move.

The byte-level I/O functions came along for the ride, because a reprojected
building has to be storable: `ST_SetSRID`, `ST_GeomFromGPB`, `ST_AsGPB` and
`ST_SRID` all used to validate by *decoding*, which refused surface
collections — breaking the pass-through promise for exactly the geometries it
was written for. They now walk the encoding instead, and `ST_AsGPB` builds a
surface's R-tree envelope from its patches.

### `ST_3DExtent`

⚠️ PostGIS returns its `box3d` type. SQLite has no such type, and kenro cannot
write a 3D geometry to stand in for one, so `ST_3DExtent` returns **the text
PostGIS renders a box3d as**: `BOX3D(minx miny minz,maxx maxy maxz)`. The
digits are Rust's shortest round-trip, not PostGIS's — PostGIS renders a box3d
through the server's `extra_float_digits`, so its own output is not a fixed
string either.

Nothing consumes that text yet: PostGIS's `ST_XMin`/`ST_ZMin` family accepts a
box3d, kenro's takes a geometry blob only. **For the six numbers, use SQLite's
own aggregates over kenro's scalars**, which is what a query wanting them
should do anyway:

```sql
SELECT min(ST_MinX(geom)), min(ST_MinY(geom)), min(ST_ZMin(geom)),
       max(ST_MaxX(geom)), max(ST_MaxY(geom)), max(ST_ZMax(geom))
FROM buildings;
```

A 2D row contributes Z = 0 rather than nothing, following `ST_ZMin`/`ST_ZMax`
(PostGIS answers `BOX3D(0 0 0,5 5 0)` for `LINESTRING(0 0,5 5)`). Empty
geometries contribute nothing; a zero-row or all-empty group is NULL.

## Hulls and triangulation

The two most expensive functions in the catalog by binary size — measured on
the wasm standard tier at **+41 KB** and **+81 KB**, against ~21 KB for a
whole group of ordinary functions. Each has its own feature, both are in
`full`, and a build without them registers stubs naming the feature.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_ConcaveHull(geom, target_percent)` | geometry | ✅ | ✅ | ✅ | `concave-hull` feature. Keeps **PostGIS's argument contract** — the fraction of the convex hull's area to aim for, 1.0 being the convex hull — by searching `geo`'s differently-scaled "concavity" parameter for it, at a few hull computations per call. A value outside [0,1] is an error, so pasting geo's own concavity (~2) fails loudly instead of returning a very different shape. ⚠️ The hull family differs from GEOS's, so the vertices are not PostGIS's; what holds is the contract (never exceeds the convex hull, monotone in the target) |
| `ST_DelaunayTriangles(geom)` | geometry | ✅ | ✅ | ❌ | `delaunay` feature. ⚠️ returns a **MULTIPOLYGON** where PostGIS returns a GEOMETRYCOLLECTION — kenro never produces collections. The `tolerance` and `flags` arguments are not implemented; `geo`'s triangulator has no snapping tolerance, and the edge output is `ST_Boundary` of this |
| `ST_TriangulatePolygon(geom)` | geometry | ✅ | ❌ | ❌ | `delaunay` feature. The **constrained** triangulation: the triangles tile the polygon exactly, so holes and concavities stay uncovered — a square with a 2×2 hole triangulates to area 96, where `ST_DelaunayTriangles` spans the convex hull and gives 100. ⚠️ MULTIPOLYGON, not a GEOMETRYCOLLECTION; a triangulation is not unique, so the individual triangles are not GEOS's. Non-areal input is an error rather than PostGIS's empty collection |

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
| `ST_IsSimple(geom)` | INTEGER | ✅ | ❌ | ✅ | No anomalous self-intersection: curves may close on themselves and may meet each other, but only end to end. A closed ring is simple; the same ring with a tail on its closing vertex is not. Areal input is judged **ring by ring** — a bow-tie ring fails, while two overlapping MULTIPOLYGON members are still simple (PostGIS agrees) |
| `ST_LineMerge(geom [, directed])` | geometry | ✅ | ✅ | ✅ | Sews lines together at nodes where exactly two ends meet. A Y junction keeps all three arms, and two lines crossing in their interiors are **not** merged — there is no vertex there, and this function does not create one. `directed` honours the original directions instead of reversing parts to fit. ⚠️ Non-lineal input is an error, where PostGIS answers `GEOMETRYCOLLECTION EMPTY`; and the direction and start vertex of a chain assembled from reversed parts are arbitrary, so `ST_AsText` can read backwards from PostGIS's |
| `ST_Split(input, blade)` | geometry | ✅ | ❌ | ✅ | `overlay` feature. Lineal input splits at the blade's points or crossings, areal input is sliced by a lineal blade. Holes survive the cut. A blade that misses returns the input unchanged. ⚠️ MULTILINESTRING or MULTIPOLYGON, not PostGIS's GEOMETRYCOLLECTION. Splitting a polygon by a point is an error, as in PostGIS |

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
| `ST_Polygon(line, srid)` | geometry | ✅ | ✅ | ✅ | `ST_MakePolygon` with an SRID |
| `ST_LineFromMultiPoint(multipoint)` | geometry / NULL | ✅ | ❌ | ✅ | The points in order |
| `ST_LineExtend(line, forward [, backward])` | geometry / NULL | ✅ | ❌ | ❌ | Extends along the end segments; the original vertices survive |
| `ST_PointInsideCircle(point, cx, cy, radius)` | INTEGER | ✅ | ❌ | ✅ | Planar |
| `ST_WrapX(geom, wrap, move)` | geometry | ✅ | ❌ | ❌ | ⚠️ kenro shifts **whole vertices**; PostGIS cuts segments that span the meridian. `ST_Segmentize` first if that matters. Z/M ride through; a surface collection is an error, as in PostGIS |
| `ST_MakeBox2D(low, high)` | geometry | ✅ | ❌ | ✅ | ⚠️ POLYGON, not `box2d` (as with `ST_Extent`, `ST_Expand`) |
| `ST_GeomFromGeoHash(hash [, precision])` | geometry | ✅ | ❌ | ✅ | The cell as a POLYGON, SRID 4326 |
| `ST_PointFromGeoHash(hash [, precision])` | geometry | ✅ | ❌ | ✅ | Its centre; round-trips with `ST_GeoHash` |
| `ST_GeometricMedian(geom [, tolerance])` | geometry / NULL | ✅ | ❌ | ❌ | Weiszfeld iteration over the vertices |
| `ST_LineCrossingDirection(a, b)` | INTEGER | ✅ | ❌ | ✅ | PostGIS's codes: 0 none, ±1 single, ±2 multiple same-side, ±3 multiple ending that way |
| `ST_Summary(geom)` | TEXT | ✅ | ❌ | ✅ | ⚠️ PostGIS prints a tree with byte offsets; kenro keeps the leading token (`Point[S]`) and adds its vertex count |
| `ST_MemSize(geom)` | INTEGER | ✅ | ❌ | ✅ | ⚠️ the length of the GeoPackage blob kenro would store — the number that means something for a SQLite column, not PostGIS's in-memory size |
| `ST_Normalize(geom)` | geometry | ✅ | ❌ | ❌ | Rings oriented, parts ordered by bounding box. ⚠️ PostGIS orders by its own internal comparison, so the two agree on orientation but not always on part order |

## 3D pass-through

kenro computes in 2D — `geo_types` has no room for Z, so decoding drops it
and every encoder refuses a geometry that had one rather than silently
writing 2D. These functions let a 3D column be *stored, indexed, filtered and
read* anyway, which is what a CityGML-style workflow needs even when the
analysis itself is planar.

The route in is the practical one: a GeoPackage written by GDAL or QGIS.
`ST_GeomFromGPB`, `ST_SetSRID` and `ST_AsGPB` carry the WKB payload across
byte-for-byte, so the Z survives — and so do surface collections, which those
three used to refuse because they validated by decoding. `ST_GeomFromWKB` still
re-encodes and so still flattens.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_HasZ(geom)` / `ST_HasM(geom)` | INTEGER | ✅ | ✅ | ✅ | Read from the encoding, not from kenro's (always 2D) decoded value |
| `ST_NDims(geom)` / `ST_CoordDim(geom)` | INTEGER | ✅ | ✅ | ✅ | 2, 3 or 4 — honestly. These used to answer a flat 2 |
| `ST_Z(point)` / `ST_M(point)` | REAL / NULL | ✅ | ✅ | ✅ | NULL when the vertex has no such ordinate |
| `ST_ZMin(geom)` / `ST_ZMax(geom)` | REAL / NULL | ✅ | ❌ | ✅ | ⚠️ a 2D geometry answers **0**, not NULL — PostGIS derives these from a bbox whose Z slot is zero, and `WHERE ST_ZMax(g) > 100` should behave the same on both. NULL only for an empty geometry |

Everything else stays planar on 3D input: the R-tree columns, every
predicate, every measure. Three things reach past reporting without needing a
3D geometry model, each in its own section — surface collections are
[read and measured](#surface-collections-polyhedralsurface-tin-triangle), the
[coordinate transforms](#3d-affine-transforms) rewrite Z in place, and a
[derived geometry](#derived-geometries-and-the-z) keeps the heights of the
vertices it reused. What is still *not* here is 3D geometry: no
`ST_3DDistance`, no volumes, no 3D predicates, and no way to *create* a Z that
was not already in the input.

## GML 2/3 I/O

`gml` feature (in `full`). Reading pulls `quick-xml`; writing is hand-rolled
like the WKT and GeoJSON emitters, so the pair costs **+31 KB raw / +13 KB
gzipped** measured on the full tier. SpatiaLite reaches for libxml2 here
because it validates against a schema and stores XmlBLOBs (`XB_*`); kenro
does neither, and a pull parser is all that is left.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_AsGML([version, ] geom [, maxdecimaldigits])` | TEXT | ✅ | ❌ | ✅ | Byte-identical to PostGIS for the shapes kenro supports, golden-tested — including GML 3's habit of writing a `Curve` with segments where a `LineString` would do, and `MultiCurve`/`MultiSurface` for the multis. Version defaults to 2 and precision to 15, as in PostGIS. The `options`, `nprefix` and `id` arguments are not implemented: always the `gml:` prefix, never an id |
| `ST_GeomFromGML(text [, srid])` / `ST_GMLToSQL` | geometry | ✅ | ❌ | ✅ | Structural, not schema-driven: elements are matched by **local name**, so any namespace prefix works and unknown elements are ignored — which is what lets a CityGML fragment be read without carrying the schema. `srsName` is read from either `EPSG:6697` or the `urn:ogc:def:crs:EPSG::6697` form. `srsDimension="3"` sets the coordinate stride, and the Z is dropped like everywhere else in kenro |

## KML and SVG output

Both are `text-encodings`. Neither uses an XML library — the only text that
is not a number is `ST_AsKML`'s namespace prefix, and that is validated as an
XML name rather than escaped, so a prefix that would break the document is an
error instead of a literally-named element. The feature exists because of the
first row's reprojection, not because of a parser.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_AsKML(geom [, maxdecimaldigits [, nprefix]])` | TEXT | ✅ | ❌ | ⚠️ named `AsKml` | **Reprojects to WGS84**, and errors on SRID 0 — KML is defined in lon/lat and PostGIS transforms rather than labelling, unlike `ST_AsGML` next door. The reprojection is kenro's gridless one ([accuracy](accuracy.md)). Rings keep their closing vertex; all three multi types become `MultiGeometry`; an empty geometry gives an empty string; a GeometryCollection is an error, as in PostGIS. ⚠️ 3D input is an error where PostGIS writes a third ordinate — `ST_Force2D` first |
| `ST_AsSVG(geom [, rel [, maxdecimaldigits]])` | TEXT | ✅ | ❌ | ⚠️ named `AsSvg` | A path fragment, not a document. **Y is negated** (SVG's axis points down), so `POINT(1 2)` is `cx="1" cy="-2"`. `rel = 1` switches to relative commands (`l`, `z`) **and renames the point attributes to `x`/`y`** — an undocumented PostGIS behaviour kenro matches. Rings drop their closing vertex in favour of `Z`. Relative deltas are taken on full-precision coordinates and rounded after. ⚠️ 3D input is an error |

## Surface collections: POLYHEDRALSURFACE, TIN, TRIANGLE

kenro **reads** surface collections but does not compute with them. `geo_types`
has no variant for one, and a second geometry model would mean two
representations that can disagree, so these functions walk the encoding
instead: they answer structural questions, measure patch by patch, and hand
the whole thing to the 2D world through `ST_Force2D`.

The route in is a GeoPackage written by GDAL, QGIS or a CityGML importer —
the same route as [3D pass-through](#3d-pass-through). `ST_GeomFromWKB` will
not do it, because it re-encodes.

**Everything that needs a decoded geometry refuses surface input, loudly and
in one place.** A predicate or an overlay function raises with a message naming
`ST_Force2D`; silently flattening a building into overlapping faces would be
the same class of mistake as writing 2D where 3D went in. The exception is the
other function that reads the encoding rather than decoding it —
[`ST_Affine`](#3d-affine-transforms) — which transforms a surface collection
directly.

| Function | Returns | PostGIS | DuckDB Spatial | SpatiaLite | Notes |
|---|---|---|---|---|---|
| `ST_NumPatches(geom)` | INTEGER / NULL | ✅ | ❌ | ❌ | NULL for anything that is not a surface collection |
| `ST_PatchN(geom, n)` | geometry / NULL | ✅ | ❌ | ❌ | Patch `n` as a 2D POLYGON, **1-based** like `ST_GeometryN`; Z readable via `ST_ZMin`/`ST_ZMax` |
| `ST_GeometryType` | TEXT | ✅ | ❌ | ❌ | `ST_PolyhedralSurface` / `ST_Tin` / `ST_Triangle` |
| `ST_Dimension` | INTEGER | ✅ | ❌ | ❌ | 2, as PostGIS reports |
| `ST_Area`, `ST_Perimeter` | REAL | ✅ | ❌ | ❌ | Summed patch by patch — the **planar** sum PostGIS reports, not a 3D surface area (measured) |
| `ST_NumGeometries`, `ST_IsEmpty` | INTEGER | ✅ | ❌ | ❌ | Patches count as members, so the R-tree triggers keep working |
| `ST_MinX` … `ST_MaxY`, `ST_ZMin`, `ST_ZMax`, `ST_HasZ`, `ST_NDims` | | ✅ | ❌ | ❌ | Walked from the patches; a surface column stays indexable |
| `ST_Force2D(geom)` | geometry | ✅ | ✅ | ❌ | → MULTIPOLYGON of the patches. ⚠️ a closed solid becomes **overlapping coplanar faces** — geometrically correct, visually surprising, and what PostGIS does |
| `ST_IsClosed(geom)` | INTEGER | ✅ | ❌ | ❌ | Is this a closed shell? Combinatorial, not geometric: every edge shared by exactly two patches, tested on the 3D coordinates |
| `ST_Affine(…)` | geometry | ✅ | ❌ | ❌ | Both arities transform a surface collection, patches and Z included, because they rewrite the encoding rather than decoding it — [3D affine transforms](#3d-affine-transforms) |
| `kenro_gpkg_extension_required(geom)` | TEXT / NULL | ❌ | ❌ | ❌ | **kenro-only.** See below |

`ST_GeomFromGML` also reads CityGML's surface wrappers — `gml:Solid`,
`gml:CompositeSurface`, `gml:Surface`, `gml:TriangulatedSurface`,
`gml:Triangle` — flattening them to a MULTIPOLYGON on the way in.

### The GeoPackage obligation

GeoPackage Annex F.1 makes an extended geometry type legal **only if the file
declares it**: one row in `gpkg_extensions` per (table, column).

```sql
INSERT INTO gpkg_extensions (table_name, column_name, extension_name, definition, scope)
VALUES ('buildings', 'geom', 'gpkg_geom_POLYHEDRALSURFACE',
        'http://www.geopackage.org/spec120/#extension_geometry_types', 'read-write');
```

…and `gpkg_geometry_columns.geometry_type_name` carries `POLYHEDRALSURFACE`
rather than a core type name.

kenro does not write that row. It registers functions; it does not manage
schemas — the same reason SpatiaLite's `InitSpatialMetadata` is out of scope,
and a function with a side effect could no longer be `SQLITE_DETERMINISTIC`
and `SQLITE_INNOCUOUS`, which the GeoPackage triggers depend on. What it does
instead is **name the obligation** so it is detectable rather than folklore:

```sql
SELECT kenro_gpkg_extension_required(geom) FROM buildings LIMIT 1;
-- 'gpkg_geom_POLYHEDRALSURFACE', or NULL when no extension is needed
```

The name is deliberately not `GPKG_*`: the spec reserves the `gpkg` author
prefix for OGC-adopted extension *names*, and although it says nothing about
SQL function names, a `GPKG_` function would read as one the standard defines.

## Deliberately out of scope

- **Raster** — kenro is vector-only.
- **Binary interchange formats** — no FlatGeobuf/Geobuf (`ST_AsFlatGeobuf`,
  `ST_AsGeobuf`, `ST_FromFlatGeobuf`): those are file formats, and kenro
  operates on geometry values.
- **The `geography` type** — no `ST_GeogFromText` and friends; kenro has one
  geometry type. Measurements on the ellipsoid are `ST_DistanceSphere`,
  `ST_DistanceSpheroid` and `ST_LengthSpheroid` instead.
- **`ST_QuantizeCoordinates`** — PostGIS's `prec` maps to mantissa bits by an
  internal rule we could not reproduce (its results for `prec` 2 and 3 are
  identical), and a same-named function that rounds differently is worse than
  none. `ST_ReducePrecision` and `ST_SnapToGrid` give a predictable grid.
- **PostGIS's Topology extension** — none of the ~18 `ST_AddEdge*` /
  `ST_CreateTopoGeo` / `ST_ModEdge*` family: that is a topology store, not a
  function set.
- **Topology / network analysis** — no `ST_Node`, `ST_Polygonize` or
  `ST_Snap`, and no routing (SpatiaLite's librttopo topology and
  VirtualRouting). These need a noding engine kenro does not carry.
  (`ST_Split` and `ST_LineMerge` were once on this list and are now
  implemented — see "Line structure" above; neither actually needed one.)
- **Set-returning functions** — no `ST_Dump`/`ST_DumpPoints`, no grid
  generators (`ST_SquareGrid`, `ST_HexagonGrid`), no `ST_VoronoiPolygons`:
  they would need SQLite table-valued functions, and kenro registers scalars
  and aggregates only. Where the shape allows it, kenro returns a MULTI\*
  instead (see `ST_Subdivide`).
- **Window functions** — no `ST_ClusterDBSCAN`/`ST_ClusterKMeans`.
- **Curved geometries** — no `CIRCULARSTRING`/`COMPOUNDCURVE` family, and so
  no `ST_CurveToLine`/`ST_HasArc`/`ST_LineToCurve`.
- **3D geometry operations** — no `ST_3DIntersects`/`ST_3DDistance`, no
  volumes, no SOLID type (which is SFCGAL's, not stock PostGIS's). Surface
  collections are read and measured, never computed with, and the
  [coordinate transforms](#3d-affine-transforms) move them without decoding
  them; the design note behind that split is `tmp/3d-geometry-design.md`.
- **Creating a Z** — no `ST_Force3D`, no `ST_MakePoint(x, y, z)`, and no
  interpolating one for a vertex the input did not have (see
  [derived geometries](#derived-geometries-and-the-z)). Reading, measuring,
  transforming and *carrying* a Z all work on the encoding; inventing a height
  is the line, and PostGIS's interpolation is on the other side of it.
- **Single-sided buffering** — no `ST_OffsetCurve`: `geo`'s buffer has no
  side option, which is also why `ST_Buffer` rejects `side=`.
- **Record-returning functions** — no `ST_IsValidDetail`,
  `ST_MaximumInscribedCircle`: SQLite has no record type. Where only one
  field is wanted, kenro exposes it (`ST_MinimumBoundingRadius`,
  `ST_IsValidReason`).
- **Clustering aggregates** — no `ST_ClusterWithin`/`ST_ClusterIntersecting`:
  they return an array of geometry collections, and kenro has neither.
- **File-format conversion** — kenro operates on geometry *values*
  (WKT/WKB/GeoJSON/GeoPackage blobs), not files; reading shapefiles,
  spreadsheets or writing whole GeoPackages is GDAL/ogr2ogr territory
  (DuckDB's `ST_Read`, SpatiaLite's VirtualShape/VirtualXL).
- **XML machinery beyond geometry** — no XmlBLOB (`XB_*`), no SLD/SE
  styling, no WFS, and no schema validation (what libxml2 buys SpatiaLite).
  kenro encodes and decodes *geometry* as text — GML, KML and SVG are all
  supported; the surrounding document machinery is not. No X3D output.
- **SQLite virtual tables** — no VirtualKNN-style modules; kenro registers
  scalar and aggregate functions only, and spatial indexing goes through
  the standard GeoPackage R-tree instead of a custom index.
- **SpatiaLite's metadata administration** — no `InitSpatialMetadata`
  schema; kenro is GeoPackage-native (`gpkg_contents`,
  `gpkg_geometry_columns`, the R-tree and type triggers).
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
  raise an error rather than silently dropping Z/M. The ordinates are
  readable and reportable — see [3D pass-through](#3d-pass-through) — and
  `ST_Force2D` is the explicit way to flatten.
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
