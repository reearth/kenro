-- Golden vector generator: computes expected values for kenro's golden
-- tests from the reference PostGIS. Run via scripts/golden/generate.sh;
-- output is one JSON object per line, appended to
-- tests/golden/predicates.jsonl (committed — CI never runs this).
--
-- Note: no POINT EMPTY here — kenro's ST_GeomFromText rejects it (a
-- documented divergence), so it cannot appear in vectors constructed
-- through WKT.
\pset tuples_only on
\pset format unaligned

WITH pairs(id, a, b) AS (VALUES
  -- polygon vs point
  ('sq_pt_inside',        'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POINT(5 5)'),
  ('sq_pt_corner',        'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POINT(0 0)'),
  ('sq_pt_edge',          'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POINT(10 5)'),
  ('sq_pt_outside',       'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POINT(20 20)'),
  ('sq_pt_near_edge',     'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POINT(10.000000001 5)'),
  -- polygon vs polygon
  ('sq_sq_identical',     'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((0 0,10 0,10 10,0 10,0 0))'),
  ('sq_sq_nested',        'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((2 2,8 2,8 8,2 8,2 2))'),
  ('sq_sq_overlap',       'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((5 5,15 5,15 15,5 15,5 5))'),
  ('sq_sq_disjoint',      'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((20 20,30 20,30 30,20 30,20 20))'),
  ('sq_sq_edge_touch',    'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((10 0,20 0,20 10,10 10,10 0))'),
  ('sq_sq_corner_touch',  'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((10 10,20 10,20 20,10 20,10 10))'),
  ('sq_sq_inner_edge',    'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((0 0,5 0,5 5,0 5,0 0))'),
  -- polygon with hole
  ('hole_pt_in_hole',     'POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,8 2,8 8,2 8,2 2))', 'POINT(5 5)'),
  ('hole_pt_in_ring',     'POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,8 2,8 8,2 8,2 2))', 'POINT(1 1)'),
  ('hole_pt_hole_edge',   'POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,8 2,8 8,2 8,2 2))', 'POINT(2 5)'),
  -- polygon vs line
  ('sq_line_crossing',    'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'LINESTRING(-5 5,15 5)'),
  ('sq_line_inside',      'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'LINESTRING(2 2,8 8)'),
  ('sq_line_on_edge',     'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'LINESTRING(0 0,10 0)'),
  ('sq_line_outside',     'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'LINESTRING(20 20,30 30)'),
  -- line vs line
  ('line_line_cross',     'LINESTRING(0 0,10 10)', 'LINESTRING(0 10,10 0)'),
  ('line_line_collinear', 'LINESTRING(0 0,10 0)', 'LINESTRING(5 0,15 0)'),
  ('line_line_sub',       'LINESTRING(0 0,10 0)', 'LINESTRING(2 0,8 0)'),
  ('line_line_disjoint',  'LINESTRING(0 0,10 0)', 'LINESTRING(0 1,10 1)'),
  ('line_line_endpoint',  'LINESTRING(0 0,10 0)', 'LINESTRING(10 0,20 0)'),
  -- line vs point
  ('line_pt_on',          'LINESTRING(0 0,10 10)', 'POINT(5 5)'),
  ('line_pt_endpoint',    'LINESTRING(0 0,10 10)', 'POINT(0 0)'),
  ('line_pt_off',         'LINESTRING(0 0,10 10)', 'POINT(5 6)'),
  -- point vs point
  ('pt_pt_same',          'POINT(3 4)', 'POINT(3 4)'),
  ('pt_pt_diff',          'POINT(3 4)', 'POINT(3.0000001 4)'),
  -- multi geometries
  ('mpt_sq',              'MULTIPOINT(1 1,20 20)', 'POLYGON((0 0,10 0,10 10,0 10,0 0))'),
  ('mpt_sq_all_in',       'MULTIPOINT(1 1,2 2)', 'POLYGON((0 0,10 0,10 10,0 10,0 0))'),
  ('mpoly_pt',            'MULTIPOLYGON(((0 0,5 0,5 5,0 5,0 0)),((20 20,25 20,25 25,20 25,20 20)))', 'POINT(22 22)'),
  ('mline_line',          'MULTILINESTRING((0 0,5 5),(10 10,15 15))', 'LINESTRING(0 5,5 0)'),
  -- empty operands (non-point empties only; see header note)
  ('sq_empty_line',       'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'LINESTRING EMPTY'),
  ('empty_poly_pt',       'POLYGON EMPTY', 'POINT(1 1)'),
  ('empty_empty',         'LINESTRING EMPTY', 'POLYGON EMPTY')
),
fns(f) AS (VALUES
  ('intersects'),('contains'),('within'),('distance'),
  ('disjoint'),('touches'),('crosses'),('overlaps'),('equals'),
  ('covers'),('coveredby'),('relate'))
SELECT row_to_json(t)::text FROM (
  SELECT p.id || ':' || f.f AS id, p.a, p.b, f.f AS "fn",
    CASE f.f
      WHEN 'intersects' THEN to_jsonb(ST_Intersects(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'contains'   THEN to_jsonb(ST_Contains(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'within'     THEN to_jsonb(ST_Within(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'distance'   THEN to_jsonb(ST_Distance(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'disjoint'   THEN to_jsonb(ST_Disjoint(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'touches'    THEN to_jsonb(ST_Touches(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'crosses'    THEN to_jsonb(ST_Crosses(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'overlaps'   THEN to_jsonb(ST_Overlaps(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'equals'     THEN to_jsonb(ST_Equals(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'covers'     THEN to_jsonb(ST_Covers(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'coveredby'  THEN to_jsonb(ST_CoveredBy(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
      WHEN 'relate'     THEN to_jsonb(ST_Relate(ST_GeomFromText(p.a), ST_GeomFromText(p.b)))
    END AS expected
  FROM pairs p CROSS JOIN fns f
  ORDER BY p.id, f.f
) t;

-- ST_Relate 3-arg pattern form.
WITH rp(id, a, b, arg_text) AS (VALUES
  ('rp_within_hit',  'POINT(5 5)',  'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'T*F**F***'),
  ('rp_within_miss', 'POINT(50 50)','POLYGON((0 0,10 0,10 10,0 10,0 0))', 'T*F**F***'),
  ('rp_touch_hit',   'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((10 0,20 0,20 10,10 10,10 0))', 'FT*******'),
  ('rp_wildcard',    'LINESTRING(0 0,5 5)', 'LINESTRING(0 5,5 0)', '0********')
)
SELECT row_to_json(t)::text FROM (
  SELECT r.id || ':relate_pattern' AS id, r.a, r.b, r.arg_text, 'relate_pattern' AS "fn",
    to_jsonb(ST_Relate(ST_GeomFromText(r.a), ST_GeomFromText(r.b), r.arg_text)) AS expected
  FROM rp r ORDER BY r.id
) t;

WITH dw(id, a, b, arg) AS (VALUES
  ('dw_exact',      'POINT(0 0)', 'POINT(3 4)', 5.0),
  ('dw_just_under', 'POINT(0 0)', 'POINT(3 4)', 4.999999999),
  ('dw_zero_touch', 'POINT(0 0)', 'POINT(0 0)', 0.0),
  ('dw_line_pt',    'LINESTRING(0 0,10 0)', 'POINT(5 3)', 3.0),
  ('dw_poly_poly',  'POLYGON((0 0,10 0,10 10,0 10,0 0))', 'POLYGON((13 0,20 0,20 10,13 10,13 0))', 2.9)
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':dwithin' AS id, a, b, 'dwithin' AS "fn", arg,
    to_jsonb(ST_DWithin(ST_GeomFromText(a), ST_GeomFromText(b), arg)) AS expected
  FROM dw ORDER BY id
) t;

-- Error-expectation vectors: PostGIS raises here, so the expected value
-- cannot be computed by evaluation — emitted as literals (verified manually:
-- PostGIS 3.5 raises 'Tolerance cannot be less than zero').
SELECT '{"id":"dw_negative:dwithin","a":"POINT(0 0)","b":"POINT(0 0)","fn":"dwithin","arg":-1.0,"expected":{"error":true},"note":"PostGIS raises on negative tolerance; kenro raises too"}';

-- WKT output formatting check: kenro's ST_AsText must match PostGIS
-- digit-for-digit (or carry a kenro_expected + note in the vector file).
WITH texts(id, a) AS (VALUES
  ('t_pt_int',       'POINT(1 2)'),
  ('t_pt_frac',      'POINT(0.1 0.2)'),
  ('t_pt_neg',       'POINT(-1.5 -2.5)'),
  ('t_pt_big',       'POINT(1234567.891 7654321.123)'),
  ('t_pt_small',     'POINT(0.000001 -0.000001)'),
  ('t_pt_15digits',  'POINT(139.766083865829 35.681382012208)'),
  ('t_line',         'LINESTRING(0 0,1 1,2 0)'),
  ('t_poly_hole',    'POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,8 2,8 8,2 8,2 2))'),
  ('t_mpt',          'MULTIPOINT(1 2,3 4)'),
  ('t_mline',        'MULTILINESTRING((0 0,1 1),(2 2,3 3))'),
  ('t_mpoly',        'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'),
  ('t_gc',           'GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))'),
  ('t_empty_line',   'LINESTRING EMPTY'),
  ('t_empty_poly',   'POLYGON EMPTY'),
  ('t_empty_mpoly',  'MULTIPOLYGON EMPTY')
)
SELECT row_to_json(t)::text FROM (
  SELECT id || ':astext' AS id, a, 'astext' AS "fn",
    to_jsonb(ST_AsText(ST_GeomFromText(a))) AS expected
  FROM texts ORDER BY id
) t;
