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
| large | `europe/portugal` | 259 MB | _see below_ | |

Extract size is a poor predictor of edge count (road density varies by an
order of magnitude); if you need a specific scale, prepare a candidate and
read the summary `prepare.py` prints.

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

_(filled in by an actual run — see the commit that added them)_
