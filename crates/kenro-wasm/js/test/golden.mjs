// Shared helpers for the kenro-wasm test suites: golden-vector loading
// (BigInt-preserving), geometry/number comparison, wasm init, and the
// per-function smoke SQL catalog.

import { readFileSync } from "node:fs";

/** Initialize the built wasm package (crates/kenro-wasm/js/pkg). */
export async function initWasm() {
  const wasm = await import("../pkg/kenro_wasm.js");
  const bytes = readFileSync(
    new URL("../pkg/kenro_wasm_bg.wasm", import.meta.url),
  );
  await wasm.default({ module_or_path: bytes });
  return wasm;
}

/**
 * Load tests/golden/<suite>.jsonl from the repo root. Integers that do not
 * fit a double (H3 cell ids) are preserved as BigInt via the JSON.parse
 * source-access reviver (Node >= 21).
 */
export function loadVectors(suite) {
  const path = new URL(
    `../../../../tests/golden/${suite}.jsonl`,
    import.meta.url,
  );
  const vectors = [];
  for (const line of readFileSync(path, "utf8").split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const value = JSON.parse(trimmed, (_key, v, ctx) =>
      typeof v === "number" &&
      Number.isInteger(v) &&
      !Number.isSafeInteger(v) &&
      ctx?.source
        ? BigInt(ctx.source)
        : v,
    );
    if (value.fn === undefined) continue; // provenance header
    vectors.push(value);
  }
  if (vectors.length === 0) throw new Error(`${suite}: no vectors`);
  return vectors;
}

export function effective(vector) {
  return vector.kenro_expected ?? vector.expected;
}

/**
 * A vector may need a cargo feature the build under test lacks (`crs-full`,
 * say). Rather than teach the runners which features are compiled in — the
 * manifest does not say — run it and skip when the failure names the feature.
 */
export function skipForMissingFeature(vector, run) {
  if (!vector.needs_feature) return false;
  try {
    run();
    return false;
  } catch (e) {
    if (String(e.message ?? e).includes(vector.needs_feature)) return true;
    throw e;
  }
}

export function expectsError(vector) {
  const want = effective(vector);
  return typeof want === "object" && want !== null && "error" in want;
}

export function assertNumberClose(id, got, want, relTol = 1e-12) {
  const tol = relTol * Math.max(1, Math.abs(want));
  if (!(Math.abs(got - want) <= tol)) {
    throw new Error(`${id}: got ${got}, want ${want} (tolerance ${tol})`);
  }
}

const NUMBER_RE = /-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g;

function wktShape(wkt) {
  return {
    type: wkt.split(/[\s(]/, 1)[0],
    numbers: (wkt.match(NUMBER_RE) ?? []).map(Number),
  };
}

/** Geometric WKT comparison: same type, coordinates within tolerance. */
export function geomApproxEqual(a, b, relTol = 1e-9) {
  if (a === b) return true;
  const ga = wktShape(a);
  const gb = wktShape(b);
  return (
    ga.type === gb.type &&
    ga.numbers.length === gb.numbers.length &&
    ga.numbers.every((n, i) => {
      const w = gb.numbers[i];
      return Math.abs(n - w) <= relTol * Math.max(1, Math.abs(w));
    })
  );
}

/** Vertex-multiset comparison for convex rings (rotation/direction-insensitive). */
export function geomSameVertexSet(a, b, relTol = 1e-9) {
  if (a === b) return true;
  const shape = (s) => {
    const type = s.split(/[\s(]/, 1)[0];
    const nums = (s.match(NUMBER_RE) ?? []).map(Number);
    const pairs = [];
    for (let i = 0; i + 1 < nums.length; i += 2) pairs.push([nums[i], nums[i + 1]]);
    pairs.sort((p, q) => p[0] - q[0] || p[1] - q[1]);
    const dedup = [];
    for (const p of pairs) {
      const last = dedup[dedup.length - 1];
      if (!last || Math.abs(last[0] - p[0]) > relTol || Math.abs(last[1] - p[1]) > relTol) {
        dedup.push(p);
      }
    }
    return { type, dedup };
  };
  const ga = shape(a);
  const gb = shape(b);
  return (
    ga.type === gb.type &&
    ga.dedup.length === gb.dedup.length &&
    ga.dedup.every((p, i) => {
      const q = gb.dedup[i];
      const t = (v) => relTol * Math.max(1, Math.abs(v));
      return Math.abs(p[0] - q[0]) <= t(q[0]) && Math.abs(p[1] - q[1]) <= t(q[1]);
    })
  );
}

const GEOGRAPHIC_SRIDS = new Set([4326]);

/** Transform comparison: per-vertex error in meters (mirror of the Rust harness). */
export function assertWithinToleranceMeters(id, got, want, toSrid, tolM = 0.01) {
  const ga = wktShape(got);
  const gb = wktShape(want);
  if (ga.numbers.length !== gb.numbers.length) {
    throw new Error(`${id}: vertex count mismatch`);
  }
  for (let i = 0; i < ga.numbers.length; i += 2) {
    const [px, py] = [ga.numbers[i], ga.numbers[i + 1]];
    const [qx, qy] = [gb.numbers[i], gb.numbers[i + 1]];
    let dx = px - qx;
    let dy = py - qy;
    if (GEOGRAPHIC_SRIDS.has(toSrid)) {
      dx *= 111_320 * Math.cos((qy * Math.PI) / 180);
      dy *= 110_540;
    }
    const err = Math.hypot(dx, dy);
    if (!(err <= tolM)) {
      throw new Error(`${id}: ${err} m off at (${px}, ${py}) vs (${qx}, ${qy})`);
    }
  }
}

/**
 * Smoke SQL per (function, arity): every manifest function must appear here
 * — the host smokes assert coverage, so adding a function without a smoke
 * fails CI. `check` receives the query result.
 */
export const SMOKE_SQL = {
  "ST_GeomFromText/1": {
    sql: "SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_GeomFromText/2": {
    sql: "SELECT ST_SRID(ST_GeomFromText('POINT(1 2)', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_GeomFromWKB/1": {
    sql: "SELECT ST_AsText(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)'))))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_GeomFromWKB/2": {
    sql: "SELECT ST_SRID(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)')), 3857))",
    check: (v) => Number(v) === 3857,
  },
  "ST_GeomFromGPB/1": {
    sql: "SELECT ST_AsText(ST_GeomFromGPB(ST_AsGPB(ST_GeomFromText('POINT(1 2)'))))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_AsText/1": {
    sql: "SELECT ST_AsText(ST_GeomFromText('LINESTRING(0 0,1 1)'))",
    check: (v) => v === "LINESTRING(0 0,1 1)",
  },
  "ST_AsBinary/1": {
    sql: "SELECT length(ST_AsBinary(ST_GeomFromText('POINT(1 2)')))",
    check: (v) => Number(v) === 21,
  },
  "ST_AsGPB/1": {
    sql: "SELECT length(ST_AsGPB(ST_GeomFromText('POINT(1 2)'))) > 0",
    check: (v) => Number(v) === 1,
  },
  "ST_SetSRID/2": {
    sql: "SELECT ST_SRID(ST_SetSRID(ST_GeomFromText('POINT(1 2)'), 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_SRID/1": {
    sql: "SELECT ST_SRID(ST_GeomFromText('POINT(1 2)'))",
    check: (v) => Number(v) === 0,
  },
  "ST_Intersects/2": {
    sql: "SELECT ST_Intersects(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_GeomFromText('POINT(2 2)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Contains/2": {
    sql: "SELECT ST_Contains(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_GeomFromText('POINT(2 2)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Within/2": {
    sql: "SELECT ST_Within(ST_GeomFromText('POINT(2 2)'), ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Disjoint/2": {
    sql: "SELECT ST_Disjoint(ST_GeomFromText('POINT(100 100)'), ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Touches/2": {
    sql: "SELECT ST_Touches(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_GeomFromText('POLYGON((4 0,8 0,8 4,4 4,4 0))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Crosses/2": {
    sql: "SELECT ST_Crosses(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_GeomFromText('LINESTRING(-1 2,5 2)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Overlaps/2": {
    sql: "SELECT ST_Overlaps(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_GeomFromText('POLYGON((2 2,6 2,6 6,2 6,2 2))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Equals/2": {
    sql: "SELECT ST_Equals(ST_GeomFromText('POINT(1 2)'), ST_GeomFromText('POINT(1 2)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Covers/2": {
    sql: "SELECT ST_Covers(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_GeomFromText('POINT(4 2)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_CoveredBy/2": {
    sql: "SELECT ST_CoveredBy(ST_GeomFromText('POINT(4 2)'), ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Relate/2": {
    sql: "SELECT ST_Relate(ST_GeomFromText('POINT(2 2)'), ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    check: (v) => v === "0FFFFF212",
  },
  "ST_Relate/3": {
    sql: "SELECT ST_Relate(ST_GeomFromText('POINT(2 2)'), ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), 'T*F**F***')",
    check: (v) => Number(v) === 1,
  },
  "ST_Distance/2": {
    sql: "SELECT ST_Distance(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)'))",
    check: (v) => Number(v) === 5,
  },
  "ST_DWithin/3": {
    sql: "SELECT ST_DWithin(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)'), 5.0)",
    check: (v) => Number(v) === 1,
  },
  "ST_MinX/1": {
    sql: "SELECT ST_MinX(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_MaxX/1": {
    sql: "SELECT ST_MaxX(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    check: (v) => Number(v) === 3,
  },
  "ST_MinY/1": {
    sql: "SELECT ST_MinY(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    check: (v) => Number(v) === 2,
  },
  "ST_MaxY/1": {
    sql: "SELECT ST_MaxY(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    check: (v) => Number(v) === 4,
  },
  "ST_IsEmpty/1": {
    sql: "SELECT ST_IsEmpty(ST_GeomFromText('LINESTRING EMPTY'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Transform/2": {
    sql: "SELECT ST_SRID(ST_Transform(ST_GeomFromText('POINT(139.767 35.681)', 4326), 32654))",
    check: (v) => Number(v) === 32654,
  },
  "ST_AsGeoJSON/1": {
    sql: "SELECT ST_AsGeoJSON(ST_GeomFromText('POINT(1 2)'))",
    check: (v) => v === '{"type":"Point","coordinates":[1,2]}',
  },
  "ST_AsGeoJSON/2": {
    sql: "SELECT ST_AsGeoJSON(ST_GeomFromText('POINT(1.23456 2)'), 2)",
    check: (v) => v === '{"type":"Point","coordinates":[1.23,2]}',
  },
  "ST_GeomFromGeoJSON/1": {
    sql: `SELECT ST_AsText(ST_GeomFromGeoJSON('{"type":"Point","coordinates":[1,2]}'))`,
    check: (v) => v === "POINT(1 2)",
  },
  "h3_latlng_to_cell/2": {
    sql: "SELECT h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)', 4326), 9)",
    check: (v) => typeof v === "bigint" || Number(v) > 0,
  },
  "h3_cell_to_parent/2": {
    sql: "SELECT h3_cell_to_parent(h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)', 4326), 9), 5)",
    check: (v) => typeof v === "bigint" || Number(v) > 0,
  },
  "h3_cell_to_string/1": {
    sql: "SELECT h3_cell_to_string(h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)', 4326), 9))",
    check: (v) => typeof v === "string" && v.length > 0,
  },
  "h3_string_to_cell/1": {
    sql: "SELECT h3_string_to_cell(h3_cell_to_string(h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)', 4326), 9)))",
    check: (v) => typeof v === "bigint" || Number(v) > 0,
  },
  "ST_Intersection/2": {
    sql: "SELECT ST_Area(ST_Intersection(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')))",
    check: (v) => Math.abs(Number(v) - 25) < 1e-9,
  },
  "ST_Difference/2": {
    sql: "SELECT ST_Area(ST_Difference(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')))",
    check: (v) => Math.abs(Number(v) - 75) < 1e-9,
  },
  "ST_SymmetricDifference/2": {
    sql: "SELECT ST_Area(ST_SymmetricDifference(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')))",
    check: (v) => Math.abs(Number(v) - 150) < 1e-9,
  },
  "ST_SymDifference/2": {
    sql: "SELECT ST_Area(ST_SymDifference(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')))",
    check: (v) => Math.abs(Number(v) - 150) < 1e-9,
  },
  "ST_Union/2": {
    sql: "SELECT ST_Area(ST_Union(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')))",
    check: (v) => Math.abs(Number(v) - 175) < 1e-9,
  },
  "ST_MakeValid/1": {
    // Bowtie splits into two triangles (structure-method repair).
    sql: "SELECT ST_Area(ST_MakeValid(ST_GeomFromText('POLYGON((0 0,2 2,2 0,0 2,0 0))')))",
    check: (v) => Math.abs(Number(v) - 2) < 1e-9,
  },
  "ST_Buffer/2": {
    sql: "SELECT ST_Area(ST_Buffer(ST_GeomFromText('POINT(0 0)'), 1.0))",
    check: (v) => Math.abs(Number(v) - Math.PI) < 0.05,
  },
  "ST_Buffer/3": {
    // Integer third arg → quad_segs normalization (conformance shared with
    // the rusqlite binding); quad_segs=1 gives a 4-vertex "circle", area 2.
    sql: "SELECT ST_Area(ST_Buffer(ST_GeomFromText('POINT(0 0)'), 1.0, 1))",
    check: (v) => Math.abs(Number(v) - 2) < 0.2,
  },
  "ST_AsMVTGeom/2": {
    // Default extent 4096 over a (0,0)-(100,100) tile: (50,90) → (2048,410).
    sql: "SELECT ST_AsText(ST_AsMVTGeom(ST_GeomFromText('POINT(50 90)'), ST_GeomFromText('POLYGON((0 0,100 0,100 100,0 100,0 0))')))",
    check: (v) => v === "POINT(2048 410)",
  },
  "ST_AsMVTGeom/3": {
    sql: "SELECT ST_AsText(ST_AsMVTGeom(ST_GeomFromText('POINT(50 90)'), ST_GeomFromText('POLYGON((0 0,100 0,100 100,0 100,0 0))'), 10))",
    check: (v) => v === "POINT(5 1)",
  },
  "ST_AsMVTGeom/4": {
    // buffer 0 → the outside point clips away to NULL.
    sql: "SELECT ST_AsMVTGeom(ST_GeomFromText('POINT(200 0)'), ST_GeomFromText('POLYGON((0 0,100 0,100 100,0 100,0 0))'), 10, 0)",
    check: (v) => v === null,
  },
  "ST_AsMVTGeom/5": {
    // clip=false keeps the outside point.
    sql: "SELECT ST_AsText(ST_AsMVTGeom(ST_GeomFromText('POINT(200 0)'), ST_GeomFromText('POLYGON((0 0,100 0,100 100,0 100,0 0))'), 10, 0, 0))",
    check: (v) => v === "POINT(20 10)",
  },
  "ST_ConvexHull/1": {
    sql: "SELECT ST_AsText(ST_ConvexHull(ST_GeomFromText('MULTIPOINT(0 0,4 0,4 4,0 4,2 2)')))",
    check: (v) => typeof v === "string" && v.startsWith("POLYGON"),
  },
  "ST_PointOnSurface/1": {
    sql: "SELECT ST_AsText(ST_PointOnSurface(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))')))",
    check: (v) => v === "POINT(2 2)",
  },
  "ST_SimplifyVW/2": {
    sql: "SELECT ST_AsText(ST_SimplifyVW(ST_GeomFromText('LINESTRING(0 0,1 0.01,2 0)'), 1.0))",
    check: (v) => v === "LINESTRING(0 0,2 0)",
  },
  "ST_ChaikinSmoothing/1": {
    sql: "SELECT ST_NPoints(ST_ChaikinSmoothing(ST_GeomFromText('LINESTRING(0 0,4 4,8 0)')))",
    check: (v) => Number(v) > 3,
  },
  "ST_ChaikinSmoothing/2": {
    sql: "SELECT ST_NPoints(ST_ChaikinSmoothing(ST_GeomFromText('LINESTRING(0 0,4 4,8 0)'), 2))",
    check: (v) => Number(v) > 4,
  },
  "ST_RemoveRepeatedPoints/1": {
    sql: "SELECT ST_AsText(ST_RemoveRepeatedPoints(ST_GeomFromText('LINESTRING(0 0,0 0,1 1)')))",
    check: (v) => v === "LINESTRING(0 0,1 1)",
  },
  "ST_OrientedEnvelope/1": {
    sql: "SELECT ST_AsText(ST_OrientedEnvelope(ST_GeomFromText('POINT(3 4)')))",
    check: (v) => v === "POINT(3 4)",
  },
  "ST_Rotate/2": {
    sql: "SELECT ST_X(ST_Rotate(ST_GeomFromText('POINT(4 5)'), 3.14159265358979323846))",
    check: (v) => Math.abs(Number(v) - -4) < 1e-9,
  },
  "ST_Rotate/4": {
    sql: "SELECT ST_X(ST_Rotate(ST_GeomFromText('POINT(4 5)'), 3.14159265358979323846, 5, 5))",
    check: (v) => Math.abs(Number(v) - 6) < 1e-9,
  },
  "ST_Translate/3": {
    sql: "SELECT ST_AsText(ST_Translate(ST_GeomFromText('POINT(1 2)'), 10, -2))",
    check: (v) => v === "POINT(11 0)",
  },
  "ST_Scale/3": {
    sql: "SELECT ST_AsText(ST_Scale(ST_GeomFromText('POINT(2 3)'), 2, 3))",
    check: (v) => v === "POINT(4 9)",
  },
  "ST_ClosestPoint/2": {
    sql: "SELECT ST_AsText(ST_ClosestPoint(ST_GeomFromText('LINESTRING(0 0,10 0)'), ST_GeomFromText('POINT(5 3)')))",
    check: (v) => v === "POINT(5 0)",
  },
  "ST_LineInterpolatePoint/2": {
    sql: "SELECT ST_AsText(ST_LineInterpolatePoint(ST_GeomFromText('LINESTRING(0 0,10 0)'), 0.5))",
    check: (v) => v === "POINT(5 0)",
  },
  "ST_LineLocatePoint/2": {
    sql: "SELECT ST_LineLocatePoint(ST_GeomFromText('LINESTRING(0 0,10 0)'), ST_GeomFromText('POINT(2.5 4)'))",
    check: (v) => Number(v) === 0.25,
  },
  "ST_HausdorffDistance/2": {
    sql: "SELECT ST_HausdorffDistance(ST_GeomFromText('LINESTRING(0 0,10 0)'), ST_GeomFromText('LINESTRING(0 3,10 3)'))",
    check: (v) => Number(v) === 3,
  },
  "ST_FrechetDistance/2": {
    sql: "SELECT ST_FrechetDistance(ST_GeomFromText('LINESTRING(0 0,10 0)'), ST_GeomFromText('LINESTRING(0 3,10 3)'))",
    check: (v) => Number(v) === 3,
  },
  "ST_Azimuth/2": {
    sql: "SELECT ST_Azimuth(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(5 0)'))",
    check: (v) => Math.abs(Number(v) - Math.PI / 2) < 1e-12,
  },
  "ST_MakePoint/2": {
    sql: "SELECT ST_AsText(ST_MakePoint(1.5, 2.5))",
    check: (v) => v === "POINT(1.5 2.5)",
  },
  "ST_Point/2": {
    sql: "SELECT ST_AsText(ST_Point(1, 2))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_Point/3": {
    sql: "SELECT ST_SRID(ST_Point(1, 2, 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_MakeEnvelope/4": {
    sql: "SELECT ST_AsText(ST_MakeEnvelope(0, 0, 2, 3))",
    check: (v) => v === "POLYGON((0 0,0 3,2 3,2 0,0 0))",
  },
  "ST_MakeEnvelope/5": {
    sql: "SELECT ST_SRID(ST_MakeEnvelope(0, 0, 2, 3, 4326))",
    check: (v) => Number(v) === 4326,
  },
  "GPKG_IsAssignable/2": {
    sql: "SELECT GPKG_IsAssignable('GEOMETRY', ST_GeometryType(ST_GeomFromText('POINT(1 2)')))",
    check: (v) => Number(v) === 1,
  },
  "ST_NPoints/1": {
    sql: "SELECT ST_NPoints(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    check: (v) => Number(v) === 5,
  },
  "ST_Perimeter/1": {
    sql: "SELECT ST_Perimeter(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    check: (v) => Number(v) === 16,
  },
  "ST_GeometryType/1": {
    sql: "SELECT ST_GeometryType(ST_GeomFromText('POINT(1 2)'))",
    check: (v) => v === "ST_Point",
  },
  "ST_NumGeometries/1": {
    sql: "SELECT ST_NumGeometries(ST_GeomFromText('MULTIPOINT(1 2,3 4)'))",
    check: (v) => Number(v) === 2,
  },
  "ST_GeometryN/2": {
    sql: "SELECT ST_AsText(ST_GeometryN(ST_GeomFromText('MULTIPOINT(1 2,3 4)'), 2))",
    check: (v) => v === "POINT(3 4)",
  },
  "ST_StartPoint/1": {
    sql: "SELECT ST_AsText(ST_StartPoint(ST_GeomFromText('LINESTRING(1 2,3 4)')))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_EndPoint/1": {
    sql: "SELECT ST_AsText(ST_EndPoint(ST_GeomFromText('LINESTRING(1 2,3 4)')))",
    check: (v) => v === "POINT(3 4)",
  },
  "ST_PointN/2": {
    sql: "SELECT ST_AsText(ST_PointN(ST_GeomFromText('LINESTRING(1 2,3 4,5 6)'), -1))",
    check: (v) => v === "POINT(5 6)",
  },
  "ST_Reverse/1": {
    sql: "SELECT ST_AsText(ST_Reverse(ST_GeomFromText('LINESTRING(1 2,3 4)')))",
    check: (v) => v === "LINESTRING(3 4,1 2)",
  },
  "ST_Area/1": {
    sql: "SELECT ST_Area(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    check: (v) => Number(v) === 16,
  },
  "ST_Length/1": {
    sql: "SELECT ST_Length(ST_GeomFromText('LINESTRING(0 0,3 4)'))",
    check: (v) => Number(v) === 5,
  },
  "ST_Centroid/1": {
    sql: "SELECT ST_AsText(ST_Centroid(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))')))",
    check: (v) => v === "POINT(2 2)",
  },
  "ST_Envelope/1": {
    sql: "SELECT ST_AsText(ST_Envelope(ST_GeomFromText('LINESTRING(1 2,5 8)')))",
    check: (v) => v === "POLYGON((1 2,1 8,5 8,5 2,1 2))",
  },
  "ST_X/1": {
    sql: "SELECT ST_X(ST_GeomFromText('POINT(3 4)'))",
    check: (v) => Number(v) === 3,
  },
  "ST_Y/1": {
    sql: "SELECT ST_Y(ST_GeomFromText('POINT(3 4)'))",
    check: (v) => Number(v) === 4,
  },
  "ST_NumPoints/1": {
    sql: "SELECT ST_NumPoints(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'))",
    check: (v) => Number(v) === 3,
  },
  "ST_IsValid/1": {
    sql: "SELECT ST_IsValid(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Simplify/2": {
    sql: "SELECT ST_AsText(ST_Simplify(ST_GeomFromText('LINESTRING(0 0,1 0.01,2 0)'), 0.1))",
    check: (v) => v === "LINESTRING(0 0,2 0)",
  },

  // --- PostGIS compatibility (functions::compat) ---
  "ST_XMin/1": {
    sql: "SELECT ST_XMin(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_XMax/1": {
    sql: "SELECT ST_XMax(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    check: (v) => Number(v) === 3,
  },
  "ST_YMin/1": {
    sql: "SELECT ST_YMin(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    check: (v) => Number(v) === 2,
  },
  "ST_YMax/1": {
    sql: "SELECT ST_YMax(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    check: (v) => Number(v) === 4,
  },
  "ST_Area2D/1": {
    sql: "SELECT ST_Area2D(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))",
    check: (v) => Number(v) === 4,
  },
  "ST_Perimeter2D/1": {
    sql: "SELECT ST_Perimeter2D(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))",
    check: (v) => Number(v) === 8,
  },
  "ST_Length2D/1": {
    sql: "SELECT ST_Length2D(ST_GeomFromText('LINESTRING(0 0,3 4)'))",
    check: (v) => Number(v) === 5,
  },
  "ST_GeometryFromText/1": {
    sql: "SELECT ST_AsText(ST_GeometryFromText('POINT(1 2)'))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_GeometryFromText/2": {
    sql: "SELECT ST_SRID(ST_GeometryFromText('POINT(1 2)', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_GeomFromEWKB/1": {
    sql: "SELECT ST_AsText(ST_GeomFromEWKB(ST_AsEWKB(ST_GeomFromText('POINT(1 2)', 4326))))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_Force2D/1": {
    sql: "SELECT ST_AsText(ST_Force2D(ST_GeomFromText('POINT(1 2)')))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_AsEWKT/1": {
    sql: "SELECT ST_AsEWKT(ST_GeomFromText('POINT(1 2)', 4326))",
    check: (v) => v === "SRID=4326;POINT(1 2)",
  },
  "ST_GeomFromEWKT/1": {
    sql: "SELECT ST_AsEWKT(ST_GeomFromEWKT('SRID=3857;POINT(1 2)'))",
    check: (v) => v === "SRID=3857;POINT(1 2)",
  },
  "ST_AsEWKB/1": {
    sql: "SELECT ST_SRID(ST_GeomFromEWKB(ST_AsEWKB(ST_GeomFromText('POINT(1 2)', 4326))))",
    check: (v) => Number(v) === 4326,
  },
  "ST_AsHexEWKB/1": {
    // Byte-identical to PostGIS 3.5's output for the same input.
    sql: "SELECT ST_AsHexEWKB(ST_GeomFromText('POINT(1 2)', 4326))",
    check: (v) => v === "0101000020E6100000000000000000F03F0000000000000040",
  },
  "ST_PointFromText/1": {
    sql: "SELECT ST_AsText(ST_PointFromText('POINT(1 2)'))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_PointFromText/2": {
    sql: "SELECT ST_SRID(ST_PointFromText('POINT(1 2)', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_LineFromText/1": {
    sql: "SELECT ST_AsText(ST_LineFromText('LINESTRING(0 0,1 1)'))",
    check: (v) => v === "LINESTRING(0 0,1 1)",
  },
  "ST_LineFromText/2": {
    sql: "SELECT ST_SRID(ST_LineFromText('LINESTRING(0 0,1 1)', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_LineStringFromText/1": {
    sql: "SELECT ST_AsText(ST_LineStringFromText('LINESTRING(0 0,1 1)'))",
    check: (v) => v === "LINESTRING(0 0,1 1)",
  },
  "ST_LineStringFromText/2": {
    sql: "SELECT ST_SRID(ST_LineStringFromText('LINESTRING(0 0,1 1)', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_PolyFromText/1": {
    sql: "SELECT ST_AsText(ST_PolyFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    check: (v) => v === "POLYGON((0 0,1 0,1 1,0 1,0 0))",
  },
  "ST_PolyFromText/2": {
    sql: "SELECT ST_SRID(ST_PolyFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_PolygonFromText/1": {
    sql: "SELECT ST_AsText(ST_PolygonFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    check: (v) => v === "POLYGON((0 0,1 0,1 1,0 1,0 0))",
  },
  "ST_PolygonFromText/2": {
    sql: "SELECT ST_SRID(ST_PolygonFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_MPointFromText/1": {
    sql: "SELECT ST_AsText(ST_MPointFromText('MULTIPOINT((1 2),(3 4))'))",
    check: (v) => v === "MULTIPOINT((1 2),(3 4))",
  },
  "ST_MPointFromText/2": {
    sql: "SELECT ST_SRID(ST_MPointFromText('MULTIPOINT((1 2),(3 4))', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_MLineFromText/1": {
    sql: "SELECT ST_AsText(ST_MLineFromText('MULTILINESTRING((0 0,1 1))'))",
    check: (v) => v === "MULTILINESTRING((0 0,1 1))",
  },
  "ST_MLineFromText/2": {
    sql: "SELECT ST_SRID(ST_MLineFromText('MULTILINESTRING((0 0,1 1))', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_MPolyFromText/1": {
    sql: "SELECT ST_AsText(ST_MPolyFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))",
    check: (v) => v === "MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))",
  },
  "ST_MPolyFromText/2": {
    sql: "SELECT ST_SRID(ST_MPolyFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))', 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_PointFromWKB/1": {
    sql: "SELECT ST_AsText(ST_PointFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)'))))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_PointFromWKB/2": {
    sql: "SELECT ST_SRID(ST_PointFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)')), 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_LineFromWKB/1": {
    sql: "SELECT ST_AsText(ST_LineFromWKB(ST_AsBinary(ST_GeomFromText('LINESTRING(0 0,1 1)'))))",
    check: (v) => v === "LINESTRING(0 0,1 1)",
  },
  "ST_LineFromWKB/2": {
    sql: "SELECT ST_SRID(ST_LineFromWKB(ST_AsBinary(ST_GeomFromText('LINESTRING(0 0,1 1)')), 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_PolyFromWKB/1": {
    sql: "SELECT ST_AsText(ST_PolyFromWKB(ST_AsBinary(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))))",
    check: (v) => v === "POLYGON((0 0,1 0,1 1,0 1,0 0))",
  },
  "ST_PolyFromWKB/2": {
    sql: "SELECT ST_SRID(ST_PolyFromWKB(ST_AsBinary(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')), 4326))",
    check: (v) => Number(v) === 4326,
  },


  // --- structural accessors and editing (functions::edit) ---
  "ST_ExteriorRing/1": {
    sql: "SELECT ST_AsText(ST_ExteriorRing(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))')))",
    check: (v) => v === "LINESTRING(0 0,4 0,4 4,0 4,0 0)",
  },
  "ST_InteriorRingN/2": {
    sql: "SELECT ST_AsText(ST_InteriorRingN(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))'), 1))",
    check: (v) => v === "LINESTRING(1 1,2 1,2 2,1 2,1 1)",
  },
  "ST_NumInteriorRings/1": {
    sql: "SELECT ST_NumInteriorRings(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_NumInteriorRing/1": {
    sql: "SELECT ST_NumInteriorRing(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_NRings/1": {
    sql: "SELECT ST_NRings(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))'))",
    check: (v) => Number(v) === 2,
  },
  "ST_Boundary/1": {
    sql: "SELECT ST_AsText(ST_Boundary(ST_GeomFromText('LINESTRING(0 0,1 1,2 0)')))",
    check: (v) => v === "MULTIPOINT((0 0),(2 0))",
  },
  "ST_IsClosed/1": {
    sql: "SELECT ST_IsClosed(ST_GeomFromText('LINESTRING(0 0,1 1,1 0,0 0)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_IsRing/1": {
    sql: "SELECT ST_IsRing(ST_GeomFromText('LINESTRING(0 0,1 1,1 0,0 0)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_AddPoint/2": {
    sql: "SELECT ST_AsText(ST_AddPoint(ST_GeomFromText('LINESTRING(0 0,1 1)'), ST_GeomFromText('POINT(2 2)')))",
    check: (v) => v === "LINESTRING(0 0,1 1,2 2)",
  },
  "ST_AddPoint/3": {
    sql: "SELECT ST_AsText(ST_AddPoint(ST_GeomFromText('LINESTRING(0 0,1 1)'), ST_GeomFromText('POINT(9 9)'), 0))",
    check: (v) => v === "LINESTRING(9 9,0 0,1 1)",
  },
  "ST_SetPoint/3": {
    sql: "SELECT ST_AsText(ST_SetPoint(ST_GeomFromText('LINESTRING(0 0,1 1)'), 0, ST_GeomFromText('POINT(9 9)')))",
    check: (v) => v === "LINESTRING(9 9,1 1)",
  },
  "ST_RemovePoint/2": {
    sql: "SELECT ST_AsText(ST_RemovePoint(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'), 0))",
    check: (v) => v === "LINESTRING(1 1,2 2)",
  },
  "ST_MakeLine/2": {
    sql: "SELECT ST_AsText(ST_MakeLine(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(1 1)')))",
    check: (v) => v === "LINESTRING(0 0,1 1)",
  },
  "ST_MakePolygon/1": {
    sql: "SELECT ST_AsText(ST_MakePolygon(ST_GeomFromText('LINESTRING(0 0,1 0,1 1,0 0)')))",
    check: (v) => v === "POLYGON((0 0,1 0,1 1,0 0))",
  },
  "ST_Multi/1": {
    sql: "SELECT ST_AsText(ST_Multi(ST_GeomFromText('POINT(1 2)')))",
    check: (v) => v === "MULTIPOINT((1 2))",
  },
  "ST_SnapToGrid/2": {
    sql: "SELECT ST_AsText(ST_SnapToGrid(ST_GeomFromText('POINT(1.23 4.57)'), 0.5))",
    check: (v) => v === "POINT(1 4.5)",
  },
  "ST_SnapToGrid/3": {
    sql: "SELECT ST_AsText(ST_SnapToGrid(ST_GeomFromText('POINT(1.23 4.57)'), 0.5, 1.0))",
    check: (v) => v === "POINT(1 5)",
  },
  "ST_FlipCoordinates/1": {
    sql: "SELECT ST_AsText(ST_FlipCoordinates(ST_GeomFromText('POINT(1 2)')))",
    check: (v) => v === "POINT(2 1)",
  },
  "ST_ShiftLongitude/1": {
    sql: "SELECT ST_AsText(ST_ShiftLongitude(ST_GeomFromText('POINT(-10 5)')))",
    check: (v) => v === "POINT(350 5)",
  },
  "ST_Expand/2": {
    sql: "SELECT ST_AsText(ST_Expand(ST_GeomFromText('POINT(1 1)'), 2))",
    check: (v) => v === "POLYGON((-1 -1,-1 3,3 3,3 -1,-1 -1))",
  },


  // --- sphere/spheroid, dimension, orientation, linear referencing ---
  "ST_DistanceSphere/2": {
    sql: "SELECT ST_DistanceSphere(ST_GeomFromText('POINT(0 0)', 4326), ST_GeomFromText('POINT(1 0)', 4326))",
    check: (v) => Math.abs(Number(v) - 111195.07973463) < 1e-3,
  },
  "ST_DistanceSpheroid/2": {
    sql: "SELECT ST_DistanceSpheroid(ST_GeomFromText('POINT(0 0)', 4326), ST_GeomFromText('POINT(1 0)', 4326))",
    check: (v) => Math.abs(Number(v) - 111319.49079327357) < 1e-3,
  },
  "ST_DistanceSpheroid/3": {
    sql: "SELECT ST_DistanceSpheroid(ST_GeomFromText('POINT(0 0)', 4326), ST_GeomFromText('POINT(1 0)', 4326), 'SPHEROID[\"WGS 84\",6378137,298.257223563]')",
    check: (v) => Math.abs(Number(v) - 111319.49079327357) < 1e-3,
  },
  "ST_LengthSpheroid/2": {
    sql: "SELECT ST_LengthSpheroid(ST_GeomFromText('LINESTRING(0 0,1 0)', 4326), 'SPHEROID[\"WGS 84\",6378137,298.257223563]')",
    check: (v) => Math.abs(Number(v) - 111319.49079327357) < 1e-3,
  },
  "ST_Length2DSpheroid/2": {
    sql: "SELECT ST_Length2DSpheroid(ST_GeomFromText('LINESTRING(0 0,1 0)', 4326), 'SPHEROID[\"WGS 84\",6378137,298.257223563]')",
    check: (v) => Math.abs(Number(v) - 111319.49079327357) < 1e-3,
  },
  "ST_Project/3": {
    sql: "SELECT ST_X(ST_Project(ST_GeomFromText('POINT(0 0)'), 100, 1.5707963267948966))",
    check: (v) => Math.abs(Number(v) - 100) < 1e-6,
  },
  "ST_Dimension/1": {
    sql: "SELECT ST_Dimension(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 0))'))",
    check: (v) => Number(v) === 2,
  },
  "ST_CoordDim/1": {
    sql: "SELECT ST_CoordDim(ST_GeomFromText('POINT(1 2)'))",
    check: (v) => Number(v) === 2,
  },
  "ST_NDims/1": {
    sql: "SELECT ST_NDims(ST_GeomFromText('POINT(1 2)'))",
    check: (v) => Number(v) === 2,
  },
  "ST_IsValidReason/1": {
    sql: "SELECT ST_IsValidReason(ST_GeomFromText('POINT(1 1)'))",
    check: (v) => v === "Valid Geometry",
  },
  "ST_ForcePolygonCW/1": {
    sql: "SELECT ST_IsPolygonCW(ST_ForcePolygonCW(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')))",
    check: (v) => Number(v) === 1,
  },
  "ST_ForceRHR/1": {
    sql: "SELECT ST_IsPolygonCW(ST_ForceRHR(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')))",
    check: (v) => Number(v) === 1,
  },
  "ST_ForcePolygonCCW/1": {
    sql: "SELECT ST_IsPolygonCCW(ST_ForcePolygonCCW(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')))",
    check: (v) => Number(v) === 1,
  },
  "ST_IsPolygonCW/1": {
    sql: "SELECT ST_IsPolygonCW(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    check: (v) => Number(v) === 0,
  },
  "ST_IsPolygonCCW/1": {
    sql: "SELECT ST_IsPolygonCCW(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Segmentize/2": {
    sql: "SELECT ST_NPoints(ST_Segmentize(ST_GeomFromText('LINESTRING(0 0,10 0)'), 4))",
    check: (v) => Number(v) === 4,
  },
  "ST_LineSubstring/3": {
    sql: "SELECT ST_AsText(ST_LineSubstring(ST_GeomFromText('LINESTRING(0 0,10 0)'), 0.3, 0.7))",
    check: (v) => v === "LINESTRING(3 0,7 0)",
  },
  "ST_ShortestLine/2": {
    sql: "SELECT ST_AsText(ST_ShortestLine(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)')))",
    check: (v) => v === "LINESTRING(0 0,2 0)",
  },
  "ST_LongestLine/2": {
    sql: "SELECT ST_NPoints(ST_LongestLine(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)')))",
    check: (v) => Number(v) === 2,
  },
  "ST_MaxDistance/2": {
    sql: "SELECT ST_MaxDistance(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)'))",
    check: (v) => Math.abs(Number(v) - 2.23606797749979) < 1e-9,
  },


  // --- smallest enclosing circle and overlay-powered areal operations ---
  "ST_MinimumBoundingRadius/1": {
    sql: "SELECT ST_MinimumBoundingRadius(ST_GeomFromText('LINESTRING(0 0,4 0)'))",
    check: (v) => Math.abs(Number(v) - 2) < 1e-9,
  },
  "ST_MinimumBoundingCircle/1": {
    sql: "SELECT ST_Covers(ST_MinimumBoundingCircle(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))')), ST_GeomFromText('POINT(4 4)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_MinimumBoundingCircle/2": {
    sql: "SELECT ST_NPoints(ST_MinimumBoundingCircle(ST_GeomFromText('LINESTRING(0 0,4 0)'), 2))",
    check: (v) => Number(v) === 9,
  },
  "ST_UnaryUnion/1": {
    sql: "SELECT ST_Area(ST_UnaryUnion(ST_GeomFromText('MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((1 1,3 1,3 3,1 3,1 1)))')))",
    check: (v) => Math.abs(Number(v) - 7) < 1e-9,
  },
  "ST_ClipByBox2D/2": {
    sql: "SELECT ST_Area(ST_ClipByBox2D(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_MakeEnvelope(2,2,5,5)))",
    check: (v) => Math.abs(Number(v) - 9) < 1e-9,
  },
  "ST_Subdivide/2": {
    sql: "SELECT ST_Area(ST_Subdivide(ST_Segmentize(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), 2), 8))",
    check: (v) => Math.abs(Number(v) - 100) < 1e-9,
  },


  // --- the rest of the reachable surface (functions::extra) ---
  "ST_ContainsProperly/2": {
    sql: "SELECT ST_ContainsProperly(ST_GeomFromText('POLYGON((0 0,3 0,3 3,0 3,0 0))'), ST_GeomFromText('POINT(1 1)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_DFullyWithin/3": {
    sql: "SELECT ST_DFullyWithin(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)'), 3)",
    check: (v) => Number(v) === 1,
  },
  "ST_RelateMatch/2": {
    sql: "SELECT ST_RelateMatch('101202FFF', 'TTTTTTFFF')",
    check: (v) => Number(v) === 1,
  },
  "ST_Affine/7": {
    sql: "SELECT ST_AsText(ST_Affine(ST_GeomFromText('LINESTRING(1 2,3 4)'), 2,0,0,2,10,20))",
    check: (v) => v === "LINESTRING(12 24,16 28)",
  },
  "ST_TransScale/5": {
    sql: "SELECT ST_AsText(ST_TransScale(ST_GeomFromText('POINT(1 2)'), 1, 2, 3, 4))",
    check: (v) => v === "POINT(6 16)",
  },
  "ST_ReducePrecision/2": {
    sql: "SELECT ST_X(ST_ReducePrecision(ST_GeomFromText('POINT(1.234 5.678)'), 0.1))",
    check: (v) => Math.abs(Number(v) - 1.2) < 1e-9,
  },
  "ST_Angle/3": {
    sql: "SELECT ST_Angle(ST_GeomFromText('POINT(1 0)'), ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(0 1)'))",
    check: (v) => Math.abs(Number(v) - (3 * Math.PI) / 2) < 1e-9,
  },
  "ST_Angle/4": {
    sql: "SELECT ST_Angle(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(1 0)'), ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(0 1)'))",
    check: (v) => Math.abs(Number(v) - (3 * Math.PI) / 2) < 1e-9,
  },
  "ST_LineInterpolatePoints/2": {
    sql: "SELECT ST_AsText(ST_LineInterpolatePoints(ST_GeomFromText('LINESTRING(0 0,10 0)'), 0.25))",
    check: (v) => v === "MULTIPOINT((2.5 0),(5 0),(7.5 0),(10 0))",
  },
  "ST_Points/1": {
    sql: "SELECT ST_AsText(ST_Points(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 0))')))",
    check: (v) => v === "MULTIPOINT((0 0),(1 0),(1 1),(0 0))",
  },
  "ST_BoundingDiagonal/1": {
    sql: "SELECT ST_AsText(ST_BoundingDiagonal(ST_GeomFromText('LINESTRING(1 2,5 9)')))",
    check: (v) => v === "LINESTRING(1 2,5 9)",
  },
  "ST_OrderingEquals/2": {
    sql: "SELECT ST_OrderingEquals(ST_GeomFromText('LINESTRING(0 0,1 1)'), ST_GeomFromText('LINESTRING(1 1,0 0)'))",
    check: (v) => Number(v) === 0,
  },
  "ST_GeoHash/1": {
    sql: "SELECT ST_GeoHash(ST_GeomFromText('POINT(139.7 35.68)', 4326))",
    check: (v) => v === "xn76fzq7jfn42q30gmb9",
  },
  "ST_GeoHash/2": {
    sql: "SELECT ST_GeoHash(ST_GeomFromText('POINT(139.7 35.68)', 4326), 5)",
    check: (v) => v === "xn76f",
  },


  // --- size-gated algorithms (functions::hull) ---
  "ST_ConcaveHull/2": {
    // 1.0 is the convex hull, which for a square of points has area 16.
    sql: "SELECT ST_Area(ST_ConcaveHull(ST_GeomFromText('MULTIPOINT(0 0,4 0,4 4,0 4)'), 1.0))",
    check: (v) => Math.abs(Number(v) - 16) < 1e-9,
  },
  "ST_DelaunayTriangles/1": {
    sql: "SELECT ST_NumGeometries(ST_DelaunayTriangles(ST_GeomFromText('MULTIPOINT(0 0,4 0,4 4,0 4)')))",
    check: (v) => Number(v) === 2,
  },
  // The constrained triangulation leaves the hole uncovered: 96, not 100.
  "ST_TriangulatePolygon/1": {
    sql: "SELECT ST_Area(ST_TriangulatePolygon(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,4 2,4 4,2 4,2 2))')))",
    check: (v) => Math.abs(Number(v) - 96) < 1e-9,
  },

  // --- line structure (functions::lines) ---
  "ST_IsSimple/1": {
    sql: "SELECT ST_IsSimple(ST_GeomFromText('LINESTRING(0 0,10 10,0 10,10 0)'))",
    check: (v) => Number(v) === 0,
  },
  "ST_LineMerge/1": {
    sql: "SELECT ST_AsText(ST_LineMerge(ST_GeomFromText('MULTILINESTRING((0 0,1 1),(1 1,2 2))')))",
    check: (v) => v === "LINESTRING(0 0,1 1,2 2)",
  },
  // The boolean argument arrives as SQLite's 0/1.
  "ST_LineMerge/2": {
    sql: "SELECT ST_AsText(ST_LineMerge(ST_GeomFromText('MULTILINESTRING((0 0,1 1),(2 2,1 1))'), 1))",
    check: (v) => v === "MULTILINESTRING((0 0,1 1),(2 2,1 1))",
  },
  "ST_Split/2": {
    sql: "SELECT ST_Area(ST_Split(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_GeomFromText('LINESTRING(5 -1,5 11)')))",
    check: (v) => Math.abs(Number(v) - 100) < 1e-9,
  },


  // --- the tail (functions::misc) ---
  "ST_RotateZ/2": {
    sql: "SELECT ST_AsText(ST_RotateZ(ST_GeomFromText('POINT(1 0)'), 1.5707963267948966))",
    check: (v) => v.startsWith("POINT(") && Math.abs(Number(v.slice(6).split(" ")[1].replace(")",""))-1) < 1e-9,
  },
  "ST_MultiPointFromText/1": {
    sql: "SELECT ST_AsText(ST_MultiPointFromText('MULTIPOINT((1 2),(3 4))'))",
    check: (v) => v === "MULTIPOINT((1 2),(3 4))",
  },
  "ST_MultiLineStringFromText/1": {
    sql: "SELECT ST_AsText(ST_MultiLineStringFromText('MULTILINESTRING((0 0,1 1))'))",
    check: (v) => v === "MULTILINESTRING((0 0,1 1))",
  },
  "ST_MultiPolygonFromText/1": {
    sql: "SELECT ST_AsText(ST_MultiPolygonFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))",
    check: (v) => v.startsWith("MULTIPOLYGON"),
  },
  "ST_PolygonFromWKB/1": {
    sql: "SELECT ST_AsText(ST_PolygonFromWKB(ST_AsBinary(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))))",
    check: (v) => v === "POLYGON((0 0,1 0,1 1,0 1,0 0))",
  },
  "ST_LineStringFromWKB/1": {
    sql: "SELECT ST_AsText(ST_LineStringFromWKB(ST_AsBinary(ST_GeomFromText('LINESTRING(0 0,1 1)'))))",
    check: (v) => v === "LINESTRING(0 0,1 1)",
  },
  "ST_MPointFromWKB/1": {
    sql: "SELECT ST_AsText(ST_MPointFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOINT((1 2),(3 4))'))))",
    check: (v) => v === "MULTIPOINT((1 2),(3 4))",
  },
  "ST_MPointFromWKB/2": {
    sql: "SELECT ST_SRID(ST_MPointFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOINT((1 2))')), 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_MLineFromWKB/1": {
    sql: "SELECT ST_AsText(ST_MLineFromWKB(ST_AsBinary(ST_GeomFromText('MULTILINESTRING((0 0,1 1))'))))",
    check: (v) => v === "MULTILINESTRING((0 0,1 1))",
  },
  "ST_MLineFromWKB/2": {
    sql: "SELECT ST_SRID(ST_MLineFromWKB(ST_AsBinary(ST_GeomFromText('MULTILINESTRING((0 0,1 1))')), 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_MPolyFromWKB/1": {
    sql: "SELECT ST_GeometryType(ST_MPolyFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))))",
    check: (v) => v === "ST_MultiPolygon",
  },
  "ST_MPolyFromWKB/2": {
    sql: "SELECT ST_SRID(ST_MPolyFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))')), 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_MultiPointFromWKB/1": {
    sql: "SELECT ST_AsText(ST_MultiPointFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOINT((1 2))'))))",
    check: (v) => v === "MULTIPOINT((1 2))",
  },
  "ST_MultiLineFromWKB/1": {
    sql: "SELECT ST_AsText(ST_MultiLineFromWKB(ST_AsBinary(ST_GeomFromText('MULTILINESTRING((0 0,1 1))'))))",
    check: (v) => v === "MULTILINESTRING((0 0,1 1))",
  },
  "ST_MultiPolyFromWKB/1": {
    sql: "SELECT ST_GeometryType(ST_MultiPolyFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))))",
    check: (v) => v === "ST_MultiPolygon",
  },
  "ST_Polygon/2": {
    sql: "SELECT ST_SRID(ST_Polygon(ST_GeomFromText('LINESTRING(0 0,1 0,1 1,0 0)'), 4326))",
    check: (v) => Number(v) === 4326,
  },
  "ST_LineFromMultiPoint/1": {
    sql: "SELECT ST_AsText(ST_LineFromMultiPoint(ST_GeomFromText('MULTIPOINT((0 0),(1 1),(2 2))')))",
    check: (v) => v === "LINESTRING(0 0,1 1,2 2)",
  },
  "ST_LineExtend/2": {
    sql: "SELECT ST_AsText(ST_LineExtend(ST_GeomFromText('LINESTRING(0 0,1 0)'), 1))",
    check: (v) => v === "LINESTRING(0 0,1 0,2 0)",
  },
  "ST_LineExtend/3": {
    sql: "SELECT ST_AsText(ST_LineExtend(ST_GeomFromText('LINESTRING(0 0,1 0)'), 1, 0.5))",
    check: (v) => v === "LINESTRING(-0.5 0,0 0,1 0,2 0)",
  },
  "ST_PointInsideCircle/4": {
    sql: "SELECT ST_PointInsideCircle(ST_GeomFromText('POINT(1 1)'), 0, 0, 2)",
    check: (v) => Number(v) === 1,
  },
  "ST_WrapX/3": {
    sql: "SELECT ST_AsText(ST_WrapX(ST_GeomFromText('LINESTRING(-170 0,170 0)'), 0, 360))",
    check: (v) => v === "LINESTRING(190 0,170 0)",
  },
  "ST_MakeBox2D/2": {
    sql: "SELECT ST_AsText(ST_MakeBox2D(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)')))",
    check: (v) => v === "POLYGON((0 0,0 4,3 4,3 0,0 0))",
  },
  "ST_GeomFromGeoHash/1": {
    sql: "SELECT ST_GeometryType(ST_GeomFromGeoHash('xn76f'))",
    check: (v) => v === "ST_Polygon",
  },
  "ST_GeomFromGeoHash/2": {
    sql: "SELECT ST_GeometryType(ST_GeomFromGeoHash('xn76fzq7', 5))",
    check: (v) => v === "ST_Polygon",
  },
  "ST_Box2dFromGeoHash/1": {
    sql: "SELECT ST_GeometryType(ST_Box2dFromGeoHash('xn76f'))",
    check: (v) => v === "ST_Polygon",
  },
  "ST_PointFromGeoHash/1": {
    sql: "SELECT ST_AsText(ST_PointFromGeoHash('xn76f'))",
    check: (v) => v === "POINT(139.68017578125 35.66162109375)",
  },
  "ST_PointFromGeoHash/2": {
    sql: "SELECT ST_GeometryType(ST_PointFromGeoHash('xn76fzq7', 5))",
    check: (v) => v === "ST_Point",
  },
  "ST_GeometricMedian/1": {
    sql: "SELECT ST_X(ST_GeometricMedian(ST_GeomFromText('MULTIPOINT((0 0),(4 0),(0 4),(4 4))')))",
    check: (v) => Math.abs(Number(v) - 2) < 1e-6,
  },
  "ST_GeometricMedian/2": {
    sql: "SELECT ST_X(ST_GeometricMedian(ST_GeomFromText('MULTIPOINT((0 0),(4 0),(0 4),(4 4))'), 1e-6))",
    check: (v) => Math.abs(Number(v) - 2) < 1e-5,
  },
  "ST_LineCrossingDirection/2": {
    sql: "SELECT ST_LineCrossingDirection(ST_GeomFromText('LINESTRING(0 0,2 2)'), ST_GeomFromText('LINESTRING(0 2,2 0)'))",
    check: (v) => Number(v) === 1,
  },
  "ST_Summary/1": {
    sql: "SELECT ST_Summary(ST_GeomFromText('POINT(1 2)', 4326))",
    check: (v) => v.startsWith("Point[S]"),
  },
  "ST_MemSize/1": {
    sql: "SELECT ST_MemSize(ST_GeomFromText('POINT(1 2)')) > 0",
    check: (v) => Number(v) === 1,
  },
  "ST_Normalize/1": {
    sql: "SELECT ST_IsPolygonCW(ST_Normalize(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')))",
    check: (v) => Number(v) === 1,
  },


  // --- 3D pass-through (functions::threed) ---
  // A 3D geometry cannot be built from WKT (kenro's reader is 2D) and cannot
  // pass through ST_GeomFromWKB either (that re-encodes, and the encoders
  // refuse 3D). A raw WKB blob is exactly how a GDAL-written GeoPackage
  // column reaches these functions, so that is what the smoke uses.
  "ST_HasZ/1": {
    sql: "SELECT ST_HasZ(x'01e9030000000000000000f03f00000000000000400000000000000840')",
    check: (v) => Number(v) === 1,
  },
  "ST_HasM/1": {
    sql: "SELECT ST_HasM(x'01e9030000000000000000f03f00000000000000400000000000000840')",
    check: (v) => Number(v) === 0,
  },
  "ST_Z/1": {
    sql: "SELECT ST_Z(x'01e9030000000000000000f03f00000000000000400000000000000840')",
    check: (v) => Number(v) === 3,
  },
  "ST_M/1": {
    sql: "SELECT ST_M(x'01e9030000000000000000f03f00000000000000400000000000000840') IS NULL",
    check: (v) => Number(v) === 1,
  },
  "ST_ZMin/1": {
    sql: "SELECT ST_ZMin(x'01ea03000002000000000000000000000000000000000000000000000000002440000000000000f03f000000000000f03f0000000000003e40')",
    check: (v) => Number(v) === 10,
  },
  "ST_ZMax/1": {
    sql: "SELECT ST_ZMax(x'01ea03000002000000000000000000000000000000000000000000000000002440000000000000f03f000000000000f03f0000000000003e40')",
    check: (v) => Number(v) === 30,
  },


  // --- GML 2/3 I/O (functions::gml) ---
  "ST_AsGML/1": {
    sql: "SELECT ST_AsGML(ST_GeomFromText('POINT(1 2)', 4326))",
    check: (v) => v === '<gml:Point srsName="EPSG:4326"><gml:coordinates>1,2</gml:coordinates></gml:Point>',
  },
  "ST_AsGML/2": {
    sql: "SELECT ST_AsGML(3, ST_GeomFromText('POINT(1 2)', 4326))",
    check: (v) => v === '<gml:Point srsName=\"EPSG:4326\"><gml:pos srsDimension=\"2\">1 2</gml:pos></gml:Point>'.replace(/\\/g, ""),
  },
  "ST_AsGML/3": {
    sql: "SELECT ST_AsGML(3, ST_GeomFromText('POINT(1.123456789 2)', 4326), 3)",
    check: (v) => v.includes("1.123 2"),
  },
  // --- KML / SVG (functions::kml, functions::svg) ---
  // KML is WGS84 by definition and reprojects rather than labelling, so the
  // input needs a real SRID.
  "ST_AsKML/1": {
    sql: "SELECT ST_AsKML(ST_GeomFromText('POINT(1 2)', 4326))",
    check: (v) => v === "<Point><coordinates>1,2</coordinates></Point>",
  },
  "ST_AsKML/2": {
    sql: "SELECT ST_AsKML(ST_GeomFromText('POINT(1.23456789 2.3456789)', 4326), 3)",
    check: (v) => v === "<Point><coordinates>1.235,2.346</coordinates></Point>",
  },
  "ST_AsKML/3": {
    sql: "SELECT ST_AsKML(ST_GeomFromText('POINT(1 2)', 4326), 15, 'kml')",
    check: (v) => v === "<kml:Point><kml:coordinates>1,2</kml:coordinates></kml:Point>",
  },
  // SVG negates Y; rel=1 swaps cx/cy for x/y as well as the path commands.
  "ST_AsSVG/1": {
    sql: "SELECT ST_AsSVG(ST_GeomFromText('POINT(1 2)'))",
    check: (v) => v === 'cx="1" cy="-2"',
  },
  "ST_AsSVG/2": {
    sql: "SELECT ST_AsSVG(ST_GeomFromText('POINT(1 2)'), 1)",
    check: (v) => v === 'x="1" y="-2"',
  },
  "ST_AsSVG/3": {
    sql: "SELECT ST_AsSVG(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), 0, 15)",
    check: (v) => v === "M 0 0 L 4 0 4 -4 0 -4 Z",
  },
  "ST_GeomFromGML/1": {
    sql: "SELECT ST_AsText(ST_GeomFromGML('<gml:Point><gml:pos>1 2</gml:pos></gml:Point>'))",
    check: (v) => v === "POINT(1 2)",
  },
  "ST_GeomFromGML/2": {
    sql: "SELECT ST_SRID(ST_GeomFromGML('<gml:Point><gml:pos>1 2</gml:pos></gml:Point>', 6697))",
    check: (v) => Number(v) === 6697,
  },
  "ST_GMLToSQL/1": {
    sql: "SELECT ST_AsText(ST_GMLToSQL('<gml:Point><gml:pos>1 2</gml:pos></gml:Point>'))",
    check: (v) => v === "POINT(1 2)",
  },


  // --- surface collections (functions::surface) ---
  // The bytes are PostGIS 3.5's own output for
  // POLYHEDRALSURFACE Z(((0 0 0,0 1 0,1 1 0,1 0 0,0 0 0))). WKT cannot build
  // one here (kenro's reader is 2D), so the blob goes in raw — which is how a
  // GDAL-written column arrives anyway.
  "ST_NumPatches/1": {
    sql: "SELECT ST_NumPatches(x'01f70300000100000001eb03000001000000050000000000000000000000000000000000000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f0000000000000000000000000000f03f00000000000000000000000000000000000000000000000000000000000000000000000000000000')",
    check: (v) => Number(v) === 1,
  },
  "ST_PatchN/2": {
    sql: "SELECT ST_AsText(ST_PatchN(x'01f70300000100000001eb03000001000000050000000000000000000000000000000000000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f0000000000000000000000000000f03f00000000000000000000000000000000000000000000000000000000000000000000000000000000', 1))",
    check: (v) => v === "POLYGON((0 0,0 1,1 1,1 0,0 0))",
  },
  "kenro_gpkg_extension_required/1": {
    sql: "SELECT kenro_gpkg_extension_required(x'01f70300000100000001eb03000001000000050000000000000000000000000000000000000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f0000000000000000000000000000f03f00000000000000000000000000000000000000000000000000000000000000000000000000000000')",
    check: (v) => v === "gpkg_geom_POLYHEDRALSURFACE",
  },

};

/**
 * Run the whole-function smoke against a host: `run(sql)` must return the
 * first column of the first row; `expectError(sql)` must return the error
 * message thrown for the SQL. `skip(entry)` marks functions the host cannot
 * support (their loud error is asserted instead).
 */
export async function smokeAllFunctions(manifest, { run, expectError, skip }) {
  for (const entry of manifest.functions) {
    const key = `${entry.sql_name}/${entry.args.length}`;
    const smoke = SMOKE_SQL[key];
    if (!smoke) throw new Error(`no smoke SQL for ${key}`);
    if (skip?.(entry)) {
      const message = await expectError(smoke.sql);
      if (!/kenro:/.test(message)) {
        throw new Error(`${key}: expected a kenro error, got: ${message}`);
      }
      continue;
    }
    const value = await run(smoke.sql);
    if (!smoke.check(value)) {
      throw new Error(`${key}: unexpected result ${String(value)}`);
    }
  }
  // Stub behavior: helpful error, not "no such function".
  const stubMessage = await expectError(
    "SELECT ST_Collect(ST_GeomFromText('POINT(0 0)'))",
  );
  if (!/not implemented in kenro/.test(stubMessage)) {
    throw new Error(`stub error text wrong: ${stubMessage}`);
  }
  // NULL-strictness through SQL.
  const nullResult = await run("SELECT ST_AsText(NULL) IS NULL");
  if (Number(nullResult) !== 1) {
    throw new Error(`NULL-strictness violated: ${String(nullResult)}`);
  }
}
