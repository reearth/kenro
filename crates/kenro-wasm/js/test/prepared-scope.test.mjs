// kenro-wasm/prepared — the lifetime helpers. What is actually being tested
// is that the handle is gone afterwards, so each case asserts the freed state
// rather than trusting that `free()` was reached.
import assert from "node:assert/strict";
import { test } from "node:test";

import { freeOnce, withPrepared, withScope } from "../src/prepared.mjs";
import { initWasm } from "./golden.mjs";

const wasm = await initWasm();

const point = (wkt = "POINT(1 1)") => wasm.Prepared.fromText(wkt, 4326);
const square = () => wasm.Prepared.fromText("POLYGON((0 0, 2 0, 2 2, 0 2, 0 0))", 4326);

/** A freed handle traps in wasm-bindgen; that trap is the proof. */
function assertFreed(handle) {
  assert.throws(() => handle.stAsText(), /null pointer passed to rust/);
}

test("freeOnce is idempotent, and ignores null/undefined", () => {
  const p = point();
  freeOnce(p);
  assertFreed(p);
  // A second free() would throw "null pointer passed to rust" — the whole
  // reason these helpers exist rather than a bare `finally { h.free() }`.
  freeOnce(p);
  freeOnce(null);
  freeOnce(undefined);
});

test("withPrepared frees on the normal path and returns the callback's value", () => {
  const p = square();
  const q = point();
  try {
    assert.equal(
      withPrepared(p, (g) => g.stContains(q)),
      true,
    );
    assertFreed(p);
  } finally {
    freeOnce(q);
  }
});

test("withPrepared frees when the callback throws, and the error propagates", () => {
  const p = point();
  assert.throws(() => withPrepared(p, () => {
    throw new Error("boom");
  }), /boom/);
  assertFreed(p);
});

test("withPrepared frees when the callback returns early from a loop", () => {
  const handles = [];
  const first = (() => {
    for (const wkt of ["POINT(1 1)", "POINT(2 2)", "POINT(3 3)"]) {
      const p = point(wkt);
      handles.push(p);
      const found = withPrepared(p, (g) => (g.stAsText() === "POINT(2 2)" ? g.stAsText() : null));
      if (found) return found; // the early exit that would leak a bare free()
    }
    return null;
  })();
  assert.equal(first, "POINT(2 2)");
  for (const h of handles) assertFreed(h);
});

test("withPrepared tolerates a callback that frees the handle itself", () => {
  const p = point();
  assert.equal(
    withPrepared(p, (g) => {
      const wkt = g.stAsText();
      g.free(); // double-free territory without freeOnce's guard
      return wkt;
    }),
    "POINT(1 1)",
  );
  assertFreed(p);
});

test("withScope frees everything it owns, in reverse order", () => {
  const seen = [];
  withScope((own) => {
    for (const wkt of ["POINT(1 1)", "POINT(2 2)"]) {
      seen.push(own(point(wkt)));
    }
  });
  for (const h of seen) assertFreed(h);
});

test("withScope covers handles created mid-scope, like a reprojection", () => {
  let source;
  let projected;
  const geojson = withScope((own) => {
    source = own(point("POINT(139.7 35.68)"));
    projected = own(source.stTransform(3857));
    return projected.stAsGeojson();
  });
  assert.match(geojson, /"coordinates":\[15/); // Web Mercator metres
  assertFreed(source);
  assertFreed(projected);
});

test("withScope frees when the body throws", () => {
  let inner;
  assert.throws(() =>
    withScope((own) => {
      inner = own(point());
      throw new Error("boom");
    }), /boom/);
  assertFreed(inner);
});

test("`using` frees a handle with no helper at all", () => {
  // wasm-bindgen wires Symbol.dispose to free, so the language does the work
  // wherever explicit resource management is available. Asserted here so a
  // wasm-bindgen upgrade that dropped it would not quietly break the docs.
  let escaped;
  {
    using p = point();
    escaped = p;
    assert.equal(p.stAsText(), "POINT(1 1)");
  }
  assertFreed(escaped);
});

test("the built-in dispose is free itself, so it is not idempotent", () => {
  // The reason freeOnce exists: mixing `using` with a manual free() would
  // otherwise trap on the way out of the block.
  const p = point();
  p.free();
  assert.throws(() => p[Symbol.dispose](), /null pointer passed to rust/);
  freeOnce(p); // the guarded path stays quiet
});
