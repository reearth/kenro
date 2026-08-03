-- Golden vector generator for the core-PostGIS 3D metric family. Run via
-- scripts/golden/generate.sh; output is appended to tests/golden/threed.jsonl
-- (committed — CI never runs this).
--
-- Scope note: only the nine functions core PostGIS has *without* SFCGAL. The
-- solid-modelling family (ST_Volume, ST_3DIntersection, ST_Extrude, …) is not
-- in this image at all — `postgis_full_version()` lists no SFCGAL — so there is
-- nothing here to generate vectors from, and kenro does not implement it. See
-- tmp/3d-predicates.md.
--
-- Every operand carries a Z. A Z-less operand means "any height" in PostGIS,
-- which is the same answer its 2D functions give, so those cases are covered by
-- the 2D suites rather than duplicated here.
\pset tuples_only on
\pset format unaligned

WITH pairs(id, a, b) AS (VALUES
  -- point / point
  ('pt_pt_diag',        'POINT Z (0 0 0)', 'POINT Z (1 1 1)'),
  ('pt_pt_same',        'POINT Z (3 4 5)', 'POINT Z (3 4 5)'),
  ('pt_pt_vertical',    'POINT Z (0 0 0)', 'POINT Z (0 0 9)'),
  -- point / line
  ('pt_line_above',     'POINT Z (0 0 10)', 'LINESTRING Z (0 0 0,10 0 0)'),
  ('pt_line_mid_above', 'POINT Z (5 0 9)',  'LINESTRING Z (0 0 0,10 0 0)'),
  ('pt_line_past_end',  'POINT Z (20 0 0)', 'LINESTRING Z (0 0 0,10 0 0)'),
  ('pt_line_on',        'POINT Z (5 0 0)',  'LINESTRING Z (0 0 0,10 0 0)'),
  -- line / line: the case 2D cannot tell apart
  ('line_line_skew',    'LINESTRING Z (0 0 0,10 0 0)', 'LINESTRING Z (5 -5 4,5 5 4)'),
  ('line_line_touch',   'LINESTRING Z (0 0 0,10 0 0)', 'LINESTRING Z (5 -5 0,5 5 0)'),
  ('line_line_parallel','LINESTRING Z (0 0 0,10 0 0)', 'LINESTRING Z (0 0 3,10 0 3)'),
  ('line_line_collin',  'LINESTRING Z (0 0 0,10 0 0)', 'LINESTRING Z (5 0 0,15 0 0)'),
  -- point / face: a filled face, so a point above the interior is its height
  ('pt_face_above',     'POINT Z (5 5 10)', 'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))'),
  ('pt_face_outside',   'POINT Z (20 20 0)','POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))'),
  -- An interior coplanar point, away from the face's exact centre. The centre
  -- itself is a reference defect and is covered separately below.
  ('pt_face_on',        'POINT Z (4 6 0)',  'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))'),
  ('pt_face_edge',      'POINT Z (10 5 0)', 'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))'),
  -- a non-planar ring: PostGIS triangulates rather than flattening to a plane
  ('pt_face_nonplanar', 'POINT Z (5 5 100)','POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 10,0 0 0))'),
  -- a vertical wall: two vertices share an (x, y)
  ('pt_wall',           'POINT Z (0 5 5)',  'POLYGON Z ((0 0 0,0 0 10,10 0 10,10 0 0,0 0 0))'),
  -- face / face
  ('face_face_stacked', 'POLYGON Z ((0 0 0,1 0 0,1 1 0,0 0 0))', 'POLYGON Z ((0 0 5,1 0 5,1 1 5,0 0 5))'),
  ('face_face_coplanar','POLYGON Z ((0 0 5,10 0 5,10 10 5,0 10 5,0 0 5))', 'POLYGON Z ((5 5 5,15 5 5,15 15 5,5 15 5,5 5 5))'),
  ('face_face_nested',  'POLYGON Z ((0 0 5,10 0 5,10 10 5,0 10 5,0 0 5))', 'POLYGON Z ((2 2 5,4 2 5,4 4 5,2 4 5,2 2 5))'),
  -- line / face
  ('line_thru_face',    'LINESTRING Z (5 5 -5,5 5 5)', 'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))'),
  ('line_over_face',    'LINESTRING Z (5 5 7,6 6 7)',  'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))'),
  -- surface collections: the CityGML shapes, which core PostGIS supports
  ('phs_pt',            'POLYHEDRALSURFACE Z(((0 0 0,0 1 0,1 1 0,1 0 0,0 0 0)))', 'POINT Z (0.5 0.5 7)'),
  ('tin_pt',            'TIN Z(((0 0 0,1 0 0,0 1 0,0 0 0)))', 'POINT Z (0 0 5)'),
  ('tri_pt',            'TRIANGLE Z((0 0 0,1 0 0,0 1 0,0 0 0))', 'POINT Z (0 0 5)'),
  -- a closed shell: the interior is NOT inside anything (measured)
  ('cube_centre',
   'POLYHEDRALSURFACE Z(((0 0 0,0 1 0,1 1 0,1 0 0,0 0 0)),((0 0 1,1 0 1,1 1 1,0 1 1,0 0 1)),((0 0 0,1 0 0,1 0 1,0 0 1,0 0 0)),((1 0 0,1 1 0,1 1 1,1 0 1,1 0 0)),((1 1 0,0 1 0,0 1 1,1 1 1,1 1 0)),((0 1 0,0 0 0,0 0 1,0 1 1,0 1 0)))',
   'POINT Z (0.5 0.5 0.5)'),
  ('cube_above',
   'POLYHEDRALSURFACE Z(((0 0 0,0 1 0,1 1 0,1 0 0,0 0 0)),((0 0 1,1 0 1,1 1 1,0 1 1,0 0 1)),((0 0 0,1 0 0,1 0 1,0 0 1,0 0 0)),((1 0 0,1 1 0,1 1 1,1 0 1,1 0 0)),((1 1 0,0 1 0,0 1 1,1 1 1,1 1 0)),((0 1 0,0 0 0,0 0 1,0 1 1,0 1 0)))',
   'POINT Z (0.5 0.5 3)'),
  -- multi geometries
  ('mpt_line',          'MULTIPOINT Z ((0 0 5),(20 20 5))', 'LINESTRING Z (0 0 0,10 0 0)'),
  ('mline_pt',          'MULTILINESTRING Z ((0 0 0,5 5 0),(10 10 8,15 15 8))', 'POINT Z (12 12 8)'),
  -- empty operands
  ('pt_empty',          'POINT Z (0 0 0)', 'LINESTRING EMPTY')
),
fns(f) AS (VALUES
  ('3ddistance'),('3dmaxdistance'),('3dintersects'),
  ('3dclosestpoint'),('3dshortestline'),('3dlongestline'))
SELECT row_to_json(t)::text FROM (
  SELECT p.id || ':' || f.f AS id, p.a, p.b, f.f AS "fn",
    CASE f.f
      WHEN '3ddistance'     THEN to_jsonb(ST_3DDistance(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN '3dmaxdistance'  THEN to_jsonb(ST_3DMaxDistance(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN '3dintersects'   THEN to_jsonb(ST_3DIntersects(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      -- The witness functions are compared as text: kenro has no 3D WKT writer,
      -- so the harness reads the ordinates back with ST_X/ST_Y/ST_Z instead.
      WHEN '3dclosestpoint' THEN to_jsonb(ST_AsEWKT(ST_3DClosestPoint(ST_GeomFromText(p.a), ST_GeomFromText(p.b))))
      WHEN '3dshortestline' THEN to_jsonb(ST_AsEWKT(ST_3DShortestLine(ST_GeomFromText(p.a), ST_GeomFromText(p.b))))
      WHEN '3dlongestline'  THEN to_jsonb(ST_AsEWKT(ST_3DLongestLine(ST_GeomFromText(p.a), ST_GeomFromText(p.b))))
    END AS expected,
    -- ⚠️ A documented divergence, and PostGIS is the one in the wrong here: on
    -- an empty operand the witness functions return *uninitialised memory*.
    -- ST_3DShortestLine(POINT Z (0 0 0), LINESTRING EMPTY) is
    -- `LINESTRING(0 0 0,0 4.63557111106545e-310 0)` — that subnormal is garbage,
    -- and ST_3DClosestPoint answers POINT(0 0 0) as if the empty operand were
    -- the origin. kenro returns NULL, matching what its own ST_3DDistance
    -- answers for the same pair.
    CASE
      WHEN f.f IN ('3dclosestpoint','3dshortestline','3dlongestline')
       AND (ST_IsEmpty(ST_GeomFromText(p.a)) OR ST_IsEmpty(ST_GeomFromText(p.b)))
      THEN true
    END AS kenro_null,
    CASE
      WHEN f.f IN ('3dclosestpoint','3dshortestline','3dlongestline')
       AND (ST_IsEmpty(ST_GeomFromText(p.a)) OR ST_IsEmpty(ST_GeomFromText(p.b)))
      THEN 'PostGIS returns uninitialised memory for an empty operand; kenro returns NULL'
    END AS note
  FROM pairs p CROSS JOIN fns f
  ORDER BY p.id, f.f
) t;

-- The tolerance-taking forms, one row per (pair, distance).
WITH dw(id, a, b, arg) AS (VALUES
  ('dw_skew_exact',   'LINESTRING Z (0 0 0,10 0 0)', 'LINESTRING Z (5 -5 4,5 5 4)', 4.0),
  ('dw_skew_under',   'LINESTRING Z (0 0 0,10 0 0)', 'LINESTRING Z (5 -5 4,5 5 4)', 3.9),
  ('dw_vertical',     'POINT Z (0 0 0)', 'POINT Z (0 0 9)', 9.0),
  ('dw_face_above',   'POINT Z (5 5 10)', 'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))', 10.0),
  ('dw_empty',        'POINT Z (0 0 0)', 'LINESTRING EMPTY', 5.0)
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':3ddwithin' AS id, a, b, '3ddwithin' AS "fn", arg,
    to_jsonb(ST_3DDWithin(ST_GeomFromText(a), ST_GeomFromText(b), arg)) AS expected
  FROM dw ORDER BY id
) t;

WITH dfw(id, a, b, arg) AS (VALUES
  ('dfw_face_corner', 'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))', 'POINT Z (0 0 0)', 14.15),
  ('dfw_too_tight',   'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))', 'POINT Z (0 0 0)', 14.14),
  ('dfw_vertical',    'POINT Z (0 0 0)', 'POINT Z (0 0 9)', 9.0)
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':3ddfullywithin' AS id, a, b, '3ddfullywithin' AS "fn", arg,
    to_jsonb(ST_3DDFullyWithin(ST_GeomFromText(a), ST_GeomFromText(b), arg)) AS expected
  FROM dfw ORDER BY id
) t;

-- ST_3DLineInterpolatePoint takes its fraction by 3D length, unlike the 2D
-- sibling. The multi-segment cases are where the two disagree.
WITH lip(id, a, arg) AS (VALUES
  ('lip_steep_half',  'LINESTRING Z (0 0 0,10 0 100)', 0.5),
  ('lip_steep_start', 'LINESTRING Z (0 0 0,10 0 100)', 0.0),
  ('lip_steep_end',   'LINESTRING Z (0 0 0,10 0 100)', 1.0),
  ('lip_multi_half',  'LINESTRING Z (0 0 0,10 0 10,20 0 30)', 0.5),
  ('lip_multi_qtr',   'LINESTRING Z (0 0 0,10 0 10,20 0 30)', 0.25),
  ('lip_flat_half',   'LINESTRING Z (0 0 5,10 0 5)', 0.5)
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':3dlineinterpolatepoint' AS id, a, '3dlineinterpolatepoint' AS "fn", arg,
    to_jsonb(ST_AsEWKT(ST_3DLineInterpolatePoint(ST_GeomFromText(a), arg))) AS expected
  FROM lip ORDER BY id
) t;

-- ⚠️ A reference defect, recorded rather than reproduced.
--
-- A point at the *exact centre* of a coplanar face makes PostGIS 3.5 contradict
-- itself:
--
--   ST_3DDistance(POINT Z (5 5 0), POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,…)))
--     -> 7.0710678118654755          the distance to the (0 0 0) corner
--   ST_3DClosestPoint(same pair)  -> POINT(5 5 0)      i.e. distance 0
--   ST_3DIntersects(same pair)    -> false
--
-- Three answers that cannot all be right. Nearby interior coplanar points —
-- (2 2 0), (3 3 0), (4 6 0), (7 3 0) — all answer 0 and `true`, including
-- others on the same fan diagonal, so it is specific to the centre rather than
-- a rule about coplanarity. kenro answers 0 / true, which is what PostGIS's own
-- ST_3DClosestPoint implies.
SELECT row_to_json(t)::text FROM (VALUES
  ('pt_face_centre:3ddistance',
   'POINT Z (5 5 0)', 'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))',
   '3ddistance', '7.0710678118654755'::jsonb, '0'::jsonb),
  ('pt_face_centre:3dintersects',
   'POINT Z (5 5 0)', 'POLYGON Z ((0 0 0,10 0 0,10 10 0,0 10 0,0 0 0))',
   '3dintersects', 'false'::jsonb, 'true'::jsonb)
) AS t(id, a, b, "fn", expected, kenro_expected);
