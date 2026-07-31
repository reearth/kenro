// Tier 1: every golden vector (predicates, transform, geojson, h3,
// accessors) replayed against the raw wasm exports — the "the core runs
// correctly on wasm" proof. Host adapters are tested separately.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  assertNumberClose,
  assertWithinToleranceMeters,
  effective,
  expectsError,
  geomApproxEqual,
  initWasm,
  loadVectors,
} from "./golden.mjs";

const wasm = await initWasm();

function geom(wkt, srid) {
  return srid !== undefined && srid !== null && srid !== 0
    ? wasm.stGeomFromTextSrid(wkt, srid)
    : wasm.stGeomFromText(wkt);
}

function runExpectingError(vector, fn) {
  if (expectsError(vector)) {
    assert.throws(fn, /kenro:/, vector.id);
    return true;
  }
  return false;
}

test("predicates vectors", () => {
  for (const v of loadVectors("predicates")) {
    const run = () => {
      const a = geom(v.a);
      const b = v.b === undefined ? null : geom(v.b);
      switch (v.fn) {
        case "intersects":
          return wasm.stIntersects(a, b);
        case "contains":
          return wasm.stContains(a, b);
        case "within":
          return wasm.stWithin(a, b);
        case "disjoint":
          return wasm.stDisjoint(a, b);
        case "touches":
          return wasm.stTouches(a, b);
        case "crosses":
          return wasm.stCrosses(a, b);
        case "overlaps":
          return wasm.stOverlaps(a, b);
        case "equals":
          return wasm.stEquals(a, b);
        case "covers":
          return wasm.stCovers(a, b);
        case "coveredby":
          return wasm.stCoveredBy(a, b);
        case "relate":
          return wasm.stRelate(a, b);
        case "relate_pattern":
          return wasm.stRelatePattern(a, b, v.arg_text);
        case "distance":
          return wasm.stDistance(a, b);
        case "dwithin":
          return wasm.stDwithin(a, b, v.arg);
        case "astext":
          return wasm.stAsText(a);
        default:
          throw new Error(`unknown fn ${v.fn}`);
      }
    };
    if (runExpectingError(v, run)) continue;
    const got = run();
    const want = effective(v);
    if (typeof want === "boolean") assert.equal(got, want, v.id);
    else if (typeof want === "number") assertNumberClose(v.id, got, want);
    else if (want === null) assert.equal(got, undefined, v.id);
    else assert.equal(got, want, v.id);
  }
});

test("transform vectors", () => {
  for (const v of loadVectors("transform")) {
    const run = () => wasm.stAsText(wasm.stTransform(geom(v.a, v.src_srid), v.to_srid));
    if (runExpectingError(v, run)) continue;
    assertWithinToleranceMeters(v.id, run(), effective(v), v.to_srid);
  }
});

test("geojson vectors", () => {
  for (const v of loadVectors("geojson")) {
    if (v.fn === "asgeojson") {
      const run = () => {
        const g = geom(v.a, v.srid ?? 0);
        return v.arg === undefined || v.arg === null
          ? wasm.stAsGeojson(g)
          : wasm.stAsGeojsonDigits(g, v.arg);
      };
      if (runExpectingError(v, run)) continue;
      assert.equal(run(), effective(v), v.id);
    } else if (v.fn === "fromgeojson") {
      const run = () => wasm.stGeomFromGeojson(v.a);
      if (runExpectingError(v, run)) continue;
      const blob = run();
      assert.ok(
        geomApproxEqual(wasm.stAsText(blob), effective(v), 1e-12),
        `${v.id}: ${wasm.stAsText(blob)} vs ${effective(v)}`,
      );
      assert.equal(wasm.stSrid(blob), v.expected_srid, v.id);
    } else {
      throw new Error(`unknown fn ${v.fn}`);
    }
  }
});

test("h3 vectors", () => {
  for (const v of loadVectors("h3")) {
    const run = () => {
      switch (v.fn) {
        case "latlng_to_cell":
          return wasm.h3LatlngToCell(geom(v.a, 4326), v.arg);
        case "cell_to_parent":
          return wasm.h3CellToParent(BigInt(v.cell), v.arg);
        case "cell_to_string":
          return wasm.h3CellToString(BigInt(v.cell));
        case "string_to_cell":
          return wasm.h3StringToCell(v.a);
        default:
          throw new Error(`unknown fn ${v.fn}`);
      }
    };
    if (runExpectingError(v, run)) continue;
    const got = run();
    const want = effective(v);
    if (typeof got === "bigint") assert.equal(got, BigInt(want), v.id);
    else assert.equal(got, want, v.id);
  }
});

test("accessors vectors", () => {
  const geometric = new Set(["centroid", "envelope", "simplify"]);
  const optText = (blob) => (blob === undefined ? undefined : wasm.stAsText(blob));
  for (const v of loadVectors("accessors")) {
    const run = () => {
      // Constructors take numeric args instead of an input geometry.
      switch (v.fn) {
        case "makepoint":
          return wasm.stAsText(wasm.stMakePoint(v.args[0], v.args[1]));
        case "point":
          return wasm.stAsText(wasm.stPoint(v.args[0], v.args[1]));
        case "point_srid":
          return wasm.stSrid(wasm.stPointSrid(v.args[0], v.args[1], v.srid));
        case "makeenvelope":
          return wasm.stAsText(wasm.stMakeEnvelope(...v.args));
        case "makeenvelope_srid":
          return wasm.stSrid(wasm.stMakeEnvelopeSrid(...v.args, v.srid));
      }
      const g = geom(v.a);
      switch (v.fn) {
        case "npoints":
          return wasm.stNPoints(g);
        case "perimeter":
          return wasm.stPerimeter(g);
        case "geomtype":
          return wasm.stGeometryType(g);
        case "numgeoms":
          return wasm.stNumGeometries(g);
        case "geometryn":
          return optText(wasm.stGeometryN(g, v.arg));
        case "startpoint":
          return optText(wasm.stStartPoint(g));
        case "endpoint":
          return optText(wasm.stEndPoint(g));
        case "pointn":
          return optText(wasm.stPointN(g, v.arg));
        case "reverse":
          return wasm.stAsText(wasm.stReverse(g));
        case "area":
          return wasm.stArea(g);
        case "length":
          return wasm.stLength(g);
        case "centroid":
          return wasm.stAsText(wasm.stCentroid(g));
        case "envelope":
          return wasm.stAsText(wasm.stEnvelope(g));
        case "x":
          return wasm.stX(g);
        case "y":
          return wasm.stY(g);
        case "numpoints":
          return wasm.stNumPoints(g);
        case "isvalid":
          return wasm.stIsValid(g);
        case "simplify":
          return wasm.stAsText(wasm.stSimplify(g, v.arg));
        default:
          throw new Error(`unknown fn ${v.fn}`);
      }
    };
    if (runExpectingError(v, run)) continue;
    const got = run();
    const want = effective(v);
    if (want === null) {
      assert.equal(got, undefined, v.id);
    } else if (typeof want === "boolean") {
      assert.equal(got, want, v.id);
    } else if (typeof want === "number") {
      assertNumberClose(v.id, typeof got === "bigint" ? Number(got) : got, want);
    } else if (geometric.has(v.fn)) {
      assert.ok(geomApproxEqual(got, want), `${v.id}: ${got} vs ${want}`);
    } else {
      assert.equal(got, want, v.id);
    }
  }
});
