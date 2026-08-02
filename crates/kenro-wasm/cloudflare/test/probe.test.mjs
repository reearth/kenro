import { env } from "cloudflare:test";
import { it } from "vitest";
it("find the compound SELECT limit", async () => {
  await env.DB.prepare("CREATE TABLE IF NOT EXISTS lim (cell INTEGER, id TEXT)").run();
  for (const n of [2, 3, 4, 5, 6, 8, 10, 16, 32, 64, 100, 200, 500]) {
    const sql = Array.from({ length: n }, () => "SELECT id FROM lim WHERE cell BETWEEN ? AND ?").join(" UNION ");
    try {
      await env.DB.prepare(sql).bind(...Array.from({ length: n * 2 }, () => 1)).all();
      console.log(`UNION arms ${n}: ok`);
    } catch (e) { console.log(`UNION arms ${n}: ${e.message}`); break; }
  }
  for (const n of [10, 50, 100, 200, 500]) {
    const sql = "SELECT id FROM lim WHERE " + Array.from({ length: n }, () => "(cell BETWEEN ? AND ?)").join(" OR ");
    try {
      await env.DB.prepare(sql).bind(...Array.from({ length: n * 2 }, () => 1)).all();
      console.log(`OR terms ${n}: ok`);
    } catch (e) { console.log(`OR terms ${n}: ${e.message}`); break; }
  }
});
