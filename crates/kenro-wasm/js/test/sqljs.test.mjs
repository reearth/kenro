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

test("ST_Union aggregate works through the __finalize shim", () => {
  const db = openDb();
  try {
    db.exec(`
      CREATE TABLE zones (grp TEXT, geom BLOB);
      INSERT INTO zones VALUES
        ('a', ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))')),
        ('a', ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')),
        ('b', ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'));
    `);
    const rows = db.exec(
      "SELECT grp, ST_Area(ST_Union(geom)) FROM zones GROUP BY grp ORDER BY grp",
    )[0].values;
    assert.ok(Math.abs(rows[0][1] - 175) < 1e-6, String(rows[0][1]));
    assert.ok(Math.abs(rows[1][1] - 4) < 1e-6, String(rows[1][1]));
    // The scalar 2-arg ST_Union coexists with the 1-arg aggregate.
    assert.equal(
      selectValue(
        db,
        "SELECT ST_Area(ST_Union(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), ST_GeomFromText('POLYGON((2 2,3 2,3 3,2 3,2 2))')))",
      ),
      2,
    );
    db.close();
  } catch (e) {
    db.close();
    throw e;
  }
});

test("the routing aggregates run over an edge table", () => {
  const db = openDb();
  try {
    db.exec(`
      CREATE TABLE edges (id INTEGER, source INTEGER, target INTEGER, cost REAL, rcost REAL);
      INSERT INTO edges VALUES (1, 1, 2, 1.1, 2.5), (2, 2, 3, 0.7, 4.0);
    `);
    // The 6-argument form: one-way, and the path comes back as JSON.
    const path = JSON.parse(
      selectValue(
        db,
        "SELECT kenro_dijkstra(id, source, target, cost, 1, 3) FROM edges",
      ),
    );
    assert.deepEqual(
      path.map((r) => r.node),
      [1, 2, 3],
    );
    assert.equal(path.at(-1).edge, -1);
    assert.ok(Math.abs(path.at(-1).agg_cost - 1.8) < 1e-9);
    // The 7-argument form: the trailing reverse_cost is the only reason
    // 3 -> 1 exists.
    assert.ok(
      Math.abs(
        selectValue(
          db,
          "SELECT kenro_dijkstra_cost(source, target, cost, 3, 1, rcost) FROM edges",
        ) - 6.5,
      ) < 1e-9,
    );
    // No path is SQL NULL, not an error.
    assert.equal(
      selectValue(
        db,
        "SELECT kenro_dijkstra(id, source, target, cost, 3, 1) FROM edges",
      ),
      null,
    );
    // Driving distance: a limit of 1.5 reaches node 2 but not node 3.
    const reach = JSON.parse(
      selectValue(
        db,
        "SELECT kenro_drivingdistance(id, source, target, cost, 1, 1.5) FROM edges",
      ),
    );
    assert.deepEqual(
      reach.map((r) => r.node),
      [1, 2],
    );
    db.close();
  } catch (e) {
    db.close();
    throw e;
  }
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
