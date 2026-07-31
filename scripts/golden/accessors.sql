-- Golden vectors for the accessor functions, computed by the reference
-- PostGIS. Geometry-returning functions (centroid/envelope/simplify) are
-- compared geometrically by the harness (coordinate tolerance), scalars by
-- value.
\pset tuples_only on
\pset format unaligned

WITH acc(id, a, f, arg) AS (VALUES
  -- area
  ('area_square',     'POLYGON((0 0,4 0,4 4,0 4,0 0))',                       'area',      NULL::float8),
  ('area_hole',       'POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,3 1,3 3,1 3,1 1))', 'area',      NULL),
  ('area_point',      'POINT(1 2)',                                           'area',      NULL),
  ('area_line',       'LINESTRING(0 0,5 5)',                                  'area',      NULL),
  ('area_empty',      'POLYGON EMPTY',                                        'area',      NULL),
  ('area_mpoly',      'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,4 2,4 4,2 4,2 2)))', 'area', NULL),
  -- length
  ('length_345',      'LINESTRING(0 0,3 4)',                                  'length',    NULL),
  ('length_mline',    'MULTILINESTRING((0 0,3 4),(0 0,6 8))',                 'length',    NULL),
  ('length_polygon',  'POLYGON((0 0,4 0,4 4,0 4,0 0))',                       'length',    NULL),
  ('length_point',    'POINT(1 2)',                                           'length',    NULL),
  ('length_gc',       'GEOMETRYCOLLECTION(LINESTRING(0 0,3 4),POINT(1 1))',   'length',    NULL),
  -- centroid
  ('centroid_square', 'POLYGON((0 0,4 0,4 4,0 4,0 0))',                       'centroid',  NULL),
  ('centroid_line',   'LINESTRING(0 0,4 0)',                                  'centroid',  NULL),
  ('centroid_mpt',    'MULTIPOINT(0 0,1 0,1 1)',                              'centroid',  NULL),
  ('centroid_tri',    'POLYGON((0 0,4 0,0 3,0 0))',                           'centroid',  NULL),
  ('centroid_empty',  'LINESTRING EMPTY',                                     'centroid',  NULL),
  -- envelope
  ('envelope_diag',   'LINESTRING(1 2,5 8)',                                  'envelope',  NULL),
  ('envelope_point',  'POINT(3 4)',                                           'envelope',  NULL),
  ('envelope_hline',  'LINESTRING(1 2,5 2)',                                  'envelope',  NULL),
  ('envelope_poly',   'POLYGON((0 0,4 0,4 4,0 4,0 0))',                       'envelope',  NULL),
  ('envelope_empty',  'LINESTRING EMPTY',                                     'envelope',  NULL),
  -- x / y
  ('x_point',         'POINT(3.5 -4.25)',                                     'x',         NULL),
  ('y_point',         'POINT(3.5 -4.25)',                                     'y',         NULL),
  -- numpoints
  ('numpoints_line',  'LINESTRING(0 0,1 1,2 2)',                              'numpoints', NULL),
  ('numpoints_point', 'POINT(0 0)',                                           'numpoints', NULL),
  ('numpoints_poly',  'POLYGON((0 0,1 0,1 1,0 1,0 0))',                       'numpoints', NULL),
  ('numpoints_mline', 'MULTILINESTRING((0 0,1 1))',                           'numpoints', NULL),
  -- isvalid
  ('isvalid_square',  'POLYGON((0 0,4 0,4 4,0 4,0 0))',                       'isvalid',   NULL),
  ('isvalid_bowtie',  'POLYGON((0 0,4 4,4 0,0 4,0 0))',                       'isvalid',   NULL),
  ('isvalid_hole_out','POLYGON((0 0,4 0,4 4,0 4,0 0),(5 5,6 5,6 6,5 6,5 5))', 'isvalid',   NULL),
  ('isvalid_line',    'LINESTRING(0 0,1 1)',                                  'isvalid',   NULL),
  -- simplify
  ('simplify_zigzag', 'LINESTRING(0 0,1 0.01,2 0,3 0.01,4 0)',                'simplify',  0.1),
  ('simplify_keep',   'LINESTRING(0 0,1 5,2 0)',                              'simplify',  0.1),
  ('simplify_poly',   'POLYGON((0 0,2 0.01,4 0,4 4,0 4,0 0))',                'simplify',  0.1),
  ('simplify_point',  'POINT(1 2)',                                           'simplify',  10.0)
)
SELECT row_to_json(t)::text FROM (
  SELECT c.id || ':' || c.f AS id, c.a, c.f AS "fn", c.arg,
    CASE c.f
      WHEN 'area'      THEN to_jsonb(ST_Area(ST_GeomFromText(c.a)))
      WHEN 'length'    THEN to_jsonb(ST_Length(ST_GeomFromText(c.a)))
      WHEN 'centroid'  THEN to_jsonb(ST_AsText(ST_Centroid(ST_GeomFromText(c.a))))
      WHEN 'envelope'  THEN to_jsonb(ST_AsText(ST_Envelope(ST_GeomFromText(c.a))))
      WHEN 'x'         THEN to_jsonb(ST_X(ST_GeomFromText(c.a)))
      WHEN 'y'         THEN to_jsonb(ST_Y(ST_GeomFromText(c.a)))
      WHEN 'numpoints' THEN to_jsonb(ST_NumPoints(ST_GeomFromText(c.a)))
      WHEN 'isvalid'   THEN to_jsonb(ST_IsValid(ST_GeomFromText(c.a)))
      WHEN 'simplify'  THEN to_jsonb(ST_AsText(ST_Simplify(ST_GeomFromText(c.a), c.arg)))
    END AS expected
  FROM acc c ORDER BY c.id
) t;

-- Error-expectation vectors.
SELECT row_to_json(t)::text FROM (
  SELECT 'x_line:x' AS id, 'LINESTRING(0 0,1 1)' AS a, 'x' AS "fn",
         jsonb_build_object('error', true) AS expected,
         'PostGIS: Argument to ST_X() must have type point; kenro raises too' AS note
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'y_poly:y' AS id, 'POLYGON((0 0,1 0,1 1,0 1,0 0))' AS a, 'y' AS "fn",
         jsonb_build_object('error', true) AS expected,
         'PostGIS: Argument to ST_Y() must have type point; kenro raises too' AS note
) t;
