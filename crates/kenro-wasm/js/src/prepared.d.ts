import type { Prepared } from "../pkg/kenro_wasm.js";

/**
 * Free a handle, at most once. A no-op on an already-freed handle, on `null`
 * and on `undefined` — unlike `Prepared.free`, which traps when called twice.
 */
export function freeOnce(handle: Prepared | null | undefined): void;

/** Own `handle` for the duration of `fn`, freeing it however the call exits. */
export function withPrepared<T>(handle: Prepared, fn: (handle: Prepared) => T): T;

/**
 * Run `fn` with an `own` function that registers handles to be freed on the
 * way out, in reverse order of registration.
 */
export function withScope<T>(fn: (own: <H extends Prepared>(handle: H) => H) => T): T;
