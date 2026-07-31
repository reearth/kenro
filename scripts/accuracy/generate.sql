-- Reference lattice for the transform-accuracy measurement: points across
-- Japan transformed by PostGIS/PROJ, consumed by examples/accuracy_report.rs.
-- Output committed as scripts/accuracy/reference.jsonl.
\pset tuples_only on
\pset format unaligned

-- Wide geographic lattice: whole Japan region, 1-degree step.
WITH lattice AS (
  SELECT lon, lat
  FROM generate_series(122.0, 146.0, 1.0) AS lon,
       generate_series(24.0, 46.0, 1.0) AS lat
),
pairs(pair, src, dst) AS (VALUES
  ('4326->3857',      4326, 3857),
  ('4326->6677_wide', 4326, 6677),
  ('4612->4326',      4612, 4326),
  ('6668->4326',      6668, 4326),
  ('4612->6668',      4612, 6668)
)
SELECT row_to_json(t)::text FROM (
  SELECT p.pair, l.lon AS x, l.lat AS y,
         ST_X(ST_Transform(ST_SetSRID(ST_MakePoint(l.lon, l.lat), p.src), p.dst)) AS ex,
         ST_Y(ST_Transform(ST_SetSRID(ST_MakePoint(l.lon, l.lat), p.src), p.dst)) AS ey
  FROM pairs p CROSS JOIN lattice l
  ORDER BY p.pair, l.lon, l.lat
) t;

-- Zone IX (EPSG 6677) restricted to its official extent, finer step.
SELECT row_to_json(t)::text FROM (
  SELECT '4326->6677' AS pair, lon AS x, lat AS y,
         ST_X(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 6677)) AS ex,
         ST_Y(ST_Transform(ST_SetSRID(ST_MakePoint(lon, lat), 4326), 6677)) AS ey
  FROM generate_series(138.5, 141.0, 0.25) AS lon,
       generate_series(29.0, 38.0, 0.5) AS lat
  ORDER BY lon, lat
) t;

-- Inverse: zone IX projected lattice back to WGS84.
SELECT row_to_json(t)::text FROM (
  SELECT '2451->4326' AS pair, px AS x, py AS y,
         ST_X(ST_Transform(ST_SetSRID(ST_MakePoint(px, py), 2451), 4326)) AS ex,
         ST_Y(ST_Transform(ST_SetSRID(ST_MakePoint(px, py), 2451), 4326)) AS ey
  FROM generate_series(-160000.0, 160000.0, 40000.0) AS px,
       generate_series(-800000.0, 200000.0, 100000.0) AS py
  ORDER BY px, py
) t;
