package kenro

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"sync"
	"testing"

	sqlite "modernc.org/sqlite"
)

// The numbers these produce are quoted in go/README.md. They measure the whole
// path — SQLite → Go callback → wasm call → geometry work — against a
// SQLite-only baseline, so the marginal cost of the wasm hop is visible.
func benchQuery(b *testing.B, query string) {
	if err := Register(); err != nil {
		b.Fatal(err)
	}
	db, err := sql.Open("sqlite", ":memory:")
	if err != nil {
		b.Fatal(err)
	}
	defer db.Close()
	db.SetMaxOpenConns(1)

	stmt, err := db.Prepare("SELECT " + query)
	if err != nil {
		b.Fatal(err)
	}
	defer stmt.Close()

	var v any
	b.ResetTimer()
	for b.Loop() {
		if err := stmt.QueryRow().Scan(&v); err != nil {
			b.Fatal(err)
		}
	}
}

// Baseline: everything except kenro.
func BenchmarkSQLiteBaseline(b *testing.B) { benchQuery(b, `1`) }

// The binding without SQLite in the way: Go → wasm → geometry work → Go.
// The gap to BenchmarkArea is what modernc's UDF trampoline costs.
func BenchmarkDirectArea(b *testing.B) {
	ctx := context.Background()
	rt, err := newRuntime(ctx, defaultModule())
	if err != nil {
		b.Fatal(err)
	}
	defer rt.close(ctx)
	bind := &binding{rt: rt, cfg: &config{}}
	m, err := bind.manifest(ctx)
	if err != nil {
		b.Fatal(err)
	}
	var fromText, area fnEntry
	for _, e := range m.Functions {
		switch {
		case e.SQLName == "ST_GeomFromText" && len(e.Args) == 1:
			fromText = e
		case e.SQLName == "ST_Area":
			area = e
		}
	}
	geom, err := bind.callScalar(ctx, fromText, []driver.Value{"POLYGON((0 0,2 0,2 3,0 3,0 0))"})
	if err != nil {
		b.Fatal(err)
	}
	args := []driver.Value{geom}
	b.ResetTimer()
	for b.Loop() {
		if _, err := bind.callScalar(ctx, area, args); err != nil {
			b.Fatal(err)
		}
	}
}

// The shape that matters: parse two geometries, run a predicate.
func BenchmarkIntersects(b *testing.B) {
	benchQuery(b, `ST_Intersects(`+poly+`, `+pt+`)`)
}

// Geometry parsing dominates here, not the boundary crossing.
func BenchmarkGeomFromText(b *testing.B) { benchQuery(b, poly) }

func BenchmarkArea(b *testing.B) { benchQuery(b, `ST_Area(`+poly+`)`) }

func BenchmarkTransform(b *testing.B) {
	benchQuery(b, `ST_Transform(`+pt+`, 3857)`)
}

// Control: a UDF that does nothing, so the driver's own callback cost is
// separated from kenro's. Everything above this line is unavoidable if you
// register any Go function at all.
var noopOnce sync.Once

func BenchmarkTrivialGoUDF(b *testing.B) {
	noopOnce.Do(func() {
		_ = sqlite.RegisterFunction("kenro_bench_noop", &sqlite.FunctionImpl{
			NArgs:         1,
			Deterministic: true,
			Scalar: func(*sqlite.FunctionContext, []driver.Value) (driver.Value, error) {
				return int64(1), nil
			},
		})
	})
	benchQuery(b, `kenro_bench_noop(1)`)
}

// A realistic geometry as a SQL blob literal, so a control can move one
// through the driver without calling kenro. (hex of the GPB for
// POLYGON((0 0,2 0,2 3,0 3,0 0)) in EPSG:4326, produced by the test above.)
var blobLiteral = func() string {
	if err := Register(); err != nil {
		panic(err)
	}
	db, err := sql.Open("sqlite", ":memory:")
	if err != nil {
		panic(err)
	}
	defer db.Close()
	var hex string
	if err := db.QueryRow(`SELECT hex(` + poly + `)`).Scan(&hex); err != nil {
		panic(err)
	}
	return "x'" + hex + "'"
}()

// Control 2: a Go UDF that takes a geometry BLOB and hands one back without
// touching wasm — isolates the driver's per-call value marshaling from
// kenro's own work.
var echoOnce sync.Once

func BenchmarkBlobEchoGoUDF(b *testing.B) {
	echoOnce.Do(func() {
		_ = sqlite.RegisterFunction("kenro_bench_echo", &sqlite.FunctionImpl{
			NArgs:         1,
			Deterministic: true,
			Scalar: func(_ *sqlite.FunctionContext, args []driver.Value) (driver.Value, error) {
				return args[0], nil
			},
		})
	})
	benchQuery(b, `length(kenro_bench_echo(`+blobLiteral+`))`)
}
