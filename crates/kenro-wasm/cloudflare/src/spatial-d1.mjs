// The same plan from spatial.mjs, on D1.
//
// D1 is a *remote* SQLite: every candidate row crosses the network as a
// geometry blob and is billed as a row read, so the coarse filter earns its
// keep here even more than it does in a Durable Object. Two host differences
// matter and are handled below:
//
//   - a BLOB comes back as a plain number[], not an ArrayBuffer
//   - there is no synchronous transaction; writes go through `batch()`,
//     which D1 runs as one atomic unit

import { OVERSIZED } from "./tiles.mjs";
import { plan, prepareFeature, refine } from "./spatial.mjs";

// The schema lives in migrations/0001_init.sql, applied by
// `wrangler d1 migrations apply` — D1 has no constructor to hang DDL off, and
// creating tables per request would cost a round trip on every call.
export class D1SpatialIndex {
  constructor(db) {
    this.db = db;
  }

  async load(featureCollection) {
    const features = featureCollection?.features ?? [];
    const statements = [];

    for (const [i, f] of features.entries()) {
      const row = prepareFeature(f, i);
      statements.push(
        this.db.prepare("DELETE FROM feature_cells WHERE id = ?").bind(row.id),
        this.db
          .prepare(
            `INSERT OR REPLACE INTO features (id, geom, props, minx, miny, maxx, maxy)
             VALUES (?, ?, ?, ?, ?, ?, ?)`,
          )
          .bind(row.id, row.geom, row.props, row.bbox.minx, row.bbox.miny, row.bbox.maxx, row.bbox.maxy),
        ...row.cells.map((cell) =>
          this.db.prepare("INSERT INTO feature_cells (cell, id) VALUES (?, ?)").bind(cell, row.id),
        ),
      );
    }

    if (statements.length) await this.db.batch(statements);
    return { inserted: features.length };
  }

  async query(request) {
    const p = plan(request);
    const { results } = await this.db
      .prepare(p.sql)
      .bind(...p.params)
      .all();
    // D1 returns a BLOB as number[]; kenro wants the bytes.
    return refine(
      results.map((r) => ({ ...r, geom: new Uint8Array(r.geom) })),
      p,
    );
  }

  async stats() {
    const [features, cells, oversized] = await this.db.batch([
      this.db.prepare("SELECT count(*) AS n FROM features"),
      this.db.prepare("SELECT count(*) AS n FROM feature_cells"),
      this.db.prepare("SELECT count(*) AS n FROM feature_cells WHERE cell = ?").bind(OVERSIZED),
    ]);
    return {
      features: features.results[0].n,
      cells: cells.results[0].n,
      oversized: oversized.results[0].n,
    };
  }

  async clear() {
    await this.db.batch([
      this.db.prepare("DELETE FROM feature_cells"),
      this.db.prepare("DELETE FROM features"),
    ]);
    return { ok: true };
  }
}
