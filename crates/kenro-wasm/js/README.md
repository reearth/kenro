# kenro-wasm

**SpatiaLite-style spatial SQL for SQLite in JavaScript** —
PostGIS-compatible `ST_` functions registered as JS-level UDFs on any wasm
SQLite host: the official [@sqlite.org/sqlite-wasm] build, [sql.js], and
[wa-sqlite]. On hosts whose SQLite takes no UDFs at all — **Cloudflare D1
and Durable Objects** — the same functions run standalone, from JS.

kenro is a spatial SQL engine in pure Rust (~205 functions: the DE-9IM
predicate family, overlay/repair/buffer, CRS transform, H3, GeoJSON, MVT
vector tiles, spatial aggregates), golden-tested against PostGIS. This
package is its wasm build plus one small adapter per host. The bundled
wasm is the **full** feature tier (~640 KB gzipped, the EPSG registry
included); smaller tiers are
attached to the [GitHub Releases].

**Live demo:** <https://reearth.github.io/kenro/> — drag a GeoPackage in
and query it, entirely client-side.

## Usage

```js
import sqlite3InitModule from "@sqlite.org/sqlite-wasm";
import initKenro, * as kenroWasm from "kenro-wasm";
import { registerKenro } from "kenro-wasm/sqlite-wasm";

await initKenro();
const sqlite3 = await sqlite3InitModule();
const db = new sqlite3.oo1.DB(":memory:");
registerKenro(db, kenroWasm, sqlite3); // sqlite3 namespace needed for aggregates

db.selectValue("SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))"); // POINT(1 2)
```

```js
// sql.js
import { registerKenro } from "kenro-wasm/sqljs";
registerKenro(db, kenroWasm);

// wa-sqlite
import { registerKenro } from "kenro-wasm/wa-sqlite";
registerKenro(sqlite3, db, kenroWasm);
```

Host support matrix, per-host limitations (sql.js: no int64 → the `h3_*`
functions error loudly; no R-tree module) and measured sizes:
[docs/wasm.md]. The full function table with PostGIS/DuckDB comparison:
[docs/functions.md].

## Cloudflare Workers, D1 and Durable Objects

Neither D1 nor Durable Object SQLite supports user-defined functions or an
R-tree, so `ST_Intersects(...)` can never appear in their SQL. The split
that does work: kenro derives a bounding box and tile cells at *write* time
for plain SQL to index, then runs the exact predicate in JS on the
survivors.

```js
import wasmModule from "kenro-wasm/pkg/kenro_wasm_bg.wasm";  // Workers hand you the Module
import * as kenro from "kenro-wasm";
import { cellFilterSql } from "kenro-wasm/quadtree";

kenro.initSync({ module: wasmModule });                      // once per isolate

using win = kenro.Prepared.fromText(wkt, 4326);              // decode once per scan
const hits = rows.filter((r) => {
  using g = kenro.Prepared.fromBlob(r.geom);
  return g.stIntersects(win);
});
```

Four subpaths carry this, all typed:

| | |
|---|---|
| `kenro-wasm` → `Prepared` | a geometry decoded once, then predicates, GeoJSON/WKT output and reprojection chained off it |
| `kenro-wasm/quadtree` | bounding box → variable-depth quadtree cell ids: the B-tree-indexable stand-in for the missing R-tree (sql.js lacks one too), with nothing to tune |
| `kenro-wasm/tiles` | the same idea at one fixed zoom — simpler, and faster for windows near that zoom |
| `kenro-wasm/prepared` | handle lifetimes where `using` isn't available |

A complete Worker on both backends, with tests that run in workerd:
[the Cloudflare example]. API reference: [docs/wasm.md].

## License

MIT OR Apache-2.0, at your option.

[@sqlite.org/sqlite-wasm]: https://www.npmjs.com/package/@sqlite.org/sqlite-wasm
[sql.js]: https://github.com/sql-js/sql.js
[wa-sqlite]: https://github.com/rhashimoto/wa-sqlite
[GitHub Releases]: https://github.com/reearth/kenro/releases
[docs/wasm.md]: https://github.com/reearth/kenro/blob/main/docs/wasm.md
[docs/functions.md]: https://github.com/reearth/kenro/blob/main/docs/functions.md
[the Cloudflare example]: https://github.com/reearth/kenro/blob/main/crates/kenro-wasm/cloudflare/README.md
