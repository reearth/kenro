// The backend-independent half of the design.
//
// Both hosts — Durable Object SQLite and D1 — run the same plan:
//
//   SQL   — indexed coarse filter (tile cells, then bounding boxes)
//   kenro — exact predicates, measures and output, in JS on the survivors
//
// Neither supports user-defined functions, so no `ST_` call can appear in
// the SQL itself. What makes the SQL half indexable is that kenro also runs
// at *write* time: `prepareFeature` derives the bounding box and the tile
// cover before the row reaches SQLite, leaving SQL nothing but REALs and
// INTEGERs with plain B-tree indexes on them.
//
// Only the row plumbing differs between hosts, so only that lives in
// spatial-do.mjs / spatial-d1.mjs.

import { kenro } from "./kenro.mjs";
import { OVERSIZED, bboxOverlaps, cellsForFeature, tileCover } from "./tiles.mjs";

export const SCHEMA = [
  `CREATE TABLE IF NOT EXISTS features (
     id    TEXT PRIMARY KEY,
     geom  BLOB NOT NULL,          -- GeoPackage blob (GPB): header envelope makes ST_MinX free
     props TEXT NOT NULL,          -- JSON
     minx  REAL NOT NULL, miny REAL NOT NULL,
     maxx  REAL NOT NULL, maxy REAL NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS feature_cells (
     cell INTEGER NOT NULL,
     id   TEXT NOT NULL,
     PRIMARY KEY (cell, id)
   ) WITHOUT ROWID`,
];

/** Exact predicates available to a query, by name. */
function predicateFn(wasm, name, distance) {
  switch (name) {
    case "intersects":
      return (a, b) => wasm.stIntersects(a, b);
    case "within":
      return (a, b) => wasm.stWithin(a, b);
    case "contains":
      return (a, b) => wasm.stContains(a, b);
    case "dwithin":
      if (typeof distance !== "number" || !Number.isFinite(distance)) {
        throw new Error("dwithin needs a numeric `distance`");
      }
      return (a, b) => wasm.stDwithin(a, b, distance);
    default:
      throw new Error(`unknown predicate: ${name}`);
  }
}

function bboxOf(wasm, geom, what) {
  const bbox = {
    minx: wasm.stMinX(geom),
    miny: wasm.stMinY(geom),
    maxx: wasm.stMaxX(geom),
    maxy: wasm.stMaxY(geom),
  };
  if (Object.values(bbox).some((v) => v === undefined)) {
    throw new Error(`${what}: empty geometry has no bounding box`);
  }
  return bbox;
}

/**
 * One GeoJSON feature → the row and cell ids the index needs.
 *
 * Stored as a GeoPackage blob rather than kenro's bare internal one: the GPB
 * header carries an envelope, so ST_MinX & co. answer from 32 extra bytes
 * without decoding the WKB at all — measured at ~330× for a 2000-vertex
 * polygon (0.72 ms → 0.0022 ms for the four calls), with no effect on
 * predicate speed. The blob is also readable by anything that speaks
 * GeoPackage.
 */
export function prepareFeature(f, i) {
  const wasm = kenro();
  const id = String(f.id ?? `f${i}`);
  const geom = wasm.stAsGpb(wasm.stGeomFromGeojson(JSON.stringify(f.geometry)));
  const bbox = bboxOf(wasm, geom, `feature ${id}`);
  return {
    id,
    geom,
    props: JSON.stringify(f.properties ?? {}),
    bbox,
    cells: cellsForFeature(bbox),
  };
}

/**
 * The coarse filter, as SQL. `wkt` is any geometry — the tile grid is purely
 * internal, and a query window of any size or shape is fair game.
 *
 * A window too large to enumerate cells for drops the cell filter entirely
 * rather than falling back to the OVERSIZED bucket: that bucket holds only
 * the features too big to file, so selecting it alone would silently return
 * a handful of continent-sized rows and nothing else.
 */
export function plan({ wkt, predicate = "intersects", distance, srid, limit = 1000 }) {
  const wasm = kenro();
  const window = wasm.stGeomFromText(wkt);
  const windowBbox = bboxOf(wasm, window, "query window");
  // dwithin reaches beyond the window's own extent; grow the coarse filter by
  // the search distance so the cheap stages cannot drop a true hit.
  const pad = predicate === "dwithin" ? (distance ?? 0) : 0;
  const search = {
    minx: windowBbox.minx - pad,
    miny: windowBbox.miny - pad,
    maxx: windowBbox.maxx + pad,
    maxy: windowBbox.maxy + pad,
  };
  const test = predicateFn(wasm, predicate, distance);
  const cover = tileCover(search);

  const columns = "f.id, f.geom, f.props, f.minx, f.miny, f.maxx, f.maxy";
  const sql =
    cover === null
      ? `SELECT ${columns} FROM features f`
      : `SELECT ${columns} FROM features f
           WHERE f.id IN (
             SELECT id FROM feature_cells
              WHERE cell IN (${cover.map(() => "?").join(", ")}) OR cell = ?
           )`;
  // The OVERSIZED bucket is always a candidate when the filter is on:
  // dropping it would lose the features too big to have a cell cover.
  const params = cover === null ? [] : [...cover, OVERSIZED];

  return { sql, params, search, window, test, srid, limit, wholeTable: cover === null };
}

/**
 * Stage 2 and 3: bbox reject in JS, then the exact predicate in kenro.
 * `rows` must expose `geom` as a Uint8Array (hosts differ — see the callers).
 */
export function refine(rows, { search, window, test, srid, limit }) {
  const wasm = kenro();
  const features = [];
  let refined = 0;

  for (const row of rows) {
    if (!bboxOverlaps(search, row)) continue; // cheap reject before wasm
    refined++;
    const geom = row.geom;
    if (!test(geom, window)) continue;
    const out = srid ? wasm.stTransform(geom, srid) : geom;
    features.push({
      type: "Feature",
      id: row.id,
      geometry: JSON.parse(wasm.stAsGeojson(out)),
      properties: JSON.parse(row.props),
    });
    if (features.length >= limit) break;
  }

  return {
    type: "FeatureCollection",
    features,
    // Deliberately visible: the point of the design is that `refined` stays
    // small relative to the table. If it does not, the tiling is wrong.
    stats: { candidates: rows.length, refined, matched: features.length },
  };
}
