package kenro

import (
	"context"
	"database/sql/driver"
	"encoding/json"
	"fmt"
	"sort"
	"strings"

	"github.com/tetratelabs/wazero/api"
	sqlite "modernc.org/sqlite"
)

// The catalog kenro-abi serves from k_manifest: the single source of truth
// for what gets registered. Nothing in this file hard-codes a function name.
type fnEntry struct {
	SQLName string   `json:"sql_name"`
	Export  string   `json:"export"`
	Args    []string `json:"args"`
	Ret     string   `json:"ret"`
}

type aggEntry struct {
	SQLName string   `json:"sql_name"`
	AggKind int      `json:"agg_kind"`
	Args    []string `json:"args"`
}

type stubEntry struct {
	Name    string `json:"name"`
	Hint    string `json:"hint"`
	Arities []int  `json:"arities"`
}

type manifest struct {
	Functions  []fnEntry   `json:"functions"`
	Aggregates []aggEntry  `json:"aggregates"`
	Stubs      []stubEntry `json:"stubs"`
}

type binding struct {
	rt  *runtime
	cfg *config
}

func (b *binding) registerAll(ctx context.Context) error {
	m, err := b.manifest(ctx)
	if err != nil {
		return err
	}

	scalars := map[string]map[int]fnEntry{} // SQL name → arity → entry
	order := []string{}
	for _, e := range m.Functions {
		if _, ok := scalars[e.SQLName]; !ok {
			scalars[e.SQLName] = map[int]fnEntry{}
			order = append(order, e.SQLName)
		}
		scalars[e.SQLName][len(e.Args)] = e
	}

	aggs := map[string]map[int]aggEntry{}
	aggOrder := []string{}
	for _, e := range m.Aggregates {
		if _, ok := aggs[e.SQLName]; !ok {
			aggs[e.SQLName] = map[int]aggEntry{}
			aggOrder = append(aggOrder, e.SQLName)
		}
		aggs[e.SQLName][len(e.Args)] = e
	}

	// One name, one implementation: modernc.org/sqlite keys its registry by
	// function name, so a name that is both scalar and aggregate in kenro
	// (ST_Union) has to pick a side. See [UnionMode].
	for name := range aggs {
		if _, clash := scalars[name]; !clash {
			continue
		}
		if b.cfg.unionMode == UnionScalar {
			delete(aggs, name)
		} else {
			delete(scalars, name)
		}
	}

	for _, name := range order {
		arities, ok := scalars[name]
		if !ok {
			continue
		}
		if err := b.registerScalar(name, arities); err != nil {
			return err
		}
	}
	for _, name := range aggOrder {
		arities, ok := aggs[name]
		if !ok {
			continue
		}
		if err := b.registerAggregate(name, arities); err != nil {
			return err
		}
	}
	for _, s := range m.Stubs {
		if err := b.registerStub(s); err != nil {
			return err
		}
	}
	return nil
}

// manifest reads the catalog out of the wasm module and checks that every
// export it names is really there — the artifact and the catalog are built
// together, so a mismatch means a stale wasm file.
func (b *binding) manifest(ctx context.Context) (*manifest, error) {
	in, err := b.rt.acquire(ctx)
	if err != nil {
		return nil, err
	}
	defer b.rt.release(ctx, in)

	f, err := in.fn("k_manifest")
	if err != nil {
		return nil, err
	}
	status, err := in.call(ctx, f)
	if err != nil {
		return nil, err
	}
	if status != statusOK {
		return nil, errf("kenro: reading the function manifest failed")
	}
	raw, err := in.out(ctx)
	if err != nil {
		return nil, err
	}
	var m manifest
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil, errf("kenro: decoding the function manifest: %w", err)
	}
	names := make([]string, 0, len(m.Functions))
	for _, e := range m.Functions {
		names = append(names, e.Export)
	}
	if missing := describeExports(in, names); missing != "" {
		return nil, errf("kenro: the wasm module is missing exports named by its own manifest (%s) — rebuild it with scripts/build-go-wasm.sh", missing)
	}
	return &m, nil
}

// registerScalar installs one variadic SQL function per name and dispatches
// on the argument count, because modernc.org/sqlite allows a single
// registration per name (SQLite itself keys on name *and* arity).
//
// FunctionImpl carries no SQLITE_INNOCUOUS: the driver's API does not expose
// it. rusqlite sets that flag so kenro can be called from GeoPackage triggers
// under `PRAGMA trusted_schema = off`; here such a call is rejected by SQLite
// instead. Pinned by TestTrustedSchemaOffIsRejectedNotIgnored.
func (b *binding) registerScalar(name string, arities map[int]fnEntry) error {
	return sqlite.RegisterFunction(name, &sqlite.FunctionImpl{
		NArgs:         -1,
		Deterministic: true,
		Scalar: func(_ *sqlite.FunctionContext, args []driver.Value) (driver.Value, error) {
			e, ok := arities[len(args)]
			if !ok {
				return nil, arityError(name, len(args), aritiesOf(arities))
			}
			return b.callScalar(context.Background(), e, args)
		},
	})
}

func (b *binding) callScalar(ctx context.Context, e fnEntry, args []driver.Value) (driver.Value, error) {
	// NULL-strict, like every other kenro binding: a NULL argument short
	// circuits to NULL and the core never sees it.
	for _, a := range args {
		if a == nil {
			return nil, nil
		}
	}

	in, err := b.rt.acquire(ctx)
	if err != nil {
		return nil, err
	}
	defer b.rt.release(ctx, in)

	f, err := in.fn(e.Export)
	if err != nil {
		return nil, err
	}
	params, err := marshalArgs(ctx, in, e.SQLName, e.Args, args)
	if err != nil {
		return nil, err
	}
	status, err := in.call(ctx, f, params...)
	if err != nil {
		return nil, err
	}
	return readResult(ctx, in, status, e.Ret)
}

// marshalArgs writes the byte-valued arguments into the instance's scratch
// block and builds the wasm parameter list.
func marshalArgs(ctx context.Context, in *instance, name string, kinds []string, args []driver.Value) ([]uint64, error) {
	params := make([]uint64, 0, 2*len(kinds))
	var (
		blobs    [][]byte
		blobSlot []int // index into params of each pointer placeholder
		total    uint32
	)
	for i, kind := range kinds {
		v := args[i]
		switch kind {
		case "blob":
			bs, err := asBlob(name, v)
			if err != nil {
				return nil, err
			}
			blobSlot = append(blobSlot, len(params))
			blobs = append(blobs, bs)
			total += uint32(len(bs))
			params = append(params, 0, api.EncodeU32(uint32(len(bs))))
		case "text", "text_or_int":
			s, err := asText(name, kind, v)
			if err != nil {
				return nil, err
			}
			bs := []byte(s)
			blobSlot = append(blobSlot, len(params))
			blobs = append(blobs, bs)
			total += uint32(len(bs))
			params = append(params, 0, api.EncodeU32(uint32(len(bs))))
		case "int":
			n, err := asInt(name, v)
			if err != nil {
				return nil, err
			}
			params = append(params, api.EncodeI32(int32(n)))
		case "bool":
			// SQLite has no boolean type; 0 and 1 are the only spellings,
			// and SQL `true`/`false` are integer literals for exactly those.
			n, err := asInt(name, v)
			if err != nil {
				return nil, err
			}
			if n != 0 && n != 1 {
				return nil, errf("kenro: %s: expected a boolean (0 or 1), got %d", name, n)
			}
			params = append(params, api.EncodeI32(int32(n)))
		case "i64":
			n, err := asInt(name, v)
			if err != nil {
				return nil, err
			}
			params = append(params, uint64(n))
		case "real":
			f, err := asReal(name, v)
			if err != nil {
				return nil, err
			}
			params = append(params, api.EncodeF64(f))
		default:
			return nil, errf("kenro: %s: unsupported argument kind %q in the manifest", name, kind)
		}
	}

	if total > 0 {
		base, err := in.reserve(ctx, total)
		if err != nil {
			return nil, err
		}
		var off uint32
		for i, bs := range blobs {
			params[blobSlot[i]] = api.EncodeU32(base + off)
			next, err := in.writeBytes(base, off, bs)
			if err != nil {
				return nil, err
			}
			off = next
		}
	}
	return params, nil
}

func readResult(ctx context.Context, in *instance, status int32, ret string) (driver.Value, error) {
	switch status {
	case statusNull:
		return nil, nil
	case statusErr:
		return nil, in.errFromOut(ctx)
	case statusOK:
	default:
		return nil, errf("kenro: unknown ABI status %d", status)
	}

	switch ret {
	case "blob", "opt_blob":
		return in.out(ctx)
	case "text":
		b, err := in.out(ctx)
		if err != nil {
			return nil, err
		}
		return string(b), nil
	case "opt_text":
		b, err := in.out(ctx)
		if err != nil {
			return nil, err
		}
		return string(b), nil
	case "int", "i64", "opt_i64", "opt_int", "bool":
		return in.i64(ctx)
	case "real", "opt_real":
		return in.f64(ctx)
	default:
		return nil, errf("kenro: unsupported result kind %q in the manifest", ret)
	}
}

// ------------------------------------------------------------- aggregates

func (b *binding) registerAggregate(name string, arities map[int]aggEntry) error {
	return sqlite.RegisterFunction(name, &sqlite.FunctionImpl{
		NArgs:         -1,
		Deterministic: true,
		MakeAggregate: func(sqlite.FunctionContext) (sqlite.AggregateFunction, error) {
			return &aggregate{b: b, name: name, arities: arities}, nil
		},
	})
}

// aggregate drives one accumulator inside the wasm module. The instance is
// pinned for the accumulator's lifetime: the handle only means anything in
// the linear memory that created it.
type aggregate struct {
	b       *binding
	name    string
	arities map[int]aggEntry

	in     *instance
	handle int32
	kind   int

	done   bool
	result driver.Value
	err    error
}

func (a *aggregate) Step(_ *sqlite.FunctionContext, args []driver.Value) error {
	if a.err != nil {
		return a.err
	}
	e, ok := a.arities[len(args)]
	if !ok {
		a.err = arityError(a.name, len(args), aritiesOf(a.arities))
		return a.err
	}
	// PostGIS aggregate semantics: NULL rows are skipped, not fatal.
	for _, v := range args {
		if v == nil {
			return nil
		}
	}

	ctx := context.Background()
	if a.in == nil {
		in, err := a.b.rt.acquire(ctx)
		if err != nil {
			a.err = err
			return err
		}
		a.in, a.kind = in, e.AggKind
		status, err := in.call(ctx, in.aggNew, api.EncodeI32(int32(e.AggKind)))
		if err != nil {
			a.err = err
			return err
		}
		if status < 0 {
			a.err = errf("kenro: %s is unavailable in this wasm module (built without the required cargo feature)", a.name)
			return a.err
		}
		a.handle = status
	}

	if err := a.step(ctx, e, args); err != nil {
		a.err = err
		return err
	}
	return nil
}

func (a *aggregate) step(ctx context.Context, e aggEntry, args []driver.Value) error {
	switch e.AggKind {
	case aggUnion:
		if a.in.unionStep == nil {
			return errf("kenro: %s is unavailable in this wasm module (built without the `overlay` cargo feature)", a.name)
		}
		params, err := marshalArgs(ctx, a.in, a.name, e.Args, args)
		if err != nil {
			return err
		}
		status, err := a.in.call(ctx, a.in.unionStep, append([]uint64{api.EncodeI32(a.handle)}, params...)...)
		if err != nil {
			return err
		}
		if status == statusErr {
			return a.in.errFromOut(ctx)
		}
		return nil

	case aggExtent:
		// ST_Extent steps one geometry per row, like ST_Union.
		params, err := marshalArgs(ctx, a.in, a.name, e.Args, args)
		if err != nil {
			return err
		}
		status, err := a.in.call(ctx, a.in.extentStep, append([]uint64{api.EncodeI32(a.handle)}, params...)...)
		if err != nil {
			return err
		}
		if status == statusErr {
			return a.in.errFromOut(ctx)
		}
		return nil

	case aggMVT:
		if a.in.mvtStep == nil {
			return errf("kenro: %s is unavailable in this wasm module (built without the `mvt` cargo feature)", a.name)
		}
		// ST_AsMVT(geom [, name [, extent [, props_json]]]): pad the omitted
		// trailing arguments out with presence flags so one export serves
		// every arity.
		padded := make([]driver.Value, 4)
		kinds := []string{"blob", "text", "int", "text"}
		copy(padded, args)
		present := make([]bool, 4)
		for i := range args {
			present[i] = true
		}
		for i := len(args); i < 4; i++ {
			switch kinds[i] {
			case "text":
				padded[i] = ""
			default:
				padded[i] = int64(0)
			}
		}
		params, err := marshalArgs(ctx, a.in, a.name, kinds, padded)
		if err != nil {
			return err
		}
		// k_agg_mvt_step(h, gp, gl, has_name, np, nl, has_extent, extent, has_props, pp, pl)
		call := []uint64{
			api.EncodeI32(a.handle),
			params[0], params[1], // geom
			boolParam(present[1]),
			params[2], params[3], // name
			boolParam(present[2]),
			params[4], // extent
			boolParam(present[3]),
			params[5], params[6], // props
		}
		status, err := a.in.call(ctx, a.in.mvtStep, call...)
		if err != nil {
			return err
		}
		if status == statusErr {
			return a.in.errFromOut(ctx)
		}
		return nil

	default:
		return errf("kenro: unknown aggregate kind %d", e.AggKind)
	}
}

func (a *aggregate) WindowInverse(*sqlite.FunctionContext, []driver.Value) error {
	return errf("kenro: %s cannot be used as a window function over a moving frame (its accumulator is not invertible)", a.name)
}

func (a *aggregate) WindowValue(*sqlite.FunctionContext) (driver.Value, error) {
	if a.err != nil {
		return nil, a.err
	}
	if a.done {
		return a.result, nil
	}
	a.done = true
	if a.in == nil {
		// Zero rows aggregated (or every row was NULL).
		return nil, nil
	}
	ctx := context.Background()
	status, err := a.in.call(ctx, a.in.aggFinish, api.EncodeI32(a.handle))
	if err != nil {
		a.err = err
		return nil, err
	}
	v, err := readResult(ctx, a.in, status, "opt_blob")
	if err != nil {
		a.err = err
		return nil, err
	}
	a.result = v
	return v, nil
}

func (a *aggregate) Final(*sqlite.FunctionContext) {
	if a.in == nil {
		return
	}
	ctx := context.Background()
	if !a.done {
		// Aborted statement: drop the accumulator so its slot is reused.
		_, _ = a.in.call(ctx, a.in.aggDrop, api.EncodeI32(a.handle))
	}
	a.b.rt.release(ctx, a.in)
	a.in = nil
}

// ------------------------------------------------------------------ stubs

// registerStub installs a function that exists only to fail with a useful
// message — "not implemented, use X instead" beats "no such function", for a
// human and for an AI writing SQL.
func (b *binding) registerStub(s stubEntry) error {
	return sqlite.RegisterFunction(s.Name, &sqlite.FunctionImpl{
		NArgs:         -1,
		Deterministic: true,
		Scalar: func(*sqlite.FunctionContext, []driver.Value) (driver.Value, error) {
			return nil, errf("kenro: %s is not implemented in kenro. %s", s.Name, s.Hint)
		},
	})
}

// ------------------------------------------------------------- conversions

func asBlob(name string, v driver.Value) ([]byte, error) {
	switch t := v.(type) {
	case []byte:
		return t, nil
	case string:
		return nil, errf("kenro: %s: got TEXT where a geometry BLOB was expected (did you mean ST_GeomFromText?)", name)
	default:
		return nil, errf("kenro: %s: expected a geometry BLOB, got %s", name, typeName(v))
	}
}

func asText(name, kind string, v driver.Value) (string, error) {
	switch t := v.(type) {
	case string:
		return t, nil
	case []byte:
		return string(t), nil
	case int64:
		// Kind::TextOrInt — ST_Buffer's PostGIS integer overload.
		if kind == "text_or_int" {
			return fmt.Sprintf("quad_segs=%d", t), nil
		}
	}
	if kind == "text_or_int" {
		return "", errf("kenro: %s: expected TEXT options or INTEGER, got %s", name, typeName(v))
	}
	return "", errf("kenro: %s: expected TEXT, got %s", name, typeName(v))
}

func asInt(name string, v driver.Value) (int64, error) {
	if n, ok := v.(int64); ok {
		return n, nil
	}
	return 0, errf("kenro: %s: expected an INTEGER, got %s", name, typeName(v))
}

func asReal(name string, v driver.Value) (float64, error) {
	switch t := v.(type) {
	case float64:
		return t, nil
	case int64:
		return float64(t), nil
	}
	return 0, errf("kenro: %s: expected a numeric value, got %s", name, typeName(v))
}

// typeName names a SQLite storage class the way SQLite's own error messages
// do, so kenro's messages read the same on every binding.
func typeName(v driver.Value) string {
	switch v.(type) {
	case nil:
		return "Null"
	case int64:
		return "Integer"
	case float64:
		return "Real"
	case string:
		return "Text"
	case []byte:
		return "Blob"
	default:
		return fmt.Sprintf("%T", v)
	}
}

func boolParam(b bool) uint64 {
	if b {
		return api.EncodeI32(1)
	}
	return api.EncodeI32(0)
}

func aritiesOf[T any](m map[int]T) []int {
	out := make([]int, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	sort.Ints(out)
	return out
}

func arityError(name string, got int, want []int) error {
	parts := make([]string, len(want))
	for i, n := range want {
		parts[i] = fmt.Sprint(n)
	}
	return errf("kenro: %s takes %s argument(s), got %d", name, strings.Join(parts, " or "), got)
}
