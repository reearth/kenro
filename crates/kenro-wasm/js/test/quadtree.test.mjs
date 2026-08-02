// kenro-wasm/quadtree — the hierarchical R-tree stand-in. No wasm involved.
//
// The contract is one sentence: if two boxes overlap, the feature's cell is in
// the query's candidate set. Most of this file is that property, hammered with
// random boxes, because everything else about the module is in service of it.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  CELL_DEPTH,
  DEFAULT_FEATURE_MAX_CELLS,
  DEFAULT_QUERY_MAX_CELLS,
  bboxOverlaps,
  cellDepth,
  cellFilterSql,
  cellsForFeature,
  cellsForQuery,
  padBbox,
  quadCover,
} from "../src/quadtree.mjs";

const bbox = (minx, miny, maxx, maxy) => ({ minx, miny, maxx, maxy });

/** Does the query's candidate set contain `cell`? This is what SQL will ask. */
function selects({ ancestors, ranges }, cell) {
  return ancestors.includes(cell) || ranges.some(([lo, hi]) => cell >= lo && cell <= hi);
}

// A deterministic PRNG: a failing property test must be reproducible.
function rng(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

test("cell ids stay safe integers at every depth", () => {
  for (let depth = 0; depth <= CELL_DEPTH; depth++) {
    const cells = quadCover(bbox(139.7, 35.68, 139.7, 35.68), { maxDepth: depth, maxCells: 1 });
    assert.equal(cells.length, 1);
    assert.ok(Number.isSafeInteger(cells[0]), `depth ${depth} produced ${cells[0]}`);
    assert.ok(cells[0] > 0);
  }
});

test("the curve is a bijection, continuous, and hierarchical", () => {
  // Probed through the public API: one cell per tile at a small depth.
  const order = 4;
  const n = 2 ** order;
  const at = (x, y) => {
    const b = tileCentre(x, y, order);
    return quadCover(b, { maxDepth: order, maxCells: 1 })[0];
  };
  const ids = [];
  for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) ids.push(at(x, y));
  assert.equal(new Set(ids).size, n * n, "ids collide: not a bijection");

  // Continuity: consecutive ids are 4-adjacent tiles. Only a real Hilbert
  // curve passes this; Z-order does not, and neither does a wrong state table.
  const byId = new Map();
  for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) byId.set(at(x, y), [x, y]);
  const sorted = [...byId.keys()].sort((a, b) => a - b);
  for (let i = 1; i < sorted.length; i++) {
    const [ax, ay] = byId.get(sorted[i - 1]);
    const [bx, by] = byId.get(sorted[i]);
    assert.equal(Math.abs(ax - bx) + Math.abs(ay - by), 1, `ids ${i - 1}→${i} are not adjacent tiles`);
  }

  // Hierarchical: a tile's cell must sit inside its parent's descendant range.
  for (let y = 0; y < n; y++) {
    for (let x = 0; x < n; x++) {
      const child = at(x, y);
      const parent = quadCover(tileCentre(x, y, order), { maxDepth: order - 1, maxCells: 1 })[0];
      const q = cellsForQuery(tileCentre(x, y, order), { maxDepth: order - 1, maxCells: 1 });
      assert.ok(selects(q, child), `child of ${parent} not under its parent`);
    }
  }
});

function tileCentre(x, y, depth) {
  const n = 2 ** depth;
  const lon = ((x + 0.5) / n) * 360 - 180;
  const lat = (Math.atan(Math.sinh(Math.PI * (1 - (2 * (y + 0.5)) / n))) * 180) / Math.PI;
  return bbox(lon, lat, lon, lat);
}

test("overlapping boxes always meet — the whole contract", () => {
  const rand = rng(20260802);
  let pairs = 0;
  for (let i = 0; i < 60000; i++) {
    // Anchor the window near the feature, or overlaps would be too rare to
    // exercise the property; sizes span eight orders of magnitude so shallow
    // and deep cells, and every straddle of a grid line, all get hit.
    const scale = 10 ** (-4 + rand() * 6);
    const fx = -179 + rand() * 358;
    const fy = -84 + rand() * 168;
    const f = bbox(fx, fy, Math.min(179.9, fx + rand() * scale), Math.min(84.9, fy + rand() * scale));
    const wx = fx + (rand() - 0.5) * scale * 2;
    const wy = fy + (rand() - 0.5) * scale * 2;
    const w = bbox(wx, wy, Math.min(179.9, wx + rand() * scale), Math.min(84.9, wy + rand() * scale));
    if (!bboxOverlaps(f, w)) continue;
    pairs++;
    const cells = cellsForFeature(f);
    const q = cellsForQuery(w);
    assert.ok(
      cells.some((c) => selects(q, c)),
      `missed: feature ${JSON.stringify(f)} vs window ${JSON.stringify(w)}`,
    );
  }
  assert.ok(pairs > 1000, `only ${pairs} overlapping pairs generated`);
});

test("the two sides need not agree on maxCells or maxDepth", () => {
  // The fixed-grid module loses rows silently when the sides disagree. Here
  // ids are encoded at a constant depth, so any combination stays complete.
  const rand = rng(7);
  const combos = [
    [{ maxCells: 1 }, { maxCells: 64 }],
    [{ maxCells: 32 }, { maxCells: 1 }],
    [{ maxDepth: 6 }, { maxDepth: 24 }],
    [{ maxDepth: 24, maxCells: 4 }, { maxDepth: 3 }],
  ];
  for (const [fOpts, qOpts] of combos) {
    let pairs = 0;
    for (let i = 0; i < 4000; i++) {
      const box = () => {
        const x = 130 + rand() * 12;
        const y = 30 + rand() * 12;
        return bbox(x, y, x + rand() ** 3 * 6, y + rand() ** 3 * 6);
      };
      const f = box();
      const w = box();
      if (!bboxOverlaps(f, w)) continue;
      pairs++;
      assert.ok(
        cellsForFeature(f, fOpts).some((c) => selects(cellsForQuery(w, qOpts), c)),
        `missed with ${JSON.stringify(fOpts)} / ${JSON.stringify(qOpts)}`,
      );
    }
    assert.ok(pairs > 100);
  }
});

test("a feature is one row by default, and covers respect their budget", () => {
  assert.equal(DEFAULT_FEATURE_MAX_CELLS, 1);
  assert.equal(cellsForFeature(bbox(139.6, 35.6, 139.8, 35.8)).length, 1);
  for (const maxCells of [1, 4, 16, 64]) {
    const cells = quadCover(bbox(139.0, 35.0, 141.0, 36.0), { maxCells });
    assert.ok(cells.length <= maxCells, `${cells.length} cells exceeds ${maxCells}`);
    assert.equal(new Set(cells).size, cells.length, "duplicate cells in a cover");
  }
});

test("a tighter query cover selects fewer ids, never fewer true hits", () => {
  const window = bbox(139.70, 35.68, 139.72, 35.70);
  const span = ({ ranges }) => ranges.reduce((n, [lo, hi]) => n + (hi - lo + 1), 0);
  const coarse = cellsForQuery(window, { maxCells: 1 });
  const fine = cellsForQuery(window, { maxCells: DEFAULT_QUERY_MAX_CELLS });
  assert.ok(span(fine) < span(coarse), "a bigger budget should narrow the scan");
});

test("no OVERSIZED bucket and no null: a huge feature is just a shallow cell", () => {
  const world = cellsForFeature(bbox(-180, -85, 180, 85));
  assert.equal(world.length, 1);
  assert.ok(world[0] > 0, "cell ids are always positive — no -1 sentinel");
  // and it is a candidate for any query, by being an ancestor
  const q = cellsForQuery(bbox(139.7, 35.68, 139.701, 35.681));
  assert.ok(selects(q, world[0]), "the root cell must answer every query");
});

test("a world-sized window degrades to a table scan, explicitly", () => {
  const q = cellsForQuery(bbox(-180, -85, 180, 85), { maxCells: 1 });
  assert.equal(q.wholeTable, true);
  const { sql, params } = cellFilterSql(bbox(-180, -85, 180, 85), { maxCells: 1 });
  assert.equal(sql, "SELECT id FROM feature_cells");
  assert.deepEqual(params, []);
});

test("north edge yields the smaller y: covers are not flipped", () => {
  const north = cellsForFeature(bbox(139.7, 40.0, 139.71, 40.01));
  const south = cellsForFeature(bbox(139.7, 30.0, 139.71, 30.01));
  assert.notDeepEqual(north, south);
  // A box spanning both must be an ancestor of each.
  const both = cellsForFeature(bbox(139.7, 30.0, 139.71, 40.01))[0];
  for (const b of [bbox(139.7, 40.0, 139.71, 40.01), bbox(139.7, 30.0, 139.71, 30.01)]) {
    assert.ok(selects(cellsForQuery(b, { maxCells: 1 }), both));
  }
});

test("latitudes beyond the Mercator limit are clamped, not infinite", () => {
  for (const cell of cellsForFeature(bbox(-10, -90, 10, 90))) {
    assert.ok(Number.isSafeInteger(cell));
  }
});

test("cellFilterSql binds every value and rejects injected identifiers", () => {
  const { sql, params } = cellFilterSql(bbox(139.70, 35.68, 139.72, 35.70));
  assert.equal(sql.match(/\?/g).length, params.length);
  assert.ok(sql.includes("BETWEEN ? AND ?"));
  assert.ok(params.every(Number.isSafeInteger));
  for (const bad of ["feature_cells; DROP TABLE features", "a b", "", 1]) {
    assert.throws(() => cellFilterSql(bbox(0, 0, 1, 1), { table: bad }), /plain SQL identifier/);
  }
  const custom = cellFilterSql(bbox(0, 0, 1, 1), { table: "t", cell: "c", id: "fid" });
  assert.ok(custom.sql.includes("SELECT fid FROM t WHERE c"));
});

test("bad options are rejected rather than silently coerced", () => {
  assert.throws(() => quadCover(bbox(0, 0, 1, 1), { maxCells: 0 }), /positive integer/);
  assert.throws(() => quadCover(bbox(0, 0, 1, 1), { maxCells: 2.5 }), /positive integer/);
  assert.throws(() => quadCover(bbox(0, 0, 1, 1), { maxDepth: 25 }), /0\.\.24/);
  assert.throws(() => quadCover(bbox(0, 0, 1, 1), { maxDepth: -1 }), /0\.\.24/);
});

test("cellDepth reads the depth back off an id", () => {
  assert.equal(cellDepth(cellsForFeature(bbox(-180, -85, 180, 85))[0]), 0);
  for (let depth = 0; depth <= CELL_DEPTH; depth++) {
    const [cell] = quadCover(bbox(139.7, 35.68, 139.7, 35.68), { maxDepth: depth, maxCells: 1 });
    assert.equal(cellDepth(cell), depth);
  }
  // A tight box goes deep; a continent-sized one stays shallow.
  assert.ok(cellDepth(cellsForFeature(bbox(139.7, 35.68, 139.7009, 35.6809))[0]) > 12);
  assert.ok(cellDepth(cellsForFeature(bbox(-20, -35, 55, 37))[0]) < 4);
  for (const bad of [0, -1, 1.5, NaN]) assert.throws(() => cellDepth(bad), /not a cell id/);
});

test("padBbox and bboxOverlaps behave as the fixed-grid module's", () => {
  assert.deepEqual(padBbox(bbox(0, 0, 1, 1), 0.5), bbox(-0.5, -0.5, 1.5, 1.5));
  assert.equal(bboxOverlaps(bbox(0, 0, 1, 1), bbox(1, 1, 2, 2)), true);
  assert.equal(bboxOverlaps(bbox(0, 0, 1, 1), bbox(1.1, 0, 2, 1)), false);
});
