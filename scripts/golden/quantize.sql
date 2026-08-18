-- Golden vectors for ST_QuantizeCoordinates, compared **bit-exactly**.
--
-- The function zeroes low mantissa bits, so a tolerance-based comparison
-- would pass on an implementation that got the bit count wrong by one. Every
-- expectation is therefore the hex EWKB of the result, and the input is given
-- as hex EWKB too, so no text round trip can round a sentinel on its way in
-- or out.
--
-- `extra_float_digits` is irrelevant here for the same reason: nothing in
-- these vectors goes through PostGIS's float printer.
\pset tuples_only on
\pset format unaligned

-- One-dimensional sweep: the rule is per-ordinate, so a POINT carrying a
-- sentinel in x and 0 in y isolates it. The prec range covers well past the
-- clamps at both ends (bits_needed < 1 and >= 52).
WITH v(lbl, x) AS (VALUES
  ('one',        1.0::float8),
  ('half',       0.5),
  ('two',        2.0),
  ('onepointfive', 1.5),
  ('seven',      7.0),
  ('tenth',      0.1),
  ('neg_tenth',  -0.1),
  ('third',      0.3333333333333333),
  ('pi',         3.14159265358979),
  ('c123',       123.456789),
  ('c12345',     12345.6789),
  ('small',      0.000123456789),
  ('neg_big',    -98765.4321),
  ('negtwohalf', -2.5),
  ('e10',        1e10),
  ('e15',        1e15),
  ('e16',        1e16),
  ('e-5',        1e-5),
  ('e-10',       1e-10),
  ('e300',       1e300),
  ('e-300',      1e-300),
  ('avogadro',   6.02214076e23),
  ('nines',      999999999999999.0),
  ('nearone',    0.9999999999999999),
  ('neg_e-7',    -1e-7),
  ('zero',       0.0),
  ('neg_zero',   -0.0),
  ('max',        1.7976931348623157e308),
  ('min_normal', 2.2250738585072014e-308),
  ('min_subnormal', 4.9e-324),
  ('tiny_subnormal', 1e-320)
), p(prec) AS (SELECT generate_series(-30, 40))
SELECT row_to_json(t)::text FROM (
  SELECT v.lbl || ':' || p.prec AS id, 'quantize' AS "fn",
         ST_AsHexEWKB(ST_MakePoint(v.x, 0.0)) AS a,
         ARRAY[p.prec] AS args,
         to_jsonb(ST_AsHexEWKB(ST_QuantizeCoordinates(ST_MakePoint(v.x, 0.0), p.prec))) AS expected
  FROM v, p ORDER BY v.lbl, p.prec
) t;

-- Per-ordinate precision, including the fallback rule: a NULL prec_y or
-- prec_z falls back to **prec_x**, not to the argument before it. kenro
-- spells the fallbacks as arities, so `xy` is (prec_x, prec_y) with z taking
-- prec_x, and `xyz` names all three.
SELECT row_to_json(t)::text FROM (VALUES
  ('xy_differ',  'POINT(1.23456789 9.87654321)',                  ARRAY[2, 15]),
  ('xy_same',    'POINT(1.23456789 9.87654321)',                  ARRAY[3, 3]),
  ('xy_negprec', 'POINT(1234.5678 8765.4321)',                    ARRAY[-2, -1])
) AS e(id, wkt, args)
CROSS JOIN LATERAL (
  SELECT e.id || ':xy' AS id, 'quantize_xy' AS "fn",
         ST_AsHexEWKB(ST_GeomFromText(e.wkt)) AS a, e.args AS args,
         to_jsonb(ST_AsHexEWKB(ST_QuantizeCoordinates(
           ST_GeomFromText(e.wkt), e.args[1], e.args[2], e.args[1]))) AS expected
) t;

SELECT row_to_json(t)::text FROM (VALUES
  ('z_all',      'POINT Z(1.23456789 9.87654321 5.55555555)',     ARRAY[3, 3, 3]),
  ('z_only',     'POINT Z(1.23456789 9.87654321 5.55555555)',     ARRAY[15, 15, 2]),
  ('z_defaults', 'POINT Z(1.23456789 9.87654321 5.55555555)',     ARRAY[2, 15, 2])
) AS e(id, wkt, args)
CROSS JOIN LATERAL (
  SELECT e.id || ':xyz' AS id, 'quantize_xyz' AS "fn",
         ST_AsHexEWKB(ST_GeomFromText(e.wkt)) AS a, e.args AS args,
         to_jsonb(ST_AsHexEWKB(ST_QuantizeCoordinates(
           ST_GeomFromText(e.wkt), e.args[1], e.args[2], e.args[3]))) AS expected
) t;

-- Structure rides through: multi-parts, rings, empties, a SRID, and a
-- surface collection (which PostGIS quantizes too — it is nested WKB like
-- any other collection).
SELECT row_to_json(t)::text FROM (VALUES
  ('line',       'LINESTRING(3.14159 2.71828,1.41421 1.73205)',                                  3),
  ('poly_hole',  'POLYGON((0 0,4.123456 0,4.123456 4.123456,0 4.123456,0 0),(1.1111111 1.1111111,2.2222222 1.1111111,2.2222222 2.2222222,1.1111111 1.1111111))', 4),
  ('multipoint', 'MULTIPOINT(1.23456789 2.3456789,3.456789 4.56789)',                            2),
  ('gc',         'GEOMETRYCOLLECTION(POINT(1.23456789 2.3456789),LINESTRING(3.14159 2.71828,1.41421 1.73205))', 3),
  ('empty',      'POLYGON EMPTY',                                                                3),
  ('point_empty','POINT EMPTY',                                                                  3),
  ('surface',    'POLYHEDRALSURFACE Z(((0 0 0,0 1.23456789 0,1.23456789 1.23456789 0,0 0 0)))',  3),
  ('srid',       'SRID=4326;POINT(139.76543210 35.68123456)',                                    5)
) AS e(id, wkt, prec)
CROSS JOIN LATERAL (
  SELECT e.id || ':structure' AS id, 'quantize' AS "fn",
         ST_AsHexEWKB(ST_GeomFromEWKT(e.wkt)) AS a, ARRAY[e.prec] AS args,
         to_jsonb(ST_AsHexEWKB(ST_QuantizeCoordinates(ST_GeomFromEWKT(e.wkt), e.prec))) AS expected
) t;

-- M: PostGIS quantizes it (prec_m defaults to prec_x); kenro refuses the
-- geometry rather than returning one whose M is silently un-quantized.
SELECT row_to_json(t)::text FROM (
  SELECT 'has_m:refused' AS id, 'quantize' AS "fn",
         ST_AsHexEWKB(ST_GeomFromEWKT('POINT M(1.23456789 9.87654321 5.55555555)')) AS a,
         ARRAY[2] AS args,
         to_jsonb(ST_AsHexEWKB(ST_QuantizeCoordinates(
           ST_GeomFromEWKT('POINT M(1.23456789 9.87654321 5.55555555)'), 2))) AS expected,
         jsonb_build_object('error', true) AS kenro_expected,
         'PostGIS quantizes the M at prec_x; kenro has no M slot in its coordinate walker and raises rather than leaving it un-quantized' AS note
) t;
