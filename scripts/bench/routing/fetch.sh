#!/usr/bin/env bash
# Downloads an OpenStreetMap extract from Geofabrik into data/.
#
#   ./fetch.sh                     # europe/monaco (the small scale)
#   ./fetch.sh europe/luxembourg   # any Geofabrik path, without -latest.osm.pbf
#
# The region is an argument, not a default baked into anything: the bench
# says nothing about which part of the world it is measuring.
set -euo pipefail
cd "$(dirname "$0")"

REGION=${1:-europe/monaco}
NAME=$(basename "$REGION")
URL="https://download.geofabrik.de/${REGION}-latest.osm.pbf"
OUT="data/${NAME}-latest.osm.pbf"

mkdir -p data
echo "fetching $URL" >&2
# -C -: resume a partial download rather than starting the (possibly large)
# extract over. --fail so a 404 on a mistyped region is an error, not an
# HTML page written to the .pbf.
curl -fL -C - --retry 3 -o "$OUT" "$URL"

SIZE=$(du -h "$OUT" | cut -f1)
echo "wrote $OUT ($SIZE)" >&2
