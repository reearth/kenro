/** The initialized kenro-wasm module — the namespace object of the package root. */
export type KenroWasm = typeof import("../pkg/kenro_wasm.js");

/** How an argument is checked before it reaches wasm. */
export type ArgKind = "blob" | "text" | "int" | "i64" | "real" | "text_or_int";

/** What comes back; the `opt_` kinds may be SQL NULL. */
export type RetKind =
  | "blob"
  | "text"
  | "int"
  | "bool"
  | "real"
  | "i64"
  | "opt_blob"
  | "opt_real"
  | "opt_i64"
  | "opt_int"
  | "opt_text";

/** A SQL value as the host adapters exchange it. */
export type SqlValue = null | Uint8Array | string | number | bigint;

export interface FunctionEntry {
  /** The name to register, e.g. `ST_GeomFromText`. */
  sql_name: string;
  /** The wasm export implementing this (name, arity) pair. */
  export: string;
  args: ArgKind[];
  ret: RetKind;
  /** True when a 64-bit value crosses the boundary (the h3 functions). */
  uses_i64: boolean;
}

export interface AggregateEntry {
  sql_name: string;
  /** The wasm class whose instances accumulate per group. */
  ctor_export: string;
  args: ArgKind[];
  uses_i64: boolean;
}

/** A function kenro deliberately does not implement, registered to fail loudly. */
export interface StubEntry {
  name: string;
  arities: number[];
  hint: string;
}

export interface Manifest {
  functions: FunctionEntry[];
  aggregates: AggregateEntry[];
  stubs: StubEntry[];
}

/** Host-independent aggregate driver: per-group state lives in `A`. */
export interface AggregateDriver<A = unknown> {
  start(): A;
  step(acc: A, args: SqlValue[]): void;
  /** Returns the group's value and frees the wasm accumulator. */
  finish(acc: A): SqlValue;
}

/** Parse the manifest JSON exported by the wasm module. */
export function loadManifest(wasm: KenroWasm): Manifest;

/** Build the UDF body for one manifest entry: NULL-strict, argument-checked. */
export function makeUdf(entry: FunctionEntry, wasm: KenroWasm): (...args: SqlValue[]) => SqlValue;

/** Build the driver for one aggregate entry. NULL rows are skipped. */
export function makeAggregate(entry: AggregateEntry, wasm: KenroWasm): AggregateDriver;

/** The loud-failure body shared by every stub registration. */
export function stubUdf(stub: StubEntry): (...args: SqlValue[]) => never;

/** The loud-failure body for hosts that cannot represent 64-bit integers. */
export function i64UnsupportedUdf(
  entry: FunctionEntry,
  hostName: string,
): (...args: SqlValue[]) => never;
