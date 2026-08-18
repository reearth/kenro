// Host-independent UDF factory: turns kenro-wasm exports into plain JS
// functions over JS values (null | Uint8Array | string | number | bigint)
// implementing kenro's value-mapping contract exactly (the mirror of
// src/sqlite/rusqlite_ext.rs). The adapters contain no function names —
// everything is driven by the manifest.

/** Parse the manifest JSON exported by the wasm module. */
export function loadManifest(wasm) {
  return JSON.parse(wasm.manifest());
}

function fail(name, message) {
  return new Error(`kenro: ${name}: ${message}`);
}

function convertArg(entry, i, v) {
  const kind = entry.args[i];
  const name = entry.sql_name;
  switch (kind) {
    case "blob":
      if (v instanceof Uint8Array) return v;
      if (typeof v === "string") {
        throw fail(
          name,
          "got TEXT where a geometry BLOB was expected (did you mean ST_GeomFromText?)",
        );
      }
      throw fail(name, `expected a geometry BLOB, got ${typeof v}`);
    case "blob_or_text":
      // The box accessors (ST_MinX … ST_ZMax). Both forms cross as bytes —
      // TEXT as its UTF-8 — because kenro tells them apart by content, and a
      // geometry encoding never starts with `B`.
      if (v instanceof Uint8Array) return v;
      if (typeof v === "string") return new TextEncoder().encode(v);
      throw fail(name, `expected a geometry BLOB or box text, got ${typeof v}`);
    case "text":
      if (typeof v === "string") return v;
      throw fail(name, `expected TEXT, got ${typeof v}`);
    case "int": {
      const n = typeof v === "bigint" ? Number(v) : v;
      if (typeof n === "number" && Number.isInteger(n)) return n;
      throw fail(name, "expected an INTEGER");
    }
    case "bool": {
      // SQLite has no boolean type: 0/1 is how `true` arrives, and callers
      // who write JS booleans should not be punished for it.
      if (typeof v === "boolean") return v;
      const n = typeof v === "bigint" ? Number(v) : v;
      if (n === 0 || n === 1) return n === 1;
      throw fail(name, "expected a boolean (0 or 1)");
    }
    case "i64":
      if (typeof v === "bigint") return v;
      if (typeof v === "number" && Number.isSafeInteger(v)) return BigInt(v);
      throw fail(name, "expected an INTEGER");
    case "real":
      if (typeof v === "bigint") return Number(v);
      if (typeof v === "number") return v;
      throw fail(name, "expected a numeric value");
    case "text_or_int": {
      // TEXT as-is; INTEGER n normalized to quad_segs=n (identical to the
      // rusqlite binding — a shared smoke vector keeps the layers in sync).
      if (typeof v === "string") return v;
      const n = typeof v === "bigint" ? Number(v) : v;
      if (typeof n === "number" && Number.isInteger(n)) return `quad_segs=${n}`;
      throw fail(name, "expected TEXT options or INTEGER");
    }
    default:
      throw fail(name, `unsupported argument kind ${kind}`);
  }
}

/**
 * Build the UDF for one manifest entry: NULL-strict (any SQL NULL argument
 * → NULL result, before wasm is called), argument kinds checked with
 * kenro-worded errors, `undefined` (Rust `Option::None`) → SQL NULL,
 * booleans → 0/1.
 */
export function makeUdf(entry, wasm) {
  const f = wasm[entry.export];
  if (typeof f !== "function") {
    throw new Error(`kenro-wasm export missing: ${entry.export}`);
  }
  return (...args) => {
    if (args.length !== entry.args.length) {
      // Defense against host arity miswiring (e.g. a registry shim failure):
      // fail loudly instead of letting wasm coerce missing arguments.
      throw fail(entry.sql_name, `expected ${entry.args.length} arguments, got ${args.length}`);
    }
    if (args.some((a) => a === null || a === undefined)) return null;
    const converted = args.map((a, i) => convertArg(entry, i, a));
    const result = f(...converted);
    if (result === undefined || result === null) return null;
    if (typeof result === "boolean") return result ? 1 : 0;
    // Small integral results (vertex counts, SRIDs) come back as BigInt from
    // wasm-bindgen i64 returns; hosts without BigInt support (sql.js) would
    // drop them. True 64-bit values (Kind "i64", the h3 family) stay BigInt.
    if (typeof result === "bigint" && entry.ret !== "i64") {
      return Number(result);
    }
    return result;
  };
}

/**
 * Build the driver for one aggregate entry. Aggregate NULL handling
 * differs from scalars: NULL rows are SKIPPED (PostGIS aggregate
 * semantics). `finish` frees the wasm accumulator in all paths.
 */
export function makeAggregate(entry, wasm) {
  const Ctor = wasm[entry.ctor_export];
  if (typeof Ctor !== "function") {
    throw new Error(`kenro-wasm export missing: ${entry.ctor_export}`);
  }
  return {
    start: () => new Ctor(),
    step: (acc, args) => {
      if (args.some((a) => a === null || a === undefined)) return;
      const converted = args.map((a, i) => convertArg(entry, i, a));
      acc.step(...converted);
    },
    finish: (acc) => {
      try {
        const result = acc.finish();
        return result === undefined || result === null ? null : result;
      } finally {
        acc.free();
      }
    },
  };
}

/** The loud-failure body shared by every stub registration. */
export function stubUdf(stub) {
  const message = `kenro: ${stub.name} is not implemented in kenro. ${stub.hint}`;
  return () => {
    throw new Error(message);
  };
}

/** The loud-failure body for hosts that cannot represent 64-bit integers. */
export function i64UnsupportedUdf(entry, hostName) {
  const message =
    `kenro: ${entry.sql_name}: ${hostName} cannot represent 64-bit H3 cell ids; ` +
    "use @sqlite.org/sqlite-wasm or wa-sqlite for the h3_* functions";
  return () => {
    throw new Error(message);
  };
}
