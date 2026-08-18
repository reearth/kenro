# Routing benchmark on OpenStreetMap data

What kenro's routing aggregates ([docs/routing.md](../../../docs/routing.md))
cost on a real road network, and what the documented `WHERE`-clause prefilter
buys — and loses.

The design trade being measured: **every query re-accumulates all the edge
rows it is fed.** `kenro_dijkstra` is an aggregate, so a full-table query is
O(E) whether the route is two blocks or two hundred kilometers. pgRouting
materializes its edge query the same way; the difference is that a SQLite
query has no plan-level shortcut around it either. The mitigation is to feed
the aggregate fewer rows with a `WHERE` clause, which is what variant (c)
below does with a bounding box — and a route that leaves that box is a route
the aggregate can no longer find, so the bench counts the wrong answers too.

Regions here are **arguments, not defaults**. The three scales below are
whatever Geofabrik extracts happen to land at those edge counts; run any
other path you like.

## Prerequisites

- `curl`, `python3`
- `pip install osmium` (pyosmium — the PBF reader; a virtualenv is fine)
- Docker, **only** for `--vs-pgrouting`

## Workflow

```sh
cd scripts/bench/routing

./fetch.sh europe/monaco                       # -> data/monaco-latest.osm.pbf
python3 prepare.py data/monaco-latest.osm.pbf  # -> data/monaco.{edges,nodes}.csv

cd ../../..
cargo run --release --features rusqlite,routing --example routing_bench -- \
    scripts/bench/routing/data/monaco.edges.csv \
    scripts/bench/routing/data/monaco.nodes.csv
```

The example prints a self-contained markdown report. Flags:

| flag | default | |
|---|---|---|
| `--pairs N` | 100 | random origin/destination pairs |
| `--seed S` | `0x2545F4914F6CDD1D` | the xorshift seed; the same seed picks the same pairs |
| `--bbox-margin-km K` | 5 | how far outside the endpoints' envelope variant (c) still feeds edges |
| `--vs-pgrouting` | off | run the same pairs through a real pgRouting container and compare |

`data/` and every generated CSV are gitignored; nothing here is committed
except the three scripts.

### The three scales

| scale | Geofabrik path | .pbf | edges | vertices |
|---|---|---|---|---|
| small | `europe/monaco` | 0.7 MB | 1,635 | 1,352 |
| medium | `europe/malta` | 8.9 MB | 44,383 | 34,750 |
| large | `europe/portugal` | 420 MB | 1,528,614 | 1,216,501 |

Extract size is a poor predictor of edge count (road density varies by an
order of magnitude — Monaco yields 2.4k edges per MB of .pbf, Malta 5.0k,
Portugal 3.6k); if you need a specific scale, prepare a candidate and read
the summary `prepare.py` prints.

Budget for the large scale: ~420 MB downloaded, ~15 minutes in `prepare.py`
(two Python passes, and pass 1 holds a use-count per node in memory), ~95 MB
of `edges.csv`, and a `bench.sqlite` written next to the CSVs.

## What `prepare.py` does

Two passes over the PBF with pyosmium:

1. Count how many kept ways use each node.
2. Split each kept way at its **graph vertices** — its two endpoints, plus
   any node that two or more kept ways share — and emit one edge per piece,
   with `cost` the summed haversine length in meters.

`ROUTABLE_HIGHWAY` at the top of the file is the explicit tag set (motorway
through residential, plus `service` unless `--no-service`). `oneway=yes/1/true`
and `junction=roundabout` give `reverse_cost = -1` (impassable backwards,
pgRouting's convention); `oneway=-1` swaps the edge's direction instead;
everything else is bidirectional at the same cost.

### The i32 renumbering is not optional

OSM node ids are 64-bit and current ones are well past 2^31. kenro's routing
aggregates take **i32** node and edge ids — `docs/routing.md`, "Ids are
32-bit", because a 64-bit argument makes a function unregisterable on some
wasm hosts. So `prepare.py` assigns the graph vertices dense ids `1..N` and
the OSM ids never reach SQLite. This is the worked example of that documented
constraint: feed raw OSM ids in and the aggregate cannot represent them.
(`docs/routing.md` suggests `DENSE_RANK()` for the same job in SQL.)

## Measured results

Apple M-series laptop, `--release`, 100 seeded pairs, default seed, kenro
0.3.0. `prepare.py` took about 15 minutes on the large extract (two Python
passes over 420 MB); everything else is seconds to minutes.

### Shortest path, one pair per query

| scale | edges | (a) `kenro_dijkstra_cost` full | (b) `kenro_dijkstra` full | (c) bbox prefilter, 5 km | unreachable |
|---|---|---|---|---|---|
| small (monaco) | 1,635 | 0.6 / 0.7 / 0.6 | 0.7 / 0.8 / 0.7 | 1.1 / 1.1 / 1.1 | 8/100 |
| medium (malta) | 44,383 | 20.3 / 21.6 / 20.3 | 21.1 / 22.3 / 21.1 | 26.4 / 36.6 / 25.4 | 21/100 |
| large (portugal) | 1,528,614 | 865 / 1188 / 985 | 874 / 953 / 879 | 272 / 955 / 347 | 12/100 |

Each cell is **median / p95 / mean, in milliseconds**. The unreachable column
is variants (a) and (b), which always agree: islands, one-way traps and
disconnected service loops. Returning the path instead of the cost (b) costs
almost nothing — the search dominates, not the JSON.

Latency tracks the edge count, not the route: 27x the edges from small to
medium buys 34x the time, and 34x again from medium to large buys 43x. That
is the O(E) trade made visible — the query pays for every row it is fed
whether the answer is 49 steps long or 1,161.

`kenro_drivingdistance`, full table, median ms (avg nodes reached):

| scale | limit 1 km | limit 5 km |
|---|---|---|
| small | 1.0 (166) | 2.4 (1,298) |
| medium | 22.4 (158) | 29.6 (5,040) |
| large | 998 (206) | 984 (2,490) |

A 1 km isochrone and a 5 km one cost the same on the large extract: the
accumulation of 1.5 M rows is the whole bill, and the sweep is noise.

### What the bbox prefilter actually buys

Variant (c) narrows the aggregate's input to edges whose bounding box
intersects the two endpoints' envelope grown by `--bbox-margin-km`, and
compares its answer against variant (a) pair by pair. On the large extract:

| margin | edges fed | vs (a) median | answers differing from (a) |
|---|---|---|---|
| 5 km | 248,200 (16.2%) | 0.31x | **31/100** |
| 25 km | 437,045 (28.6%) | 0.54x | **6/100** |
| 50 km | 644,318 (42.2%) | 0.80x | **2/100** |

So: **the 5 km default is a city-scale setting, and it is not safe for the
pairs this bench picks.** Two uniformly random vertices in a country are
hundreds of kilometers apart, and a real road route bulges far outside a
5 km-wide corridor around the straight line between them — a third of the
answers change, and they change by going NULL or by reporting a longer
detour that happens to stay inside the box. The margin has to be a fraction
of the trip length, not a constant, and it is a *heuristic either way*: no
margin makes the prefilter exact. Route a city with a 5 km box and it is
free and correct; route a country with one and it is fast and wrong.

At the small and medium scales the prefilter never changed an answer (0/100
both times) and never paid off either — the whole of Monaco is inside a
5 km margin, so (c) feeds 100% of the table and loses 60% to the `WHERE`
clause it added; Malta feeds 63% and still loses. **Below roughly a hundred
thousand edges, do not bother.**

### vs pgRouting

`--vs-pgrouting` loads the same CSV into `pgrouting/pgrouting:17-3.5-3.7`
(the image the golden vectors come from), builds the standard
source/target indexes, and runs `pgr_dijkstraCost` for the same pairs.

| scale | agg_cost agreement (1e-6 relative) | pgRouting server-side median | kenro median |
|---|---|---|---|
| small (monaco) | **100/100** (8 both-NULL) | 1.5 ms | 0.6 ms |
| medium (malta) | **100/100** (21 both-NULL) | 44.3 ms | 20.3 ms |

Every pair agrees, unreachable pairs included — kenro's NULL lands exactly
where pgRouting's empty result set does.

> The latency columns are not a like-for-like comparison. kenro is timed
> in-process around `query_row`; pgRouting by its own `\timing`, server-side,
> inside a batch driven through `docker exec`, in an amd64 container running
> under emulation on an arm64 host. Read the agreement as exact and the
> latency as an order of magnitude — the point is that a SQLite file is in
> the same league as a PostgreSQL server for this workload, not that it wins
> by 2x.

The large scale was **not run against pgRouting**: `\copy` of 1.5 M edges
plus 100 searches inside the emulated container is a long wait for a number
the caveat above already says not to read precisely.

