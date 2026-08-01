// A spatial store on Durable Object SQLite.
//
// Neither D1 nor DO SQLite supports user-defined functions, so no `ST_` call
// can appear in the SQL itself. The split instead is:
//
//   SQL   — indexed coarse filter (tile cells, then bounding boxes)
//   kenro — exact predicates, measures and output, in JS on the survivors
//
// A Durable Object is the better half of that split: `sql.exec` is
// synchronous and the wasm lives in the same isolate, so the refine loop is
// a function call per candidate rather than a network round trip. The same
// code shape works against D1 — see README.md.

import { DurableObject } from "cloudflare:workers";

import { kenro } from "./kenro.mjs";
import { OVERSIZED, bboxOverlaps, cellsForBbox } from "./tiles.mjs";

const SCHEMA = `
CREATE TABLE IF NOT EXISTS features (
  id    TEXT PRIMARY KEY,
  geom  BLOB NOT NULL,          -- kenro's internal geometry blob
  props TEXT NOT NULL,          -- JSON
  minx  REAL NOT NULL, miny REAL NOT NULL,
  maxx  REAL NOT NULL, maxy REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS feature_cells (
  cell INTEGER NOT NULL,
  id   TEXT NOT NULL,
  PRIMARY KEY (cell, id)
) WITHOUT ROWID;
`;

/** Exact predicates available to a query, by name. */
function predicateFn(wasm, name, distance) {
  switch (name) {
    case "intersects":
      return (a, b) => wasm.stIntersects(a, b);
    case "within":
      return (a, b) => wasm.stWithin(a, b);
    case "contains":
      return (a, b) => wasm.stContains(a, b);
    case "dwithin":
      if (typeof distance !== "number" || !Number.isFinite(distance)) {
        throw new Error("dwithin needs a numeric `distance`");
      }
      return (a, b) => wasm.stDwithin(a, b, distance);
    default:
      throw new Error(`unknown predicate: ${name}`);
  }
}

export class SpatialIndex extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.sql.exec(SCHEMA);
  }

  /**
   * Insert or replace features. Accepts a GeoJSON FeatureCollection; the
   * geometry is parsed once, here, and everything the index needs — the
   * bounding box, the tile cover — is derived at write time. That is the
   * whole trick: the columns SQL can index are computed by kenro before the
   * row ever reaches SQLite.
   */
  load(featureCollection) {
    const wasm = kenro();
    const features = featureCollection?.features ?? [];
    let count = 0;

    this.ctx.storage.transactionSync(() => {
      for (const [i, f] of features.entries()) {
        const id = String(f.id ?? `f${i}`);
        const geom = wasm.stGeomFromGeojson(JSON.stringify(f.geometry));
        const bbox = {
          minx: wasm.stMinX(geom),
          miny: wasm.stMinY(geom),
          maxx: wasm.stMaxX(geom),
          maxy: wasm.stMaxY(geom),
        };
        if (Object.values(bbox).some((v) => v === undefined)) {
          throw new Error(`feature ${id}: empty geometry has no bounding box`);
        }

        this.sql.exec("DELETE FROM feature_cells WHERE id = ?", id);
        this.sql.exec(
          `INSERT OR REPLACE INTO features (id, geom, props, minx, miny, maxx, maxy)
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
          id,
          geom,
          JSON.stringify(f.properties ?? {}),
          bbox.minx,
          bbox.miny,
          bbox.maxx,
          bbox.maxy,
        );
        for (const cell of cellsForBbox(bbox)) {
          this.sql.exec("INSERT INTO feature_cells (cell, id) VALUES (?, ?)", cell, id);
        }
        count++;
      }
    });

    return { inserted: count };
  }

  /**
   * Window query: `{ wkt, predicate, distance, srid, limit }`.
   *
   * Three stages, cheapest first — tile cells and bounding boxes in SQL,
   * the DE-9IM predicate in kenro. Only the last stage is exact, and only it
   * touches full geometry.
   */
  query({ wkt, predicate = "intersects", distance, srid, limit = 1000 }) {
    const wasm = kenro();
    const window = wasm.stGeomFromText(wkt);
    const windowBbox = {
      minx: wasm.stMinX(window),
      miny: wasm.stMinY(window),
      maxx: wasm.stMaxX(window),
      maxy: wasm.stMaxY(window),
    };
    // dwithin reaches beyond the window's own extent; grow the coarse filter
    // by the search distance so the cheap stages cannot drop a true hit.
    const pad = predicate === "dwithin" ? (distance ?? 0) : 0;
    const search = {
      minx: windowBbox.minx - pad,
      miny: windowBbox.miny - pad,
      maxx: windowBbox.maxx + pad,
      maxy: windowBbox.maxy + pad,
    };
    const test = predicateFn(wasm, predicate, distance);

    const cells = cellsForBbox(search);
    const placeholders = cells.map(() => "?").join(", ");
    // OVERSIZED rows are the features whose cover was too large to enumerate;
    // they are always candidates. Dropping them would silently lose hits.
    const candidates = this.sql
      .exec(
        `SELECT f.id, f.geom, f.props, f.minx, f.miny, f.maxx, f.maxy
           FROM features f
          WHERE f.id IN (
            SELECT id FROM feature_cells WHERE cell IN (${placeholders}) OR cell = ?
          )`,
        ...cells,
        OVERSIZED,
      )
      .toArray();

    const features = [];
    let refined = 0;
    for (const row of candidates) {
      if (!bboxOverlaps(search, row)) continue; // cheap reject before wasm
      refined++;
      const geom = new Uint8Array(row.geom);
      if (!test(geom, window)) continue;
      const out = srid ? wasm.stTransform(geom, srid) : geom;
      features.push({
        type: "Feature",
        id: row.id,
        geometry: JSON.parse(wasm.stAsGeojson(out)),
        properties: JSON.parse(row.props),
      });
      if (features.length >= limit) break;
    }

    return {
      type: "FeatureCollection",
      features,
      // Deliberately visible: the point of the design is that `refined` stays
      // small relative to the table. If it does not, the tiling is wrong.
      stats: { candidates: candidates.length, refined, matched: features.length },
    };
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
