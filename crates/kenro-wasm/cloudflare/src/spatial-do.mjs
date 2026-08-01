// The plan from spatial.mjs, on Durable Object SQLite.
//
// A Durable Object is the better host for it than D1: `sql.exec` is
// synchronous and the wasm lives in the same isolate, so refining a candidate
// is a function call rather than a network round trip. One DO per
// region/tile/tenant shards naturally — each holds its own SQLite and its own
// copy of the wasm, and they run in parallel with no coordination.

import { DurableObject } from "cloudflare:workers";

import { OVERSIZED } from "../../js/src/tiles.mjs";
import { SCHEMA, plan, prepareFeature, refine } from "./spatial.mjs";

export class SpatialIndex extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    for (const stmt of SCHEMA) this.sql.exec(stmt);
  }

  /** Insert or replace features from a GeoJSON FeatureCollection. */
  load(featureCollection) {
    const features = featureCollection?.features ?? [];
    let count = 0;

    this.ctx.storage.transactionSync(() => {
      for (const [i, f] of features.entries()) {
        const row = prepareFeature(f, i);
        this.sql.exec("DELETE FROM feature_cells WHERE id = ?", row.id);
        this.sql.exec(
          `INSERT OR REPLACE INTO features (id, geom, props, minx, miny, maxx, maxy)
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
          row.id,
          row.geom,
          row.props,
          row.bbox.minx,
          row.bbox.miny,
          row.bbox.maxx,
          row.bbox.maxy,
        );
        for (const cell of row.cells) {
          this.sql.exec("INSERT INTO feature_cells (cell, id) VALUES (?, ?)", cell, row.id);
        }
        count++;
      }
    });

    return { inserted: count };
  }

  /** Window query: `{ wkt, predicate, distance, srid, limit }`. */
  query(request) {
    const p = plan(request);
    const rows = this.sql.exec(p.sql, ...p.params).toArray();
    // DO SQLite hands back an ArrayBuffer for a BLOB column.
    return refine(
      rows.map((r) => ({ ...r, geom: new Uint8Array(r.geom) })),
      p,
    );
  }

  stats() {
    const one = (sql) => this.sql.exec(sql).one().n;
    return {
      features: one("SELECT count(*) AS n FROM features"),
      cells: one("SELECT count(*) AS n FROM feature_cells"),
      oversized: one(`SELECT count(*) AS n FROM feature_cells WHERE cell = ${OVERSIZED}`),
    };
  }

  clear() {
    this.sql.exec("DELETE FROM feature_cells");
    this.sql.exec("DELETE FROM features");
    return { ok: true };
  }
}
