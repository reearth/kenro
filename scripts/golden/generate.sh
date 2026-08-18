#!/usr/bin/env bash
# Regenerates tests/golden/*.jsonl against the reference PostGIS
# (predicates, transform, geojson, accessors — H3 vectors come from
# h3_generate.py instead).
#
# The output files are committed; CI never touches Docker/PostGIS. Re-run
# only when adding vectors or bumping the reference PostGIS, and diff the
# result against the committed files to detect reference drift.
#
# Name suites on the command line to regenerate only those:
#
#     ./generate.sh box_text
#
# Prefer that to a blind full run. Several committed suites carry hand-edits
# a regeneration would clobber — `transform.jsonl` and `geojson.jsonl` have
# hand-added `needs_feature` / version notes, and `threed.jsonl` has two
# vectors whose reference values come from uninitialised memory and are not
# reproducible run to run.
set -euo pipefail
cd "$(dirname "$0")"

IMAGE=postgis/postgis:17-3.5
CONTAINER=kenro-golden-postgis
ALL_SUITES=(predicates transform geojson accessors processing bool_ops buffer threed threed_sfcgal box_text)
if [ $# -gt 0 ]; then
  SUITES=("$@")
  for s in "${SUITES[@]}"; do
    [ -f "$s.sql" ] || { echo "no such suite: $s" >&2; exit 1; }
  done
else
  SUITES=("${ALL_SUITES[@]}")
fi

# threed_sfcgal needs ST_3DArea/ST_Volume, which live in the `postgis_sfcgal`
# extension. The image has shipped it all along (SFCGAL 1.3.8 over CGAL); this
# script simply never asked for it, which is why the suite is new rather than
# the image being. Loading it was measured to leave all eight pre-existing
# suites byte-identical, so it is loaded once, up front, for every suite —
# PostGIS 3.x has no backend-switching GUC for SFCGAL to take over with.

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
docker exec "$CONTAINER" psql -U postgres -q -c 'CREATE EXTENSION IF NOT EXISTS postgis;' 2>/dev/null
docker exec "$CONTAINER" psql -U postgres -q -c 'CREATE EXTENSION IF NOT EXISTS postgis_sfcgal;' 2>/dev/null

VERSION=$(docker exec "$CONTAINER" psql -U postgres -tA -c "SELECT split_part(postgis_full_version(), '\"', 2)")
SFCGAL=$(docker exec "$CONTAINER" psql -U postgres -tA -c "SELECT postgis_sfcgal_version()")
for suite in "${SUITES[@]}"; do
  OUT=../../tests/golden/$suite.jsonl
  # Only the SFCGAL suite's answers come from SFCGAL, so only its provenance
  # names the version — the rest stay on the header they have always had.
  PROV="PostGIS $VERSION"
  [ "$suite" = threed_sfcgal ] && PROV="PostGIS $VERSION + SFCGAL $SFCGAL"
  {
    printf '{"_generated_by":"%s (%s)","_script":"scripts/golden/%s.sql"}\n' \
      "$PROV" "$IMAGE" "$suite"
    docker exec -i "$CONTAINER" psql -U postgres -q -tA -v ON_ERROR_STOP=1 <"$suite.sql"
  } >"$OUT"
  echo "wrote $OUT ($(grep -c '"fn"' "$OUT") vectors)" >&2
done
