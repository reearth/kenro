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

## Without a SQLite host

The exports work standalone too, for hosts whose SQLite takes no UDFs at all
(Cloudflare D1, Durable Objects) — SQL filters on columns kenro computed at
write time, kenro runs the exact predicate in JS:

- **`Prepared`** — decode a geometry once, then chain predicates, GeoJSON/WKT
  output and reprojection off that one handle instead of re-decoding a blob
  per call (16–41% over a refine loop, depending on how much is chained). A
  handle must be freed: `using` does it where available, and
  **`kenro-wasm/prepared`** (`freeOnce` / `withPrepared` / `withScope`) does
  it everywhere else
- **`kenro-wasm/tiles`** — bounding box → Web Mercator tile ids, the
  B-tree-indexable stand-in for the R-tree that sql.js and D1/DO SQLite lack

Every subpath ships TypeScript types, checked in CI against the exports map.

Both are documented in [docs/wasm.md](../../docs/wasm.md#without-sqlite-prepared-and-kenro-wasmtiles).

## Demo

`demo/` is a static drag-a-GeoPackage-and-query page:

```sh
wasm-pack build crates/kenro-wasm --target web --release --out-dir js/pkg -- --features full
crates/kenro-wasm/demo/serve.sh   # serves http://localhost:8000
```

## Cloudflare

`cloudflare/` is a Worker that gives D1 and Durable Object SQLite a spatial
index and PostGIS-style predicates — SQL does the indexed coarse filter,
kenro does the exact predicate. See
[cloudflare/README.md](cloudflare/README.md).

## Tests

```sh
cd crates/kenro-wasm/js && npm ci && npm test
```

Tier 1 replays all committed golden vectors (PostGIS / H3 reference
library) against the raw wasm exports; Tier 2 runs every registered
function through SQL on each of the three hosts. `npm test` then runs
`tsc` over `type-tests/api.ts`, which imports every subpath through the
package's own exports map — the hand-written `.d.ts` files are the public
API, and nothing else would notice them drifting from `src/*.mjs`.
