// The `Prepared` handle must be indistinguishable from the blob functions —
// same answers, same errors, same wording. Proven by replaying the same
// golden vectors (PostGIS reference) through both APIs and comparing, so the
// handle cannot drift into its own semantics.

import assert from "node:assert/strict";
import { test } from "node:test";

import { initWasm, loadVectors } from "./golden.mjs";

const wasm = await initWasm();

/** Blob API and handle API for each predicate the handle exposes. */
const PAIRED = {
  intersects: [(a, b) => wasm.stIntersects(a, b), (a, b) => a.stIntersects(b)],
  contains: [(a, b) => wasm.stContains(a, b), (a, b) => a.stContains(b)],
  within: [(a, b) => wasm.stWithin(a, b), (a, b) => a.stWithin(b)],
  covers: [(a, b) => wasm.stCovers(a, b), (a, b) => a.stCovers(b)],
  distance: [(a, b) => wasm.stDistance(a, b), (a, b) => a.stDistance(b)],
  dwithin: [
    (a, b, d) => wasm.stDwithin(a, b, d),
    (a, b, d) => a.stDwithin(b, d),
  ],
};

/** Run `fn`, returning either its value or its error message. */
function outcome(fn) {
  try {
    return { value: fn() };
  } catch (e) {
    return { error: String(e.message ?? e) };
  }
}

test("every predicate vector agrees between the blob and handle APIs", () => {
  let compared = 0;

  for (const v of loadVectors("predicates")) {
    const pair = PAIRED[v.fn];
    if (!pair) continue;
    const [viaBlob, viaHandle] = pair;
    if (v.b === undefined) continue;

    const a = wasm.stGeomFromText(v.a);
    const b = wasm.stGeomFromText(v.b);
    const blob = outcome(() => viaBlob(a, b, v.arg));

    const pa = wasm.Prepared.fromBlob(a);
    const pb = wasm.Prepared.fromBlob(b);
    try {
      const handle = outcome(() => viaHandle(pa, pb, v.arg));
      assert.deepEqual(handle, blob, `${v.id ?? v.fn}: ${v.a} / ${v.b}`);
    } finally {
      pa.free();
      pb.free();
    }
    compared++;
  }

  assert.ok(compared > 20, `only ${compared} vectors compared`);
});

test("fromText and fromBlob describe the same geometry", () => {
  const wkt = "POLYGON((0 0, 2 0, 2 2, 0 2, 0 0))";
  const fromText = wasm.Prepared.fromText(wkt, 4326);
  const fromBlob = wasm.Prepared.fromBlob(wasm.stGeomFromTextSrid(wkt, 4326));
  const point = wasm.Prepared.fromText("POINT(1 1)", 4326);
  try {
    assert.equal(fromText.stContains(point), true);
    assert.equal(fromBlob.stContains(point), true);
  } finally {
    fromText.free();
    fromBlob.free();
    point.free();
  }
});

test("a GeoPackage blob decodes as readily as an internal one", () => {
  const internal = wasm.stGeomFromTextSrid("POLYGON((0 0, 2 0, 2 2, 0 2, 0 0))", 4326);
  const p = wasm.Prepared.fromBlob(wasm.stAsGpb(internal));
  const q = wasm.Prepared.fromText("POINT(1 1)", 4326);
  try {
    assert.equal(p.stIntersects(q), true);
  } finally {
    p.free();
    q.free();
  }
});

test("mixed known SRIDs error exactly as the blob API does", () => {
  const a = wasm.Prepared.fromText("POINT(0 0)", 4326);
  const b = wasm.Prepared.fromText("POINT(0 0)", 3857);
  try {
    assert.throws(() => a.stIntersects(b), /kenro:.*SRID/i);
  } finally {
    a.free();
    b.free();
  }
});

test("invalid input fails at construction, with kenro's wording", () => {
  assert.throws(() => wasm.Prepared.fromText("NOT WKT", 4326), /^Error: kenro: /);
  assert.throws(() => wasm.Prepared.fromBlob(new Uint8Array([1, 2, 3])), /^Error: kenro: /);
});
