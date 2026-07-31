-- Golden vectors for the Tier B measure/processing functions, computed by
-- the reference PostGIS.
\pset tuples_only on
\pset format unaligned

WITH m(id, a, b, f, arg) AS (VALUES
  -- closestpoint (g2 restricted to POINT in kenro)
  ('closest_line_pt',   'LINESTRING(0 0,10 0)', 'POINT(5 3)',    'closestpoint', NULL::float8),
  ('closest_poly_pt',   'POLYGON((0 0,4 0,4 4,0 4,0 0))', 'POINT(10 2)', 'closestpoint', NULL),
  ('closest_pt_pt',     'POINT(1 2)', 'POINT(9 9)',              'closestpoint', NULL),
  -- lineinterpolate (arg = fraction)
  ('interp_start',      'LINESTRING(0 0,10 0)', NULL,            'lineinterpolate', 0.0),
  ('interp_mid',        'LINESTRING(0 0,10 0)', NULL,            'lineinterpolate', 0.5),
  ('interp_end',        'LINESTRING(0 0,10 0)', NULL,            'lineinterpolate', 1.0),
  ('interp_multiseg',   'LINESTRING(0 0,4 0,4 4)', NULL,         'lineinterpolate', 0.35),
  -- linelocate
  ('locate_mid',        'LINESTRING(0 0,10 0)', 'POINT(2.5 4)',  'linelocate', NULL),
  ('locate_start',      'LINESTRING(0 0,10 0)', 'POINT(-5 0)',   'linelocate', NULL),
  ('locate_end',        'LINESTRING(0 0,10 0)', 'POINT(15 3)',   'linelocate', NULL),
  ('locate_multiseg',   'LINESTRING(0 0,4 0,4 4)', 'POINT(4 1)', 'linelocate', NULL),
  -- hausdorff (vertex-aligned cases where geo and GEOS agree)
  ('hausdorff_parallel','LINESTRING(0 0,10 0)', 'LINESTRING(0 3,10 3)', 'hausdorff', NULL),
  ('hausdorff_same',    'LINESTRING(0 0,10 0)', 'LINESTRING(0 0,10 0)', 'hausdorff', NULL),
  ('hausdorff_pt_line', 'POINT(5 5)', 'LINESTRING(0 0,10 0)',    'hausdorff', NULL),
  -- frechet
  ('frechet_parallel',  'LINESTRING(0 0,10 0)', 'LINESTRING(0 3,10 3)', 'frechet', NULL),
  ('frechet_reversed',  'LINESTRING(0 0,10 0)', 'LINESTRING(10 3,0 3)', 'frechet', NULL),
  ('frechet_same',      'LINESTRING(0 0,10 0)', 'LINESTRING(0 0,10 0)', 'frechet', NULL),
  -- azimuth
  ('azimuth_north',     'POINT(0 0)', 'POINT(0 5)',              'azimuth', NULL),
  ('azimuth_east',      'POINT(0 0)', 'POINT(5 0)',              'azimuth', NULL),
  ('azimuth_sw',        'POINT(0 0)', 'POINT(-1 -1)',            'azimuth', NULL),
  ('azimuth_same',      'POINT(3 4)', 'POINT(3 4)',              'azimuth', NULL)
)
SELECT row_to_json(t)::text FROM (
  SELECT m.id || ':' || m.f AS id, m.a, m.b, m.f AS "fn", m.arg,
    CASE m.f
      WHEN 'closestpoint'    THEN to_jsonb(ST_AsText(ST_ClosestPoint(ST_GeomFromText(m.a), ST_GeomFromText(m.b))))
      WHEN 'lineinterpolate' THEN to_jsonb(ST_AsText(ST_LineInterpolatePoint(ST_GeomFromText(m.a), m.arg)))
      WHEN 'linelocate'      THEN to_jsonb(ST_LineLocatePoint(ST_GeomFromText(m.a), ST_GeomFromText(m.b)))
      WHEN 'hausdorff'       THEN to_jsonb(ST_HausdorffDistance(ST_GeomFromText(m.a), ST_GeomFromText(m.b)))
      WHEN 'frechet'         THEN to_jsonb(ST_FrechetDistance(ST_GeomFromText(m.a), ST_GeomFromText(m.b)))
      WHEN 'azimuth'         THEN to_jsonb(ST_Azimuth(ST_GeomFromText(m.a), ST_GeomFromText(m.b)))
    END AS expected
  FROM m ORDER BY m.id
) t;

-- Error-expectation vectors.
SELECT row_to_json(t)::text FROM (
  SELECT 'interp_oob:lineinterpolate' AS id, 'LINESTRING(0 0,10 0)' AS a, 1.5 AS arg,
         'lineinterpolate' AS "fn", jsonb_build_object('error', true) AS expected,
         'PostGIS raises on fraction outside [0,1]; kenro raises too (geo would clamp)' AS note
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'frechet_poly:frechet' AS id, 'POLYGON((0 0,1 0,1 1,0 1,0 0))' AS a,
         'LINESTRING(0 0,1 1)' AS b, 'frechet' AS "fn",
         jsonb_build_object('error', true) AS expected,
         'kenro restricts ST_FrechetDistance to LINESTRING x LINESTRING' AS note
) t;
