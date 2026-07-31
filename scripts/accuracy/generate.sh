#!/usr/bin/env bash
# Regenerates scripts/accuracy/reference.jsonl: global point lattices
# transformed by the reference PostGIS/PROJ. Consumed by
# `cargo run --example accuracy_report`, which produces docs/accuracy.md.
#
# NOTE what the reference is: this PostGIS image ships PROJ *without*
# datum grids, so the comparison measures proj4rs vs gridless PROJ
# (projection math), not survey-grade truth. See docs/accuracy.md.
set -euo pipefail
cd "$(dirname "$0")"

IMAGE=postgis/postgis:17-3.5
CONTAINER=kenro-accuracy-postgis
OUT=reference.jsonl

docker run --rm -d --name "$CONTAINER" --platform linux/amd64 \
  -e POSTGRES_PASSWORD=kenro "$IMAGE" >/dev/null
trap 'docker stop "$CONTAINER" >/dev/null' EXIT

echo "waiting for postgres..." >&2
until docker logs "$CONTAINER" 2>&1 | grep -q "PostgreSQL init process complete"; do sleep 1; done
until docker exec "$CONTAINER" psql -U postgres -tAc 'SELECT 1' >/dev/null 2>&1; do sleep 1; done
docker exec "$CONTAINER" psql -U postgres -q -c 'CREATE EXTENSION IF NOT EXISTS postgis;' 2>/dev/null

FULL_VERSION=$(docker exec "$CONTAINER" psql -U postgres -tA -c 'SELECT postgis_full_version()')
{
  python3 -c "import json,sys; print(json.dumps({'_generated_by': sys.argv[1], '_script': 'scripts/accuracy/generate.sql'}))" "$FULL_VERSION"
  docker exec -i "$CONTAINER" psql -U postgres -q -tA -v ON_ERROR_STOP=1 <generate.sql
} >"$OUT"

echo "wrote $OUT ($(grep -c '"pair"' "$OUT") points)" >&2
