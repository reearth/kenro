#!/usr/bin/env bash
# Regenerates tests/golden/routing.jsonl against real pgRouting.
#
# Separate from generate.sh because it needs a different image: the
# pgrouting/pgrouting tags are <postgres>-<postgis>-<pgrouting>, and the plain
# postgis/postgis image the other suites use has no pgrouting extension.
#
# The output file is committed; CI never touches Docker. Re-run only when
# adding vectors or bumping the reference, and diff the result against the
# committed file to detect reference drift.
set -euo pipefail
cd "$(dirname "$0")"

IMAGE=pgrouting/pgrouting:17-3.5-3.7
CONTAINER=kenro-golden-pgrouting

# --platform: the reference image is amd64-only; runs under emulation on
# arm64 hosts (fine — this script is run rarely, offline from CI).
docker run --rm -d --name "$CONTAINER" --platform linux/amd64 \
  -e POSTGRES_PASSWORD=kenro "$IMAGE" >/dev/null
trap 'docker stop "$CONTAINER" >/dev/null' EXIT

echo "waiting for postgres..." >&2
until docker logs "$CONTAINER" 2>&1 | grep -q "PostgreSQL init process complete"; do sleep 1; done
until docker exec "$CONTAINER" psql -U postgres -tAc 'SELECT 1' >/dev/null 2>&1; do sleep 1; done
docker exec "$CONTAINER" psql -U postgres -q \
  -c 'CREATE EXTENSION IF NOT EXISTS postgis;' \
  -c 'CREATE EXTENSION IF NOT EXISTS pgrouting;' 2>/dev/null

VERSION=$(docker exec "$CONTAINER" psql -U postgres -tA -c 'SELECT pgr_version()')
OUT=../../tests/golden/routing.jsonl
{
  printf '{"_generated_by":"pgRouting %s (%s)","_script":"scripts/golden/routing.sql"}\n' \
    "$VERSION" "$IMAGE"
  docker exec -i "$CONTAINER" psql -U postgres -q -tA -v ON_ERROR_STOP=1 <routing.sql
} >"$OUT"
echo "wrote $OUT ($(grep -c '"fn"' "$OUT") vectors)" >&2
