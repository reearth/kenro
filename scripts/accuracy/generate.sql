-- Reference lattice for the transform-accuracy measurement: global point
-- lattices transformed by PostGIS/PROJ, consumed by
-- examples/accuracy_report.rs. Output committed as
-- scripts/accuracy/reference.jsonl.
\pset tuples_only on
\pset format unaligned

-- Worldwide geographic lattice (Web Mercator's usable band), 4-degree step.
SELECT row_to_json(t)::text FROM (
  SELECT '4326->3857' AS pair, lon AS x, lat AS y,
         ST_X(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 3857)) AS ex,
         ST_Y(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 3857)) AS ey
  FROM generate_series(-176.0, 176.0, 4.0) AS lon,
       generate_series(-84.0, 84.0, 4.0) AS lat
  ORDER BY lon, lat
) t;

-- UTM 33N (central Europe) restricted to its zone, fine step.
SELECT row_to_json(t)::text FROM (
  SELECT '4326->32633' AS pair, lon AS x, lat AS y,
         ST_X(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 32633)) AS ex,
         ST_Y(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 32633)) AS ey
  FROM generate_series(12.0, 18.0, 0.5) AS lon,
       generate_series(36.0, 70.0, 2.0) AS lat
  ORDER BY lon, lat
) t;

-- UTM 33N far out-of-zone (the wide-usage regime web maps hit).
SELECT row_to_json(t)::text FROM (
  SELECT '4326->32633_wide' AS pair, lon AS x, lat AS y,
         ST_X(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 32633)) AS ex,
         ST_Y(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 32633)) AS ey
  FROM generate_series(0.0, 30.0, 2.0) AS lon,
       generate_series(30.0, 72.0, 2.0) AS lat
  ORDER BY lon, lat
) t;

-- UTM 56S (southern hemisphere: the false-northing path).
SELECT row_to_json(t)::text FROM (
  SELECT '4326->32756' AS pair, lon AS x, lat AS y,
         ST_X(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 32756)) AS ex,
         ST_Y(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 32756)) AS ey
  FROM generate_series(150.0, 156.0, 0.5) AS lon,
       generate_series(-44.0, -10.0, 2.0) AS lat
  ORDER BY lon, lat
) t;

-- Inverse: UTM 33N projected lattice back to WGS84.
SELECT row_to_json(t)::text FROM (
  SELECT '32633->4326' AS pair, px AS x, py AS y,
         ST_X(ST_Transform(ST_SetSRID(ST_MakePoint(px, py), 32633), 4326)) AS ex,
         ST_Y(ST_Transform(ST_SetSRID(ST_MakePoint(px, py), 32633), 4326)) AS ey
  FROM generate_series(200000.0, 800000.0, 100000.0) AS px,
       generate_series(4000000.0, 7800000.0, 400000.0) AS py
  ORDER BY px, py
) t;
