#!/usr/bin/env bash
# Fail when the wasm module the Go binding embeds went stale.
#
# `go/internal/wasmbin/kenro.wasm` is committed so that `go get` works without
# a Rust toolchain, which means it can silently fall behind the Rust sources
# it was built from. A signature change is caught at run time (registration
# checks the manifest against the module's exports), but a behavior-only
# change to the core is not: the old module still registers and still passes
# every test, just with yesterday's bug in it.
#
# So this compares *what changed in the commit range*, not bytes — rustc and
# binaryen versions move, and a byte-for-byte rebuild is not reproducible
# across machines.
#
# Usage: scripts/check-go-wasm-fresh.sh <base-ref> [head-ref]
set -euo pipefail

cd "$(dirname "$0")/.."

BASE="${1:?usage: check-go-wasm-fresh.sh <base-ref> [head-ref]}"
HEAD_REF="${2:-HEAD}"
ARTIFACT="go/internal/wasmbin/kenro.wasm"

# Sources the artifact is built from. The Go binding itself is not on this
# list: it is compiled from source by the consumer.
SOURCES_RE='^(src/|crates/kenro-abi/|Cargo\.toml$|Cargo\.lock$)'

if ! git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null; then
    echo "base ref $BASE is not in this clone — skipping the freshness check" >&2
    exit 0
fi

CHANGED="$(git diff --name-only "$BASE" "$HEAD_REF")"

CORE_CHANGED="$(printf '%s\n' "$CHANGED" | grep -E "$SOURCES_RE" || true)"
if [ -z "$CORE_CHANGED" ]; then
    echo "no changes under the kenro core or kenro-abi — $ARTIFACT is fine as is"
    exit 0
fi

if printf '%s\n' "$CHANGED" | grep -qxF "$ARTIFACT"; then
    echo "core changed and $ARTIFACT was rebuilt — ok"
    exit 0
fi

cat >&2 <<EOF
error: the Go binding's embedded wasm module is stale.

These changed:

$(printf '%s\n' "$CORE_CHANGED" | sed 's/^/  /')

but $ARTIFACT did not. Go users get the committed module verbatim, so the
change above is not in it. Rebuild and commit:

  scripts/build-go-wasm.sh full

If the change genuinely cannot affect the module (docs, a test, a
rusqlite-only path), say so in the commit message and re-run with the
artifact touched, or narrow SOURCES_RE in this script.
EOF
exit 1
