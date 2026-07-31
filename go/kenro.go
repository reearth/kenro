// Package kenro registers kenro's spatial SQL functions — PostGIS-compatible
// ST_ functions, GeoPackage R-tree maintenance, CRS transform, H3, MVT — on
// the pure-Go SQLite driver modernc.org/sqlite.
//
// modernc.org/sqlite cannot load a native SQLite extension, so kenro's Rust
// core runs as a WebAssembly module inside [wazero] and is wired in as
// user-defined functions. Both halves are pure Go: a binary built with
// CGO_ENABLED=0 keeps working, which is the whole reason to be on
// modernc.org/sqlite in the first place.
//
//	if err := kenro.Register(); err != nil { ... }
//	db, err := sql.Open("sqlite", "parks.gpkg")
//
// Register affects connections opened afterwards — modernc.org/sqlite's
// user-defined-function registry is process-global — so call it once during
// start-up, before opening a database.
//
// [wazero]: https://wazero.io
package kenro

import (
	"context"
	"fmt"
	"sync"

	"github.com/reearth/kenro/go/internal/wasmbin"
)

// Option configures Register.
type Option func(*config)

type config struct {
	wasm      []byte
	unionMode UnionMode
}

// UnionMode selects which ST_Union kenro registers.
//
// SQLite itself is happy to hold a scalar ST_Union(a, b) and an aggregate
// ST_Union(geom) under one name, because it keys functions by (name, arity).
// modernc.org/sqlite's registry keys them by name alone, so on this driver
// the two forms are mutually exclusive. Whichever form is not registered
// fails loudly, naming this option — it never silently does the wrong thing.
type UnionMode int

const (
	// UnionAggregate registers ST_Union(geom) — the dissolve aggregate. This
	// is the default. ST_Union(a, b) then returns an error.
	UnionAggregate UnionMode = iota
	// UnionScalar registers ST_Union(a, b) — the two-geometry overlay.
	// ST_Union(geom) as an aggregate then returns an error.
	UnionScalar
)

// WithModule replaces the embedded wasm module, e.g. with a smaller tier
// built by scripts/build-go-wasm.sh. Functions missing from the module
// register as stubs that name the absent cargo feature.
func WithModule(wasm []byte) Option {
	return func(c *config) { c.wasm = wasm }
}

// WithUnionMode selects the ST_Union form to register. See [UnionMode].
func WithUnionMode(m UnionMode) Option {
	return func(c *config) { c.unionMode = m }
}

var (
	registerOnce sync.Mutex
	registered   bool
)

// Register registers every kenro spatial function on the modernc.org/sqlite
// driver, for connections opened after it returns. It is safe to call
// concurrently but registers only once; later calls are no-ops.
func Register(opts ...Option) error {
	return RegisterContext(context.Background(), opts...)
}

// RegisterContext is [Register] with a context governing wasm compilation.
func RegisterContext(ctx context.Context, opts ...Option) error {
	registerOnce.Lock()
	defer registerOnce.Unlock()
	if registered {
		return nil
	}

	cfg := &config{wasm: wasmbin.Module, unionMode: UnionAggregate}
	for _, o := range opts {
		o(cfg)
	}

	rt, err := newRuntime(ctx, cfg.wasm)
	if err != nil {
		return err
	}
	b := &binding{rt: rt, cfg: cfg}
	if err := b.registerAll(ctx); err != nil {
		_ = rt.close(ctx)
		return err
	}
	registered = true
	return nil
}

// Registered reports whether kenro's functions have been registered.
func Registered() bool {
	registerOnce.Lock()
	defer registerOnce.Unlock()
	return registered
}

func errf(format string, a ...any) error {
	return fmt.Errorf(format, a...)
}

// defaultModule is the embedded wasm artifact, used when WithModule is not
// given.
func defaultModule() []byte { return wasmbin.Module }
