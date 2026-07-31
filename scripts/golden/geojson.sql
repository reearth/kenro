-- Golden vectors for ST_AsGeoJSON (string equality) and ST_GeomFromGeoJSON.
\pset tuples_only on
\pset format unaligned

WITH gj(id, a, digits, srid) AS (VALUES
  ('gj_pt',         'POINT(1 2)',                                            NULL::int, NULL::int),
  ('gj_pt_frac',    'POINT(139.745433012 35.6585805)',                       NULL, NULL),
  ('gj_pt_digits3', 'POINT(139.745433012 35.6585805)',                       3,    NULL),
  ('gj_pt_digits0', 'POINT(139.7454 35.65)',                                 0,    NULL),
  ('gj_pt_small',   'POINT(0.000001 -0.000001)',                             NULL, NULL),
  ('gj_pt_int',     'POINT(139 35)',                                         NULL, NULL),
  ('gj_line',       'LINESTRING(0 0,1 1)',                                   NULL, NULL),
  ('gj_poly_hole',  'POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))', NULL, NULL),
  ('gj_mpt',        'MULTIPOINT(1 2,3 4)',                                   NULL, NULL),
  ('gj_mline',      'MULTILINESTRING((0 0,1 1),(2 2,3 3))',                  NULL, NULL),
  ('gj_mpoly',      'MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))',                 NULL, NULL),
  ('gj_gc',         'GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))',    NULL, NULL),
  ('gj_empty_line', 'LINESTRING EMPTY',                                      NULL, NULL),
  ('gj_crs_3857',   'POINT(1 2)',                                            NULL, 3857)
)
SELECT row_to_json(t)::text FROM (
  SELECT g.id || ':asgeojson' AS id, g.a, g.digits AS arg, g.srid, 'asgeojson' AS "fn",
    to_jsonb(CASE
      WHEN g.digits IS NULL THEN ST_AsGeoJSON(ST_SetSRID(ST_GeomFromText(g.a), coalesce(g.srid, 0)))
      ELSE ST_AsGeoJSON(ST_SetSRID(ST_GeomFromText(g.a), coalesce(g.srid, 0)), g.digits)
    END) AS expected
  FROM gj g ORDER BY g.id
) t;

WITH fg(id, a) AS (VALUES
  ('fg_pt',    '{"type":"Point","coordinates":[139.7,35.7]}'),
  ('fg_line',  '{"type":"LineString","coordinates":[[0,0],[1,1]]}'),
  ('fg_poly',  '{"type":"Polygon","coordinates":[[[0,0],[4,0],[4,4],[0,4],[0,0]]]}'),
  ('fg_mpoly', '{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,1],[0,0]]]]}'),
  ('fg_gc',    '{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1,2]}]}')
)
SELECT row_to_json(t)::text FROM (
  SELECT f.id || ':fromgeojson' AS id, f.a, 'fromgeojson' AS "fn",
    to_jsonb(ST_AsText(ST_GeomFromGeoJSON(f.a))) AS expected,
    ST_SRID(ST_GeomFromGeoJSON(f.a)) AS expected_srid
  FROM fg f ORDER BY f.id
) t;

-- Divergence + error vectors.
SELECT row_to_json(t)::text FROM (
  SELECT 'fg_3d:fromgeojson' AS id, '{"type":"Point","coordinates":[1,2,3]}' AS a,
         'fromgeojson' AS "fn",
         to_jsonb(ST_AsText(ST_GeomFromGeoJSON('{"type":"Point","coordinates":[1,2,3]}'))) AS expected,
         jsonb_build_object('error', true) AS kenro_expected,
         'PostGIS keeps Z; kenro 0.1 rejects 3D rather than silently dropping it' AS note
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'fg_feature:fromgeojson' AS id,
         '{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{}}' AS a,
         'fromgeojson' AS "fn", jsonb_build_object('error', true) AS expected,
         'PostGIS raises on Feature input; kenro raises too' AS note
) t;
