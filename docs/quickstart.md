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

Everything below assumes the extension binary exists — build it once:

## Building the loadable extension

```sh
cargo build -p kenro-ext --release
# → target/release/libkenro_ext.so    (Linux)
#   target/release/libkenro_ext.dylib (macOS)
#   target/release/kenro_ext.dll      (Windows)
```

No C toolchain and no SQLite development files are needed — any machine
with Rust builds it. Notes that apply everywhere:

- The **host SQLite must be ≥ 3.34**; on older hosts the load fails with a
  clear version-mismatch message.
- Loading is **per-connection**: every new connection must load the
  extension again (most drivers have a hook for this).
- Renamed copies load fine (e.g. `libkenro.so`): the binary exports
  `sqlite3_extension_init`, `sqlite3_kenroext_init` and
  `sqlite3_kenro_init`.
- Cross-compiling (e.g. a Linux `.so` from macOS for Lambda/Cloud Run):
  add the Rust target and use [cargo-zigbuild] or a `rust` Docker image —
  `cargo zigbuild -p kenro-ext --release --target x86_64-unknown-linux-gnu`.
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
con.load_extension("./target/release/libkenro_ext")  # suffix optional
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
db.loadExtension("./target/release/libkenro_ext");

// or the built-in node:sqlite (Node ≥ 22.5)
const { DatabaseSync } = require("node:sqlite");
const db2 = new DatabaseSync("parks.gpkg", { allowExtension: true });
db2.enableLoadExtension(true);
db2.loadExtension("./target/release/libkenro_ext");
```

## Bun

```js
import { Database } from "bun:sqlite";

// macOS only: Apple's bundled SQLite disables extension loading — point
// Bun at a Homebrew build BEFORE opening any database:
// Database.setCustomSQLite("/opt/homebrew/opt/sqlite/lib/libsqlite3.dylib");

const db = new Database("parks.gpkg");
db.loadExtension("./target/release/libkenro_ext");
```

## Deno

With the FFI-based [`@db/sqlite`](https://jsr.io/@db/sqlite) driver
(requires `--allow-ffi`):

```ts
import { Database } from "jsr:@db/sqlite";

const db = new Database("parks.gpkg", { enableLoadExtension: true });
db.loadExtension("./target/release/libkenro_ext");
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
        Extensions: []string{"./target/release/libkenro_ext"},
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
db.load_extension("./target/release/libkenro_ext")
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
sqlite3_load_extension(db, "./target/release/libkenro_ext", NULL, &err);
```

## sqlite3 CLI

```
$ sqlite3 parks.gpkg
sqlite> .load ./target/release/libkenro_ext
sqlite> SELECT ST_AsGeoJSON(ST_Transform(ST_GeomFromGPB(geom), 4326)) FROM parks LIMIT 1;
```

macOS: the system `/usr/bin/sqlite3` is compiled without extension loading —
`brew install sqlite` and run `$(brew --prefix sqlite)/bin/sqlite3`.

## Containers (Cloud Run, Fly.io, ECS, …)

Any platform that runs a container runs kenro: build the `.so` in a Rust
stage, copy it next to your app, load it like on any Linux host. A Node
example (the pattern is identical for Python/Go/Ruby images):

```dockerfile
FROM rust:1 AS kenro
WORKDIR /src
COPY . .
RUN cargo build -p kenro-ext --release

FROM node:22-slim
WORKDIR /app
COPY --from=kenro /src/target/release/libkenro_ext.so ./libkenro_ext.so
COPY . .
RUN npm ci --omit=dev
CMD ["node", "server.js"]   # server.js: db.loadExtension("./libkenro_ext.so")
```

Build for the platform's architecture (`--platform linux/amd64` on Cloud
Run's default; arm64 images work the same with an arm64 build stage).

## AWS Lambda

Two routes:

- **Container image** (simplest): exactly the Dockerfile pattern above on
  an AWS base image.
- **Zip + layer**: build `libkenro_ext.so` for Amazon Linux
  (`x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu`, matching the
  function architecture — cross-compile with [cargo-zigbuild] or build in
  an `amazonlinux:2023` container), ship it in a layer, and load it from
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

Workers cannot load native extensions, and **D1 does not support
user-defined functions at all** — kenro cannot run *inside* D1 queries.
Two patterns that do work:

- **Process geometry in the Worker** with kenro-wasm's exports directly:
  store GeoPackage blobs in D1 columns, `SELECT` them out, then call
  `stAsText` / `stIntersects` / `stTransform` … on the values in JS. The
  wasm module is well inside Worker size limits (lite build 589 KB,
  240 KB gzip).
- **Run a full SQLite inside the Worker** with [sql.js] or [wa-sqlite]
  over bytes fetched from R2/KV (read-only analytics on a shipped
  `.gpkg`/`.sqlite`), and `registerKenro` as usual — the same adapters
  used in the browser. See [docs/wasm.md](wasm.md) for the per-host
  matrix (note: sql.js has no R-tree module and no int64/h3).

## Browser

See [docs/wasm.md](wasm.md) — adapters for the official SQLite WASM build,
sql.js and wa-sqlite, plus a drag-a-GeoPackage demo in
`crates/kenro-wasm/demo/`.

[cargo-zigbuild]: https://github.com/rust-cross/cargo-zigbuild
[mattn/go-sqlite3]: https://github.com/mattn/go-sqlite3
[sql.js]: https://github.com/sql-js/sql.js
[wa-sqlite]: https://github.com/rhashimoto/wa-sqlite
