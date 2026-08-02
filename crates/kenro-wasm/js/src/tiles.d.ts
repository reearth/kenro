/** A WGS84 bounding box in degrees. */
export interface Bbox {
  minx: number;
  miny: number;
  maxx: number;
  maxy: number;
}

export interface TileOptions {
  /** Web Mercator zoom the cells are taken at. Default 8. */
  zoom?: number;
  /** Above this many cells a cover counts as too large. Default 64. */
  maxCells?: number;
}

export const DEFAULT_ZOOM: 8;
export const DEFAULT_MAX_CELLS: 64;

/** The cell a feature too large to enumerate is filed under. */
export const OVERSIZED: -1;

/**
 * Tile ids covering `bbox` (`y * 2**zoom + x`), or `null` when the cover
 * would exceed `maxCells`.
 *
 * `null` means "too big to enumerate" and the two sides of the index read it
 * differently — prefer `cellsForFeature` / `cellsForQuery`, which encode that.
 */
export function tileCover(bbox: Bbox, options?: TileOptions): number[] | null;

/** Cells to store a feature under: its cover, or `[OVERSIZED]`. */
export function cellsForFeature(bbox: Bbox, options?: TileOptions): number[];

/**
 * Cells to search for a query window — its cover plus `OVERSIZED` — or
 * `null` meaning "too large: drop the cell filter and scan the table".
 */
export function cellsForQuery(bbox: Bbox, options?: TileOptions): number[] | null;

/** True if two bounding boxes overlap, edges included. */
export function bboxOverlaps(a: Bbox, b: Bbox): boolean;

/** Grow a bounding box by `d` on every side (the ST_DWithin search area). */
export function padBbox(bbox: Bbox, d: number): Bbox;
