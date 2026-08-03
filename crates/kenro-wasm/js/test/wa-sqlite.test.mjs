// Tier 2: wa-sqlite (synchronous build). Full-function smoke through the
// adapter, whose try/catch → result_error is load-bearing (wa-sqlite does
// not catch UDF exceptions itself).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { test } from "node:test";

import * as SQLite from "wa-sqlite";
import SQLiteESMFactory from "wa-sqlite/dist/wa-sqlite.mjs";

import { loadManifest } from "../src/core.mjs";
import { registerKenro } from "../src/wa-sqlite.mjs";
import { initWasm, smokeAllFunctions } from "./golden.mjs";

const require = createRequire(import.meta.url);
const wasm = await initWasm();
// Node's fetch cannot read file:// URLs, so hand the Emscripten loader the
// wasm binary directly.
const module = await SQLiteESMFactory({
  wasmBinary: readFileSync(require.resolve("wa-sqlite/dist/wa-sqlite.wasm")),
});
const sqlite3 = SQLite.Factory(module);

async function selectValue(db, sql) {
  let value = null;
  for await (const stmt of sqlite3.statements(db, sql)) {
    if ((await sqlite3.step(stmt)) === SQLite.SQLITE_ROW) {
      value = sqlite3.column(stmt, 0);
    }
  }
  return value;
}

test("every function works through SQL", async () => {
  const db = await sqlite3.open_v2(":memory:");
  registerKenro(sqlite3, db, wasm);
  try {
    await smokeAllFunctions(loadManifest(wasm), {
      run: (sql) => selectValue(db, sql),
      expectError: async (sql) => {
        try {
          await selectValue(db, sql);
        } catch (e) {
          return String(e.message ?? e);
        }
        throw new Error(`expected an error: ${sql}`);
      },
      skip: () => false,
    });
  } finally {
    await sqlite3.close(db);
  }
});

test("ST_Union aggregate dissolves per group", async () => {
  const db = await sqlite3.open_v2(":memory:");
  registerKenro(sqlite3, db, wasm);
  try {
    for await (const stmt of sqlite3.statements(
      db,
      `CREATE TABLE zones (grp TEXT, geom BLOB);
       INSERT INTO zones VALUES
         ('a', ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))')),
         ('a', ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')),
         ('b', ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'));`,
    )) {
      while ((await sqlite3.step(stmt)) === SQLite.SQLITE_ROW) {
        // no rows expected
      }
    }
    const areas = [];
    for await (const stmt of sqlite3.statements(
      db,
      "SELECT ST_Area(ST_Union(geom)) FROM zones GROUP BY grp ORDER BY grp",
    )) {
      while ((await sqlite3.step(stmt)) === SQLite.SQLITE_ROW) {
        areas.push(sqlite3.column(stmt, 0));
      }
    }
    assert.equal(areas.length, 2);
    assert.ok(Math.abs(areas[0] - 175) < 1e-6, String(areas[0]));
    assert.ok(Math.abs(areas[1] - 4) < 1e-6, String(areas[1]));
  } finally {
    await sqlite3.close(db);
  }
});

test("h3 cells survive as 64-bit values", async () => {
  const db = await sqlite3.open_v2(":memory:");
  registerKenro(sqlite3, db, wasm);
  try {
    const cell = await selectValue(
      db,
      "SELECT h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)', 4326), 9)",
    );
    assert.ok(BigInt(cell) > 2n ** 53n, `expected a >53-bit cell, got ${cell}`);
  } finally {
    await sqlite3.close(db);
  }
});

test("documented limitation: the wa-sqlite build has no R-tree module", async () => {
  // Same shape as the sql.js limitation, and it was mis-documented as ✅
  // until measured: neither wa-sqlite build (sync or async, both SQLite
  // 3.44.0) carries the rtree module, so GeoPackage spatial-index
  // maintenance is impossible on this host. Not a kenro restriction — the
  // module is absent. Pinned so a future build that adds it flips this test
  // and the docs together.
  const db = await sqlite3.open_v2(":memory:");
  try {
    await assert.rejects(
      async () => {
        for await (const stmt of sqlite3.statements(
          db,
          "CREATE VIRTUAL TABLE r USING rtree(id, minx, maxx, miny, maxy)",
        )) {
          await sqlite3.step(stmt);
        }
      },
      /no such module: rtree/,
    );
  } finally {
    await sqlite3.close(db);
  }
});

test("json_each and unhex are available, so the row-splitting recipes work", async () => {
  // docs/functions.md's "Getting N rows out" recipes rely on JSON1 and
  // unhex. wa-sqlite pins the oldest SQLite of any host kenro supports
  // (3.44.0 against 3.49–3.53 elsewhere), so it is the one worth asserting:
  // unhex needs 3.41+.
  const db = await sqlite3.open_v2(":memory:");
  try {
    assert.equal(
      await selectValue(db, "SELECT count(*) FROM json_each('[10,20,30]')"),
      3,
    );
    assert.equal(await selectValue(db, "SELECT hex(unhex('414243'))"), "414243");
    // The MULTI* → rows shape, end to end.
    assert.equal(
      await selectValue(
        db,
        `SELECT count(*) FROM json_each(json_extract(
           '{"type":"MultiPolygon","coordinates":[[[[0,0]]],[[[1,1]]]]}', '$.coordinates'))`,
      ),
      2,
    );
  } finally {
    await sqlite3.close(db);
  }
});
