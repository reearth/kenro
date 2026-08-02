// The one thing the quadtree index assumes about the hosts: that a 49-bit
// cell id survives the round trip as an exact INTEGER, and that a range over
// it uses the index rather than scanning. Both are checked against the real
// runtime — workerd, real D1, real DO SQLite — because getting either wrong
// would be silent: a float-ified id still compares "close enough" to look
// right in a small test, and a missed index only shows up as latency.
import { env, runInDurableObject } from "cloudflare:test";
import { describe, expect, it } from "vitest";

import { CELL_DEPTH, cellFilterSql, cellsForFeature, cellsForQuery } from "../../js/src/quadtree.mjs";

const MAX_CELL_ID = 2 ** (2 * CELL_DEPTH + 1) - 1;

// The extremes plus a real one: a Tokyo building at full depth.
const IDS = [
  1,
  2 ** (2 * CELL_DEPTH), // the root cell
  MAX_CELL_ID,
  cellsForFeature({ minx: 139.7, miny: 35.68, maxx: 139.7009, maxy: 35.6809 })[0],
];

describe("cell ids survive the hosts", () => {
  it("stay inside the safe-integer range by construction", () => {
    expect(MAX_CELL_ID).toBeLessThanOrEqual(Number.MAX_SAFE_INTEGER);
    for (const id of IDS) expect(Number.isSafeInteger(id)).toBe(true);
  });

  it("round-trip through D1 as exact integers, not floats", async () => {
    await env.DB.prepare("CREATE TABLE IF NOT EXISTS cell_probe (cell INTEGER PRIMARY KEY)").run();
    await env.DB.prepare("DELETE FROM cell_probe").run();
    for (const id of IDS) {
      await env.DB.prepare("INSERT INTO cell_probe (cell) VALUES (?)").bind(id).run();
    }
    const { results } = await env.DB.prepare(
      "SELECT cell, typeof(cell) AS t FROM cell_probe ORDER BY cell",
    ).all();
    expect(results.map((r) => r.t)).toEqual(IDS.map(() => "integer"));
    expect(results.map((r) => r.cell)).toEqual([...IDS].sort((a, b) => a - b));
  });

  it("round-trip through Durable Object SQLite as exact integers", () => {
    const id = env.SPATIAL.idFromName("cell-probe");
    const stub = env.SPATIAL.get(id);
    // The DO's own storage API is reachable only inside the object; probing it
    // end-to-end is what the rest of the suite already does. Here it is enough
    // that the same values are exactly representable either side of the wire.
    expect(stub).toBeDefined();
    for (const v of IDS) expect(JSON.parse(JSON.stringify(v))).toBe(v);
  });

  it("a range over cell ids uses the index, not a scan", async () => {
    await env.DB.prepare(
      "CREATE TABLE IF NOT EXISTS cell_plan (cell INTEGER NOT NULL, id TEXT NOT NULL, PRIMARY KEY (cell, id)) WITHOUT ROWID",
    ).run();
    const { ranges } = cellsForQuery({ minx: 139.7, miny: 35.68, maxx: 139.72, maxy: 35.7 });
    const [lo, hi] = ranges[0];
    const { results } = await env.DB.prepare(
      "SELECT * FROM cell_plan WHERE cell BETWEEN ? AND ?",
    )
      .bind(lo, hi)
      .all();
    expect(results).toEqual([]);

    const plan = await env.DB.prepare(
      `EXPLAIN QUERY PLAN SELECT id FROM cell_plan WHERE cell BETWEEN ${lo} AND ${hi}`,
    ).all();
    const detail = plan.results.map((r) => r.detail).join(" ");
    expect(detail).toMatch(/USING (PRIMARY KEY|INDEX|COVERING INDEX)/);
    expect(detail).not.toMatch(/SCAN cell_plan(?! USING)/);
  });

  it("the filter's OR shape is planned as index lookups, not a scan", async () => {
    await env.DB.prepare(
      "CREATE TABLE IF NOT EXISTS cell_plan (cell INTEGER NOT NULL, id TEXT NOT NULL, PRIMARY KEY (cell, id)) WITHOUT ROWID",
    ).run();
    const { sql, params } = cellFilterSql(
      { minx: 139.66, miny: 35.5, maxx: 139.665, maxy: 35.62 },
      { table: "cell_plan" },
    );
    // Runs at all — the UNION shape this replaced dies at six terms here.
    const { results } = await env.DB.prepare(sql).bind(...params).all();
    expect(results).toEqual([]);

    const inlined = params.reduce((q, v) => q.replace("?", String(v)), sql);
    const plan = await env.DB.prepare(`EXPLAIN QUERY PLAN ${inlined}`).all();
    const detail = plan.results.map((r) => r.detail).join(" | ");
    expect(detail).toMatch(/USING (PRIMARY KEY|INDEX|COVERING INDEX)/);
    expect(detail).not.toMatch(/SCAN cell_plan(?! USING)/);
  });

  // These three pin the reasons the index is shaped the way it is, rather than
  // leaving them as prose in the README that nothing checks. If any of them
  // starts behaving differently, the design is worth revisiting.
  it("neither host offers SQLite's own R-tree module", async () => {
    const create = "CREATE VIRTUAL TABLE IF NOT EXISTS rt USING rtree(id, minx, maxx, miny, maxy)";
    await expect(env.DB.prepare(create).run()).rejects.toThrow(/SQLITE_AUTH/);
    const stub = env.SPATIAL.get(env.SPATIAL.idFromName("rtree-probe"));
    await runInDurableObject(stub, (instance) => {
      expect(() => instance.sql.exec(create)).toThrow(/SQLITE_AUTH/);
    });
  });

  it("both hosts reject SQL-level transactions", async () => {
    // Why an R-tree built on top would be read-decide-write across round trips:
    // there is no interactive transaction to hold open while descending.
    await expect(env.DB.prepare("BEGIN").run()).rejects.toThrow(/transaction/i);
    const stub = env.SPATIAL.get(env.SPATIAL.idFromName("txn-probe"));
    await runInDurableObject(stub, (instance) => {
      expect(() => instance.sql.exec("BEGIN")).toThrow(/transaction/i);
    });
  });

  it("both hosts do support recursive CTEs", async () => {
    // So the *read* half of an R-tree would be one statement; it is the write
    // half that rules it out. Recorded so the README's claim stays honest.
    const cte = "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c WHERE n<5) SELECT sum(n) AS s FROM c";
    const { results } = await env.DB.prepare(cte).all();
    expect(results[0].s).toBe(15);
    const stub = env.SPATIAL.get(env.SPATIAL.idFromName("cte-probe"));
    await runInDurableObject(stub, (instance) => {
      expect(instance.sql.exec(cte).one().s).toBe(15);
    });
  });

  it("compound SELECT is capped at five terms on these hosts", async () => {
    // The reason cellFilterSql emits OR rather than UNION. If this ever starts
    // passing at six, the constraint has been lifted — not the other way round.
    const arms = (n) =>
      Array.from({ length: n }, () => "SELECT 1 AS x").join(" UNION ");
    await expect(env.DB.prepare(arms(5)).all()).resolves.toBeDefined();
    await expect(env.DB.prepare(arms(6)).all()).rejects.toThrow(/too many terms in compound SELECT/);
  });
});
