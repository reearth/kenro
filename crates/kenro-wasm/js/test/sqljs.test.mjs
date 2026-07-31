// Tier 2: sql.js. Same full-function smoke, with the host's documented
// limitations asserted rather than papered over: h3_* functions raise a
// loud error (no int64 path), and arity overloads work through the
// registry shim (including db.close() afterwards).

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";

import initSqlJs from "sql.js";

import { loadManifest } from "../src/core.mjs";
import { registerKenro } from "../src/sqljs.mjs";
import { initWasm, smokeAllFunctions } from "./golden.mjs";

const require = createRequire(import.meta.url);
const wasm = await initWasm();
const SQL = await initSqlJs({
  locateFile: (file) => require.resolve(`sql.js/dist/${file}`),
});

function openDb() {
  const db = new SQL.Database();
  registerKenro(db, wasm);
  return db;
}

function selectValue(db, sql) {
  const results = db.exec(sql);
  if (!results.length) return null;
  return results[0].values[0][0];
}

test("every function works through SQL (h3 = loud error)", async () => {
  const db = openDb();
  try {
    await smokeAllFunctions(loadManifest(wasm), {
      run: (sql) => selectValue(db, sql),
      expectError: (sql) => {
        try {
          selectValue(db, sql);
        } catch (e) {
          return String(e.message ?? e);
        }
        throw new Error(`expected an error: ${sql}`);
      },
      skip: (entry) => entry.uses_i64,
    });
  } finally {
    db.close();
  }
});

test("arity overloads coexist and close() survives the shim", () => {
  const db = openDb();
  assert.equal(
    selectValue(db, "SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))"),
    "POINT(1 2)",
  );
  assert.equal(
    selectValue(db, "SELECT ST_SRID(ST_GeomFromText('POINT(1 2)', 4326))"),
    4326,
  );
  assert.equal(
    selectValue(db, "SELECT ST_AsGeoJSON(ST_GeomFromText('POINT(1 2)'))"),
    '{"type":"Point","coordinates":[1,2]}',
  );
  assert.equal(
    selectValue(db, "SELECT ST_AsGeoJSON(ST_GeomFromText('POINT(1.234 2)'), 1)"),
    '{"type":"Point","coordinates":[1.2,2]}',
  );
  db.close(); // must not throw despite the renamed registry keys
});

test("h3 functions fail loudly, never lossily", () => {
  const db = openDb();
  try {
    assert.throws(
      () =>
        selectValue(
          db,
          "SELECT h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)', 4326), 9)",
        ),
      /64-bit H3 cell ids/,
    );
  } finally {
    db.close();
  }
});

test("documented limitation: stock sql.js has no R-tree module", () => {
  // GeoPackage spatial-index maintenance is impossible on this host — not a
  // kenro restriction, the module is absent from the sql.js build. Pinned
  // here so a future sql.js that adds R-tree flips this test and the docs.
  const db = openDb();
  try {
    assert.throws(
      () =>
        db.exec(
          "CREATE VIRTUAL TABLE r USING rtree(id, minx, maxx, miny, maxy)",
        ),
      /no such module: rtree/,
    );
  } finally {
    db.close();
  }
});
