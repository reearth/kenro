# kenro in pure Go (modernc.org/sqlite + wazero)

[modernc.org/sqlite] is SQLite transpiled to Go: no cgo, one static binary,
`CGO_ENABLED=0` everywhere. The trade-off is that it cannot load a native
SQLite extension — `sqlite3_load_extension` wants a shared library and a C
ABI. What it *can* do is register Go functions as SQL user-defined functions.

So the Go binding uses the same shape as the browser adapters: kenro's
SQLite-free core compiled to WebAssembly, run inside [wazero] (also pure Go),
and wired in as UDFs. Both halves stay cgo-free, which is the entire reason
to be on modernc.org/sqlite in the first place.

```go
import (
    "database/sql"

    kenro "github.com/reearth/kenro/go"
    _ "modernc.org/sqlite"
)

if err := kenro.Register(); err != nil { ... }

db, _ := sql.Open("sqlite", "parks.gpkg")
var wkt string
db.QueryRow(`SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1`).Scan(&wkt)
```

`Register` is process-global (so is modernc.org/sqlite's UDF registry) and
affects connections opened afterwards — call it once at start-up.

## Why wasm32-wasip1, not wasm32-unknown-unknown

The module the Go binding embeds is built for `wasm32-wasip1`. That is not a
style choice:

- **proj4rs declares `[target.wasm32-unknown-unknown.dependencies]` on
  wasm-bindgen / js-sys / web-sys / console_log.** On that triple the build
  therefore imports `__wbindgen_placeholder__`, and wazero refuses to
  instantiate it (`module[__wbindgen_placeholder__] not instantiated`). With
  `transform` enabled, an unknown-unknown build simply cannot run in a
  non-JS host.
- **Size goes the other way from what you would guess.** wasip1 costs a flat
  ~15 KB of std/wasi-libc overhead, but resolves `sin`/`cos`/`atan2`/`pow`
  against wasi-libc instead of compiling in the whole Rust `libm` crate.
  Measured on an equivalent probe: the standard tier is 510 KB on wasip1 vs
  817 KB on unknown-unknown, the full tier 941 KB vs 1227 KB.
- **Panics are readable.** A Rust panic on wasip1 writes its message to
  stderr before aborting; on unknown-unknown you get a bare trap with no
  explanation.

The module's only imports are five `wasi_snapshot_preview1` calls
(`random_get`, `environ_get`, `environ_sizes_get`, `fd_write`, `proc_exit`).
It is instantiated with no filesystem, no environment and no clock.

Rust cdylibs on wasip1 export neither `_start` nor `_initialize`, so the
binding instantiates with `WithStartFunctions()` — an empty list. Configuring
wazero's default (`_start`) instead makes instantiation fail.

## Calling convention

`crates/kenro-abi` exposes one export per (SQL function, arity), named `k_` +
the manifest's export column, plus `kenro_alloc` / `kenro_free`, an OUT
buffer and two scalar return slots. Every export returns a status: `0` ok,
`1` error (message in OUT), `2` SQL NULL.

Nothing in the Go code hard-codes a function name. At registration it reads
`k_manifest()` — the same catalog the rusqlite binding is tested against and
the JS adapters are generated from — and derives every registration, argument
kind and result kind from it. A manifest entry with no matching wasm export
fails registration loudly rather than at first use.

## Concurrency

A wasm instance is single-threaded and holds the ABI's result slots in its
own linear memory, so one call gets one instance. Instances are pooled;
`database/sql` can run as many connections as it likes. An instance whose
call traps (a Rust panic) is discarded rather than returned to the pool,
because its allocator state is no longer trustworthy.

Aggregates pin an instance for the accumulator's lifetime — the handle only
means anything inside the memory that created it — and release it in
`Final`.

## Two driver limitations, both loud

**One implementation per function name.** SQLite keys user-defined functions
by *(name, arity)*; modernc.org/sqlite's registry keys them by name alone.
The binding works around this for ordinary overloads by registering one
variadic function per name and dispatching on the argument count, so
`ST_GeomFromText/1` and `ST_GeomFromText/2` both work. It cannot work around
the one name that is a scalar *and* an aggregate:

| | registered | the other form |
|---|---|---|
| `kenro.UnionAggregate` (default) | `ST_Union(geom)` — dissolve | `ST_Union(a, b)` errors |
| `kenro.WithUnionMode(kenro.UnionScalar)` | `ST_Union(a, b)` — overlay | `ST_Union(geom)` errors |

Whichever form you did not pick fails with an error; it never silently
returns the wrong shape.

**No `SQLITE_INNOCUOUS`.** modernc.org/sqlite's registration API exposes
`SQLITE_UTF8` and `SQLITE_DETERMINISTIC`, and nothing else. Functions
registered without `SQLITE_INNOCUOUS` cannot be called from triggers once
`PRAGMA trusted_schema = off`:

```
SQL logic error: unsafe use of ST_IsEmpty() (1)
```

SQLite's default is `trusted_schema = on`, and modernc.org/sqlite does not
compile with `SQLITE_TRUSTED_SCHEMA=0`, so **GeoPackage R-tree triggers work
out of the box** — `go/gpkg_test.go` runs the spec's Annex F.3 DDL verbatim
and checks insert / update / rowid-change / delete maintenance, plus the
bbox-filter-then-refine query. If you harden a database with
`trusted_schema = off`, kenro's functions stop working inside triggers; the
failure is an error, not a silently stale index. Lifting this needs an
upstream `FunctionImpl.Innocuous` flag in modernc.org/sqlite.

R-tree itself is available: modernc.org/sqlite is built with
`-DSQLITE_ENABLE_RTREE`.

## Performance

`go test -bench .` on an Apple M1, `-benchtime 5000x`, ns per query through
`database/sql` unless noted:

| benchmark | ns/op | what it measures |
|---|---:|---|
| `SQLiteBaseline` (`SELECT 1`) | 694 | everything except kenro |
| `TrivialGoUDF` (int in, int out) | 790 | + the driver's UDF trampoline (~100 ns) |
| `BlobEchoGoUDF` (93-byte blob in/out) | 1002 | + the driver's BLOB marshaling (~300 ns) |
| `DirectArea` (no SQLite in the path) | 1450 | Go → wasm → geometry → Go, end to end |
| `Area` (`ST_Area(ST_GeomFromText(…))`) | 7406 | two kenro calls |
| `GeomFromText` | 5428 | WKT parsing dominates |
| `Transform` | 5841 | reprojection |

The boundary crossing is around a microsecond; past that you are paying for
geometry work, the same work any implementation would do. It is cheap enough
to ignore for the query shape kenro is built for — filter on the R-tree
index, then refine with a precise predicate on the survivors — and expensive
enough that a full-table-scan `ST_Intersects` over millions of rows will be
slower than a native extension.

## Rebuilding the embedded module

The committed artifact is the `full` tier (~980 KB). Rebuild it, or swap in a
smaller one:

```sh
scripts/build-go-wasm.sh full       # → go/internal/wasmbin/kenro.wasm
scripts/build-go-wasm.sh minimal    # I/O, predicates, R-tree, accessors, …
```

```go
//go:embed my-kenro.wasm
var myModule []byte

kenro.Register(kenro.WithModule(myModule))
```

Functions outside the tier you built register as stubs that name the missing
cargo feature, the same as every other kenro host.

[modernc.org/sqlite]: https://pkg.go.dev/modernc.org/sqlite
[wazero]: https://wazero.io
