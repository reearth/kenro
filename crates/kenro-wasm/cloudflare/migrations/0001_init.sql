-- D1 schema. Apply with `wrangler d1 migrations apply kenro-spatial`.
--
-- Deliberately the same tables the Durable Object creates in code (SCHEMA in
-- src/spatial.mjs) — test/schema.test.mjs fails if the two drift apart. A DO
-- can run its DDL in the constructor; D1 has no such hook, so its copy lives
-- here.
--
-- Nothing here is spatial: kenro computes the bbox and the tile cells at
-- write time, leaving SQL plain REALs and INTEGERs it can index normally.
-- Neither D1 nor DO SQLite has an R-tree module or user-defined functions.
CREATE TABLE IF NOT EXISTS features (
  id    TEXT PRIMARY KEY,
  geom  BLOB NOT NULL,          -- GeoPackage blob (GPB): header envelope makes ST_MinX free
  props TEXT NOT NULL,          -- JSON
  minx  REAL NOT NULL, miny REAL NOT NULL,
  maxx  REAL NOT NULL, maxy REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS feature_cells (
  cell INTEGER NOT NULL,
  id   TEXT NOT NULL,
  PRIMARY KEY (cell, id)
) WITHOUT ROWID;
