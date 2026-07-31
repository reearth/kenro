-- Golden vectors for the overlay functions. `mode` selects the harness
-- comparison: "areal" (symmetric-difference area ratio — i_overlay and
-- GEOS build the same arrangement but different vertex chains),
-- "geometric" (coordinate tolerance), or exact (default; empties, point
-- filtering, errors).
\pset tuples_only on
\pset format unaligned

WITH pairs(id, a, b) AS (VALUES
  ('overlap',  'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((5 5,15 5,15 15,5 15,5 5))'),
  ('nested',   'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((2 2,8 2,8 8,2 8,2 2))'),
  ('hole_hit', 'POLYGON((0 0,10 0,10 10,0 10,0 0),(3 3,7 3,7 7,3 7,3 3))', 'POLYGON((4 4,12 4,12 6,4 6,4 4))'),
  ('star',     'POLYGON((0 0,10 0,5 4,10 10,0 10,0 0))', 'POLYGON((3 -2,12 3,6 12,3 -2))')
),
ops(f) AS (VALUES ('intersection'),('difference'),('symdifference'),('union'))
SELECT row_to_json(t)::text FROM (
  SELECT p.id || ':' || o.f AS id, p.a, p.b, o.f AS "fn", 'areal' AS mode,
    CASE o.f
      WHEN 'intersection'  THEN to_jsonb(ST_AsText(ST_Intersection(ST_GeomFromText(p.a), ST_GeomFromText(p.b))))
      WHEN 'difference'    THEN to_jsonb(ST_AsText(ST_Difference(ST_GeomFromText(p.a), ST_GeomFromText(p.b))))
      WHEN 'symdifference' THEN to_jsonb(ST_AsText(ST_SymDifference(ST_GeomFromText(p.a), ST_GeomFromText(p.b))))
      WHEN 'union'         THEN to_jsonb(ST_AsText(ST_Union(ST_GeomFromText(p.a), ST_GeomFromText(p.b))))
    END AS expected
  FROM pairs p CROSS JOIN ops o
  ORDER BY p.id, o.f
) t;

-- Exact / geometric cases: point filtering and line clipping.
WITH e(id, a, b, f, mode) AS (VALUES
  ('pts_in_sq',   'MULTIPOINT(5 5,20 20,10 5)', 'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'intersection', 'exact'),
  ('pts_diff',    'MULTIPOINT(5 5,20 20)', 'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'difference', 'exact'),
  ('pt_union',    'POINT(1 1)', 'MULTIPOINT(1 1,2 2)', 'union', 'exact'),
  ('pts_symdiff', 'MULTIPOINT(1 1,2 2)', 'MULTIPOINT(2 2,3 3)', 'symdifference', 'exact'),
  ('line_clip',   'LINESTRING(-5 5,15 5)', 'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'intersection', 'geometric'),
  ('line_diff',   'LINESTRING(-5 5,15 5)', 'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'difference', 'geometric'),
  ('disjoint_aa', 'POLYGON((0 0,1 0,1 1,0 1,0 0))', 'POLYGON((5 5,6 5,6 6,5 6,5 5))', 'intersection', 'exact'),
  ('pt_missing',  'POINT(50 50)', 'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'intersection', 'exact')
)
SELECT row_to_json(t)::text FROM (
  SELECT e.id || ':' || e.f AS id, e.a, e.b, e.f AS "fn", e.mode,
    CASE e.f
      WHEN 'intersection'  THEN to_jsonb(ST_AsText(ST_Intersection(ST_GeomFromText(e.a), ST_GeomFromText(e.b))))
      WHEN 'difference'    THEN to_jsonb(ST_AsText(ST_Difference(ST_GeomFromText(e.a), ST_GeomFromText(e.b))))
      WHEN 'symdifference' THEN to_jsonb(ST_AsText(ST_SymDifference(ST_GeomFromText(e.a), ST_GeomFromText(e.b))))
      WHEN 'union'         THEN to_jsonb(ST_AsText(ST_Union(ST_GeomFromText(e.a), ST_GeomFromText(e.b))))
    END AS expected
  FROM e ORDER BY e.id
) t;

-- The headline documented divergence: touching polygons. GEOS returns the
-- shared edge; kenro's areal-only overlay returns POLYGON EMPTY.
SELECT row_to_json(t)::text FROM (
  SELECT 'touching:intersection' AS id,
         'POLYGON((0 0,10 0,10 10,0 10,0 0))' AS a,
         'POLYGON((10 0,20 0,20 10,10 10,10 0))' AS b,
         'intersection' AS "fn", 'exact' AS mode,
         to_jsonb(ST_AsText(ST_Intersection(
           ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'),
           ST_GeomFromText('POLYGON((10 0,20 0,20 10,10 10,10 0))')))) AS expected,
         to_jsonb('POLYGON EMPTY'::text) AS kenro_expected,
         'i_overlay produces areal results only; the shared boundary line is not returned' AS note
) t;

-- kenro-only unsupported combinations (PostGIS supports them).
SELECT row_to_json(t)::text FROM (
  SELECT 'line_line:intersection' AS id, 'LINESTRING(0 0,10 10)' AS a,
         'LINESTRING(0 10,10 0)' AS b, 'intersection' AS "fn", 'exact' AS mode,
         to_jsonb(ST_AsText(ST_Intersection(ST_GeomFromText('LINESTRING(0 0,10 10)'), ST_GeomFromText('LINESTRING(0 10,10 0)')))) AS expected,
         jsonb_build_object('error', true) AS kenro_expected,
         'kenro cannot node line-line intersections; loud Unsupported error' AS note
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'mixed_union:union' AS id, 'LINESTRING(0 0,10 10)' AS a,
         'POLYGON((0 0,4 0,4 4,0 4,0 0))' AS b, 'union' AS "fn", 'exact' AS mode,
         to_jsonb(ST_AsText(ST_Union(ST_GeomFromText('LINESTRING(0 0,10 10)'), ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))')))) AS expected,
         jsonb_build_object('error', true) AS kenro_expected,
         'mixed-dimension unions produce GeometryCollections; kenro raises instead' AS note
) t;

-- ST_MakeValid: kenro repairs polygons with GEOS's *structure*-method
-- semantics (areal results only). The reference image's GEOS (3.9) predates
-- the structure method, so `expected` is the default linework repair;
-- shapes where kenro legitimately differs carry an explicit kenro_expected
-- override. jsonb_strip_nulls drops the key when there is no override.
WITH shapes(id, a, mode, kenro_override, shape_note) AS (VALUES
  ('mv_bowtie',        'POLYGON((0 0,2 2,2 0,0 2,0 0))', 'areal', NULL, NULL),
  ('mv_figure8',       'POLYGON((0 0,1 1,2 0,2 2,1 1,0 2,0 0))', 'areal', NULL, NULL),
  ('mv_hole_outside',  'POLYGON((0 0,4 0,4 4,0 4,0 0),(5 5,6 5,6 6,5 6,5 5))', 'areal', NULL, NULL),
  ('mv_hole_crossing', 'POLYGON((0 0,4 0,4 4,0 4,0 0),(2 2,6 2,6 3,2 3,2 2))', 'areal', NULL, NULL),
  ('mv_overlap_multi', 'MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((1 1,3 1,3 3,1 3,1 1)))', 'areal', NULL, NULL),
  ('mv_collinear_sliver', 'POLYGON((0 0,2 0,1 0,0 0))', 'exact',
   'POLYGON((0 0,2 0,1 0,0 0))',
   'georust validation does not flag collinear zero-area rings (the documented ST_IsValid gap), so kenro returns the input unchanged where PostGIS collapses it to lines'),
  ('mv_already_valid', 'POLYGON((0 0,3 0,3 3,0 3,0 0))', 'exact', NULL, NULL),
  ('mv_cw_exterior',   'POLYGON((0 0,0 3,3 3,3 0,0 0))', 'exact', NULL, NULL),
  ('mv_point',         'POINT(1 2)', 'exact', NULL, NULL),
  ('mv_crossing_line', 'LINESTRING(0 0,2 2,2 0,0 2)', 'exact', NULL, NULL)
)
SELECT jsonb_strip_nulls(row_to_json(t)::jsonb)::text FROM (
  SELECT s.id || ':makevalid' AS id, s.a, 'makevalid' AS "fn", s.mode,
         to_jsonb(ST_AsText(ST_MakeValid(ST_GeomFromText(s.a)))) AS expected,
         to_jsonb(s.kenro_override) AS kenro_expected,
         coalesce(s.shape_note, 'kenro repairs with structure-method semantics (areal-only results)') AS note
  FROM shapes s ORDER BY s.id
) t;
