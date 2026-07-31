//! Pure counterparts of the SQL functions. No SQLite types appear anywhere
//! in this module tree: every function takes `&[u8]` / `&str` / scalars and
//! returns `Result` — the binding layers (rusqlite, and later the loadable
//! extension and WASM) are thin value-mapping shims over these.

pub mod accessors;
pub mod affine;
#[cfg(feature = "geojson")]
pub mod geojson;
#[cfg(feature = "h3")]
pub mod h3;
pub mod io;
pub mod manifest;
pub mod measures;
pub mod overlay;
pub mod predicates;
pub mod processing;
pub mod rtree;
pub mod stubs;
#[cfg(feature = "transform")]
pub mod transform;
