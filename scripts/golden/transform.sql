-- Golden vectors for ST_Transform, computed by the reference PostGIS.
-- The harness compares kenro's result against `expected` with a metric
-- tolerance (see tests/golden_transform.rs and docs/accuracy.md).
\pset tuples_only on
\pset format unaligned

WITH pts(id, a, src, dst) AS (VALUES
  ('tokyo_tower_zone9',   'POINT(139.745433 35.658581)', 4326, 6677),
  ('tokyo_tower_3857',    'POINT(139.745433 35.658581)', 4326, 3857),
  ('tokyo_tower_utm54',   'POINT(139.745433 35.658581)', 4326, 32654),
  ('sapporo_zone12',      'POINT(141.354376 43.062096)', 4326, 6680),
  ('naha_zone15',         'POINT(127.679245 26.212401)', 4326, 6683),
  ('osaka_zone6',         'POINT(135.502165 34.693738)', 4326, 6674),
  ('tokyo_jgd2000_zone9', 'POINT(139.745433 35.658581)', 4612, 2451),
  ('jgd2000_to_wgs84',    'POINT(139.745433 35.658581)', 4612, 4326),
  ('jgd2011_to_wgs84',    'POINT(139.745433 35.658581)', 6668, 4326),
  ('wgs84_to_jgd2011',    'POINT(139.745433 35.658581)', 4326, 6668),
  ('zone9_inverse',       'POINT(-7000 -35000)',         6677, 4326),
  ('mercator_inverse',    'POINT(15556200 4257415)',     3857, 4326),
  ('line_zone9',          'LINESTRING(139.7 35.6,139.8 35.7)', 4326, 6677),
  ('polygon_3857',        'POLYGON((139.7 35.6,139.8 35.6,139.8 35.7,139.7 35.6))', 4326, 3857)
)
SELECT row_to_json(t)::text FROM (
  SELECT p.id || ':transform' AS id, p.a, p.src AS src_srid, p.dst AS to_srid,
         'transform' AS "fn",
         ST_AsText(ST_Transform(ST_SetSRID(ST_GeomFromText(p.a), p.src), p.dst)) AS expected
  FROM pts p ORDER BY p.id
) t;

-- Error-expectation vectors (not computable by evaluation).
SELECT row_to_json(t)::text FROM (
  SELECT 'srid0:transform' AS id, 'POINT(1 2)' AS a, 0 AS src_srid, 4326 AS to_srid,
         'transform' AS "fn", jsonb_build_object('error', true) AS expected,
         'PostGIS raises "Input geometry has unknown (0) SRID"; kenro raises too' AS note
) t;
SELECT row_to_json(t)::text FROM (
  SELECT 'unknown_epsg:transform' AS id, 'POINT(1 2)' AS a, 4326 AS src_srid, 27700 AS to_srid,
         'transform' AS "fn", jsonb_build_object('error', true) AS expected,
         'kenro-only: EPSG:27700 is outside the curated CRS table (PostGIS supports it); documented limitation' AS note
) t;
