#!/usr/bin/env bash
# Regenerates tests/golden/predicates.jsonl against the reference PostGIS.
#
# The output file is committed; CI never touches Docker or PostGIS.
# Re-run only when adding vectors or bumping the reference PostGIS, and diff
# the result against the committed file to detect reference drift.
set -euo pipefail
cd "$(dirname "$0")"

IMAGE=postgis/postgis:17-3.5
CONTAINER=kenro-golden-postgis
OUT=../../tests/golden/predicates.jsonl

# --platform: the reference image is amd64-only; runs under emulation on
# arm64 hosts (fine — this script is run rarely, offline from CI).
docker run --rm -d --name "$CONTAINER" --platform linux/amd64 \
  -e POSTGRES_PASSWORD=kenro "$IMAGE" >/dev/null
trap 'docker stop "$CONTAINER" >/dev/null' EXIT

# The entrypoint starts a temporary server during init and restarts it, so
# pg_isready alone races; wait for the init-complete log line first.
echo "waiting for postgres..." >&2
until docker logs "$CONTAINER" 2>&1 | grep -q "PostgreSQL init process complete"; do sleep 1; done
until docker exec "$CONTAINER" psql -U postgres -tAc 'SELECT 1' >/dev/null 2>&1; do sleep 1; done
docker exec "$CONTAINER" psql -U postgres -q -c 'CREATE EXTENSION IF NOT EXISTS postgis;'

VERSION=$(docker exec "$CONTAINER" psql -U postgres -tA -c "SELECT split_part(postgis_full_version(), '\"', 2)")
{
  printf '{"_generated_by":"PostGIS %s (%s)","_script":"scripts/golden/generate.sql"}\n' "$VERSION" "$IMAGE"
  docker exec -i "$CONTAINER" psql -U postgres -q -tA -v ON_ERROR_STOP=1 <generate.sql
} >"$OUT"

echo "wrote $OUT ($(grep -c '"fn"' "$OUT") vectors)" >&2
