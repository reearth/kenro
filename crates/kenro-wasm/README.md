# kenro-wasm

[kenro](../../README.md) — spatial functions for SQLite in pure Rust — for
**browser and Node SQLite hosts**: the official SQLite WASM build, sql.js,
and wa-sqlite.

Browser SQLite builds cannot load native extensions, but they all expose
JS-level user-defined-function registration. kenro-wasm is kenro's
SQLite-free pure core compiled to `wasm32-unknown-unknown` (~950 KB,
~350 KB gzipped — no SQLite inside), plus one small manifest-driven adapter
per host in `js/src/`.

## Build

```sh
wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg -- --features full
```

## Use

```js
import sqlite3InitModule from "@sqlite.org/sqlite-wasm";
import initKenro, * as kenroWasm from "kenro-wasm";
import { registerKenro } from "kenro-wasm/sqlite-wasm";

await initKenro();
const sqlite3 = await sqlite3InitModule();
const db = new sqlite3.oo1.DB(":memory:");
registerKenro(db, kenroWasm, sqlite3); // sqlite3 namespace needed for aggregates
```

Adapters: `kenro-wasm/sqlite-wasm` (primary), `kenro-wasm/wa-sqlite`,
`kenro-wasm/sqljs`. Host support matrix, measured sizes, and per-host
limitations (sql.js: no int64 → h3 functions error loudly; no R-tree
module): [docs/wasm.md](../../docs/wasm.md).

## Demo

`demo/` is a static drag-a-GeoPackage-and-query page:

```sh
wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg -- --features full
crates/kenro-wasm/demo/serve.sh   # serves http://localhost:8000
```

## Tests

```sh
cd crates/kenro-wasm/js && npm ci && npm test
```

Tier 1 replays all committed golden vectors (PostGIS / H3 reference
library) against the raw wasm exports; Tier 2 runs every registered
function through SQL on each of the three hosts.
