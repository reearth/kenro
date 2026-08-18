-- Golden vectors for the box accessors' TEXT argument: PostGIS's
-- ST_XMin/ST_XMax/ST_YMin/ST_YMax/ST_ZMin/ST_ZMax applied to a `BOX3D(…)`
-- string literal, which is how their one and only overload (`box3d`) is
-- reached without a cast.
--
-- Only the spellings PostGIS itself accepts appear here — the suite's job is
-- to pin agreement with the reference. kenro's deliberate leniencies (case,
-- surrounding whitespace, the `BOX(…)` spelling) and its deliberate
-- strictnesses (trailing junk, missing close paren) are PostGIS deviations
-- by definition, so they live in `src/functions/box3d.rs`'s unit tests with
-- the measured PostGIS behaviour recorded beside them.
\pset tuples_only on
\pset format unaligned

WITH b(id, a) AS (VALUES
  ('box3d_3d',        'BOX3D(1 2 3,4 5 6)'),
  ('box3d_2d',        'BOX3D(1 2,4 5)'),
  ('box3d_swapped',   'BOX3D(4 5 6,1 2 3)'),
  ('box3d_negative',  'BOX3D(-98765.4321 -2.5 -3,4 5.5 6)'),
  ('box3d_degenerate','BOX3D(1 2 3,1 2 3)'),
  ('box3d_sci',       'BOX3D(1e2 2 3,4 5 6)'),
  ('box3d_plus',      'BOX3D(+1 2 3,4 5 6)'),
  ('box3d_leadingdot','BOX3D(.5 2 3,4 5 6)'),
  ('box3d_zeros',     'BOX3D(0 0 0,0 0 0)'),
  ('box3d_bigsmall',  'BOX3D(1e-9 2 3,1e9 5 6)'),
  ('box3d_wide_z',    'BOX3D(0 0 -1000.25,10 10 3500.75)'),
  ('box3d_inner_ws',  'BOX3D( 1 2 3 , 4 5 6 )'),
  ('box3d_trail_ws',  'BOX3D(1 2 3,4 5 6)   '),
  -- What ST_3DExtent renders: the round trip a caller actually writes.
  ('box3d_from_extent',
   (SELECT ST_3DExtent(g)::text FROM (VALUES ('LINESTRING Z(1 2 3,7 8 9)'::geometry)) v(g)))
), ops(f) AS (VALUES ('xmin'),('xmax'),('ymin'),('ymax'),('zmin'),('zmax'))
SELECT row_to_json(t)::text FROM (
  SELECT b.id || ':' || ops.f AS id, b.a, ops.f AS "fn",
    CASE ops.f
      WHEN 'xmin' THEN to_jsonb(ST_XMin(b.a::box3d))
      WHEN 'xmax' THEN to_jsonb(ST_XMax(b.a::box3d))
      WHEN 'ymin' THEN to_jsonb(ST_YMin(b.a::box3d))
      WHEN 'ymax' THEN to_jsonb(ST_YMax(b.a::box3d))
      WHEN 'zmin' THEN to_jsonb(ST_ZMin(b.a::box3d))
      WHEN 'zmax' THEN to_jsonb(ST_ZMax(b.a::box3d))
    END AS expected
  FROM b, ops ORDER BY b.id, ops.f
) t;

-- Divergence and error vectors. `expected` is what the reference does (each
-- one measured against this image before it was written down); where kenro
-- deliberately differs, `kenro_expected` carries kenro's answer and the note
-- says why. Both directions of deviation appear, and both come from the same
-- fact: PostGIS's `box3d_in` is a pair of `sscanf` calls, so it neither
-- case-folds nor anchors the tail.
SELECT row_to_json(t)::text FROM (VALUES
  -- kenro is more lenient: the spelling PostGIS's own ST_Extent renders.
  ('box_2d_spelling:xmin', 'BOX(1 2,4 5)', 'xmin',
   jsonb_build_object('error', true), to_jsonb(1.0),
   'PostGIS: "BOX3D parser - doesn''t start with BOX3D(". kenro accepts it: PostGIS renders ST_Extent as BOX(…) and SQLite has no cast to route the string through'),
  ('lowercase:xmin', 'box3d(1 2 3,4 5 6)', 'xmin',
   jsonb_build_object('error', true), to_jsonb(1.0),
   'PostGIS is case-sensitive (sscanf); kenro case-folds the keyword'),
  ('leading_ws:xmin', '   BOX3D(1 2 3,4 5 6)', 'xmin',
   jsonb_build_object('error', true), to_jsonb(1.0),
   'PostGIS rejects leading whitespace; kenro trims it'),
  ('space_before_paren:xmin', 'BOX3D (1 2 3,4 5 6)', 'xmin',
   jsonb_build_object('error', true), to_jsonb(1.0),
   'PostGIS rejects a space before the paren; kenro allows it'),
  -- kenro is stricter: sscanf accidents, not contracts.
  ('junk_tail:xmin', 'BOX3D(1 2 3,4 5 6)junk', 'xmin',
   to_jsonb(1.0), jsonb_build_object('error', true),
   'PostGIS accepts and ignores the tail (sscanf does not anchor); kenro rejects it rather than silently reading a truncated string as a box'),
  ('no_close_paren:xmin', 'BOX3D(1 2 3,4 5 6', 'xmin',
   to_jsonb(1.0), jsonb_build_object('error', true),
   'PostGIS accepts a missing close paren (sscanf); kenro rejects it'),
  ('corner_2d_then_3d:zmax', 'BOX3D(1 2,4 5 6)', 'zmax',
   to_jsonb(0.0), jsonb_build_object('error', true),
   'PostGIS falls back to the 2D scan and drops the 6, answering zmax 0; kenro rejects the mismatched corners'),
  -- Rejected by both.
  ('corner_3d_then_2d:xmin', 'BOX3D(1 2 3,4 5)', 'xmin',
   jsonb_build_object('error', true), NULL,
   'PostGIS: "BOX3D parser - couldn''t parse"; kenro raises too'),
  ('one_corner:xmin', 'BOX3D(1 2 3)', 'xmin',
   jsonb_build_object('error', true), NULL,
   'PostGIS: "BOX3D parser - couldn''t parse"; kenro raises too'),
  ('too_many_ords:xmin', 'BOX3D(1 2 3 9,4 5 6 9)', 'xmin',
   jsonb_build_object('error', true), NULL,
   'PostGIS: "BOX3D parser - couldn''t parse"; kenro raises too'),
  ('empty_parens:xmin', 'BOX3D()', 'xmin',
   jsonb_build_object('error', true), NULL,
   'PostGIS: "BOX3D parser - couldn''t parse"; kenro raises too'),
  ('box3d_empty_kw:xmin', 'BOX3D EMPTY', 'xmin',
   jsonb_build_object('error', true), NULL,
   'PostGIS: "doesn''t start with BOX3D("; kenro raises too'),
  ('not_a_box:xmin', 'POINT(1 2)', 'xmin',
   jsonb_build_object('error', true), NULL,
   'PostGIS: "doesn''t start with BOX3D("; kenro raises too — and the error names ST_GeomFromText, which is what this caller wanted'),
  ('not_a_number:xmin', 'BOX3D(a b c,d e f)', 'xmin',
   jsonb_build_object('error', true), NULL,
   'PostGIS: "BOX3D parser - couldn''t parse"; kenro raises too')
) AS t(id, a, "fn", expected, kenro_expected, note);
