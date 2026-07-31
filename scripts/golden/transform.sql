-- Golden vectors for ST_Transform, computed by the reference PostGIS.
-- The harness compares kenro's result against `expected` with a metric
-- tolerance (see tests/golden_transform.rs and docs/accuracy.md).
-- Coverage is deliberately global: both hemispheres, several UTM zones,
-- Web Mercator, and inverse directions.
\pset tuples_only on
\pset format unaligned

WITH pts(id, a, src, dst) AS (VALUES
  ('tokyo_3857',        'POINT(139.745433 35.658581)',  4326, 3857),
  ('tokyo_utm54',       'POINT(139.745433 35.658581)',  4326, 32654),
  ('berlin_utm33',      'POINT(13.377704 52.516275)',   4326, 32633),
  ('newyork_utm18',     'POINT(-74.044502 40.689247)',  4326, 32618),
  ('sydney_utm56s',     'POINT(151.215297 -33.856784)', 4326, 32756),
  ('nairobi_utm37s',    'POINT(36.821946 -1.292066)',   4326, 32737),
  ('quito_equator',     'POINT(-78.467834 -0.180653)',  4326, 32717),
  ('london_3857',       'POINT(-0.127647 51.503459)',   4326, 3857),
  ('utm54_inverse',     'POINT(385000 3946000)',        32654, 4326),
  ('utm56s_inverse',    'POINT(334000 6252000)',        32756, 4326),
  ('mercator_inverse',  'POINT(15556200 4257415)',      3857, 4326),
  ('mercator_to_utm',   'POINT(15556200 4257415)',      3857, 32654),
  ('line_utm33',        'LINESTRING(13.3 52.5,13.5 52.6)', 4326, 32633),
  ('polygon_3857',      'POLYGON((139.7 35.6,139.8 35.6,139.8 35.7,139.7 35.6))', 4326, 3857)
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
         'kenro-only: EPSG:27700 is outside the built-in CRS table (PostGIS supports it); use the crs-full feature' AS note
) t;
