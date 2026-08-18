// Size regression gate: the "KB-scale, ~30x smaller than DuckDB-WASM" claim
// in docs/wasm.md is enforced, not aspirational. Fails when the built wasm
// exceeds the thresholds; prints the measured table either way.

import { readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";

// The full tier carries the whole EPSG registry (crs-full), so a national
// grid works without a rebuild; that is ~155 KB of the gzip budget. Raised
// deliberately, with the measured table in docs/wasm.md — still an order of
// magnitude under mod_spatialite's ~25 MB chain.
//
// Raised again for `voronoi`, which measured +52 KB raw / +11 KB gzip — the
// same class as `concave-hull` (+41 KB) and `delaunay` (+81 KB), and behind
// its own feature, so no build that does not ask for it pays. Note which
// number moved: the **wire** size is what a browser downloads and it still
// has headroom, so the gzip cap is deliberately left where it was. If a
// change ever needs the gzip cap raised, that is the one to argue about.
//
// 2026-08-19: the gzip cap raised, and here is the argument. What crossed it
// is not a gated feature anyone can decline: ST_3DArea/kenro_volume, box3d
// text in the box accessors, and ST_QuantizeCoordinates are ungated
// PostGIS-parity surface, ~5 KB of wire between them against a 700 KB cap.
// CI's toolchain also gzips ~3 KB larger than the local builds the old caps
// were calibrated on (701,810 measured in CI where 699,114 measured
// locally), so the caps now carry CI-vs-local headroom explicitly.
const MAX_RAW_BYTES = 2_350_000;
const MAX_GZIP_BYTES = 715_000;

const wasmPath = new URL("../pkg/kenro_wasm_bg.wasm", import.meta.url);
const raw = readFileSync(wasmPath);
const gzipped = gzipSync(raw, { level: 9 });

const kb = (n) => `${(n / 1024).toFixed(0)} KB`;
console.log("| artifact | size |");
console.log("|---|---|");
console.log(`| kenro_wasm_bg.wasm | ${kb(raw.length)} |`);
console.log(`| gzipped | ${kb(gzipped.length)} |`);

if (raw.length > MAX_RAW_BYTES || gzipped.length > MAX_GZIP_BYTES) {
  console.error(
    `size gate FAILED: raw ${raw.length} (max ${MAX_RAW_BYTES}), ` +
      `gzip ${gzipped.length} (max ${MAX_GZIP_BYTES})`,
  );
  process.exit(1);
}
console.log("size gate OK");
