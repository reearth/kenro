import type { Bbox } from "./tiles.js";

export type { Bbox };

export interface QuadOptions {
  /**
   * Most cells a cover may use. More cells track the box more tightly and make
   * the SQL longer. Default 1 for a feature, 16 for a query.
   */
  maxCells?: number;
  /**
   * Deepest level a cover may descend to, 0..24. Only caps how fine the cover
   * gets — cell ids are always encoded at `CELL_DEPTH`, so this never has to
   * match between the write side and the query side.
   */
  maxDepth?: number;
}

export interface CellFilterOptions extends QuadOptions {
  /** Table holding `(cell, id)` rows. Default `"feature_cells"`. */
  table?: string;
  /** Cell id column. Default `"cell"`. */
  cell?: string;
  /** Feature id column. Default `"id"`. */
  id?: string;
}

/** The candidate cells for a query window. Never null — see `cellsForQuery`. */
export interface CellQuery {
  /** Coarser cells containing the window: equality lookups. */
  ancestors: number[];
  /** Inclusive `[lo, hi]` id ranges, each one contiguous in the B-tree. */
  ranges: [number, number][];
  /** True when the window covers the world and the filter is pointless. */
  wholeTable: boolean;
}

/** The depth cell ids are encoded at. A constant, not an option. */
export const CELL_DEPTH: 24;

/** Cells one feature is filed under by default. */
export const DEFAULT_FEATURE_MAX_CELLS: 1;

/** Cells a query window is covered with by default. */
export const DEFAULT_QUERY_MAX_CELLS: 16;

/**
 * Cell ids covering `bbox`, at most `maxCells` of them, as deep as that
 * budget allows. Never null: a box too large to cover finely just lands in a
 * shallower cell.
 */
export function quadCover(bbox: Bbox, options?: QuadOptions): number[];

/** The cells to store a feature under. One cell — so one row — by default. */
export function cellsForFeature(bbox: Bbox, options?: QuadOptions): number[];

/**
 * The cells a matching feature may be filed under: coarser cells containing
 * the window (`ancestors`), plus the id `ranges` holding each cover cell and
 * everything below it.
 *
 * Complete for any combination of options on the two sides — there is no
 * setting here that has to be kept in sync.
 */
export function cellsForQuery(bbox: Bbox, options?: QuadOptions): CellQuery;

/**
 * The coarse filter as SQL selecting feature ids, with every value bound:
 *
 *     const { sql, params } = cellFilterSql(bbox);
 *     db.prepare(`SELECT * FROM features WHERE id IN (${sql})`).bind(...params);
 */
export function cellFilterSql(
  bbox: Bbox,
  options?: CellFilterOptions,
): { sql: string; params: number[]; wholeTable: boolean };

/**
 * The depth a cell id sits at: 0 is the whole world, `CELL_DEPTH` the finest.
 * The shallowest cell in a table is the feature that answers the most queries.
 */
export function cellDepth(cell: number): number;

/** True if two bounding boxes overlap, edges included. */
export function bboxOverlaps(a: Bbox, b: Bbox): boolean;

/** Grow a bounding box by `d` on every side (the ST_DWithin search area). */
export function padBbox(bbox: Bbox, d: number): Bbox;
