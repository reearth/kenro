# kenro-wasm

**SpatiaLite-style spatial SQL for SQLite in the browser** —
PostGIS-compatible `ST_` functions registered as JS-level UDFs on any wasm
SQLite host: the official [@sqlite.org/sqlite-wasm] build, [sql.js], and
[wa-sqlite].

kenro is a full spatial SQL engine in pure Rust (~80 functions: the DE-9IM
predicate family, overlay/repair/buffer, CRS transform, H3, GeoJSON, MVT
vector tiles, spatial aggregates), golden-tested against PostGIS. This
package is its wasm build plus one small adapter per host. The bundled
wasm is the **full** feature tier (~353 KB gzipped); smaller tiers are
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

## License

MIT OR Apache-2.0, at your option.

[@sqlite.org/sqlite-wasm]: https://www.npmjs.com/package/@sqlite.org/sqlite-wasm
[sql.js]: https://github.com/sql-js/sql.js
[wa-sqlite]: https://github.com/rhashimoto/wa-sqlite
[GitHub Releases]: https://github.com/reearth/kenro/releases
[docs/wasm.md]: https://github.com/reearth/kenro/blob/main/docs/wasm.md
[docs/functions.md]: https://github.com/reearth/kenro/blob/main/docs/functions.md
