# Routing

> **Related:** [Function reference](functions.md) · [3D geometry](3d.md) ·
> [Scope and semantics](scope.md) · [Quickstart](quickstart.md) ·
> [WebAssembly hosts](wasm.md)

Shortest paths over a road, rail or utility network, in the same SQLite file
as the geometry. kenro's reference here is **pgRouting**, not PostGIS —
PostGIS has no routing at all — and the golden vectors in
`tests/golden/routing.jsonl` come from a real `pgrouting/pgrouting` container
(`scripts/golden/routing_generate.sh`).

| | |
|---|---|
| [The functions](#the-functions) | signatures, arities and what each returns |
| [Why the aggregate](#why-an-aggregate-and-not-a-table-valued-function) | the `WHERE` clause is pgRouting's SQL-string argument |
| [Semantics](#semantics) | direction, closed edges, NULL rows, constant arguments, i32 ids |
| [Getting rows out](#getting-rows-out-of-a-path) | the `json_each` recipe |
| [Building the edge table](#building-an-edge-table-the-pgr_createtopology-job) | a `pgr_createTopology` replacement in plain SQL |

Routing is behind the **`routing` cargo feature**, which is in `full` and not
in the default set. It is pure code with no new dependency; a default build
registers the names as stubs that say so.

---

## The functions

| Function | Returns | pgRouting counterpart |
|---|---|---|
| `kenro_dijkstra(id, source, target, cost, start_vid, end_vid [, reverse_cost])` | TEXT (JSON array) / NULL | `pgr_dijkstra` |
| `kenro_dijkstra_cost(source, target, cost, start_vid, end_vid [, reverse_cost])` | REAL / NULL | `pgr_dijkstraCost` |

Both are **aggregates**. Each input row is one edge; `start_vid` and
`end_vid` are constants repeated on every row (SQLite has no other way to
pass a scalar to an aggregate).

`kenro_dijkstra` returns the `pgr_dijkstra` row shape as a JSON array:

```json
[{"agg_cost":0.0,"cost":1.1,"edge":1,"node":1,"seq":1},
 {"agg_cost":1.1,"cost":0.7,"edge":2,"node":2,"seq":2},
 {"agg_cost":1.8,"cost":0.0,"edge":-1,"node":3,"seq":3}]
```

`node` is the vertex reached, `edge` the one leaving it — `-1` on the last
row — `cost` that edge's cost, and `agg_cost` the running total *on arrival*.
The last row's `agg_cost` is the total, which is exactly what
`kenro_dijkstra_cost` returns without materializing the path.

> ⚠️ **`reverse_cost` is the last argument**, where `pgr_dijkstra` has it as a
> column of the edge query between `cost` and the rest. Every kenro host
> treats *trailing* arguments as the optional ones — wasm-bindgen makes only
> trailing parameters optional, and the C ABI pads omitted trailing arguments
> with a presence flag — so an optional argument has to be last or it needs a
> second export per arity. `ST_AsMVT` diverges from PostGIS's signature for
> the same structural reason.

---

## Why an aggregate and not a table-valued function

pgRouting takes a SQL *string* and runs it to get its edges. kenro registers
scalar and aggregate functions only ([scope](scope.md#deliberately-out-of-scope)),
so the edge query is the query the aggregate is *in*, and its `WHERE` clause
is what the SQL string would have said:

```sql
-- pgRouting: pgr_dijkstra('SELECT id, source, target, cost FROM roads
--                          WHERE kind <> ''service''', 1, 42)
SELECT kenro_dijkstra(id, source, target, cost, 1, 42)
FROM   roads WHERE kind <> 'service';
```

This is not a compromise on memory: pgRouting also materializes every row its
query returns before it searches. And because it is an ordinary aggregate,
`GROUP BY` gives one route per group for free:

```sql
SELECT region, kenro_dijkstra_cost(source, target, cost, 1, 42)
FROM   roads GROUP BY region;
```

---

## Semantics

**Directed.** An edge row means `source → target` at `cost`. The graph is
always directed; there is no undirected mode. To make an edge two-way, give
it a `reverse_cost` — the cost of `target → source`, which may differ.

**A negative cost closes that direction.** `cost < 0` makes the edge
impassable `source → target`, `reverse_cost < 0` impassable the other way.
This is pgRouting's convention, and it is how a one-way street is spelled:

```sql
-- open forwards at 1.1, closed backwards
INSERT INTO roads VALUES (7, 1, 2, 1.1, -1);
```

**NULL rows are skipped.** If any argument of a row is NULL, the row does not
enter the graph — the same rule kenro's other aggregates follow, and the same
one PostGIS aggregates follow. An edge whose `cost` is unknown is simply not
an edge.

**Zero rows, no path, and a missing endpoint are all NULL.** pgRouting
returns the empty result set for each of them and makes no distinction; kenro
returns SQL NULL. That includes `start_vid = end_vid`, which is not a
zero-length path but an empty answer — pinned by golden vectors, not chosen.

**`start_vid` and `end_vid` must be constant within a group.** They are
scalar parameters wearing a column's clothes; a group where they change is an
error, not a silently-chosen winner. (`ST_AsMVT`'s layer name and extent work
the same way.)

**A non-finite cost is an error.** `NaN` and infinities have no shortest-path
meaning, so they are rejected at step time rather than producing a nonsense
total.

**Ids are 32-bit.** `id`, `source`, `target`, `start_vid` and `end_vid` are
all `INTEGER` in the i32 sense. kenro keeps 64-bit integers to the H3 family:
a 64-bit argument makes a function unregisterable on some wasm hosts, so the
whole catalog outside H3 stays inside i32. If your node ids come from
somewhere wider, renumber them with `DENSE_RANK()` — which is what the recipe
below does anyway.

**Ties are not arbitrated.** Where two paths cost exactly the same, kenro
picks one deterministically but not necessarily the one pgRouting picks. The
*cost* is the same either way, which is why the golden suite's tie fixture
only pins `kenro_dijkstra_cost`.

---

## Getting rows out of a path

The path is a JSON array, so `json_each` turns it into rows — the same
JSON1-based recipe as
[Getting N rows out](scope.md#getting-n-rows-out):

```sql
WITH p(j) AS (
  SELECT kenro_dijkstra(id, source, target, cost, 1, 42) FROM roads
)
SELECT json_extract(value, '$.seq')      AS seq,
       json_extract(value, '$.node')     AS node,
       json_extract(value, '$.edge')     AS edge,
       json_extract(value, '$.cost')     AS cost,
       json_extract(value, '$.agg_cost') AS agg_cost
FROM   p, json_each(p.j)
ORDER  BY seq;
```

Joining back to the geometry to draw the route:

```sql
WITH p(j) AS (
  SELECT kenro_dijkstra(id, source, target, cost, 1, 42) FROM roads
)
SELECT r.geom
FROM   p, json_each(p.j) e
JOIN   roads r ON r.id = json_extract(e.value, '$.edge')
ORDER  BY json_extract(e.value, '$.seq');
```

The terminal row's `edge` is `-1`, which matches no road, so the join drops it
on its own.

---

## Building an edge table: the `pgr_createTopology` job

pgRouting's `pgr_createTopology` walks a line table, gives every distinct
endpoint a vertex id and fills in `source`/`target`. That is plain SQL, and
every function it needs is in kenro:

```sql
-- 1. Every endpoint of every line, snapped to a tolerance so that
--    coordinates that "should" be the same actually are. Choose the grid in
--    the units of your data's CRS.
CREATE TEMP TABLE ends AS
SELECT id,
       'start' AS which,
       ST_AsBinary(ST_SnapToGrid(ST_StartPoint(geom), 0.001)) AS pt
FROM   roads
UNION ALL
SELECT id, 'end',
       ST_AsBinary(ST_SnapToGrid(ST_EndPoint(geom), 0.001))
FROM   roads;

-- 2. Number the distinct endpoints. The WKB blob is the identity: two
--    endpoints are the same vertex exactly when their snapped coordinates
--    are byte-identical.
CREATE TEMP TABLE vertices AS
SELECT pt, DENSE_RANK() OVER (ORDER BY pt) AS vid
FROM   (SELECT DISTINCT pt FROM ends);

-- 3. The edge table.
CREATE TABLE edges AS
SELECT r.id,
       vs.vid AS source,
       ve.vid AS target,
       ST_Length(r.geom) AS cost
FROM   roads r
JOIN   ends es ON es.id = r.id AND es.which = 'start'
JOIN   ends ee ON ee.id = r.id AND ee.which = 'end'
JOIN   vertices vs ON vs.pt = es.pt
JOIN   vertices ve ON ve.pt = ee.pt;
```

Use a projected CRS (`ST_Transform` first) if `cost` is meant to be metres —
`ST_Length` in degrees is not a distance. For a travel-time cost, divide by a
speed column instead.

> ⚠️ **This connects lines that share an endpoint, and nothing else.** Two
> roads that cross in the middle without a shared vertex stay unconnected,
> because splitting them there is *noding* — the one thing on this path that
> kenro [deliberately does not do](scope.md#deliberately-out-of-scope). If
> your data has unsplit crossings, node it upstream (PostGIS `ST_Node`,
> GDAL, osm2pgrouting) before loading. Data that already comes from a road
> network export is usually noded already.
