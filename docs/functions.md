# Function reference

Every SQL function kenro registers, with its support status in PostGIS,
DuckDB Spatial and SpatiaLite for comparison (columns verified against
PostGIS 3.5, a live DuckDB 1.4.0 + spatial session, and a live
mod_spatialite 5.1 session, July–August 2026). ✅ = present with the same
name and compatible semantics; deviations are spelled out.

Functions marked with the `overlay` or `spheroid` feature need a `full`
build (default builds register them as stubs naming the feature); everything
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
| `ST_Transform(geom, to_srid)` | geometry | ✅ 4 overloads, full PROJ | ⚠️ `(geom, source_crs, target_crs [, always_xy])` | ✅ | kenro: PostGIS-exact 2-arg form, source = embedded SRID, curated EPSG table (see [Supported CRS](../README.md#supported-crs), [accuracy](accuracy.md)). DuckDB must be told the source CRS on every call |
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
| `ST_Rotate(geom, radians [, x0, y0])` | geometry | ✅ | ✅ | ⚠️ named `RotateCoords` | About the origin (or the given point) — PostGIS semantics, **not** geo's centroid default |
| `ST_Translate(geom, dx, dy)` | geometry | ✅ | ✅ | ✅ | |
| `ST_Scale(geom, xf, yf)` | geometry | ✅ | ✅ | ⚠️ named `ScaleCoords` | About the origin, like PostGIS |
| **GeoPackage triggers** | | | | | |
| `ST_MinX` / `ST_MaxX` / `ST_MinY` / `ST_MaxY` | REAL | ⚠️ named `ST_XMin` … | ⚠️ named `ST_XMin` … | ✅ | kenro uses the GeoPackage spec's trigger names (Annex F.3) — required verbatim for gpkg index maintenance; the other two spell it `ST_XMin` |
| `ST_IsEmpty(geom)` | 0/1 | ✅ | ✅ | ✅ | gpkg R-tree contract; NULL on NULL |
| `GPKG_IsAssignable(expected, actual)` | 0/1 | ❌ | ❌ | ✅ | kenro-only: the geometry-type-trigger helper (Annex F.4); accepts both `'POINT'` and `'ST_Point'` spellings so the spec DDL works with kenro's `ST_GeometryType` |
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
| `ST_XMin` / `ST_XMax` / `ST_YMin` / `ST_YMax` | `ST_MinX` / `ST_MaxX` / `ST_MinY` / `ST_MaxY` | kenro's primary names are the GeoPackage trigger spellings (Annex F.3), required verbatim for index maintenance |
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
| `ST_PointFromWKB` / `ST_LineFromWKB` / `ST_PolyFromWKB` `(bytes [, srid])` | geometry / NULL | ✅ | ❌ | ✅ | Same contract over WKB/EWKB/GeoPackage input |

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
| `ST_SnapToGrid(geom, size)` / `(geom, sizex, sizey)` | geometry | ✅ | ❌ | ✅ | Grid anchored at the origin; size 0 leaves that axis alone. The origin-offset arities are not implemented |
| `ST_FlipCoordinates(geom)` | geometry | ✅ | ✅ | ✅ | The lat/lon-order fix |
| `ST_ShiftLongitude(geom)` | geometry | ✅ | ❌ | ✅ | x from [-180,180) into [0,360) |
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

## Deliberately out of scope

- **Raster** — kenro is vector-only.
- **Topology / network analysis** — no `ST_Node`/`ST_Polygonize`, no
  routing (SpatiaLite's librttopo topology and VirtualRouting).
- **File-format conversion** — kenro operates on geometry *values*
  (WKT/WKB/GeoJSON/GeoPackage blobs), not files; reading shapefiles,
  spreadsheets or writing whole GeoPackages is GDAL/ogr2ogr territory
  (DuckDB's `ST_Read`, SpatiaLite's VirtualShape/VirtualXL).
- **Other text encodings** — no GML/KML/SVG output (SpatiaLite's
  `AsGml`/`AsKml`/`AsSvg`) and no XML machinery (XB_*, SLD/SE styling,
  WFS); kenro speaks WKT, WKB, GeoJSON, GeoPackage blobs and MVT.
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
