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

test("every geojson vector agrees between the blob and handle APIs", () => {
  let compared = 0;
  for (const v of loadVectors("geojson")) {
    if (v.fn !== "asgeojson") continue;
    const blob = wasm.stGeomFromTextSrid(v.a, v.srid ?? 0);
    const digits = v.arg ?? null;
    const viaBlob = outcome(() =>
      digits === null ? wasm.stAsGeojson(blob) : wasm.stAsGeojsonDigits(blob, digits),
    );
    const p = wasm.Prepared.fromBlob(blob);
    try {
      const viaHandle = outcome(() =>
        digits === null ? p.stAsGeojson() : p.stAsGeojsonDigits(digits),
      );
      assert.deepEqual(viaHandle, viaBlob, v.id);
    } finally {
      p.free();
    }
    compared++;
  }
  assert.ok(compared > 5, `only ${compared} vectors compared`);
});

test("every transform vector agrees between the blob and handle APIs", () => {
  let compared = 0;
  for (const v of loadVectors("transform")) {
    if (v.fn !== "transform") continue;
    const blob = wasm.stGeomFromTextSrid(v.a, v.src_srid);
    const viaBlob = outcome(() => wasm.stAsText(wasm.stTransform(blob, v.to_srid)));

    const p = wasm.Prepared.fromBlob(blob);
    let projected;
    try {
      const viaHandle = outcome(() => {
        projected = p.stTransform(v.to_srid);
        return projected.stAsText();
      });
      assert.deepEqual(viaHandle, viaBlob, v.id);
    } finally {
      projected?.free();
      p.free();
    }
    compared++;
  }
  assert.ok(compared > 5, `only ${compared} vectors compared`);
});

test("stTransform leaves the source handle untouched", () => {
  // It returns a new handle rather than reprojecting in place — the source
  // must still be usable, and at its original SRID.
  const p = wasm.Prepared.fromText("POINT(139.7 35.68)", 4326);
  const projected = p.stTransform(3857);
  try {
    assert.equal(p.stSrid(), 4326);
    assert.equal(p.stAsText(), "POINT(139.7 35.68)");
    assert.equal(projected.stSrid(), 3857);
  } finally {
    projected.free();
    p.free();
  }
});

test("text, WKB and GPB output match the blob functions", () => {
  const blob = wasm.stGeomFromTextSrid("POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))", 4326);
  const p = wasm.Prepared.fromBlob(blob);
  try {
    assert.equal(p.stAsText(), wasm.stAsText(blob));
    assert.deepEqual(p.stAsBinary(), wasm.stAsBinary(blob));
    assert.deepEqual(p.stAsGpb(), wasm.stAsGpb(blob));
    // Round-trips back through the blob API.
    assert.equal(wasm.stAsText(p.stAsGpb()), "POLYGON((0 0,4 0,4 4,0 4,0 0))");
  } finally {
    p.free();
  }
});

test("an unknown SRID refuses to transform, in PostGIS's words", () => {
  const p = wasm.Prepared.fromText("POINT(1 2)", 0);
  try {
    assert.throws(() => p.stTransform(3857), /unknown \(0\) SRID/);
  } finally {
    p.free();
  }
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
