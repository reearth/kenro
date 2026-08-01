// A B-tree-indexable stand-in for the R-tree that neither D1 nor Durable
// Object SQLite ships.
//
// Every feature is tagged with the Web Mercator tiles its bounding box
// covers at a fixed zoom, in a side table indexed on the tile id. A window
// query then becomes `WHERE cell IN (...)` — equality lookups, which a plain
// index serves perfectly — instead of the half-open `minx <= ?` range scan
// that bbox columns alone can give you.
//
// Features whose cover is too large (a country outline at z8) are filed
// under OVERSIZED and scanned on every query. That keeps the index honest:
// a bounded number of rows is always read, and correctness never depends on
// the cover being complete.

export const ZOOM = 8;
export const MAX_CELLS = 64;
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
 * Tile ids covering a WGS84 bounding box, or `[OVERSIZED]` if the cover
 * exceeds MAX_CELLS. Ids are `y * n + x`, which fits in a JS-safe integer
 * for any zoom below 26.
 */
export function cellsForBbox({ minx, miny, maxx, maxy }) {
  const n = 2 ** ZOOM;
  const x0 = tileX(minx, n);
  const x1 = tileX(maxx, n);
  // Web Mercator y grows southward: the north edge yields the smaller index.
  const y0 = tileY(maxy, n);
  const y1 = tileY(miny, n);

  if ((x1 - x0 + 1) * (y1 - y0 + 1) > MAX_CELLS) return [OVERSIZED];

  const cells = [];
  for (let y = y0; y <= y1; y++) {
    for (let x = x0; x <= x1; x++) cells.push(y * n + x);
  }
  return cells;
}

/** True if two bounding boxes overlap (the cheap pre-refine check). */
export function bboxOverlaps(a, b) {
  return a.minx <= b.maxx && a.maxx >= b.minx && a.miny <= b.maxy && a.maxy >= b.miny;
}
