// A hierarchical spatial index for hosts with no R-tree — Cloudflare D1,
// Durable Object SQLite, sql.js.
//
// `kenro-wasm/tiles` files every feature under the fixed-zoom tiles its bbox
// covers, so the zoom has to suit the whole dataset at once and a feature too
// large to enumerate needs a special OVERSIZED bucket. This module drops the
// fixed zoom: each feature is filed under the *deepest* quadtree cell that
// contains it, so a building lands in a small cell and a prefecture in a large
// one, with no parameter to choose and no special case for either.
//
// Two quadtree cells are always either nested or disjoint. So if a feature's
// bbox and a query window overlap, the feature's cell is necessarily an
// ancestor or a descendant of one of the window's cells — never off to the
// side. That is the whole correctness argument, and it is what makes the
// index complete:
//
//   ancestors   → a handful of equality lookups   (`cell IN (…)`)
//   descendants → one contiguous range per cell   (`cell BETWEEN ? AND ?`)
//
// Both are B-tree lookups on a plain INTEGER column. The range works because
// cell ids are a Hilbert code with a trailing sentinel bit marking the depth
// (the S2 design): the code's prefix *is* the ancestor cell, so a cell's
// descendants occupy the ids immediately around it.
//
// Hilbert rather than Z-order/Morton because adjacent cover cells then tend to
// be adjacent in id order, which lets neighbouring ranges merge — measured at
// ~40% fewer range terms for the same set of rows. The curve is confined to
// `hilbert()`; nothing else depends on which one it is.
//
// Nothing here is kenro-specific or async; it is plain arithmetic.

// The cheap bbox helpers are identical to the ones in the fixed-grid module;
// re-exported rather than copied so the two cannot drift.
export { bboxOverlaps, padBbox } from "./tiles.mjs";

/**
 * The depth cell ids are encoded at — a module constant, deliberately *not* an
 * option. Ids therefore never depend on how a caller configured anything, so
 * the write side and the query side cannot encode incompatible values.
 *
 * 2 * 24 + 1 = 49 bits, comfortably inside `Number.MAX_SAFE_INTEGER`, so ids
 * are ordinary JS numbers and SQLite stores them as INTEGER.
 */
export const CELL_DEPTH = 24;

/** Cells to file one feature under. One cell keeps writes at one row each. */
export const DEFAULT_FEATURE_MAX_CELLS = 1;

/** Cells to cover a query window with. More cells = tighter, longer SQL. */
export const DEFAULT_QUERY_MAX_CELLS = 16;

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

/** Longitude/latitude bounds of tile `(x, y)` at `depth`. */
function tileBounds(x, y, depth) {
  const n = 2 ** depth;
  const lat = (ty) => (Math.atan(Math.sinh(Math.PI * (1 - (2 * ty) / n))) * 180) / Math.PI;
  return { minx: (x / n) * 360 - 180, maxx: ((x + 1) / n) * 360 - 180, miny: lat(y + 1), maxy: lat(y) };
}

/**
 * Hilbert index of tile `(x, y)` at `depth`.
 *
 * Hierarchical: the high 2 bits are the quadrant at depth 1, so shifting off
 * 2k bits yields the index of the ancestor k levels up. That property is what
 * makes a prefix a cell, and it is verified in the tests rather than assumed.
 */
function hilbert(x, y, depth) {
  if (depth === 0) return 0n;
  const n = 2 ** depth;
  let h = 0n;
  let cx = x;
  let cy = y;
  for (let s = n / 2; s >= 1; s /= 2) {
    const rx = (cx & s) > 0 ? 1 : 0;
    const ry = (cy & s) > 0 ? 1 : 0;
    h += BigInt(s) * BigInt(s) * BigInt((3 * rx) ^ ry);
    // Reflect and transpose so the next level is expressed in the sub-square's
    // own orientation — this is what keeps the curve continuous.
    if (ry === 0) {
      if (rx === 1) {
        cx = n - 1 - cx;
        cy = n - 1 - cy;
      }
      const t = cx;
      cx = cy;
      cy = t;
    }
  }
  return h;
}

/**
 * Tile `(x, y, depth)` → cell id: the Hilbert code left-aligned in the id, then
 * a trailing 1 bit marking where the code stops. Without that sentinel a depth-3
 * cell and a depth-5 cell could be the same integer.
 */
function encode(x, y, depth) {
  return Number(((hilbert(x, y, depth) << 1n) | 1n) << BigInt(2 * (CELL_DEPTH - depth)));
}

/** The single deepest cell wholly containing `bbox`, as `{x, y, depth}`. */
function fitCell({ minx, miny, maxx, maxy }, maxDepth) {
  const n = 2 ** CELL_DEPTH;
  const x0 = tileX(minx, n);
  const x1 = tileX(maxx, n);
  // Web Mercator y grows southward: the north edge yields the smaller index.
  const y0 = tileY(maxy, n);
  const y1 = tileY(miny, n);

  // The deepest common cell is the common binary prefix of the two corners.
  let diff = (x0 ^ x1) | (y0 ^ y1);
  let bits = 0;
  while (diff > 0) {
    bits++;
    diff = Math.floor(diff / 2);
  }
  const shift = Math.max(bits, CELL_DEPTH - maxDepth);
  return { x: Math.floor(x0 / 2 ** shift), y: Math.floor(y0 / 2 ** shift), depth: CELL_DEPTH - shift };
}

function resolveDepth(maxDepth) {
  if (maxDepth === undefined) return CELL_DEPTH;
  if (!Number.isInteger(maxDepth) || maxDepth < 0 || maxDepth > CELL_DEPTH) {
    throw new RangeError(`maxDepth must be an integer in 0..${CELL_DEPTH}, got ${maxDepth}`);
  }
  return maxDepth;
}

/**
 * Cover `bbox` with at most `maxCells` cells, as deep as that budget allows,
 * returning `{x, y, depth}` records all at the same depth.
 */
function coverCells(bbox, maxCells, maxDepth) {
  let cells = [fitCell(bbox, maxDepth)];
  while (cells[0].depth < maxDepth) {
    const depth = cells[0].depth + 1;
    const next = [];
    for (const { x, y } of cells) {
      for (const [dx, dy] of [
        [0, 0],
        [1, 0],
        [0, 1],
        [1, 1],
      ]) {
        const cx = x * 2 + dx;
        const cy = y * 2 + dy;
        const b = tileBounds(cx, cy, depth);
        if (b.minx <= bbox.maxx && b.maxx >= bbox.minx && b.miny <= bbox.maxy && b.maxy >= bbox.miny) {
          next.push({ x: cx, y: cy, depth });
        }
      }
    }
    if (next.length > maxCells) break;
    cells = next;
  }
  return cells;
}

/**
 * Cell ids covering `bbox`, at most `maxCells` of them.
 *
 * Unlike the fixed-grid module this never returns `null` and never needs an
 * OVERSIZED bucket: a box too large to cover finely is simply covered by a
 * shallower cell, which is an ordinary cell like any other.
 */
export function quadCover(bbox, { maxCells = DEFAULT_QUERY_MAX_CELLS, maxDepth } = {}) {
  if (!Number.isInteger(maxCells) || maxCells < 1) {
    throw new RangeError(`maxCells must be a positive integer, got ${maxCells}`);
  }
  return coverCells(bbox, maxCells, resolveDepth(maxDepth)).map(({ x, y, depth }) => encode(x, y, depth));
}

/**
 * The cells to file a feature under. Defaults to a single cell, so one feature
 * is one row — the fixed-grid module writes one row per tile it spans.
 */
export function cellsForFeature(bbox, { maxCells = DEFAULT_FEATURE_MAX_CELLS, maxDepth } = {}) {
  return quadCover(bbox, { maxCells, maxDepth });
}

/**
 * The cells a feature's own cell may be found at, for a query window:
 *
 *   `ancestors` — coarser cells containing the window; equality lookups
 *   `ranges`    — `[lo, hi]` id ranges holding each cover cell and everything
 *                 below it; contiguous, so one B-tree range scan each
 *
 * Neither side of the index has to agree with the other on `maxCells` or
 * `maxDepth`: cell ids are encoded at the fixed `CELL_DEPTH`, and nesting makes
 * the result complete for any combination. There is nothing here to keep in
 * sync — unlike the fixed grid, where a mismatched `zoom` silently loses rows.
 */
export function cellsForQuery(bbox, { maxCells = DEFAULT_QUERY_MAX_CELLS, maxDepth } = {}) {
  if (!Number.isInteger(maxCells) || maxCells < 1) {
    throw new RangeError(`maxCells must be a positive integer, got ${maxCells}`);
  }
  const cells = coverCells(bbox, maxCells, resolveDepth(maxDepth));

  const ancestors = new Set();
  for (const { x, y, depth } of cells) {
    for (let up = 1; up <= depth; up++) {
      ancestors.add(encode(Math.floor(x / 2 ** up), Math.floor(y / 2 ** up), depth - up));
    }
  }

  // A cell's descendants are the ids within half a cell's span either side of
  // it — `lsb` is where the sentinel sits, which encodes the depth.
  const spans = cells
    .map(({ x, y, depth }) => {
      const id = encode(x, y, depth);
      const lsb = 2 ** (2 * (CELL_DEPTH - depth));
      return [id - lsb + 1, id + lsb - 1];
    })
    .sort((a, b) => a[0] - b[0]);

  // Neighbouring cover cells are separated by a few ids belonging to coarser
  // cells, which are already candidates via `ancestors`. Swallowing gaps that
  // small therefore costs no precision and merges the ranges into fewer, longer
  // scans. Bounded by one cell's span so distant cells never merge.
  const tolerance = 2 ** (2 * (CELL_DEPTH - cells[0].depth) + 1);
  const ranges = [spans[0].slice()];
  for (const span of spans.slice(1)) {
    const last = ranges[ranges.length - 1];
    if (span[0] <= last[1] + 1 + tolerance) last[1] = Math.max(last[1], span[1]);
    else ranges.push(span.slice());
  }

  return { ancestors: [...ancestors], ranges, wholeTable: cells.length === 1 && cells[0].depth === 0 };
}

/**
 * The depth a cell id sits at — 0 is the whole world, `CELL_DEPTH` the finest.
 *
 * The sentinel is the lowest set bit, so the depth is readable straight off
 * the id. Useful for reporting: the shallowest cell in a table is the feature
 * that is a candidate for the widest range of queries, which is the thing to
 * look at when a query refines more rows than it should. SQL can find it
 * without this function — `SELECT max(cell & -cell) FROM …` — and then this
 * turns the answer into a depth.
 */
export function cellDepth(cell) {
  if (!Number.isSafeInteger(cell) || cell <= 0) {
    throw new RangeError(`not a cell id: ${cell}`);
  }
  const big = BigInt(cell);
  let lsb = big & -big;
  let depth = CELL_DEPTH;
  while (lsb > 1n) {
    lsb >>= 2n;
    depth--;
  }
  return depth;
}

const IDENT = /^[A-Za-z_][A-Za-z0-9_]*$/;

function ident(name, what) {
  if (typeof name !== "string" || !IDENT.test(name)) {
    throw new Error(`${what} must be a plain SQL identifier, got ${JSON.stringify(name)}`);
  }
  return name;
}

/**
 * Bound parameters a single statement may use. D1 and Durable Object SQLite
 * both refuse at 100 ("too many SQL variables"), and the caller needs a few of
 * its own, so the filter keeps itself under this.
 */
export const DEFAULT_MAX_PARAMS = 90;

/**
 * The coarse filter as ready-to-run SQL selecting feature ids — the shape the
 * index wants, so callers do not have to rediscover it:
 *
 *     const { sql, params } = cellFilterSql(bbox);
 *     const rows = await db.prepare(
 *       `SELECT * FROM features WHERE id IN (${sql})`).bind(...params).all();
 *
 * One statement with `OR`, not a `UNION` per range: D1 and Durable Object
 * SQLite allow only **five** terms in a compound SELECT, which a cover of any
 * useful size blows straight through. SQLite plans the `OR` as one index
 * lookup per term, so the shape costs nothing.
 *
 * If the cover would need more parameters than `maxParams`, it is recomputed
 * with a smaller budget until it fits. That widens the search — never narrows
 * it — so the result stays complete; only precision gives way, and only for
 * windows shaped awkwardly enough to need dozens of cells.
 *
 * Table and column names are interpolated, so they are checked against a plain
 * identifier pattern; everything else is bound.
 */
export function cellFilterSql(
  bbox,
  { table = "feature_cells", cell = "cell", id = "id", maxParams = DEFAULT_MAX_PARAMS, ...options } = {},
) {
  const t = ident(table, "table");
  const c = ident(cell, "cell column");
  const i = ident(id, "id column");

  let maxCells = options.maxCells ?? DEFAULT_QUERY_MAX_CELLS;
  let query = cellsForQuery(bbox, { ...options, maxCells });
  const cost = (q) => q.ancestors.length + q.ranges.length * 2;
  while (cost(query) > maxParams && maxCells > 1) {
    maxCells = Math.floor(maxCells / 2);
    query = cellsForQuery(bbox, { ...options, maxCells });
  }
  const { ancestors, ranges, wholeTable } = query;

  if (wholeTable) return { sql: `SELECT ${i} FROM ${t}`, params: [], wholeTable: true };

  const terms = [];
  const params = [];
  if (ancestors.length > 0) {
    terms.push(`${c} IN (${ancestors.map(() => "?").join(", ")})`);
    params.push(...ancestors);
  }
  for (const [lo, hi] of ranges) {
    terms.push(`${c} BETWEEN ? AND ?`);
    params.push(lo, hi);
  }
  // DISTINCT because a feature filed under several cells could match more than
  // one term — `UNION` used to fold those together for free.
  return { sql: `SELECT DISTINCT ${i} FROM ${t} WHERE ${terms.join(" OR ")}`, params, wholeTable: false };
}
