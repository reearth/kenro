#!/usr/bin/env python3
"""Generate MVT golden vectors from the reference PostGIS into
tests/golden/mvt.jsonl. Committed output; CI never runs this.

Needs Docker (the same postgis image as generate.sh) and the
mapbox-vector-tile package for decoding ST_AsMVT output:

    python3 -m venv .venv && .venv/bin/pip install mapbox-vector-tile
    .venv/bin/python scripts/golden/mvt_generate.py

Two vector kinds:
- fn "asmvtgeom": a/b = geometry/bounds WKT, args = [extent, buffer, clip]
  (trailing omitted → PostGIS defaults), expected = tile-space WKT or null.
- fn "asmvt": rows = [[wkt, props|null], ...] piped through ST_AsMVTGeom
  with the shared bounds, arg_text = layer name, arg = extent; expected =
  the tile decoded to normalized JSON (raw Y-down integer coordinates).
"""

import base64
import json
import pathlib
import subprocess
import sys
import time

import mapbox_vector_tile

OUT = pathlib.Path(__file__).resolve().parents[2] / "tests" / "golden" / "mvt.jsonl"
IMAGE = "postgis/postgis:17-3.5"
CONTAINER = "kenro-golden-postgis-mvt"

BOUNDS = "POLYGON((0 0,100 0,100 100,0 100,0 0))"

# (id, geom_wkt, [extent, buffer, clip] — trailing None trimmed)
ASMVTGEOM = [
    ("point_inside", "POINT(50 90)", [100, 0]),
    ("point_default_args", "POINT(50 90)", []),
    ("point_outside", "POINT(200 50)", [100, 0]),
    ("point_outside_noclip", "POINT(200 50)", [100, 0, 0]),
    ("point_in_buffer", "POINT(-5 50)", [100, 10]),
    ("point_on_edge", "POINT(100 100)", [100, 0]),
    ("multipoint_mixed", "MULTIPOINT(10 10,200 200)", [100, 0]),
    ("line_inside", "LINESTRING(10 10,90 90)", [100, 0]),
    ("line_crossing", "LINESTRING(-50 50,150 50)", [100, 0]),
    ("line_crossing_buffer", "LINESTRING(-50 50,150 50)", [100, 10]),
    ("line_outside", "LINESTRING(200 200,300 300)", [100, 0]),
    ("poly_inside", "POLYGON((10 10,90 10,90 90,10 90,10 10))", [100, 0]),
    ("poly_overlap", "POLYGON((50 50,150 50,150 60,50 60,50 50))", [100, 0]),
    ("poly_outside", "POLYGON((200 200,300 200,300 300,200 300,200 200))", [100, 0]),
    ("poly_with_hole", "POLYGON((10 10,90 10,90 90,10 90,10 10),(40 40,60 40,60 60,40 60,40 40))", [100, 0]),
    ("poly_sliver", "POLYGON((10 10,20 10,20 10.001,10 10.001,10 10))", [100, 0]),
    ("poly_extent_512", "POLYGON((10 10,90 10,90 90,10 90,10 10))", [512, 0]),
    # (kenro's WKT reader rejects POINT EMPTY by design, so the empty-input
    # vector uses an empty linestring.)
    ("empty_line", "LINESTRING EMPTY", [100, 0]),
]

# (id, layer_name, extent, rows = [(wkt, props|None)])
ASMVT = [
    (
        "single_point_props",
        "parks",
        100,
        [("POINT(50 90)", {"name": "yoyogi", "rank": 3, "score": 54.5, "open": True})],
    ),
    (
        "shared_values_dedup",
        "parks",
        100,
        [
            ("POINT(10 10)", {"name": "a", "rank": 7}),
            ("POINT(20 20)", {"name": "b", "rank": 7}),
        ],
    ),
    (
        "mixed_geometries_no_props",
        "shapes",
        100,
        [
            ("LINESTRING(10 10,90 90)", None),
            ("POLYGON((10 10,90 10,90 90,10 90,10 10))", None),
        ],
    ),
    (
        "clipped_away_row_skipped",
        "parks",
        100,
        [
            ("POINT(50 50)", {"name": "in"}),
            ("POINT(500 500)", {"name": "out"}),
        ],
    ),
    (
        "negative_int_prop",
        "depths",
        100,
        [("POINT(30 30)", {"depth": -12})],
    ),
    (
        "extent_256",
        "roads",
        256,
        [("LINESTRING(0 0,100 100)", None)],
    ),
]


def sh(*args, input=None):
    return subprocess.run(args, input=input, capture_output=True, text=True, check=True).stdout


def psql(sql):
    out = subprocess.run(
        ["docker", "exec", "-i", CONTAINER, "psql", "-U", "postgres", "-q", "-tA",
         "-v", "ON_ERROR_STOP=1"],
        input=sql, capture_output=True, text=True,
    )
    if out.returncode != 0:
        raise RuntimeError(f"psql failed:\n{sql}\n{out.stderr}")
    return out.stdout.strip()


def sql_quote(s):
    return "'" + s.replace("'", "''") + "'"


def asmvtgeom_sql(wkt, args):
    call = [f"ST_GeomFromText({sql_quote(wkt)})", f"ST_GeomFromText({sql_quote(BOUNDS)})::box2d"]
    if len(args) >= 1:
        call.append(str(args[0]))
    if len(args) >= 2:
        call.append(str(args[1]))
    if len(args) >= 3:
        call.append("true" if args[2] else "false")
    return f"ST_AsMVTGeom({', '.join(call)})"


PGTYPE = {str: "text", int: "int8", float: "float8", bool: "boolean"}


def asmvt_sql(name, extent, rows):
    # One typed column per property key (schema shared across the vector's
    # rows), so PostGIS emits typed MVT values.
    keys = []
    for _, props in rows:
        for k in props or {}:
            if k not in keys:
                keys.append(k)
    types = {}
    for k in keys:
        for _, props in rows:
            if props and props.get(k) is not None:
                types[k] = PGTYPE[type(props[k])]
                break
    cols = "".join(f", {t}" for t in (types[k] for k in keys))
    values = []
    for wkt, props in rows:
        vals = [sql_quote(wkt)]
        for k in keys:
            v = (props or {}).get(k)
            # Every value carries an explicit cast: a bare literal like 54.5
            # would be inferred as `numeric`, which ST_AsMVT encodes as a
            # STRING value rather than a double.
            if v is None:
                vals.append(f"NULL::{types[k]}")
            elif isinstance(v, bool):
                vals.append(f"{'true' if v else 'false'}::boolean")
            elif isinstance(v, str):
                vals.append(f"{sql_quote(v)}::text")
            else:
                vals.append(f"{v}::{types[k]}")
        values.append(f"({', '.join(vals)})")
    colnames = ", ".join(["wkt"] + [f'"{k}"' for k in keys])
    return f"""
WITH src(wkt{cols and ''.join(f', "{k}"' for k in keys)}) AS (VALUES {', '.join(values)}),
r AS (
  SELECT ST_AsMVTGeom(ST_GeomFromText(wkt), ST_GeomFromText({sql_quote(BOUNDS)})::box2d, {extent}, 0) AS geom
         {''.join(f', "{k}"' for k in keys)}
  FROM src
)
SELECT encode(ST_AsMVT(r, {sql_quote(name)}, {extent}, 'geom'), 'base64') FROM r WHERE geom IS NOT NULL;
"""


def decode_tile(b64, layer_name):
    raw = base64.b64decode(b64)
    decoded = mapbox_vector_tile.decode(
        raw, default_options={"y_coord_down": True}
    )
    layer = decoded[layer_name]
    features = []
    for f in layer["features"]:
        features.append({
            "type": f["geometry"]["type"],
            "coordinates": f["geometry"]["coordinates"],
            "properties": f.get("properties", {}),
        })
    return {"name": layer_name, "extent": layer["extent"], "features": features}


def main():
    subprocess.run(["docker", "rm", "-f", CONTAINER], capture_output=True)
    sh("docker", "run", "--rm", "-d", "--name", CONTAINER, "--platform", "linux/amd64",
       "-e", "POSTGRES_PASSWORD=kenro", IMAGE)
    try:
        print("waiting for postgres...", file=sys.stderr)
        while "PostgreSQL init process complete" not in subprocess.run(
            ["docker", "logs", CONTAINER], capture_output=True, text=True
        ).stdout + subprocess.run(
            ["docker", "logs", CONTAINER], capture_output=True, text=True
        ).stderr:
            time.sleep(1)
        while subprocess.run(
            ["docker", "exec", CONTAINER, "psql", "-U", "postgres", "-tAc", "SELECT 1"],
            capture_output=True,
        ).returncode != 0:
            time.sleep(1)
        psql("CREATE EXTENSION IF NOT EXISTS postgis;")
        version = psql("SELECT split_part(postgis_full_version(), '\"', 2);")

        lines = [json.dumps({
            "_generated_by": f"PostGIS {version} ({IMAGE}) + mapbox-vector-tile "
                             f"{mapbox_vector_tile.__version__ if hasattr(mapbox_vector_tile, '__version__') else ''}".strip(),
            "_script": "scripts/golden/mvt_generate.py",
        }, separators=(",", ":"))]

        for id_, wkt, args in ASMVTGEOM:
            expected = psql(f"SELECT ST_AsText({asmvtgeom_sql(wkt, args)});")
            vec = {
                "id": f"{id_}:asmvtgeom",
                "a": wkt,
                "b": BOUNDS,
                "fn": "asmvtgeom",
                "args": args,
                "mode": "mvt_geom",
                "expected": expected if expected else None,
            }
            lines.append(json.dumps(vec, separators=(",", ":")))

        for id_, name, extent, rows in ASMVT:
            b64 = asmvt_sql(name, extent, rows).strip()
            b64 = psql(b64).replace("\n", "")
            vec = {
                "id": f"{id_}:asmvt",
                "fn": "asmvt",
                "b": BOUNDS,
                "arg_text": name,
                "arg": extent,
                "rows": [[wkt, props] for wkt, props in rows],
                "mode": "mvt_tile",
                "expected": decode_tile(b64, name),
            }
            lines.append(json.dumps(vec, separators=(",", ":")))

        OUT.write_text("\n".join(lines) + "\n")
        print(f"wrote {OUT} ({len(lines) - 1} vectors)", file=sys.stderr)
    finally:
        subprocess.run(["docker", "stop", CONTAINER], capture_output=True)


if __name__ == "__main__":
    main()
