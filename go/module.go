package kenro

import (
	"context"
	"fmt"
	"math"
	"strings"
	"sync"

	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
	"github.com/tetratelabs/wazero/imports/wasi_snapshot_preview1"
)

// Status codes returned by every kenro-abi export.
const (
	statusOK   = 0
	statusErr  = 1
	statusNull = 2
)

// Aggregate kinds, matching k_agg_new in kenro-abi.
const (
	aggUnion        = 0
	aggMVT          = 1
	aggExtent       = 2
	aggExtent3D     = 3
	aggDijkstra     = 4
	aggDijkstraCost = 5
)

// runtime owns the compiled wasm module and a pool of instances.
//
// A wasm instance is single-threaded and holds the ABI's result slots in its
// own linear memory, so a call must have one to itself. Instances are pooled
// rather than created per call: instantiation costs a fresh linear memory,
// which is far more than a spatial predicate.
type runtime struct {
	wz       wazero.Runtime
	compiled wazero.CompiledModule
	cfg      wazero.ModuleConfig

	mu   sync.Mutex
	free []*instance
	n    int // instances handed out, for naming
}

func newRuntime(ctx context.Context, wasm []byte) (*runtime, error) {
	wz := wazero.NewRuntime(ctx)
	if _, err := wasi_snapshot_preview1.Instantiate(ctx, wz); err != nil {
		wz.Close(ctx)
		return nil, fmt.Errorf("kenro: instantiating WASI: %w", err)
	}
	compiled, err := wz.CompileModule(ctx, wasm)
	if err != nil {
		wz.Close(ctx)
		return nil, fmt.Errorf("kenro: compiling the kenro wasm module: %w", err)
	}
	return &runtime{
		wz:       wz,
		compiled: compiled,
		// No filesystem, no environment, no clock: the module is a pure
		// function library. Rust cdylibs on wasip1 export neither _start nor
		// _initialize, so there is no start function to run.
		cfg: wazero.NewModuleConfig().WithStartFunctions().WithName(""),
	}, nil
}

func (r *runtime) close(ctx context.Context) error {
	r.mu.Lock()
	r.free = nil
	r.mu.Unlock()
	return r.wz.Close(ctx)
}

func (r *runtime) acquire(ctx context.Context) (*instance, error) {
	r.mu.Lock()
	if n := len(r.free); n > 0 {
		in := r.free[n-1]
		r.free = r.free[:n-1]
		r.mu.Unlock()
		return in, nil
	}
	r.n++
	r.mu.Unlock()

	mod, err := r.wz.InstantiateModule(ctx, r.compiled, r.cfg)
	if err != nil {
		return nil, fmt.Errorf("kenro: instantiating the wasm module: %w", err)
	}
	return newInstance(mod)
}

// release returns an instance to the pool. A poisoned instance (one whose
// last call trapped, which in Rust means a panic — the allocator state is no
// longer trustworthy) is closed instead.
func (r *runtime) release(ctx context.Context, in *instance) {
	if in == nil {
		return
	}
	if in.poisoned {
		_ = in.mod.Close(ctx)
		return
	}
	r.mu.Lock()
	r.free = append(r.free, in)
	r.mu.Unlock()
}

// instance is one wasm module instance plus the ABI entry points, resolved
// once so the hot path is a map lookup at worst.
type instance struct {
	mod api.Module
	mem api.Memory

	alloc  api.Function
	free   api.Function
	outPtr api.Function
	outLen api.Function
	retI64 api.Function
	retF64 api.Function

	aggNew           api.Function
	aggFinish        api.Function
	aggDrop          api.Function
	unionStep        api.Function
	extentStep       api.Function
	extent3dStep     api.Function
	mvtStep          api.Function
	dijkstraStep     api.Function
	dijkstraCostStep api.Function

	fns map[string]api.Function

	// Scratch block for argument bytes, grown on demand and reused. One
	// allocation per instance beats one per call.
	scratch    uint32
	scratchLen uint32

	stack    []uint64
	poisoned bool
}

func newInstance(mod api.Module) (*instance, error) {
	in := &instance{
		mod:   mod,
		mem:   mod.Memory(),
		fns:   map[string]api.Function{},
		stack: make([]uint64, 0, 12),
	}
	required := map[string]*api.Function{
		"kenro_alloc":   &in.alloc,
		"kenro_free":    &in.free,
		"kenro_out_ptr": &in.outPtr,
		"kenro_out_len": &in.outLen,
		"kenro_ret_i64": &in.retI64,
		"kenro_ret_f64": &in.retF64,
		"k_agg_new":     &in.aggNew,
		"k_agg_finish":  &in.aggFinish,
		"k_agg_drop":    &in.aggDrop,
	}
	for name, dst := range required {
		f := mod.ExportedFunction(name)
		if f == nil {
			return nil, fmt.Errorf("kenro: wasm module is missing the %q export", name)
		}
		*dst = f
	}
	// Feature-gated: absent when the module was built without overlay/mvt.
	in.unionStep = mod.ExportedFunction("k_agg_union_step")
	in.extentStep = mod.ExportedFunction("k_agg_extent_step")
	in.extent3dStep = mod.ExportedFunction("k_agg_extent3d_step")
	in.mvtStep = mod.ExportedFunction("k_agg_mvt_step")
	in.dijkstraStep = mod.ExportedFunction("k_agg_dijkstra_step")
	in.dijkstraCostStep = mod.ExportedFunction("k_agg_dijkstra_cost_step")
	return in, nil
}

// fn resolves an export by name, caching the lookup.
func (in *instance) fn(name string) (api.Function, error) {
	if f, ok := in.fns[name]; ok {
		return f, nil
	}
	f := in.mod.ExportedFunction(name)
	if f == nil {
		return nil, fmt.Errorf("kenro: wasm module is missing the %q export", name)
	}
	in.fns[name] = f
	return f, nil
}

// call runs f with params and returns the ABI status. A trap poisons the
// instance: after a Rust panic the linear memory is not in a defined state.
func (in *instance) call(ctx context.Context, f api.Function, params ...uint64) (int32, error) {
	in.stack = append(in.stack[:0], params...)
	// Room for the result.
	if len(in.stack) == 0 {
		in.stack = append(in.stack, 0)
	}
	if err := f.CallWithStack(ctx, in.stack); err != nil {
		in.poisoned = true
		return 0, fmt.Errorf("kenro: wasm trap (this is a bug in kenro): %w", err)
	}
	return api.DecodeI32(in.stack[0]), nil
}

// reserve makes sure the scratch block can hold n bytes and returns its
// offset.
func (in *instance) reserve(ctx context.Context, n uint32) (uint32, error) {
	if n <= in.scratchLen {
		return in.scratch, nil
	}
	size := uint32(1024)
	for size < n {
		size *= 2
	}
	if in.scratch != 0 {
		if _, err := in.call(ctx, in.free, api.EncodeI32(int32(in.scratch)), api.EncodeU32(in.scratchLen)); err != nil {
			return 0, err
		}
	}
	in.stack = append(in.stack[:0], api.EncodeU32(size))
	if err := in.alloc.CallWithStack(ctx, in.stack); err != nil {
		in.poisoned = true
		return 0, fmt.Errorf("kenro: wasm trap in kenro_alloc: %w", err)
	}
	ptr := api.DecodeU32(in.stack[0])
	if ptr == 0 {
		return 0, fmt.Errorf("kenro: out of wasm memory reserving %d bytes", size)
	}
	in.scratch, in.scratchLen = ptr, size
	return ptr, nil
}

// out copies the OUT buffer out of wasm memory.
func (in *instance) out(ctx context.Context) ([]byte, error) {
	if _, err := in.call(ctx, in.outPtr); err != nil {
		return nil, err
	}
	ptr := api.DecodeU32(in.stack[0])
	if _, err := in.call(ctx, in.outLen); err != nil {
		return nil, err
	}
	n := api.DecodeU32(in.stack[0])
	if n == 0 {
		return nil, nil
	}
	b, ok := in.mem.Read(ptr, n)
	if !ok {
		return nil, fmt.Errorf("kenro: OUT buffer (%d bytes at %#x) is outside wasm memory", n, ptr)
	}
	// Copy: the next call may overwrite it, and SQLite keeps the result.
	return append([]byte(nil), b...), nil
}

// errFromOut turns a statusErr into a Go error carrying kenro's message.
func (in *instance) errFromOut(ctx context.Context) error {
	msg, err := in.out(ctx)
	if err != nil {
		return err
	}
	if len(msg) == 0 {
		return fmt.Errorf("kenro: unknown error")
	}
	return errString(msg)
}

// errString keeps kenro's own message verbatim (it is already `kenro: `
// prefixed) so SQL error text matches the rusqlite and JS bindings.
type errString []byte

func (e errString) Error() string { return string(e) }

func (in *instance) i64(ctx context.Context) (int64, error) {
	if _, err := in.call(ctx, in.retI64); err != nil {
		return 0, err
	}
	return int64(in.stack[0]), nil
}

func (in *instance) f64(ctx context.Context) (float64, error) {
	if _, err := in.call(ctx, in.retF64); err != nil {
		return 0, err
	}
	return math.Float64frombits(in.stack[0]), nil
}

// writeBytes copies b into the scratch block at off and returns the next
// free offset.
func (in *instance) writeBytes(base, off uint32, b []byte) (uint32, error) {
	if len(b) == 0 {
		return off, nil
	}
	if !in.mem.Write(base+off, b) {
		return 0, fmt.Errorf("kenro: writing %d argument bytes past the end of wasm memory", len(b))
	}
	return off + uint32(len(b)), nil
}

func describeExports(in *instance, names []string) string {
	missing := make([]string, 0, len(names))
	for _, n := range names {
		if in.mod.ExportedFunction(n) == nil {
			missing = append(missing, n)
		}
	}
	return strings.Join(missing, ", ")
}
