# Platform quickstarts

kenro reaches your SQLite connection through one of three delivery forms.
Pick the row that matches your platform, then jump to its section:

| Platform | Delivery | Section |
|---|---|---|
| Rust | `kenro` crate, `rusqlite` feature | [Rust](#rust-rusqlite) |
| Python | loadable extension | [Python](#python) |
| Node.js | loadable extension | [Node.js](#nodejs) |
| Bun | loadable extension | [Bun](#bun) |
| Deno | loadable extension | [Deno](#deno) |
| Go | loadable extension | [Go](#go) |
| Ruby | loadable extension | [Ruby](#ruby) |
| C / C++ | loadable extension | [C / C++](#c--c) |
| `sqlite3` CLI | loadable extension | [sqlite3 CLI](#sqlite3-cli) |
| Docker / Cloud Run / Fly.io / ECS | loadable extension in a container | [Containers](#containers-cloud-run-flyio-ecs-) |
| AWS Lambda | loadable extension (layer or container) | [AWS Lambda](#aws-lambda) |
| Cloudflare Workers | kenro-wasm | [Cloudflare Workers](#cloudflare-workers) |
| Browser | kenro-wasm | [docs/wasm.md](wasm.md) |

Everything below assumes the extension binary exists — grab it first:

## Getting the loadable extension

Prebuilt binaries are attached to every [GitHub Release]. Download the
one matching your OS/architecture (the `latest` URLs below always point
at the newest release):

| OS / arch | asset |
|---|---|
| Linux x86_64 (glibc) | `kenro-ext-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 (glibc) | `kenro-ext-aarch64-unknown-linux-gnu.tar.gz` |
| macOS (universal: Apple Silicon + Intel) | `kenro-ext-macos-universal.tar.gz` |
| Windows x86_64 | `kenro-ext-x86_64-pc-windows-msvc.zip` |

```sh
curl -fsSL -o kenro-ext.tar.gz \
  https://github.com/reearth/kenro/releases/latest/download/kenro-ext-x86_64-unknown-linux-gnu.tar.gz
tar xzf kenro-ext.tar.gz   # → libkenro_ext.so (+ licenses)
```

`SHA256SUMS` on the release page verifies the download. Each archive
contains the bare library — put it wherever your app can read it. The
examples below assume it sits in the working directory (`./libkenro_ext.so`
/ `.dylib` / `kenro_ext.dll`).

Building from source works on any machine with Rust, no C toolchain or
SQLite development files needed:

```sh
cargo build -p kenro-ext --release   # → target/release/libkenro_ext.*
```

Notes that apply everywhere:

- The **host SQLite must be ≥ 3.34**; on older hosts the load fails with a
  clear version-mismatch message.
- Loading is **per-connection**: every new connection must load the
  extension again (most drivers have a hook for this).
- Renamed copies load fine (e.g. `libkenro.so`): the binary exports
  `sqlite3_extension_init`, `sqlite3_kenroext_init` and
  `sqlite3_kenro_init`.
- The prebuilt extension ships the **full** function set including
  overlay/buffer/MVT (`kenro-ext`'s default features).

## Rust (rusqlite)

No extension binary needed — kenro registers directly on the connection:

```toml
[dependencies]
kenro = { version = "0.1", features = ["rusqlite"] }
# add "full" for overlay/buffer/MVT: features = ["rusqlite", "full"]
```

```rust
let conn = rusqlite::Connection::open("parks.gpkg")?;
kenro::register(&conn)?;
let wkt: String = conn.query_row(
    "SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1",
    [], |r| r.get(0),
)?;
```

## Python

The stdlib `sqlite3` module loads extensions directly:

```python
import sqlite3

con = sqlite3.connect("parks.gpkg")
con.enable_load_extension(True)
con.load_extension("./libkenro_ext")  # suffix optional
con.enable_load_extension(False)

print(con.execute(
    "SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1").fetchone())
```

macOS note: python.org installers ship a `sqlite3` module without
`enable_load_extension`; Homebrew Python has it
(`hasattr(con, "enable_load_extension")` tells you which you have).

## Node.js

```js
// better-sqlite3
const Database = require("better-sqlite3");
const db = new Database("parks.gpkg");
db.loadExtension("./libkenro_ext");

// or the built-in node:sqlite (Node ≥ 22.5)
const { DatabaseSync } = require("node:sqlite");
const db2 = new DatabaseSync("parks.gpkg", { allowExtension: true });
db2.enableLoadExtension(true);
db2.loadExtension("./libkenro_ext");
```

## Bun

```js
import { Database } from "bun:sqlite";

// macOS only: Apple's bundled SQLite disables extension loading — point
// Bun at a Homebrew build BEFORE opening any database:
// Database.setCustomSQLite("/opt/homebrew/opt/sqlite/lib/libsqlite3.dylib");

const db = new Database("parks.gpkg");
db.loadExtension("./libkenro_ext");
```

## Deno

With the FFI-based [`@db/sqlite`](https://jsr.io/@db/sqlite) driver
(requires `--allow-ffi`):

```ts
import { Database } from "jsr:@db/sqlite";

const db = new Database("parks.gpkg", { enableLoadExtension: true });
db.loadExtension("./libkenro_ext");
```

## Go

[mattn/go-sqlite3] (CGO) loads extensions per-driver:

```go
import (
    "database/sql"
    sqlite3 "github.com/mattn/go-sqlite3"
)

func init() {
    sql.Register("sqlite3_kenro", &sqlite3.SQLiteDriver{
        Extensions: []string{"./libkenro_ext"},
    })
}

func main() {
    db, err := sql.Open("sqlite3_kenro", "parks.gpkg")
    // ...
    var wkt string
    err = db.QueryRow(
        "SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1",
    ).Scan(&wkt)
}
```

The pure-Go `modernc.org/sqlite` driver cannot load native extensions —
use mattn/go-sqlite3 (or run kenro-ext-less and do geometry work outside
SQL).

## Ruby

The `sqlite3` gem:

```ruby
require "sqlite3"

db = SQLite3::Database.new("parks.gpkg")
db.enable_load_extension(true)
db.load_extension("./libkenro_ext")
db.enable_load_extension(false)

puts db.get_first_value(
  "SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1")
```

## C / C++

```c
sqlite3 *db;
sqlite3_open("parks.gpkg", &db);
sqlite3_enable_load_extension(db, 1);
char *err = NULL;
sqlite3_load_extension(db, "./libkenro_ext", NULL, &err);
```

## sqlite3 CLI

```
$ sqlite3 parks.gpkg
sqlite> .load ./libkenro_ext
sqlite> SELECT ST_AsGeoJSON(ST_Transform(ST_GeomFromGPB(geom), 4326)) FROM parks LIMIT 1;
```

macOS: the system `/usr/bin/sqlite3` is compiled without extension loading —
`brew install sqlite` and run `$(brew --prefix sqlite)/bin/sqlite3`.

## Containers (Cloud Run, Fly.io, ECS, …)

Any platform that runs a container runs kenro: fetch the release binary
into the image, load it like on any Linux host. A Node example (the
pattern is identical for Python/Go/Ruby images):

```dockerfile
FROM node:22-slim
WORKDIR /app
ADD https://github.com/reearth/kenro/releases/latest/download/kenro-ext-x86_64-unknown-linux-gnu.tar.gz /tmp/kenro.tar.gz
RUN tar xzf /tmp/kenro.tar.gz -C /app libkenro_ext.so && rm /tmp/kenro.tar.gz
COPY . .
RUN npm ci --omit=dev
CMD ["node", "server.js"]   # server.js: db.loadExtension("./libkenro_ext.so")
```

Match the asset to the image architecture (`aarch64-unknown-linux-gnu`
for arm64 images). Pin a version by replacing `latest/download/…` with
`download/vX.Y.Z/…`. If you prefer hermetic builds, a `rust:1` build
stage running `cargo build -p kenro-ext --release` works too.

## AWS Lambda

Two routes:

- **Container image** (simplest): exactly the Dockerfile pattern above on
  an AWS base image.
- **Zip + layer**: put the release `libkenro_ext.so`
  (`x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu`, matching
  the function architecture) into a layer under `lib/`, and load it from
  `/opt`:

```python
import sqlite3

def handler(event, context):
    con = sqlite3.connect("/tmp/parks.gpkg")  # e.g. downloaded from S3
    con.enable_load_extension(True)
    con.load_extension("/opt/lib/libkenro_ext.so")
    con.enable_load_extension(False)
    return con.execute("SELECT count(*) FROM parks").fetchone()[0]
```

(The Lambda-managed Python runtimes ship a `sqlite3` module with extension
loading enabled.)

## Cloudflare Workers

Workers cannot load native extensions, and neither **D1** nor **Durable
Object SQLite** supports user-defined functions (the supported extension set
is FTS5, JSON and math — no R-tree either) — kenro cannot run *inside* their
queries. Three patterns that do work:

- **Process geometry in the Worker** with kenro-wasm's exports directly:
  store GeoPackage blobs in D1 columns, `SELECT` them out, then call
  `stAsText` / `stIntersects` / `stTransform` … on the values in JS. The
  wasm module is well inside Worker size limits (standard tier
  617 KB / 251 KB gzip, minimal 412 KB / 167 KB).
- **Index in SQL, refine in kenro** — the scalable version of the above:
  derive the bounding box and a tile cell with kenro at write time, let SQL
  filter on those with a plain B-tree index, and run the exact predicate in
  JS on the survivors. A complete Worker + Durable Object doing this, with
  tests that run in workerd, lives in
  [`crates/kenro-wasm/cloudflare/`](../crates/kenro-wasm/cloudflare/README.md).
- **Run a full SQLite inside the Worker** with [sql.js] or [wa-sqlite]
  over bytes fetched from R2/KV (read-only analytics on a shipped
  `.gpkg`/`.sqlite`), and `registerKenro` as usual — the same adapters
  used in the browser. See [docs/wasm.md](wasm.md) for the per-host
  matrix (note: sql.js has no R-tree module and no int64/h3).

## Browser

See [docs/wasm.md](wasm.md) — adapters for the official SQLite WASM build,
sql.js and wa-sqlite, plus a drag-a-GeoPackage demo in
`crates/kenro-wasm/demo/`.

[GitHub Release]: https://github.com/reearth/kenro/releases
[mattn/go-sqlite3]: https://github.com/mattn/go-sqlite3
[sql.js]: https://github.com/sql-js/sql.js
[wa-sqlite]: https://github.com/rhashimoto/wa-sqlite
