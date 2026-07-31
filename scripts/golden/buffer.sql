-- Golden vectors for ST_Buffer. Arc tessellation genuinely differs from
-- GEOS, so round-style results use `mode: "buffer"` (symmetric-difference
-- area ratio bound); degenerate/empty cases are exact.
\pset tuples_only on
\pset format unaligned

WITH b(id, a, dist, opts) AS (VALUES
  ('pt_round',      'POINT(0 0)',                          1.0,  NULL),
  ('pt_big',        'POINT(5 5)',                          10.0, NULL),
  ('line_round',    'LINESTRING(0 0,10 0)',                0.5,  NULL),
  ('line_flat',     'LINESTRING(0 0,10 0)',                0.5,  'endcap=flat'),
  ('line_square',   'LINESTRING(0 0,10 0)',                0.5,  'endcap=square'),
  ('poly_expand',   'POLYGON((0 0,10 0,10 10,0 10,0 0))',  1.0,  NULL),
  ('poly_mitre',    'POLYGON((0 0,10 0,10 10,0 10,0 0))',  1.0,  'join=mitre mitre_limit=5'),
  ('poly_bevel',    'POLYGON((0 0,10 0,10 10,0 10,0 0))',  1.0,  'join=bevel'),
  ('poly_erode',    'POLYGON((0 0,10 0,10 10,0 10,0 0))',  -1.0, NULL),
  ('quad_segs_2',   'POINT(0 0)',                          1.0,  'quad_segs=2')
)
SELECT row_to_json(t)::text FROM (
  SELECT b.id || ':buffer' AS id, b.a, b.dist AS arg, b.opts AS arg_text,
         'buffer' AS "fn", 'buffer' AS mode,
    CASE WHEN b.opts IS NULL
      THEN to_jsonb(ST_AsText(ST_Buffer(ST_GeomFromText(b.a), b.dist)))
      ELSE to_jsonb(ST_AsText(ST_Buffer(ST_GeomFromText(b.a), b.dist, b.opts)))
    END AS expected
  FROM b ORDER BY b.id
) t;

-- Exact cases: full erosion and negative non-areal buffers empty out.
WITH e(id, a, dist) AS (VALUES
  ('erode_all',   'POLYGON((0 0,10 0,10 10,0 10,0 0))', -100.0),
  ('pt_negative', 'POINT(0 0)',                         -1.0),
  ('line_negative', 'LINESTRING(0 0,10 0)',             -0.5)
)
SELECT row_to_json(t)::text FROM (
  SELECT e.id || ':buffer' AS id, e.a, e.dist AS arg, 'buffer' AS "fn", 'exact' AS mode,
         to_jsonb(ST_AsText(ST_Buffer(ST_GeomFromText(e.a), e.dist))) AS expected
  FROM e ORDER BY e.id
) t;

-- kenro-only error: side= option unsupported.
SELECT row_to_json(t)::text FROM (
  SELECT 'side_opt:buffer' AS id, 'LINESTRING(0 0,10 0)' AS a, 0.5 AS arg,
         'side=left' AS arg_text, 'buffer' AS "fn", 'exact' AS mode,
         to_jsonb(ST_AsText(ST_Buffer(ST_GeomFromText('LINESTRING(0 0,10 0)'), 0.5, 'side=left'))) AS expected,
         jsonb_build_object('error', true) AS kenro_expected,
         'kenro does not support side= buffers' AS note
) t;
