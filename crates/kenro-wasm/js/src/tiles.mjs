// A B-tree-indexable stand-in for SQLite's R-tree, for the hosts that don't
// have one: sql.js ships no R-tree module, and neither Cloudflare D1 nor
// Durable Object SQLite exposes one (or user-defined functions).
//
// Tag every feature with the Web Mercator tiles its bounding box covers at a
// fixed zoom, in a side table indexed on the tile id. A window query is then
// `WHERE cell IN (...)` — equality lookups, which a plain index serves
// exactly — instead of the half-open `minx <= ?` range scan that bbox
// columns alone degrade into. kenro computes the bounding box (`ST_MinX` &
// co., free on a GeoPackage blob, whose header carries an envelope); this
// module turns it into integers SQL can index.
//
// The tile grid is internal to the index. Queries take an arbitrary bounding
// box — any size, aligned to nothing — and its cover is computed the same
// way. Tiles are how rows are found, never what can be asked for.
//
// Nothing here is kenro-specific or async; it is plain arithmetic.

export const DEFAULT_ZOOM = 8;
export const DEFAULT_MAX_CELLS = 64;

/**
 * The cell a feature is filed under when its cover is too large to
 * enumerate. Always include it in the candidate set — see `cellsForFeature`.
 */
export const OVERSIZED = -1;

function clampLat(lat) {
  return Math.min(85.05112878, Math.max(-85.05112878, lat));
}

function tileX(lon, n) {
  return Math.min(n - 1, Math.max(0, Math.floor(((lon + 180) / 360) * n)));
}

function tileY(lat, n) {
  const rad = (clampLat(lat) * Math.PI) / 180;
  const y = ((1 - Math.log(Math.tan(rad) + 1 / Math.cos(rad)) / Math.PI) / 2) * n;
  return Math.min(n - 1, Math.max(0, Math.floor(y)));
}

/**
 * Tile ids covering a WGS84 bounding box `{minx, miny, maxx, maxy}`, or
 * `null` if the cover would exceed `maxCells`. Ids are `y * 2**zoom + x`,
 * which stays a safe integer for any zoom below 26.
 *
 * `null` means "too big to enumerate", and the two sides of the index must
 * read it differently — this is the one thing to get right here:
 *
 *   write side → file the feature under OVERSIZED (`cellsForFeature`)
 *   query side → drop the cell filter and scan (`cellsForQuery`)
 *
 * Reading `null` as OVERSIZED on the query side is the subtle bug: that
 * bucket holds only the features too big to file, so a large window would
 * return a handful of continent-sized rows and silently miss the rest.
 */
export function tileCover({ minx, miny, maxx, maxy }, { zoom = DEFAULT_ZOOM, maxCells = DEFAULT_MAX_CELLS } = {}) {
  const n = 2 ** zoom;
  const x0 = tileX(minx, n);
  const x1 = tileX(maxx, n);
  // Web Mercator y grows southward: the north edge yields the smaller index.
  const y0 = tileY(maxy, n);
  const y1 = tileY(miny, n);

  if ((x1 - x0 + 1) * (y1 - y0 + 1) > maxCells) return null;

  const cells = [];
  for (let y = y0; y <= y1; y++) {
    for (let x = x0; x <= x1; x++) cells.push(y * n + x);
  }
  return cells;
}

/**
 * Cells to store a feature under: its cover, or `[OVERSIZED]` when that is
 * too large. An OVERSIZED feature is a candidate for every query, so the
 * index stays complete at the cost of scanning a bounded few.
 */
export function cellsForFeature(bbox, options) {
  return tileCover(bbox, options) ?? [OVERSIZED];
}

/**
 * Cells to search for a query window: its cover *plus* OVERSIZED, or `null`
 * meaning "too large — drop the cell filter and scan the table".
 *
 *     const cells = cellsForQuery(bbox);
 *     const sql = cells
 *       ? `SELECT … WHERE id IN (SELECT id FROM feature_cells
 *                                 WHERE cell IN (${cells.map(() => "?")}))`
 *       : `SELECT … FROM features`;
 */
export function cellsForQuery(bbox, options) {
  const cover = tileCover(bbox, options);
  return cover === null ? null : [...cover, OVERSIZED];
}

/** True if two bounding boxes overlap — the cheap reject before any wasm call. */
export function bboxOverlaps(a, b) {
  return a.minx <= b.maxx && a.maxx >= b.minx && a.miny <= b.maxy && a.maxy >= b.miny;
}

/** Grow a bounding box by `d` on every side (the ST_DWithin search area). */
export function padBbox({ minx, miny, maxx, maxy }, d) {
  return { minx: minx - d, miny: miny - d, maxx: maxx + d, maxy: maxy + d };
}
