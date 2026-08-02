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

import { cellDepth } from "../../js/src/quadtree.mjs";
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
    // `cell & -cell` isolates the sentinel bit, which is where the depth is
    // recorded — so the shallowest cell in the table is a plain SQL max().
    const [features, cells, shallowest] = await this.db.batch([
      this.db.prepare("SELECT count(*) AS n FROM features"),
      this.db.prepare("SELECT count(*) AS n FROM feature_cells"),
      this.db.prepare("SELECT max(cell & -cell) AS lsb FROM feature_cells"),
    ]);
    const lsb = shallowest.results[0].lsb;
    return {
      features: features.results[0].n,
      cells: cells.results[0].n,
      // The broadest feature in the table: the one that stays a candidate for
      // the widest range of queries. Low means something is filed very coarsely.
      shallowestDepth: lsb === null ? null : cellDepth(lsb),
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
