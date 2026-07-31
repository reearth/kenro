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

const GEOGRAPHIC_SRIDS = new Set([4326, 4612, 6668]);

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
    sql: "SELECT ST_SRID(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)')), 6677))",
    check: (v) => Number(v) === 6677,
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
    sql: "SELECT ST_SRID(ST_Transform(ST_GeomFromText('POINT(139.767 35.681)', 4326), 6677))",
    check: (v) => Number(v) === 6677,
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
    "SELECT ST_Buffer(ST_GeomFromText('POINT(0 0)'), 1.0)",
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
