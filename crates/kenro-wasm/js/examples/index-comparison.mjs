// Measures the two spatial indexes against each other and prints the table in
// docs/wasm.md and crates/kenro-wasm/cloudflare/README.md.
//
//   node examples/index-comparison.mjs           — print the tables
//   node examples/index-comparison.mjs --check    — assert the documented
//   numbers still hold (CI regression gate; fails if either index changes
//   shape without the docs following)
//
// Neither module touches wasm, so this needs no build. The fixture is
// synthetic but shaped like a real extract: a dense field of buildings, a few
// hundred long thin line features, and a handful of large polygons — the mix
// that makes a fixed grid hard to tune, because no single zoom suits all three.
//
// What the numbers mean: candidate rows the coarse filter hands to the exact
// predicate. Fewer is better, and `true hits` is the floor.

import * as quadtree from "../src/quadtree.mjs";
import * as tiles from "../src/tiles.mjs";

const CHECK = process.argv.includes("--check");

// Documented in docs/wasm.md. Tolerance is generous — this guards against a
// change of shape (a cliff appearing, an index regressing by an order of
// magnitude), not against arithmetic drift.
const DOCUMENTED = {
  0.005: { z10: 10417, z12: 826, z14: 106, quadtree: 485 },
  0.05: { z10: 13241, z12: 1905, z14: 616, quadtree: 1085 },
  0.25: { z10: 21997, z12: 8636, z14: 50520, quadtree: 10754 },
  1: { z10: 25868, z12: 50520, z14: 50520, quadtree: 30233 },
};
const TOLERANCE = 0.05;

const ZOOMS = [10, 12, 14];
const WINDOWS = [0.005, 0.02, 0.05, 0.1, 0.25, 0.5, 1];
const MAX_CELLS = 64;

const bbox = (minx, miny, maxx, maxy) => ({ minx, miny, maxx, maxy });

// Deterministic, so the committed numbers are reproducible on any machine.
function rng(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

function fixture() {
  const rand = rng(42);
  const features = [];
  // ~50 m buildings over a metropolitan area
  for (let i = 0; i < 50000; i++) {
    const x = 139.5 + rand() * 0.8;
    const y = 35.5 + rand() * 0.5;
    features.push(bbox(x, y, x + 0.0005, y + 0.0005));
  }
  // roads and rivers: long in one axis, thin in the other
  for (let i = 0; i < 500; i++) {
    const x = 139.5 + rand() * 0.8;
    const y = 35.5 + rand() * 0.5;
    features.push(bbox(x, y, x + rand() * 0.6, y + 0.002));
  }
  // administrative polygons, larger than any query window here
  for (let i = 0; i < 20; i++) {
    const x = 139.4 + rand() * 0.3;
    const y = 35.4 + rand() * 0.3;
    features.push(bbox(x, y, x + 0.5, y + 0.4));
  }
  return features;
}

/** The fixed grid at one zoom: cell id → features, exactly as SQL would hold it. */
function buildTiles(features, zoom) {
  const options = { zoom, maxCells: MAX_CELLS };
  const byCell = new Map();
  let rows = 0;
  for (const f of features) {
    for (const cell of tiles.cellsForFeature(f, options)) {
      rows++;
      if (!byCell.has(cell)) byCell.set(cell, []);
      byCell.get(cell).push(f);
    }
  }
  return {
    rows,
    candidates(window) {
      const cells = tiles.cellsForQuery(window, options);
      if (cells === null) return features.length; // over maxCells → table scan
      const hit = new Set();
      for (const cell of cells) for (const f of byCell.get(cell) ?? []) hit.add(f);
      return hit.size;
    },
  };
}

/** The quadtree: ids sorted, so a descendant range is a binary-search slice. */
function buildQuadtree(features) {
  const ids = [];
  const perCell = new Map();
  for (const f of features) {
    for (const cell of quadtree.cellsForFeature(f)) {
      ids.push(cell);
      perCell.set(cell, (perCell.get(cell) ?? 0) + 1);
    }
  }
  ids.sort((a, b) => a - b);
  const lowerBound = (v) => {
    let lo = 0;
    let hi = ids.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (ids[mid] < v) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  };
  return {
    rows: ids.length,
    candidates(window) {
      const { ancestors, ranges } = quadtree.cellsForQuery(window);
      let n = 0;
      for (const [lo, hi] of ranges) n += lowerBound(hi + 1) - lowerBound(lo);
      for (const cell of ancestors) n += perCell.get(cell) ?? 0;
      return n;
    },
    params(window) {
      return quadtree.cellFilterSql(window).params.length;
    },
  };
}

function measure() {
  const features = fixture();
  const indexes = { quadtree: buildQuadtree(features) };
  for (const z of ZOOMS) indexes[`z${z}`] = buildTiles(features, z);

  const rand = rng(4242);
  const rows = [];
  for (const size of WINDOWS) {
    const totals = Object.fromEntries(Object.keys(indexes).map((k) => [k, 0]));
    let hits = 0;
    let params = 0;
    const N = 200;
    for (let i = 0; i < N; i++) {
      const x = 139.5 + rand() * 0.8;
      const y = 35.5 + rand() * 0.5;
      const window = bbox(x, y, x + size, y + size);
      hits += features.filter((f) => quadtree.bboxOverlaps(f, window)).length;
      for (const [name, idx] of Object.entries(indexes)) totals[name] += idx.candidates(window);
      params += indexes.quadtree.params(window);
    }
    const mean = (v) => Math.round(v / N);
    rows.push({
      size,
      hits: mean(hits),
      params: mean(params),
      ...Object.fromEntries(Object.entries(totals).map(([k, v]) => [k, mean(v)])),
    });
  }
  return { rows, indexes, total: features.length };
}

function print({ rows, indexes, total }) {
  const cols = [...ZOOMS.map((z) => `z${z}`), "quadtree"];
  const head = ["window", "true hits", ...ZOOMS.map((z) => `\`tiles\` z${z}`), "`quadtree`"];
  console.log(`Candidate rows per window over ${total.toLocaleString()} features.`);
  console.log("Fewer is better; **bold** is the best index for that window.\n");
  console.log(`| ${head.join(" | ")} |`);
  console.log(`|${head.map(() => "---").join("|")}|`);
  for (const r of rows) {
    const best = Math.min(...cols.map((c) => r[c]));
    const cells = cols.map((c) => (r[c] === best ? `**${r[c].toLocaleString()}**` : r[c].toLocaleString()));
    console.log(`| ${r.size}° | ${r.hits.toLocaleString()} | ${cells.join(" | ")} |`);
  }

  console.log("\nIndex rows stored (one per feature is the floor):\n");
  console.log("| index | rows |");
  console.log("|---|---|");
  for (const z of ZOOMS) console.log(`| \`tiles\` z${z} | ${indexes[`z${z}`].rows.toLocaleString()} |`);
  console.log(`| \`quadtree\` | ${indexes.quadtree.rows.toLocaleString()} |`);

  const maxParams = Math.max(...rows.map((r) => r.params));
  console.log(`\nBound parameters per quadtree filter: ${maxParams} at most (D1 refuses at 100).`);
}

function check({ rows }) {
  const failures = [];
  for (const r of rows) {
    const expected = DOCUMENTED[r.size];
    if (!expected) continue;
    for (const [name, want] of Object.entries(expected)) {
      const got = r[name];
      const drift = Math.abs(got - want) / Math.max(want, 1);
      if (drift > TOLERANCE) {
        failures.push(`${r.size}° ${name}: documented ${want}, measured ${got} (${(drift * 100).toFixed(0)}% off)`);
      }
    }
  }
  // The property the whole design rests on: no window may cost more than the
  // table, and the quadtree must never be the one that falls off a cliff.
  for (const r of rows) {
    if (r.quadtree > 50520) failures.push(`${r.size}°: quadtree scanned more than the table (${r.quadtree})`);
    if (r.params > 90) failures.push(`${r.size}°: ${r.params} bound parameters, over the 90 budget`);
  }
  if (failures.length > 0) {
    console.error("index-comparison: the documented numbers no longer hold\n");
    for (const f of failures) console.error(`  ${f}`);
    console.error("\nRerun without --check, and update docs/wasm.md and the Cloudflare README.");
    process.exit(1);
  }
  console.log(`index-comparison: ${rows.length} window sizes match the documented numbers`);
}

const result = measure();
if (CHECK) check(result);
else print(result);
