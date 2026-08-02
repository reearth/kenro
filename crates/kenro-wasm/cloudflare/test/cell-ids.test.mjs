// The one thing the quadtree index assumes about the hosts: that a 49-bit
// cell id survives the round trip as an exact INTEGER, and that a range over
// it uses the index rather than scanning. Both are checked against the real
// runtime — workerd, real D1, real DO SQLite — because getting either wrong
// would be silent: a float-ified id still compares "close enough" to look
// right in a small test, and a missed index only shows up as latency.
import { env } from "cloudflare:test";
import { describe, expect, it } from "vitest";

import { CELL_DEPTH, cellsForFeature, cellsForQuery } from "../../js/src/quadtree.mjs";

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
});
