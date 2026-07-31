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
  ('simplify_point',  'POINT(1 2)',                                           'simplify',  10.0),
  -- npoints
  ('npoints_square',  'POLYGON((0 0,4 0,4 4,0 4,0 0))',                       'npoints',   NULL),
  ('npoints_hole',    'POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))', 'npoints',   NULL),
  ('npoints_mpt',     'MULTIPOINT(1 2,3 4)',                                  'npoints',   NULL),
  ('npoints_gc',      'GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))',   'npoints',   NULL),
  ('npoints_empty',   'POLYGON EMPTY',                                        'npoints',   NULL),
  -- perimeter
  ('perimeter_square','POLYGON((0 0,4 0,4 4,0 4,0 0))',                       'perimeter', NULL),
  ('perimeter_hole',  'POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))', 'perimeter', NULL),
  ('perimeter_mpoly', 'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,4 2,4 4,2 4,2 2)))', 'perimeter', NULL),
  ('perimeter_line',  'LINESTRING(0 0,3 4)',                                  'perimeter', NULL),
  ('perimeter_point', 'POINT(1 2)',                                           'perimeter', NULL),
  -- geometrytype
  ('geomtype_pt',     'POINT(1 2)',                                           'geomtype',  NULL),
  ('geomtype_line',   'LINESTRING(0 0,1 1)',                                  'geomtype',  NULL),
  ('geomtype_poly',   'POLYGON((0 0,1 0,1 1,0 1,0 0))',                       'geomtype',  NULL),
  ('geomtype_mpt',    'MULTIPOINT(1 2)',                                      'geomtype',  NULL),
  ('geomtype_mline',  'MULTILINESTRING((0 0,1 1))',                           'geomtype',  NULL),
  ('geomtype_mpoly',  'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))',                'geomtype',  NULL),
  ('geomtype_gc',     'GEOMETRYCOLLECTION(POINT(1 2))',                       'geomtype',  NULL),
  ('geomtype_empty',  'POLYGON EMPTY',                                        'geomtype',  NULL),
  -- numgeometries
  ('numgeoms_mpoly',  'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,4 2,4 4,2 4,2 2)))', 'numgeoms', NULL),
  ('numgeoms_gc',     'GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1),POINT(5 5))', 'numgeoms', NULL),
  ('numgeoms_point',  'POINT(1 2)',                                           'numgeoms',  NULL),
  ('numgeoms_empty',  'MULTIPOLYGON EMPTY',                                   'numgeoms',  NULL),
  ('numgeoms_line_empty', 'LINESTRING EMPTY',                                 'numgeoms',  NULL),
  -- geometryn (arg = n)
  ('geometryn_1',     'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,4 2,4 4,2 4,2 2)))', 'geometryn', 1),
  ('geometryn_2',     'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,4 2,4 4,2 4,2 2)))', 'geometryn', 2),
  ('geometryn_oob',   'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,4 2,4 4,2 4,2 2)))', 'geometryn', 3),
  ('geometryn_zero',  'MULTIPOINT(1 2,3 4)',                                  'geometryn', 0),
  ('geometryn_single','POINT(1 2)',                                           'geometryn', 1),
  ('geometryn_gc',    'GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))',   'geometryn', 2),
  -- start/end point
  ('startpoint_line', 'LINESTRING(1 2,3 4,5 6)',                              'startpoint', NULL),
  ('startpoint_pt',   'POINT(1 2)',                                           'startpoint', NULL),
  ('startpoint_mline','MULTILINESTRING((1 2,3 4))',                           'startpoint', NULL),
  ('startpoint_empty','LINESTRING EMPTY',                                     'startpoint', NULL),
  ('endpoint_line',   'LINESTRING(1 2,3 4,5 6)',                              'endpoint',  NULL),
  ('endpoint_poly',   'POLYGON((0 0,1 0,1 1,0 1,0 0))',                       'endpoint',  NULL),
  -- pointn (arg = n)
  ('pointn_first',    'LINESTRING(1 2,3 4,5 6,7 8,9 10)',                     'pointn',    1),
  ('pointn_last',     'LINESTRING(1 2,3 4,5 6,7 8,9 10)',                     'pointn',    5),
  ('pointn_neg',      'LINESTRING(1 2,3 4,5 6,7 8,9 10)',                     'pointn',    -1),
  ('pointn_oob',      'LINESTRING(1 2,3 4,5 6,7 8,9 10)',                     'pointn',    6),
  ('pointn_zero',     'LINESTRING(1 2,3 4,5 6,7 8,9 10)',                     'pointn',    0),
  ('pointn_poly',     'POLYGON((0 0,1 0,1 1,0 1,0 0))',                       'pointn',    1),
  -- reverse
  ('reverse_line',    'LINESTRING(1 2,3 4,5 6)',                              'reverse',   NULL),
  ('reverse_hole',    'POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))', 'reverse',  NULL),
  ('reverse_mline',   'MULTILINESTRING((0 0,1 1),(2 2,3 3))',                 'reverse',   NULL),
  ('reverse_point',   'POINT(1 2)',                                           'reverse',   NULL)
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
      WHEN 'npoints'   THEN to_jsonb(ST_NPoints(ST_GeomFromText(c.a)))
      WHEN 'perimeter' THEN to_jsonb(ST_Perimeter(ST_GeomFromText(c.a)))
      WHEN 'geomtype'  THEN to_jsonb(ST_GeometryType(ST_GeomFromText(c.a)))
      WHEN 'numgeoms'  THEN to_jsonb(ST_NumGeometries(ST_GeomFromText(c.a)))
      WHEN 'geometryn' THEN to_jsonb(ST_AsText(ST_GeometryN(ST_GeomFromText(c.a), c.arg::int)))
      WHEN 'startpoint' THEN to_jsonb(ST_AsText(ST_StartPoint(ST_GeomFromText(c.a))))
      WHEN 'endpoint'  THEN to_jsonb(ST_AsText(ST_EndPoint(ST_GeomFromText(c.a))))
      WHEN 'pointn'    THEN to_jsonb(ST_AsText(ST_PointN(ST_GeomFromText(c.a), c.arg::int)))
      WHEN 'reverse'   THEN to_jsonb(ST_AsText(ST_Reverse(ST_GeomFromText(c.a))))
    END AS expected
  FROM acc c ORDER BY c.id
) t;

-- Constructors (no input geometry; numeric args in `args`).
SELECT row_to_json(t)::text FROM (
  SELECT 'makepoint:makepoint' AS id, ARRAY[1.5, 2.5] AS args, 'makepoint' AS "fn",
         to_jsonb(ST_AsText(ST_MakePoint(1.5, 2.5))) AS expected
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'point2:point' AS id, ARRAY[-3.25, 4.0] AS args, 'point' AS "fn",
         to_jsonb(ST_AsText(ST_Point(-3.25, 4.0))) AS expected
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'point3:point_srid' AS id, ARRAY[1.0, 2.0] AS args, 3857 AS srid, 'point_srid' AS "fn",
         to_jsonb(ST_SRID(ST_Point(1.0, 2.0, 3857))) AS expected
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'envelope4:makeenvelope' AS id, ARRAY[0.0, 0.0, 2.0, 3.0] AS args, 'makeenvelope' AS "fn",
         to_jsonb(ST_AsText(ST_MakeEnvelope(0.0, 0.0, 2.0, 3.0))) AS expected
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'envelope_degenerate:makeenvelope' AS id, ARRAY[1.0, 2.0, 1.0, 2.0] AS args, 'makeenvelope' AS "fn",
         to_jsonb(ST_AsText(ST_MakeEnvelope(1.0, 2.0, 1.0, 2.0))) AS expected
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'envelope5:makeenvelope_srid' AS id, ARRAY[0.0, 0.0, 2.0, 3.0] AS args, 4326 AS srid, 'makeenvelope_srid' AS "fn",
         to_jsonb(ST_SRID(ST_MakeEnvelope(0.0, 0.0, 2.0, 3.0, 4326))) AS expected
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
