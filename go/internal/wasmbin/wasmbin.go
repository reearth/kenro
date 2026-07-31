// Package wasmbin carries the prebuilt kenro wasm module so that `go get`
// works without a Rust toolchain.
//
// Rebuild with `scripts/build-go-wasm.sh [minimal|standard|full]` from the
// repository root; the committed artifact is the `full` tier.
package wasmbin

import _ "embed"

//go:embed kenro.wasm
var Module []byte
