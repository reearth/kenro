//! Pure counterparts of the SQL functions. No SQLite types appear anywhere
//! in this module tree: every function takes `&[u8]` / `&str` / scalars and
//! returns `Result` — the binding layers (rusqlite, and later the loadable
//! extension and WASM) are thin value-mapping shims over these.

pub mod io;
pub mod predicates;
pub mod rtree;
pub mod stubs;
