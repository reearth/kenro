// Compile-only: every subpath is imported through the package's own exports
// map (self-reference), so this fails if a `types` condition is missing, a
// .d.ts is malformed, or a signature stops matching how the code is used.
// Nothing here runs — `tsc --noEmit` is the assertion.
import * as kenro from "kenro-wasm";
import { loadManifest, makeAggregate, makeUdf, stubUdf } from "kenro-wasm/core";
import type { AggregateEntry, FunctionEntry, SqlValue, StubEntry } from "kenro-wasm/core";
import { freeOnce, withPrepared, withScope } from "kenro-wasm/prepared";
import { registerKenro as registerSqliteWasm } from "kenro-wasm/sqlite-wasm";
import { registerKenro as registerSqljs } from "kenro-wasm/sqljs";
import {
  DEFAULT_ZOOM,
  OVERSIZED,
  bboxOverlaps,
  cellsForFeature,
  cellsForQuery,
  padBbox,
  tileCover,
} from "kenro-wasm/tiles";
import type { Bbox } from "kenro-wasm/tiles";
import {
  CELL_DEPTH,
  cellDepth,
  cellFilterSql,
  cellsForFeature as quadCellsForFeature,
  cellsForQuery as quadCellsForQuery,
  quadCover,
} from "kenro-wasm/quadtree";
import type { CellQuery } from "kenro-wasm/quadtree";
import { registerKenro as registerWaSqlite } from "kenro-wasm/wa-sqlite";

declare const db: object;
declare const sqlite3: object;

// Adapters — sqlite3 is optional only on the official-build adapter.
registerSqliteWasm(db, kenro, sqlite3);
registerSqliteWasm(db, kenro);
registerSqljs(db, kenro);
registerWaSqlite(sqlite3, 0, kenro);

// Manifest-driven registration, as an adapter would do it.
const manifest = loadManifest(kenro);
const fn: FunctionEntry = manifest.functions[0];
const agg: AggregateEntry = manifest.aggregates[0];
const stub: StubEntry = manifest.stubs[0];
const value: SqlValue = makeUdf(fn, kenro)("POINT(1 2)", 4326);
const driver = makeAggregate(agg, kenro);
const folded: SqlValue = driver.finish(driver.start());
const boom: (...args: SqlValue[]) => never = stubUdf(stub);

// Tiles — the null-returning pair must stay distinguishable.
const bbox: Bbox = { minx: 139, miny: 35, maxx: 140, maxy: 36 };
const stored: number[] = cellsForFeature(bbox, { zoom: DEFAULT_ZOOM });
const searched: number[] | null = cellsForQuery(padBbox(bbox, 0.5));
const raw: number[] | null = tileCover(bbox, { maxCells: 128 });
const overlaps: boolean = bboxOverlaps(bbox, bbox);
const sentinel: number = OVERSIZED;

// Quadtree — the variable-depth index. Nothing here is nullable, and the
// query side is a record rather than an array.
const filed: number[] = quadCellsForFeature(bbox);
const query: CellQuery = quadCellsForQuery(bbox, { maxCells: 32 });
const lo: number = query.ranges[0][0];
const covered: number[] = quadCover(bbox, { maxDepth: 12 });
const filter: { sql: string; params: number[] } = cellFilterSql(bbox, { table: "cells", maxParams: 60 });
const depth: number = CELL_DEPTH;
const d0: number = cellDepth(filed[0]);

// Handle lifetime.
const wkt: string = withScope((own) => {
  const g = own(kenro.Prepared.fromText("POINT(1 1)", 4326));
  return own(g.stTransform(3857)).stAsText();
});
const hit: boolean = withPrepared(kenro.Prepared.fromText("POINT(1 1)", 4326), (g) =>
  g.stIntersects(g),
);
freeOnce(null);

export { value, folded, boom, stored, searched, raw, overlaps, sentinel, wkt, hit };
export { filed, query, lo, covered, filter, depth, d0 };
