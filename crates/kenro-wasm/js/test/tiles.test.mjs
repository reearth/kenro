// kenro-wasm/tiles — the R-tree stand-in. No wasm involved; plain arithmetic.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  DEFAULT_ZOOM,
  OVERSIZED,
  bboxOverlaps,
  cellsForFeature,
  cellsForQuery,
  padBbox,
  tileCover,
} from "../src/tiles.mjs";

const bbox = (minx, miny, maxx, maxy) => ({ minx, miny, maxx, maxy });

test("a point covers exactly one cell", () => {
  const cells = tileCover(bbox(139.7, 35.68, 139.7, 35.68));
  assert.equal(cells.length, 1);
});

test("cell ids are y * 2**zoom + x, and distinct per tile", () => {
  const n = 2 ** DEFAULT_ZOOM;
  const cells = tileCover(bbox(139.0, 35.0, 141.0, 36.0));
  assert.ok(cells.length > 1);
  assert.equal(new Set(cells).size, cells.length);
  for (const c of cells) {
    assert.ok(Number.isSafeInteger(c) && c >= 0 && c < n * n, `cell ${c} out of range`);
  }
});

test("a feature's cover and a query's cover meet wherever they overlap", () => {
  // The index is only correct if these two intersect for any overlapping
  // pair of boxes — that is the whole contract.
  const feature = cellsForFeature(bbox(139.6, 35.6, 139.8, 35.8));
  const query = cellsForQuery(bbox(139.75, 35.75, 140.2, 36.1));
  assert.ok(feature.some((c) => query.includes(c)));
});

test("north edge yields the smaller y: covers are not flipped", () => {
  const north = tileCover(bbox(139.7, 35.9, 139.7, 35.9))[0];
  const south = tileCover(bbox(139.7, 35.1, 139.7, 35.1))[0];
  assert.ok(north < south, "Web Mercator y must grow southward");
});

test("latitude beyond the Mercator limit is clamped, not NaN", () => {
  for (const cell of tileCover(bbox(-179, -89.9, 179, 89.9), { maxCells: 1e9 })) {
    assert.ok(Number.isSafeInteger(cell), `got ${cell}`);
  }
});

test("an over-large cover is null, and the two sides read it differently", () => {
  const huge = bbox(-170, -80, 170, 80);
  assert.equal(tileCover(huge), null);
  // Write side: file it under OVERSIZED so it stays a candidate for everything.
  assert.deepEqual(cellsForFeature(huge), [OVERSIZED]);
  // Query side: null means "drop the cell filter and scan". Returning
  // [OVERSIZED] here would match only the features too big to file — the
  // large-window bug this module exists to spell out.
  assert.equal(cellsForQuery(huge), null);
});

test("a query's cell list always includes OVERSIZED", () => {
  // Otherwise a window would miss the features filed there.
  assert.ok(cellsForQuery(bbox(139.7, 35.6, 139.8, 35.7)).includes(OVERSIZED));
});

test("zoom and maxCells are tunable", () => {
  const b = bbox(139.0, 35.0, 141.0, 36.0);
  assert.ok(tileCover(b, { zoom: 4 }).length < tileCover(b, { zoom: 10 }).length);
  assert.equal(tileCover(b, { zoom: 10, maxCells: 4 }), null);
});

test("bboxOverlaps is inclusive at the edges", () => {
  assert.equal(bboxOverlaps(bbox(0, 0, 1, 1), bbox(1, 1, 2, 2)), true);
  assert.equal(bboxOverlaps(bbox(0, 0, 1, 1), bbox(1.001, 1, 2, 2)), false);
});

test("padBbox grows every side", () => {
  assert.deepEqual(padBbox(bbox(1, 1, 2, 2), 0.5), bbox(0.5, 0.5, 2.5, 2.5));
});
