#!/usr/bin/env python3
"""Turns an OSM .pbf extract into the edge/node CSVs the routing bench loads.

    python3 prepare.py data/monaco-latest.osm.pbf [--out-dir data] [--prefix monaco]

Writes `<prefix>.edges.csv` and `<prefix>.nodes.csv`:

    edges: id,source,target,cost,reverse_cost,x1,y1,x2,y2
    nodes: node,lon,lat

`cost` is the edge's length in meters (summed haversine over its geometry) and
`x1,y1,x2,y2` is its bounding box in degrees, which the bench uses for the
WHERE-clause prefilter.

Requires pyosmium:  pip install osmium
"""

import argparse
import math
import os
import sys
from collections import defaultdict

try:
    import osmium
except ImportError:
    sys.exit(
        "prepare.py needs pyosmium: pip install osmium\n"
        "(a virtualenv is fine; nothing else here imports it)"
    )

# The highway values that become graph edges. Explicit rather than a
# blocklist, so what the bench measures is a stated set and not whatever OSM
# happens to tag today. `service` is in by default because leaving it out
# disconnects parking aisles and driveways from the network; --no-service
# drops it (and roughly a fifth of the edges) if you want a purer road graph.
ROUTABLE_HIGHWAY = {
    "motorway",
    "motorway_link",
    "trunk",
    "trunk_link",
    "primary",
    "primary_link",
    "secondary",
    "secondary_link",
    "tertiary",
    "tertiary_link",
    "unclassified",
    "residential",
    "living_street",
    "road",
}
OPTIONAL_HIGHWAY = {"service"}

EARTH_RADIUS_M = 6371008.8


def haversine(lon1, lat1, lon2, lat2):
    """Great-circle distance in meters on a sphere of EARTH_RADIUS_M."""
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dp = p2 - p1
    dl = math.radians(lon2 - lon1)
    a = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * EARTH_RADIUS_M * math.asin(math.sqrt(a))


def oneway_kind(tags):
    """`fwd` (reverse closed), `rev` (edge runs the other way), or `both`."""
    ow = tags.get("oneway", "").strip().lower()
    if ow in ("yes", "1", "true"):
        return "fwd"
    if ow == "-1" or ow == "reverse":
        return "rev"
    if ow in ("no", "0", "false"):
        return "both"
    # A roundabout is one-way by convention even without the tag.
    if tags.get("junction", "").lower() in ("roundabout", "circular"):
        return "fwd"
    return "both"


def keep(tags, routable):
    hw = tags.get("highway")
    return hw is not None and hw in routable


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pbf")
    ap.add_argument("--out-dir", default=None, help="default: the .pbf's directory")
    ap.add_argument("--prefix", default=None, help="default: the .pbf's basename")
    ap.add_argument(
        "--no-service",
        action="store_true",
        help="drop highway=service (parking aisles, driveways)",
    )
    args = ap.parse_args()

    routable = set(ROUTABLE_HIGHWAY)
    if not args.no_service:
        routable |= OPTIONAL_HIGHWAY

    out_dir = args.out_dir or os.path.dirname(os.path.abspath(args.pbf))
    prefix = args.prefix or os.path.basename(args.pbf).split(".")[0].replace(
        "-latest", ""
    )
    os.makedirs(out_dir, exist_ok=True)
    edges_path = os.path.join(out_dir, prefix + ".edges.csv")
    nodes_path = os.path.join(out_dir, prefix + ".nodes.csv")

    # --- pass 1: how many kept ways touch each node -----------------------
    # A node used by two or more kept ways is a junction, and so a graph
    # vertex; a node used once is interior geometry unless it is an endpoint.
    print("pass 1: counting node usage...", file=sys.stderr)
    uses = defaultdict(int)
    ways_kept = 0
    for w in osmium.FileProcessor(args.pbf, osmium.osm.WAY):
        if not keep(w.tags, routable):
            continue
        ways_kept += 1
        refs = [n.ref for n in w.nodes]
        if len(refs) < 2:
            continue
        for r in refs:
            uses[r] += 1

    print(f"pass 1: {ways_kept} ways kept, {len(uses)} distinct nodes", file=sys.stderr)

    # --- pass 2: split each way at its vertices ---------------------------
    # CRITICAL: OSM node ids are 64-bit and routinely exceed 2^31, while
    # kenro's routing aggregates take i32 node ids (docs/routing.md,
    # "Ids are 32-bit"). So the graph vertices get dense ids 1..N here; the
    # OSM ids never reach SQLite.
    print("pass 2: splitting ways into edges...", file=sys.stderr)
    vid = {}  # osm node id -> dense 1..N
    vcoord = []  # dense id order: (lon, lat)

    def vertex(osm_id, lon, lat):
        v = vid.get(osm_id)
        if v is None:
            v = len(vcoord) + 1
            vid[osm_id] = v
            vcoord.append((lon, lat))
        return v

    edge_id = 0
    skipped_ways = 0
    with open(edges_path, "w") as f:
        f.write("id,source,target,cost,reverse_cost,x1,y1,x2,y2\n")
        fp = osmium.FileProcessor(args.pbf).with_locations()
        for w in fp:
            if w.type_str() != "w" or not keep(w.tags, routable):
                continue
            try:
                pts = [(n.ref, n.lon, n.lat) for n in w.nodes if n.location.valid()]
            except (osmium.InvalidLocationError, RuntimeError):
                skipped_ways += 1
                continue
            if len(pts) < 2:
                skipped_ways += 1
                continue
            kind = oneway_kind(w.tags)
            last = len(pts) - 1

            seg_start = 0
            for i in range(1, len(pts)):
                is_vertex = i == last or uses[pts[i][0]] >= 2
                if not is_vertex:
                    continue
                chunk = pts[seg_start : i + 1]
                cost = 0.0
                xs, ys = [], []
                for (_, lo, la), (_, lo2, la2) in zip(chunk, chunk[1:]):
                    cost += haversine(lo, la, lo2, la2)
                for _, lo, la in chunk:
                    xs.append(lo)
                    ys.append(la)
                seg_start = i
                if cost <= 0.0:
                    continue
                s = vertex(chunk[0][0], chunk[0][1], chunk[0][2])
                t = vertex(chunk[-1][0], chunk[-1][1], chunk[-1][2])
                if s == t:
                    continue  # a closed loop segment routes nowhere
                if kind == "rev":
                    s, t = t, s
                rev = -1.0 if kind in ("fwd", "rev") else cost
                edge_id += 1
                f.write(
                    "%d,%d,%d,%.3f,%.3f,%.7f,%.7f,%.7f,%.7f\n"
                    % (
                        edge_id,
                        s,
                        t,
                        cost,
                        rev,
                        min(xs),
                        min(ys),
                        max(xs),
                        max(ys),
                    )
                )

    with open(nodes_path, "w") as f:
        f.write("node,lon,lat\n")
        for i, (lo, la) in enumerate(vcoord, start=1):
            f.write("%d,%.7f,%.7f\n" % (i, lo, la))

    print(
        f"ways kept: {ways_kept}\nedges: {edge_id}\nvertices: {len(vcoord)}"
        + (f"\nways skipped (missing locations): {skipped_ways}" if skipped_ways else ""),
        file=sys.stderr,
    )
    print(f"wrote {edges_path}\nwrote {nodes_path}", file=sys.stderr)


if __name__ == "__main__":
    main()
