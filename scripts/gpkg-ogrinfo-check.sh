#!/usr/bin/env bash
# Cross-check the POLYHEDRALSURFACE GeoPackage against GDAL.
#
# `tests/gpkg_polyhedralsurface.rs` builds two real .gpkg files with SQLite
# and kenro and asserts that kenro's own view of them is right. A second
# implementation is what catches the mistakes kenro cannot see in its own
# output — a `gpkg_extensions` row no other reader recognises — so this hands
# the files to ogrinfo.
#
# Offline by design, like scripts/golden/generate.sh: CI has no Docker. Run it
# when the fixture DDL in that test changes, and update the recorded verdict
# below if the output moves.
#
#     scripts/gpkg-ogrinfo-check.sh
#
# ===========================================================================
# MEASURED VERDICT — GDAL 3.11.0 "Eganville", ghcr.io/osgeo/gdal:alpine-small-3.11.0
#
# ## The declared file: clean
#
#     INFO: Open of `declared.gpkg' using driver `GPKG' successful.
#     Layer name: buildings
#     Geometry: 3D PolyhedralSurface
#     Feature Count: 3
#     Extent: (0.000000, 0.000000) - (11.000000, 11.000000)
#     Layer SRS WKT: GEOGCRS["WGS 84", … ID["EPSG",4326]]
#     FID Column = fid   Geometry Column = geom   name: String (0.0)
#     OGRFeature(buildings):1
#       POLYHEDRALSURFACE Z (((0 0 0,0 1 0,1 1 0,1 0 0,0 0 0)),
#                            ((0 0 12,1 0 12,1 1 12,0 1 12,0 0 12)), … )
#
# No errors, no warnings. GDAL re-reads every patch and every height kenro's
# `ST_AsGPB` wrote, the layer type reflects
# `gpkg_geometry_columns.geometry_type_name` + `z = 1`, and the extent
# matches the 2D footprint the R-tree holds.
#
# ## What ogrinfo does *not* check — measured, and it corrects an assumption
#
# `tmp/3d-geometry-design.md` §7.4 called this "the one that catches a wrong
# `definition` string". It is not. Four mutations of the declared file were
# measured against GDAL 3.11:
#
# | mutation | ogrinfo |
# |---|---|
# | `definition` → `http://example.com/nope` | **silent**, reads normally |
# | `extension_name` → `gpkg_geom_TIN` (wrong but known) | **silent** |
# | the `gpkg_extensions` row removed entirely | **silent**, reads normally |
# | `extension_name` → `acme_frobnicate` (unknown) | **Warning 1: Layer buildings relies on the 'acme_frobnicate' (…) extension that should be implemented in order to read it safely, but is not currently.** |
# | `geometry_type_name` → `POLYGON` | silent, but reports `Geometry: 3D Polygon` while the features still print as POLYHEDRALSURFACE |
#
# So GDAL validates the extension **name** against the set it implements and
# ignores the `definition` URL and the row's absence entirely. That is still
# worth having — an unrecognised name is exactly the typo a hand-written
# Annex F.1 row invites, and nothing in kenro's own test suite can see it —
# but the claim has to be the true one. The `definition` string is checked
# here only by `tests/gpkg_polyhedralsurface.rs` comparing it to the constant,
# which is a spelling check, not a conformance check; no tool available to
# this project validates it.
#
# The undeclared file reading cleanly is the same finding from the other
# side, and it is the reason kenro does not enforce Annex F.1 either: the
# reference implementation is liberal here, so a reader that refused would
# refuse files GDAL writes and reads.
# ===========================================================================
set -euo pipefail

cd "$(dirname "$0")/.."

IMAGE="${KENRO_GDAL_IMAGE:-ghcr.io/osgeo/gdal:alpine-small-3.11.0}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# --platform: the amd64 image runs under emulation on arm64 hosts, which is
# fine for a script that is run by hand.
ogr() {
    docker run --rm --platform linux/amd64 -v "$WORK:/data" -w /data "$IMAGE" \
        sh -c "$1" 2>&1
}

echo "building the fixtures with tests/gpkg_polyhedralsurface.rs..." >&2
KENRO_GPKG_OUT="$WORK" cargo test --all-features --test gpkg_polyhedralsurface >/dev/null
for f in declared undeclared; do
    [ -s "$WORK/$f.gpkg" ] || { echo "the test wrote no $f.gpkg" >&2; exit 1; }
done

echo >&2
ogr 'ogrinfo --version'
echo >&2
echo "--- declared.gpkg (the conformant file) ---" >&2
ogr 'ogrinfo -al declared.gpkg' | tee "$WORK/declared.txt"

# A GeoPackage GDAL is unhappy with still exits 0 while printing a warning, so
# the failure condition is the text, not the status.
if grep -qiE '^(ERROR|Warning)' "$WORK/declared.txt"; then
    echo >&2
    echo "ogrinfo reported an error or warning on the conformant file — that is a" >&2
    echo "finding to fix, not to hide." >&2
    exit 1
fi
if ! grep -q 'Geometry: 3D PolyhedralSurface' "$WORK/declared.txt"; then
    echo >&2
    echo "GDAL did not recognise the layer as a 3D PolyhedralSurface. Check" >&2
    echo "gpkg_geometry_columns.geometry_type_name and its z flag." >&2
    exit 1
fi
if ! grep -q 'POLYHEDRALSURFACE Z' "$WORK/declared.txt"; then
    echo >&2
    echo "GDAL did not read the surfaces back. Check ST_AsGPB's surface path." >&2
    exit 1
fi

# Negative control: prove the check has teeth. If GDAL stopped warning about
# an extension name it does not implement, the clean verdict above would mean
# nothing, and this script would be asserting a tautology.
echo >&2
echo "--- negative control: an extension name GDAL does not implement ---" >&2
cp "$WORK/declared.gpkg" "$WORK/mutated.gpkg"
python3 - "$WORK/mutated.gpkg" <<'PY'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
c.execute(
    "UPDATE gpkg_extensions SET extension_name = 'acme_frobnicate' "
    "WHERE extension_name = 'gpkg_geom_POLYHEDRALSURFACE'"
)
c.commit()
PY
ogr 'ogrinfo -al -so mutated.gpkg' > "$WORK/mutated.txt"
grep -i "relies on the" "$WORK/mutated.txt" || true
if ! grep -qi "relies on the 'acme_frobnicate'" "$WORK/mutated.txt"; then
    echo >&2
    echo "GDAL did NOT warn about an unimplemented extension name. The clean" >&2
    echo "verdict on declared.gpkg therefore proves nothing — re-measure what" >&2
    echo "this GDAL version actually validates before trusting this script." >&2
    exit 1
fi

echo >&2
echo "--- undeclared.gpkg (no gpkg_extensions row for the surface) ---" >&2
ogr 'ogrinfo -al -so undeclared.gpkg' > "$WORK/undeclared.txt"
sed -n '1,8p' "$WORK/undeclared.txt"
if grep -qiE '^(ERROR|Warning)' "$WORK/undeclared.txt"; then
    echo >&2
    echo "NOTE: this GDAL flags the undeclared file. The recorded verdict says it" >&2
    echo "does not — re-measure and update the table at the top of this script." >&2
    exit 1
fi

echo >&2
echo "ogrinfo verdict: declared.gpkg clean and read as 3D PolyhedralSurface;" >&2
echo "the unimplemented-extension warning fires as expected; undeclared.gpkg" >&2
echo "reads without complaint, which is why kenro does not enforce Annex F.1." >&2
