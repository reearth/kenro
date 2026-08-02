// Lifetime helpers for `Prepared`, the decoded-geometry handle.
//
// A handle is a wasm-heap allocation that JS cannot collect: forget `free()`
// and the geometry stays alive for the life of the isolate. Two failure modes,
// both easy to hit in a refine loop:
//
//   - an early `continue`/`break`/`throw` skips the `free()`
//   - `free()` twice throws "null pointer passed to rust"
//
// If your toolchain has explicit resource management, you do not need this
// module at all — wasm-bindgen already wires `Symbol.dispose` to `free`, so
//
//     using g = kenro.Prepared.fromBlob(row.geom);
//
// frees at the end of the block (verified on Node and in workerd). What is
// here is the callback equivalent for everywhere else, plus the idempotence
// that the built-in dispose does not have: it *is* `free`, so pairing `using`
// with a manual `free()` still double-frees.
//
// Nothing in this module touches wasm — it only sequences calls on handles.

/**
 * Free a handle, at most once. Safe on an already-freed handle, on `null`
 * and on `undefined`, so it composes with optional handles:
 *
 *     let projected;   // only assigned on a hit
 *     try { … } finally { freeOnce(projected); freeOnce(candidate); }
 */
export function freeOnce(handle) {
  // wasm-bindgen zeroes __wbg_ptr on free and throws if asked again; reading
  // it is how we stay idempotent without swallowing real errors from free().
  if (handle && handle.__wbg_ptr !== 0) handle.free();
}

/**
 * Own a handle for the duration of `fn` and free it afterwards, whatever
 * happens — including a `throw`. Returns whatever `fn` returns.
 *
 *     const hits = rows.filter((row) =>
 *       withPrepared(kenro.Prepared.fromBlob(row.geom), (g) => g.stIntersects(win)));
 *
 * One handle per call, so a loop cannot accumulate them.
 */
export function withPrepared(handle, fn) {
  try {
    return fn(handle);
  } finally {
    freeOnce(handle);
  }
}

/**
 * A scope that owns any number of handles. `own` registers one and hands it
 * straight back, so handles created part-way through — a reprojection, say —
 * are covered too:
 *
 *     const out = withScope((own) => {
 *       const g = own(kenro.Prepared.fromBlob(row.geom));
 *       if (!g.stIntersects(win)) return null;
 *       return own(g.stTransform(3857)).stAsGeojson();
 *     });
 *
 * Freed in reverse order of registration on the way out. Keep a scope inside
 * the loop body, not around it, or it will hold every row's geometry at once.
 */
export function withScope(fn) {
  const owned = [];
  try {
    return fn((handle) => {
      owned.push(handle);
      return handle;
    });
  } finally {
    for (let i = owned.length - 1; i >= 0; i--) freeOnce(owned[i]);
  }
}
