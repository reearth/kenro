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
