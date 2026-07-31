// Tier 2, primary host: the official SQLite WASM build. Every registered
// function through SQL, stub + NULL-strictness behavior, and the gpkg
// R-tree trigger flow under trusted_schema=off (INNOCUOUS must survive the
// JS-UDF path).

import assert from "node:assert/strict";
import { test } from "node:test";

import sqlite3InitModule from "@sqlite.org/sqlite-wasm";

import { loadManifest } from "../src/core.mjs";
import { registerKenro } from "../src/sqlite-wasm.mjs";
import { initWasm, smokeAllFunctions } from "./golden.mjs";

const wasm = await initWasm();
const sqlite3 = await sqlite3InitModule();

function openDb() {
  const db = new sqlite3.oo1.DB(":memory:");
  registerKenro(db, wasm, sqlite3);
  return db;
}

test("every function works through SQL", async () => {
  const db = openDb();
  try {
    await smokeAllFunctions(loadManifest(wasm), {
      run: (sql) => db.selectValue(sql),
      expectError: (sql) => {
        try {
          db.selectValue(sql);
        } catch (e) {
          return String(e.message ?? e);
        }
        throw new Error(`expected an error: ${sql}`);
      },
      skip: () => false,
    });
  } finally {
    db.close();
  }
});

test("gpkg rtree triggers run under trusted_schema=off", () => {
  const db = openDb();
  try {
    db.exec("PRAGMA trusted_schema = OFF");
    db.exec(`
      CREATE TABLE parks (fid INTEGER PRIMARY KEY, geom BLOB);
      CREATE VIRTUAL TABLE rtree_parks_geom USING rtree(id, minx, maxx, miny, maxy);
      CREATE TRIGGER rtree_parks_geom_insert AFTER INSERT ON parks
        WHEN (new.geom NOT NULL AND NOT ST_IsEmpty(NEW.geom))
      BEGIN
        INSERT OR REPLACE INTO rtree_parks_geom VALUES (
          NEW.fid,
          ST_MinX(NEW.geom), ST_MaxX(NEW.geom),
          ST_MinY(NEW.geom), ST_MaxY(NEW.geom)
        );
      END;
      INSERT INTO parks (fid, geom)
        VALUES (1, ST_AsGPB(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))', 4326)));
    `);
    assert.equal(db.selectValue("SELECT count(*) FROM rtree_parks_geom"), 1);
    assert.equal(
      db.selectValue(
        "SELECT ST_Within(ST_GeomFromGPB(geom), ST_GeomFromText('POLYGON((-1 -1,5 -1,5 5,-1 5,-1 -1))', 4326)) FROM parks",
      ),
      1,
    );
  } finally {
    db.close();
  }
});

test("ST_Union aggregate dissolves per group", () => {
  const db = openDb();
  try {
    db.exec(`
      CREATE TABLE zones (grp TEXT, geom BLOB);
      INSERT INTO zones VALUES
        ('a', ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))')),
        ('a', ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')),
        ('b', ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')),
        ('b', NULL);
    `);
    const rows = [];
    db.exec({
      sql: "SELECT grp, ST_Area(ST_Union(geom)) FROM zones GROUP BY grp ORDER BY grp",
      rowMode: "array",
      callback: (row) => rows.push(row),
    });
    assert.equal(rows.length, 2);
    assert.ok(Math.abs(rows[0][1] - 175) < 1e-6, String(rows[0][1]));
    assert.ok(Math.abs(rows[1][1] - 4) < 1e-6, String(rows[1][1])); // NULL row skipped
    // Zero rows → NULL.
    assert.equal(
      db.selectValue("SELECT ST_Union(geom) FROM zones WHERE grp = 'nope'"),
      null,
    );
  } finally {
    db.close();
  }
});

test("ST_AsMVT aggregate produces a tile", () => {
  const db = openDb();
  try {
    const tile = db.selectValue(`
      SELECT ST_AsMVT(
        ST_AsMVTGeom(ST_GeomFromText(wkt), ST_GeomFromText('POLYGON((0 0,100 0,100 100,0 100,0 0))'), 100, 0),
        'parks', 100, props)
      FROM (SELECT 'POINT(50 90)' AS wkt, json_object('name', 'yoyogi', 'rank', 3) AS props
            UNION ALL
            SELECT 'POINT(500 500)', json_object('name', 'out'))
    `);
    assert.ok(tile instanceof Uint8Array && tile.length > 0);
    const text = new TextDecoder("latin1").decode(tile);
    assert.ok(text.includes("parks"), "layer name embedded");
    assert.ok(text.includes("yoyogi"), "in-tile feature kept");
    assert.ok(!text.includes("out"), "clipped-away row skipped");
  } finally {
    db.close();
  }
});

test("h3 cells survive as 64-bit values", () => {
  const db = openDb();
  try {
    const cell = db.selectValue(
      "SELECT h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)', 4326), 9)",
    );
    const roundtrip = db.selectValue(
      "SELECT h3_string_to_cell(h3_cell_to_string(h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)', 4326), 9)))",
    );
    assert.equal(BigInt(cell), BigInt(roundtrip));
    assert.ok(BigInt(cell) > 2n ** 53n, `expected a >53-bit cell, got ${cell}`);
  } finally {
    db.close();
  }
});
