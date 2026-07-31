# kenro-ext

[kenro](../../README.md) — spatial functions for SQLite in pure Rust — packaged
as a standard **SQLite loadable extension** (`.so` / `.dylib` / `.dll`) for
Python, Node, the sqlite3 CLI, and anything else that can call
`load_extension`.

## Build

Any OS; no C toolchain or SQLite dev files needed:

```sh
cargo build -p kenro-ext --release
# → target/release/libkenro_ext.so (Linux)
#   target/release/libkenro_ext.dylib (macOS)
#   target/release/kenro_ext.dll (Windows)
```

Requirements: host SQLite ≥ 3.34 (older hosts fail with a clear
version-mismatch message). Loading is per-connection.

## Load

```python
# Python (stdlib sqlite3)
import sqlite3
con = sqlite3.connect("parks.gpkg")
con.enable_load_extension(True)
con.load_extension("./target/release/libkenro_ext")  # suffix optional
con.enable_load_extension(False)
```

```js
// Node (better-sqlite3)
const db = new (require("better-sqlite3"))("parks.gpkg");
db.loadExtension("./target/release/libkenro_ext");

// Node (built-in node:sqlite)
const { DatabaseSync } = require("node:sqlite");
const db2 = new DatabaseSync("parks.gpkg", { allowExtension: true });
db2.enableLoadExtension(true);
db2.loadExtension("./target/release/libkenro_ext");
```

```
-- sqlite3 CLI (macOS: use Homebrew sqlite; the system binary can't load extensions)
sqlite> .load ./target/release/libkenro_ext
sqlite> SELECT ST_AsGeoJSON(ST_Transform(ST_GeomFromGPB(geom), 4326)) FROM parks LIMIT 1;
```

## Entry points

Three symbols are exported, so any filename loads with a NULL entry-point
name — including renamed copies like `libkenro.so`:

- `sqlite3_extension_init` (tried first, covers every filename)
- `sqlite3_kenroext_init` (`libkenro_ext.so` / `kenro_ext.dll`)
- `sqlite3_kenro_init` (`libkenro.so` / `kenro.dll`)

## Functions & semantics

See the [function reference in the root README](../../README.md#function-reference).
Cargo features mirror kenro's: `transform`, `h3`, `geojson` (all default-on),
plus `crs-full`.
