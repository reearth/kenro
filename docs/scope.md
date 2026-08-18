# Scope and semantics

> **Related:** [Function reference](functions.md) · [3D geometry](3d.md) ·
> [Routing](routing.md) · [Quickstart](quickstart.md) ·
> [Transform accuracy](accuracy.md) · [WebAssembly hosts](wasm.md)

What kenro deliberately does not do and what to reach for instead, the practical
way around the biggest of those omissions, and the rule that settles every
question of behaviour.

| | |
|---|---|
| [Deliberately out of scope](#deliberately-out-of-scope) | the omissions, each with its reason and a pointer to what does the job |
| [Getting N rows out](#getting-n-rows-out) | kenro has no table-valued functions; two measured recipes turn a MULTI\* into rows, one exact and quadratic, one fast and lossy |
| [PostGIS is the reference](#semantics-postgis-is-the-reference) | how divergences are decided, tested and written down |

---

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
- **Noding** — no `ST_Node`, `ST_Polygonize` or `ST_Snap` (SpatiaLite's
  librttopo topology). These need a noding engine kenro does not carry: an
  algorithm that finds every crossing in a line collection and splits the
  lines there. (`ST_Split` and `ST_LineMerge` were once on this list and are
  now implemented — see "Line structure" above; neither actually needed one.)
  Routing was on this list too and is now implemented — `kenro_dijkstra`,
  `kenro_dijkstra_cost` and `kenro_drivingdistance`, golden-tested against
  pgRouting, see [Routing](routing.md). Shortest paths over an edge table
  need no noding engine; building that edge table from crossing lines still
  does.
- **Set-returning functions** — no `ST_Dump`/`ST_DumpPoints`/`ST_DumpRings`,
  and no grid generators (`ST_SquareGrid`, `ST_HexagonGrid`). kenro registers
  scalars and aggregates, not table-valued functions. Two things this does
  *not* mean, both worth stating because the earlier wording implied
  otherwise:
  - It does not mean the results are unreachable — see
    [Getting N rows out](#getting-n-rows-out) below.
  - It no longer covers the grid generators or `ST_VoronoiPolygons`, which
    were on this list by mistake. PostGIS's Voronoi functions return one
    geometry rather than a set, and the grids are the same MULTI\*
    accommodation `ST_Subdivide` already makes. All four are implemented.
- **Window functions** — no `ST_ClusterDBSCAN`/`ST_ClusterKMeans`.
- **Curved geometries** — no `CIRCULARSTRING`/`COMPOUNDCURVE` family, and so
  no `ST_CurveToLine`/`ST_HasArc`/`ST_LineToCurve`.
- **The SFCGAL solid-modelling family** — no `ST_3DIntersection`, `ST_3DUnion`,
  `ST_3DDifference`, `ST_Extrude`, `ST_Tesselate`, `ST_MakeSolid`, `ST_IsSolid`,
  `ST_3DConvexHull`, `ST_ApproximateMedialAxis`, `ST_MinkowskiSum` or
  `ST_StraightSkeleton`, and no SOLID type. (`ST_3DArea` and an enclosed volume
  were once on this list and are now implemented — see [3D area and enclosed
  volume](3d.md#3d-area-and-enclosed-volume); those two are theorems rather
  than library conventions, which is exactly why they could graduate and the
  rest cannot.) **The reference is no longer what blocks them**: the same image
  kenro's golden vectors have always come from ships `postgis_sfcgal`
  (SFCGAL 1.3.8), `scripts/golden/generate.sh` now loads it, and
  `tests/golden/threed_sfcgal.jsonl` holds the vectors — measured to leave every
  pre-existing suite byte-identical. What blocks the rest is the arithmetic:
  CGAL-grade exact predicates, robust construction and a topology model, in pure
  Rust. That is a different project from a SQLite extension, and having a
  reference to check it against does not make it smaller.
  The **3D metric family that core PostGIS does
  have** is implemented: see [3D distance and predicates](3d.md#3d-distance-and-predicates).
- **Guessing a Z** — extrapolating one past the end of a line, or averaging two
  that disagree (see [derived geometries](3d.md#derived-geometries-and-the-z)).
  Reading, measuring, transforming, carrying, *interpolating between two known
  heights* and *setting* one all work on the encoding. The line is where there
  is no single honest answer: `ST_LineExtend` past the end, an overlay crossing
  between two surfaces, a vertical wall's two heights at one plan position.
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

---

## Getting N rows out

kenro has no table-valued functions, so a MULTI\* is how a many-part result
arrives. Two ways to turn one into rows. Both work on every host — the second
needs only JSON1, which is in every SQLite build measured (3.49 and up) —
and the difference between them is large enough to matter.

**The exact one, and it is quadratic.** `ST_GeometryN` over a recursive CTE:

```sql
WITH RECURSIVE g(b) AS (SELECT ST_Subdivide(geom, 64) FROM parks WHERE id = 1),
     i(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM i, g WHERE n < ST_NumGeometries(b))
SELECT n, ST_GeometryN(g.b, i.n) FROM i, g;
```

Bit-exact, and no new functions. But `ST_GeometryN` re-decodes the whole blob
on every call, so an n-part geometry costs n decodes: **measured at 0.4 s for
1,600 parts and 104 s for 25,600**, against 11 ms for a single call that
touches every part once. Fine for tens of parts.

**The fast one, and it is lossy.** One `ST_AsGeoJSON` decodes once; `json_each`
splits the coordinate array; each part is rebuilt:

```sql
SELECT j.key + 1 AS path,
       ST_GeomFromGeoJSON(json_object('type', 'Polygon', 'coordinates', j.value))
FROM   parks p, json_each(json_extract(ST_AsGeoJSON(p.geom), '$.coordinates')) j;
```

Measured on the same geometries: **17 ms at 1,600 parts, 105 ms at 25,600** —
about a thousand times faster at the top end, and linear rather than
quadratic. Use `'LineString'` for a MULTILINESTRING and `'Point'` for a
MULTIPOINT.

⚠️ **It does not round-trip exactly.** GeoJSON is text, and
`ST_AsGeoJSON`'s precision caps at 15 decimals: the default 9 turns
`139.76770019531247` into `139.767700195` and `1e-12` into `0`, and even at
15 a coordinate of `1.2000000000000002` comes back `1.2`. A full `f64` needs
17 significant digits. So this is the right recipe for rendering, tiling and
counting, and the wrong one for anything that must preserve the input bit for
bit. (`ST_AsText`, by contrast, *is* round-trip exact — but WKT does not
split with `json_each`.)

Neither route carries the `path[]` of PostGIS's `geometry_dump`, and neither
reaches inside a nested collection.

---

## Semantics: PostGIS is the reference

Function names, signatures, and semantics follow PostGIS (SQL/MM `ST_`
prefix). Results are validated against PostGIS-generated golden vectors
committed in this repo (`tests/golden/*.jsonl` — 700+ vectors across twelve
suites: predicates, transforms, GeoJSON, accessors, processing, overlay,
buffer, 3D, 3D-SFCGAL, H3, MVT and routing. H3 vectors come from the reference
C library, MVT tiles are cross-decoded by two independent decoders, the
3D-SFCGAL suite's reference is the `postgis_sfcgal` extension of the same
image, and the routing suite's reference is pgRouting rather than PostGIS —
see [Routing](routing.md)).
Where kenro deviates, it
does so **loudly and documentedly** — never a silently different result.
The cross-cutting divergences:

- **`POINT EMPTY`** cannot be constructed from WKT/GeoJSON (the underlying
  geometry model cannot represent it) — reading one from a GeoPackage/WKB
  blob works, and `ST_AsText` prints `POINT EMPTY` like PostGIS.
- **3D/M geometries** are accepted as *input* to predicates and R-tree
  functions (2D result, same as PostGIS); output and constructor functions
  raise an error rather than silently dropping Z/M. The ordinates are
  readable and reportable — see [3D pass-through](3d.md#3d-pass-through) — and
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

---
