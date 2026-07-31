#!/usr/bin/env python3
"""Generate H3 golden vectors from the reference C library (python `h3` v4
bindings) into tests/golden/h3.jsonl. Committed output; CI never runs this.

    pip install 'h3>=4' && python3 scripts/golden/h3_generate.py
"""

import json
import pathlib

import h3

OUT = pathlib.Path(__file__).resolve().parents[2] / "tests" / "golden" / "h3.jsonl"

POINTS = [
    ("tokyo", 139.745433, 35.658581),
    ("sapporo", 141.354376, 43.062096),
    ("naha", 127.679245, 26.212401),
    ("greenwich", -0.0014, 51.4779),
    ("null_island", 0.0, 0.0),
    ("sydney", 151.2093, -33.8688),
]

lines = []


def emit(obj):
    lines.append(json.dumps(obj, separators=(",", ":")))


emit({
    "_generated_by": f"python h3 {h3.versions()['python']} / h3 C library {h3.versions()['c']}",
    "_script": "scripts/golden/h3_generate.py",
})

for name, lng, lat in POINTS:
    for res in (0, 7, 9, 15):
        cell = h3.latlng_to_cell(lat, lng, res)
        emit({
            "id": f"{name}_r{res}:latlng_to_cell",
            "a": f"POINT({lng} {lat})",
            "arg": res,
            "fn": "latlng_to_cell",
            "expected": int(cell, 16),
        })

# Parent chains and string conversions for one well-known cell.
tokyo9 = h3.latlng_to_cell(35.658581, 139.745433, 9)
for res in (0, 5, 8, 9):
    emit({
        "id": f"tokyo_parent_r{res}:cell_to_parent",
        "cell": int(tokyo9, 16),
        "arg": res,
        "fn": "cell_to_parent",
        "expected": int(h3.cell_to_parent(tokyo9, res), 16),
    })
emit({
    "id": "tokyo_to_string:cell_to_string",
    "cell": int(tokyo9, 16),
    "fn": "cell_to_string",
    "expected": tokyo9,
})
emit({
    "id": "tokyo_from_string:string_to_cell",
    "a": tokyo9,
    "fn": "string_to_cell",
    "expected": int(tokyo9, 16),
})

OUT.write_text("\n".join(lines) + "\n")
print(f"wrote {OUT} ({len(lines) - 1} vectors)")
