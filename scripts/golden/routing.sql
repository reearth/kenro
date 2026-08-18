-- Golden vector generator for the routing aggregates, run against real
-- pgRouting via scripts/golden/routing_generate.sh. Output goes to
-- tests/golden/routing.jsonl (committed — CI never runs this).
--
-- Each vector is self-contained: `rows` carries the whole edge table, so the
-- Rust harness rebuilds the graph without a fixture registry. A row of four
-- elements is the 6-argument call form, five elements the 7-argument one
-- (kenro puts reverse_cost last — see docs/routing.md).
--
-- Costs are deliberately irregular (1.1, 0.7, 2.9, …) so that no fixture
-- except `tie` has two shortest paths of equal cost: pgRouting picks one of
-- them by internal order, and kenro is under no obligation to pick the same
-- one. `tie` therefore only ever emits dijkstra_cost vectors, where the
-- answer is unique whatever path produced it.
\pset tuples_only on
\pset format unaligned

CREATE TABLE fx(fixture text, id int, source int, target int, cost float8, reverse_cost float8);

-- A straight chain, one-way. The simplest possible path.
INSERT INTO fx VALUES
  ('chain', 1, 1, 2, 1.1, NULL),
  ('chain', 2, 2, 3, 0.7, NULL),
  ('chain', 3, 3, 4, 2.9, NULL),
  ('chain', 4, 4, 5, 1.3, NULL);

-- A 3x3 grid, every edge two-way with a different cost each way, so the
-- route out and the route home differ.
--   1 2 3
--   4 5 6
--   7 8 9
INSERT INTO fx VALUES
  ('grid9',  1, 1, 2, 1.1, 2.3),
  ('grid9',  2, 2, 3, 0.7, 1.9),
  ('grid9',  3, 4, 5, 2.9, 0.6),
  ('grid9',  4, 5, 6, 1.3, 3.1),
  ('grid9',  5, 7, 8, 0.9, 1.7),
  ('grid9',  6, 8, 9, 2.1, 0.8),
  ('grid9',  7, 1, 4, 1.7, 0.4),
  ('grid9',  8, 4, 7, 3.3, 1.2),
  ('grid9',  9, 2, 5, 0.3, 2.7),
  ('grid9', 10, 5, 8, 1.9, 0.5),
  ('grid9', 11, 3, 6, 2.6, 1.4),
  ('grid9', 12, 6, 9, 0.2, 3.7);

-- One-way streets spelled the pgRouting way: a negative cost closes the
-- forward direction, a negative reverse_cost closes the backward one.
INSERT INTO fx VALUES
  ('oneway', 1, 1, 2,  1.1, -1),
  ('oneway', 2, 2, 3,  0.7, -1),
  ('oneway', 3, 3, 4, -1,    0.9),
  ('oneway', 4, 1, 4,  6.5,  6.5),
  ('oneway', 5, 4, 5,  1.3, -1);

-- Two components that never touch.
INSERT INTO fx VALUES
  ('disconnected', 1, 1, 2, 1.1, NULL),
  ('disconnected', 2, 2, 3, 0.7, NULL),
  ('disconnected', 3, 8, 9, 2.9, NULL);

-- Parallel edges between the same pair: only the cheap one may be used.
INSERT INTO fx VALUES
  ('parallel', 1, 1, 2, 9.4, NULL),
  ('parallel', 2, 1, 2, 2.2, NULL),
  ('parallel', 3, 2, 3, 0.7, NULL);

-- A self loop, which can never shorten anything, plus a real path past it.
INSERT INTO fx VALUES
  ('self_loop', 1, 2, 2, 0.5, NULL),
  ('self_loop', 2, 1, 2, 1.1, NULL),
  ('self_loop', 3, 2, 3, 0.7, NULL);

-- Two routes of exactly equal cost. Cost vectors only, by the note above.
INSERT INTO fx VALUES
  ('tie', 1, 1, 2, 1.0, NULL),
  ('tie', 2, 2, 4, 1.0, NULL),
  ('tie', 3, 1, 3, 1.0, NULL),
  ('tie', 4, 3, 4, 1.0, NULL);

CREATE TABLE cases(id text, fixture text, has_rev bool, start_vid bigint, end_vid bigint, paths bool);
INSERT INTO cases VALUES
  ('chain_1_4',        'chain',        false, 1,  4,  true),
  ('chain_1_5',        'chain',        false, 1,  5,  true),
  ('chain_2_4',        'chain',        false, 2,  4,  true),
  ('chain_backwards',  'chain',        false, 4,  1,  true),
  ('chain_start_eq_end','chain',       false, 3,  3,  true),
  ('chain_missing_end','chain',        false, 1,  99, true),
  ('chain_missing_start','chain',      false, 99, 4,  true),
  ('grid9_corner',     'grid9',        true,  1,  9,  true),
  ('grid9_home',       'grid9',        true,  9,  1,  true),
  ('grid9_across',     'grid9',        true,  3,  7,  true),
  ('grid9_short',      'grid9',        true,  2,  5,  true),
  ('grid9_diag',       'grid9',        true,  4,  6,  true),
  ('grid9_start_eq_end','grid9',       true,  5,  5,  true),
  ('grid9_missing',    'grid9',        true,  1,  42, true),
  ('oneway_forward',   'oneway',       true,  1,  3,  true),
  ('oneway_detour',    'oneway',       true,  1,  5,  true),
  ('oneway_blocked',   'oneway',       true,  5,  1,  true),
  ('oneway_back_edge', 'oneway',       true,  4,  3,  true),
  ('disc_within',      'disconnected', false, 1,  3,  true),
  ('disc_across',      'disconnected', false, 1,  9,  true),
  ('parallel_cheap',   'parallel',     false, 1,  2,  true),
  ('parallel_through', 'parallel',     false, 1,  3,  true),
  ('self_loop_past',   'self_loop',    false, 1,  3,  true),
  ('self_loop_at',     'self_loop',    false, 2,  3,  true),
  ('tie_two_ways',     'tie',          false, 1,  4,  false),
  ('tie_one_way',      'tie',          false, 1,  2,  false);

-- The edge table for one fixture, as the aggregate's input rows.
CREATE FUNCTION fx_rows(f text, rev bool) RETURNS jsonb LANGUAGE sql STABLE AS $$
  SELECT jsonb_agg(
           CASE WHEN rev
                THEN jsonb_build_array(id, source, target, cost, reverse_cost)
                ELSE jsonb_build_array(id, source, target, cost) END
           ORDER BY id)
  FROM fx WHERE fixture = f
$$;

-- The same edge table as the SQL string pgRouting wants.
CREATE FUNCTION fx_sql(f text, rev bool) RETURNS text LANGUAGE sql STABLE AS $$
  SELECT format('SELECT id, source, target, cost%s FROM fx WHERE fixture = %L ORDER BY id',
                CASE WHEN rev THEN ', reverse_cost' ELSE '' END, f)
$$;

-- pgr_dijkstra: the full path. NULL (an empty result set) covers no path,
-- start = end, and an endpoint that is not in the graph — pgRouting makes no
-- distinction between them, and neither does kenro.
SELECT row_to_json(t)::text FROM (
  SELECT c.id || ':dijkstra' AS id, 'dijkstra' AS "fn", c.fixture AS mode,
    jsonb_build_array(c.start_vid, c.end_vid) AS args,
    fx_rows(c.fixture, c.has_rev) AS rows,
    (SELECT jsonb_agg(jsonb_build_object(
              'seq', d.path_seq, 'node', d.node, 'edge', d.edge,
              'cost', d.cost, 'agg_cost', d.agg_cost) ORDER BY d.path_seq)
     FROM pgr_dijkstra(fx_sql(c.fixture, c.has_rev), c.start_vid, c.end_vid, true) d
    ) AS expected
  FROM cases c WHERE c.paths ORDER BY c.id
) t;

-- pgr_drivingDistance: every node within `limit` of `start_vid`. Its own
-- `seq` order is a traversal order of the shortest-path tree, not a contract
-- — the Rust harness compares the rows as a set keyed by node — so the
-- vectors are emitted sorted by node to keep the file diffable.
--
-- `start_vid` here is the args[0] the other suites use, and args[1] is the
-- limit rather than an end vertex.
CREATE TABLE dd_cases(id text, fixture text, has_rev bool, start_vid bigint, lim float8);
INSERT INTO dd_cases VALUES
  ('dd_chain_all',      'chain',        false, 1,  10),
  ('dd_chain_cut',      'chain',        false, 1,  1.8),
  ('dd_chain_just_shy', 'chain',        false, 1,  1.79),
  ('dd_chain_zero',     'chain',        false, 2,  0),
  ('dd_chain_negative', 'chain',        false, 1,  -1),
  ('dd_chain_missing',  'chain',        false, 99, 5),
  ('dd_grid9_wide',     'grid9',        true,  1,  4),
  ('dd_grid9_tight',    'grid9',        true,  5,  2.5),
  ('dd_grid9_corner',   'grid9',        true,  9,  3),
  ('dd_oneway',         'oneway',       true,  1,  8),
  ('dd_disconnected',   'disconnected', false, 1,  100),
  ('dd_parallel',       'parallel',     false, 1,  3),
  ('dd_self_loop',      'self_loop',    false, 1,  2);

SELECT row_to_json(t)::text FROM (
  SELECT c.id || ':drivingdistance' AS id, 'drivingdistance' AS "fn", c.fixture AS mode,
    jsonb_build_array(c.start_vid, c.lim) AS args,
    fx_rows(c.fixture, c.has_rev) AS rows,
    (SELECT jsonb_agg(jsonb_build_object(
              'depth', d.depth, 'pred', d.pred, 'node', d.node, 'edge', d.edge,
              'cost', d.cost, 'agg_cost', d.agg_cost) ORDER BY d.node)
     FROM pgr_drivingDistance(fx_sql(c.fixture, c.has_rev), c.start_vid, c.lim, true) d
    ) AS expected
  FROM dd_cases c ORDER BY c.id
) t;

-- pgr_dijkstraCost: the total only, which is well defined even where the
-- path is not.
SELECT row_to_json(t)::text FROM (
  SELECT c.id || ':dijkstra_cost' AS id, 'dijkstra_cost' AS "fn", c.fixture AS mode,
    jsonb_build_array(c.start_vid, c.end_vid) AS args,
    fx_rows(c.fixture, c.has_rev) AS rows,
    (SELECT to_jsonb(d.agg_cost)
     FROM pgr_dijkstraCost(fx_sql(c.fixture, c.has_rev), c.start_vid, c.end_vid, true) d
    ) AS expected
  FROM cases c ORDER BY c.id
) t;
