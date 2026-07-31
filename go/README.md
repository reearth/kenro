# kenro for Go

[![Go Reference](https://pkg.go.dev/badge/github.com/reearth/kenro/go.svg)](https://pkg.go.dev/github.com/reearth/kenro/go)

Spatial SQL for [modernc.org/sqlite] — PostGIS-compatible `ST_` functions,
GeoPackage R-tree maintenance, CRS transform, H3, MVT — with **no cgo**. A
`CGO_ENABLED=0` static binary keeps working, which is the reason to be on
modernc.org/sqlite in the first place.

```sh
go get github.com/reearth/kenro/go
```

```go
import (
    "database/sql"

    kenro "github.com/reearth/kenro/go"
    _ "modernc.org/sqlite"
)

func main() {
    if err := kenro.Register(); err != nil {
        log.Fatal(err)
    }

    db, err := sql.Open("sqlite", "parks.gpkg")
    ...
    var wkt string
    db.QueryRow(`SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1`).Scan(&wkt)
}
```

`Register` affects connections opened afterwards — modernc.org/sqlite's
user-defined-function registry is process-global — so call it once during
start-up, before opening a database. It is safe to call from multiple
goroutines, and the registered functions are safe to use across as many
connections as `database/sql` wants to open.

No Rust toolchain is needed: the wasm module is committed to the repository.

## Examples

Runnable, in [example_test.go](example_test.go) and on
[pkg.go.dev](https://pkg.go.dev/github.com/reearth/kenro/go#pkg-examples):

| example | what it shows |
|---|---|
| `Example` | register, then query |
| `Example_storeAndQuery` | store geometries in a BLOB column and query them back |
| `Example_spatialIndex` | R-tree bbox filter, then a precise predicate refine |
| `Example_dissolve` | `GROUP BY` with the `ST_Union` aggregate |
| `Example_reproject` | `ST_Transform` |
| `Example_unimplemented` | what an unsupported function does instead of failing silently |

## Functions

The same ~80 functions as every other kenro binding, listed with their
PostGIS / DuckDB Spatial / SpatiaLite comparison in
[docs/functions.md](../docs/functions.md). Behavior matches the rusqlite
binding: functions are NULL-strict (any NULL argument gives NULL), errors are
prefixed `kenro: `, and aggregates skip NULL rows.

GeoPackage R-tree triggers work — modernc.org/sqlite is built with
`SQLITE_ENABLE_RTREE`, so a database written by GDAL or QGIS keeps its
spatial index in sync through inserts, updates and deletes.

## Two things this driver cannot do

Both are limitations of modernc.org/sqlite's user-defined-function API, not
of kenro, and both fail loudly rather than silently doing the wrong thing.

### `ST_Union` is either the scalar or the aggregate, not both

SQLite keys functions by *(name, arity)*, so `ST_Union(a, b)` and
`ST_Union(geom)` normally coexist. modernc.org/sqlite keys them by name
alone. Ordinary overloads such as `ST_GeomFromText/1` and
`ST_GeomFromText/2` are handled for you; `ST_Union` has to pick a side:

```go
kenro.Register()                                     // ST_Union(geom) — dissolve (default)
kenro.Register(kenro.WithUnionMode(kenro.UnionScalar)) // ST_Union(a, b) — two-geometry overlay
```

Calling the form you did not register returns an error.

### Functions are unavailable inside triggers under `trusted_schema = off`

modernc.org/sqlite's registration API exposes no `SQLITE_INNOCUOUS` flag, and
SQLite refuses to run a non-innocuous function from a trigger once the schema
is untrusted:

```
SQL logic error: unsafe use of ST_IsEmpty() (1)
```

SQLite's default is `trusted_schema = on`, so GeoPackage triggers work out of
the box. If you harden a database with `trusted_schema = off`, writes that
would touch the spatial index fail with the error above — the index never
goes quietly stale. Lifting this needs an `Innocuous` flag upstream in
modernc.org/sqlite.

## Performance

`go test -bench .`, Apple M1, `-benchtime 5000x`, ns per query through
`database/sql`:

| benchmark | ns/op | |
|---|---:|---|
| `SQLiteBaseline` (`SELECT 1`) | 694 | everything except kenro |
| `TrivialGoUDF` (int in, int out) | 790 | + the driver's UDF trampoline |
| `BlobEchoGoUDF` (93-byte blob in/out) | 1002 | + the driver's BLOB marshaling |
| `DirectArea` (no SQLite in the path) | 1450 | Go → wasm → geometry → Go |
| `GeomFromText` | 5428 | WKT parsing dominates |
| `Transform` | 5841 | reprojection |
| `Area` (`ST_Area(ST_GeomFromText(…))`) | 7406 | two calls |

The boundary crossing costs about a microsecond; past that you are paying for
geometry work any implementation would do. Cheap enough to ignore for the
query shape kenro is built for — filter on the R-tree index, then refine the
survivors — and expensive enough that a full-table-scan `ST_Intersects` over
millions of rows will be slower than a native extension.

## Choosing a smaller build

The embedded module is the `full` tier (~980 KB). Smaller tiers drop features
you may not need; functions outside the tier register as stubs naming the
missing cargo feature.

```sh
scripts/build-go-wasm.sh minimal   # I/O, predicates, R-tree, accessors, measures, processing
scripts/build-go-wasm.sh standard  # + ST_Transform, H3, GeoJSON, MVT
scripts/build-go-wasm.sh full      # + overlay, ST_Buffer, ST_MakeValid  (the default)
```

```go
//go:embed my-kenro.wasm
var myModule []byte

kenro.Register(kenro.WithModule(myModule))
```

## License

MIT OR Apache-2.0, same as the rest of kenro.

[modernc.org/sqlite]: https://pkg.go.dev/modernc.org/sqlite
