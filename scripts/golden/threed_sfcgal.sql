-- Golden vector generator for the SFCGAL 3D measurement pair: ST_3DArea and
-- ST_Volume. Run via scripts/golden/generate.sh (which loads
-- `postgis_sfcgal` before this suite); output is tests/golden/threed_sfcgal.jsonl,
-- committed — CI never runs this.
--
-- Why a suite of its own, when the image is the same one every other suite
-- uses: these two functions live in the `postgis_sfcgal` extension, which
-- `CREATE EXTENSION postgis` alone does not install. The image
-- (postgis/postgis:17-3.5) has shipped it all along — SFCGAL 1.3.8 over
-- CGAL — the generator simply never asked for it. Enabling it was measured to
-- leave all eight pre-existing suites byte-identical, so the other vectors do
-- not move.
--
-- ⚠️ The measurement this suite exists for. SFCGAL splits the two functions
-- across two type families, and the split is what decides which names kenro
-- may wear:
--
--   ST_3DArea(POLYHEDRALSURFACE) -> the summed face area
--   ST_3DArea(SOLID)             -> 0
--   ST_Volume(POLYHEDRALSURFACE) -> 0            <- a surface encloses nothing
--   ST_Volume(SOLID)             -> the volume, SIGNED by shell orientation
--
-- kenro has no SOLID type (docs/scope.md), so a kenro `ST_Volume` returning
-- the enclosed volume of a closed POLYHEDRALSURFACE would answer a number
-- where the reference answers 0 — a silently different result under a shared
-- name, which kenro's naming rule forbids. Hence `kenro_volume` instead; the
-- `volume_solid` vectors below record exactly what the reference does with
-- ST_MakeSolid so the divergence is on the record rather than in a memory.
--
-- The box is 3.3 x 1.7 x 3.6 at an irregular origin: area 47.22, volume
-- 20.196, both derivable by hand.
\pset tuples_only on
\pset format unaligned

-- Closed axis-aligned box, outward-oriented, in three encodings plus two
-- damaged variants. `open` drops the bottom face; `flip` reverses one face's
-- ring; `rev` reverses every face (still consistent, but inward).
WITH shapes(id, a) AS (VALUES
  ('box_ps',
   'POLYHEDRALSURFACE Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))'),
  ('box_tin',
   'TIN Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,0.4 1.2 0.5)),((0.4 1.2 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 0.5)),((0.4 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 0.5)),((3.7 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))'),
  ('box_rev',
   'POLYHEDRALSURFACE Z(((3.7 1.2 0.5,3.7 2.9 0.5,0.4 2.9 0.5,0.4 1.2 0.5,3.7 1.2 0.5)),((0.4 2.9 4.1,3.7 2.9 4.1,3.7 1.2 4.1,0.4 1.2 4.1,0.4 2.9 4.1)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 1.2 0.5,0.4 1.2 0.5,0.4 1.2 4.1)),((3.7 1.2 4.1,3.7 2.9 4.1,3.7 2.9 0.5,3.7 1.2 0.5,3.7 1.2 4.1)),((3.7 2.9 4.1,0.4 2.9 4.1,0.4 2.9 0.5,3.7 2.9 0.5,3.7 2.9 4.1)),((0.4 2.9 4.1,0.4 1.2 4.1,0.4 1.2 0.5,0.4 2.9 0.5,0.4 2.9 4.1)))'),
  ('box_open',
   'POLYHEDRALSURFACE Z(((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))'),
  -- A tetrahedron with no round coordinates and no axis-aligned face, so the
  -- fan triangulation and the divergence-theorem sum are both exercised
  -- properly. `tet` is inward-oriented, `tet_out` is its reverse.
  ('tet',
   'POLYHEDRALSURFACE Z(((0.3 0.7 1.1,4.2 0.9 1.3,1.4 3.6 2.7,0.3 0.7 1.1)),((0.3 0.7 1.1,1.4 3.6 2.7,2.1 1.5 6.4,0.3 0.7 1.1)),((0.3 0.7 1.1,2.1 1.5 6.4,4.2 0.9 1.3,0.3 0.7 1.1)),((4.2 0.9 1.3,2.1 1.5 6.4,1.4 3.6 2.7,4.2 0.9 1.3)))'),
  ('tet_out',
   'POLYHEDRALSURFACE Z(((0.3 0.7 1.1,1.4 3.6 2.7,4.2 0.9 1.3,0.3 0.7 1.1)),((0.3 0.7 1.1,2.1 1.5 6.4,1.4 3.6 2.7,0.3 0.7 1.1)),((0.3 0.7 1.1,4.2 0.9 1.3,2.1 1.5 6.4,0.3 0.7 1.1)),((4.2 0.9 1.3,1.4 3.6 2.7,2.1 1.5 6.4,4.2 0.9 1.3)))')
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':3darea' AS id, a, '3darea' AS "fn",
         to_jsonb(ST_3DArea(ST_GeomFromText(a))) AS expected
  FROM shapes ORDER BY id
) t;

-- The landmine, spelled out: every one of these is a closed or nearly-closed
-- shell, and ST_Volume answers 0 for all of them because none is a SOLID.
WITH shapes(id, a) AS (VALUES
  ('box_ps',
   'POLYHEDRALSURFACE Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))'),
  ('box_tin',
   'TIN Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,0.4 1.2 0.5)),((0.4 1.2 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 0.5)),((0.4 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 0.5)),((3.7 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))'),
  ('box_rev',
   'POLYHEDRALSURFACE Z(((3.7 1.2 0.5,3.7 2.9 0.5,0.4 2.9 0.5,0.4 1.2 0.5,3.7 1.2 0.5)),((0.4 2.9 4.1,3.7 2.9 4.1,3.7 1.2 4.1,0.4 1.2 4.1,0.4 2.9 4.1)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 1.2 0.5,0.4 1.2 0.5,0.4 1.2 4.1)),((3.7 1.2 4.1,3.7 2.9 4.1,3.7 2.9 0.5,3.7 1.2 0.5,3.7 1.2 4.1)),((3.7 2.9 4.1,0.4 2.9 4.1,0.4 2.9 0.5,3.7 2.9 0.5,3.7 2.9 4.1)),((0.4 2.9 4.1,0.4 1.2 4.1,0.4 1.2 0.5,0.4 2.9 0.5,0.4 2.9 4.1)))'),
  ('box_open',
   'POLYHEDRALSURFACE Z(((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))'),
  ('tet',
   'POLYHEDRALSURFACE Z(((0.3 0.7 1.1,4.2 0.9 1.3,1.4 3.6 2.7,0.3 0.7 1.1)),((0.3 0.7 1.1,1.4 3.6 2.7,2.1 1.5 6.4,0.3 0.7 1.1)),((0.3 0.7 1.1,2.1 1.5 6.4,4.2 0.9 1.3,0.3 0.7 1.1)),((4.2 0.9 1.3,2.1 1.5 6.4,1.4 3.6 2.7,4.2 0.9 1.3)))'),
  ('tet_out',
   'POLYHEDRALSURFACE Z(((0.3 0.7 1.1,1.4 3.6 2.7,4.2 0.9 1.3,0.3 0.7 1.1)),((0.3 0.7 1.1,2.1 1.5 6.4,1.4 3.6 2.7,0.3 0.7 1.1)),((0.3 0.7 1.1,4.2 0.9 1.3,2.1 1.5 6.4,0.3 0.7 1.1)),((4.2 0.9 1.3,1.4 3.6 2.7,2.1 1.5 6.4,4.2 0.9 1.3)))')
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':volume' AS id, a, 'volume' AS "fn",
         to_jsonb(ST_Volume(ST_GeomFromText(a))) AS expected
  FROM shapes ORDER BY id
) t;

-- What the reference does once the same coordinates are wrapped as a SOLID.
-- kenro has no SOLID type, so nothing here is implementable under a PostGIS
-- name; the vectors exist to document the reference and to pin `kenro_volume`,
-- which reproduces this column for the encodings kenro does have.
--
-- ⚠️ `box_tin` is the one row `kenro_volume` does NOT reproduce: ST_MakeSolid
-- on a TIN yields a SOLID whose shell is still a TIN, and SFCGAL's volume()
-- answers 0 for it rather than 20.196. Same coordinates, same closed box, and
-- the POLYHEDRALSURFACE spelling answers 20.196 — so the 0 is a property of
-- ST_MakeSolid, not of the geometry. Recorded, with kenro's answer beside it.
WITH shapes(id, a, kx, note) AS (VALUES
  ('box_ps',
   'POLYHEDRALSURFACE Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))',
   NULL::jsonb, NULL),
  ('box_tin',
   'TIN Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,0.4 1.2 0.5)),((0.4 1.2 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 0.5)),((0.4 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 0.5)),((3.7 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))',
   '20.196'::jsonb,
   'ST_MakeSolid on a TIN leaves the shell a TIN and SFCGAL volume() answers 0; the identical box as POLYHEDRALSURFACE answers 20.196. kenro_volume reads the closed shell either way.'),
  ('box_rev',
   'POLYHEDRALSURFACE Z(((3.7 1.2 0.5,3.7 2.9 0.5,0.4 2.9 0.5,0.4 1.2 0.5,3.7 1.2 0.5)),((0.4 2.9 4.1,3.7 2.9 4.1,3.7 1.2 4.1,0.4 1.2 4.1,0.4 2.9 4.1)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 1.2 0.5,0.4 1.2 0.5,0.4 1.2 4.1)),((3.7 1.2 4.1,3.7 2.9 4.1,3.7 2.9 0.5,3.7 1.2 0.5,3.7 1.2 4.1)),((3.7 2.9 4.1,0.4 2.9 4.1,0.4 2.9 0.5,3.7 2.9 0.5,3.7 2.9 4.1)),((0.4 2.9 4.1,0.4 1.2 4.1,0.4 1.2 0.5,0.4 2.9 0.5,0.4 2.9 4.1)))',
   NULL::jsonb, NULL),
  ('tet',
   'POLYHEDRALSURFACE Z(((0.3 0.7 1.1,4.2 0.9 1.3,1.4 3.6 2.7,0.3 0.7 1.1)),((0.3 0.7 1.1,1.4 3.6 2.7,2.1 1.5 6.4,0.3 0.7 1.1)),((0.3 0.7 1.1,2.1 1.5 6.4,4.2 0.9 1.3,0.3 0.7 1.1)),((4.2 0.9 1.3,2.1 1.5 6.4,1.4 3.6 2.7,4.2 0.9 1.3)))',
   NULL::jsonb, NULL),
  ('tet_out',
   'POLYHEDRALSURFACE Z(((0.3 0.7 1.1,1.4 3.6 2.7,4.2 0.9 1.3,0.3 0.7 1.1)),((0.3 0.7 1.1,2.1 1.5 6.4,1.4 3.6 2.7,0.3 0.7 1.1)),((0.3 0.7 1.1,4.2 0.9 1.3,2.1 1.5 6.4,0.3 0.7 1.1)),((4.2 0.9 1.3,1.4 3.6 2.7,2.1 1.5 6.4,4.2 0.9 1.3)))',
   NULL::jsonb, NULL)
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':volume_solid' AS id, a, 'volume_solid' AS "fn",
         to_jsonb(ST_Volume(ST_MakeSolid(ST_GeomFromText(a)))) AS expected,
         kx AS kenro_expected, note
  FROM shapes ORDER BY id
) t;

-- ST_3DArea over the flat and the near-degenerate. A 2D polygon and the same
-- polygon lifted to a constant Z give the same number, which is the answer to
-- "does ST_3DArea fall back to ST_Area when there is no relief" — it does.
-- The tilted one does not: 3 x 2.6 in plan, 5 x 2.6 = 13 on the slope.
WITH flat(id, a) AS (VALUES
  ('poly_2d',      'POLYGON((0 0,3.4 0,3.4 2.6,0 2.6,0 0))'),
  ('poly_flat_z',  'POLYGON Z((0 0 7.2,3.4 0 7.2,3.4 2.6 7.2,0 2.6 7.2,0 0 7.2))'),
  ('poly_tilted',  'POLYGON Z((0 0 0,3 0 4,3 2.6 4,0 2.6 0,0 0 0))'),
  ('poly_hole',    'POLYGON Z((0 0 0,4 0 0,4 4 0,0 4 0,0 0 0),(1 1 0,1 2 0,2 2 0,2 1 0,1 1 0))'),
  ('triangle',     'TRIANGLE Z((0 0 0,4 0 0,0 3 0,0 0 0))'),
  ('mpoly_z',      'MULTIPOLYGON Z(((0 0 0,3 0 0,3 2 0,0 2 0,0 0 0)),((0 0 5,1 0 5,1 1 5,0 1 5,0 0 5)))'),
  ('tin_single',   'TIN Z(((0 0 0,4 0 0,0 3 0,0 0 0)))'),
  ('point_z',      'POINT Z(1.3 2.7 3.9)'),
  ('line_z',       'LINESTRING Z(0 0 0,1.4 1.9 2.3)'),
  ('poly_empty',   'POLYGON EMPTY')
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':3darea' AS id, a, '3darea' AS "fn",
         to_jsonb(ST_3DArea(ST_GeomFromText(a))) AS expected
  FROM flat ORDER BY id
) t;

SELECT row_to_json(t)::text FROM (
  SELECT id || ':volume' AS id, a, 'volume' AS "fn",
         to_jsonb(ST_Volume(ST_GeomFromText(a))) AS expected
  FROM (VALUES
    ('poly_2d',    'POLYGON((0 0,3.4 0,3.4 2.6,0 2.6,0 0))'),
    ('point_z',    'POINT Z(1.3 2.7 3.9)'),
    ('poly_empty', 'POLYGON EMPTY')
  ) AS v(id, a) ORDER BY id
) t;

-- The error rows, transcribed rather than executed: psql aborts the whole
-- statement on an ERROR, so these carry the message measured by hand against
-- this exact image (SFCGAL 1.3.8). Each was run as
-- `SELECT ST_3DArea(ST_GeomFromText('…'))` and friends.
--
-- Two families:
--   * inconsistent ring orientation — SFCGAL validates the surface before
--     measuring it, and refuses. kenro's ST_3DArea does not validate: a face's
--     area does not depend on which way its ring runs, so it answers 47.22.
--     That is a divergence of strictness, never of value — there is no input
--     where both produce a number and the numbers differ.
--   * a degenerate ring (three collinear vertices) — "Polygon is invalid".
--     kenro answers 0, the area such a ring actually has.
SELECT row_to_json(t)::text FROM (VALUES
  ('box_flip:3darea',
   'POLYHEDRALSURFACE Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 4.1,0.4 1.2 4.1,0.4 1.2 0.5,0.4 2.9 0.5,0.4 2.9 4.1)))',
   '3darea', jsonb_build_object('error', true), '47.22'::jsonb,
   'SFCGAL: ERROR "inconsistent orientation of PolyhedralSurface detected at edge 3 (1-7) of polygon 5". kenro sums face areas without validating orientation; 47.22 is the same box as box_ps.'),
  ('box_flip:volume',
   'POLYHEDRALSURFACE Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 4.1,0.4 1.2 4.1,0.4 1.2 0.5,0.4 2.9 0.5,0.4 2.9 4.1)))',
   'volume', jsonb_build_object('error', true), jsonb_build_object('error', true),
   'SFCGAL refuses the inconsistent orientation before reaching the SOLID-vs-surface question. kenro_volume refuses it too, and says so: the signed sum would be a number with no meaning.'),
  ('box_flip:volume_solid',
   'POLYHEDRALSURFACE Z(((0.4 1.2 0.5,0.4 2.9 0.5,3.7 2.9 0.5,3.7 1.2 0.5,0.4 1.2 0.5)),((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 4.1,0.4 1.2 4.1,0.4 1.2 0.5,0.4 2.9 0.5,0.4 2.9 4.1)))',
   'volume_solid', jsonb_build_object('error', true), jsonb_build_object('error', true),
   'SFCGAL: ERROR "Solid is invalid : PolyhedralSurface (shell) 0 is invalid: not connected".'),
  ('box_open:volume_solid',
   'POLYHEDRALSURFACE Z(((0.4 1.2 4.1,3.7 1.2 4.1,3.7 2.9 4.1,0.4 2.9 4.1,0.4 1.2 4.1)),((0.4 1.2 0.5,3.7 1.2 0.5,3.7 1.2 4.1,0.4 1.2 4.1,0.4 1.2 0.5)),((3.7 1.2 0.5,3.7 2.9 0.5,3.7 2.9 4.1,3.7 1.2 4.1,3.7 1.2 0.5)),((3.7 2.9 0.5,0.4 2.9 0.5,0.4 2.9 4.1,3.7 2.9 4.1,3.7 2.9 0.5)),((0.4 2.9 0.5,0.4 1.2 0.5,0.4 1.2 4.1,0.4 2.9 4.1,0.4 2.9 0.5)))',
   'volume_solid', jsonb_build_object('error', true), jsonb_build_object('error', true),
   'SFCGAL: ERROR "Solid is invalid : PolyhedralSurface (shell) 0 is not closed". kenro_volume refuses an open shell with the same reason, naming ST_IsClosed.'),
  ('poly_degenerate:3darea',
   'POLYGON Z((0 0 0,1 1 1,2 2 2,0 0 0))',
   '3darea', jsonb_build_object('error', true), '0'::jsonb,
   'SFCGAL: ERROR "Polygon is invalid". kenro measures the ring it was given; three collinear vertices enclose no area, so 0.')
) AS t(id, a, "fn", expected, kenro_expected, note);
