//! Pure counterparts of the SQL functions. No SQLite types appear anywhere
//! in this module tree: every function takes `&[u8]` / `&str` / scalars and
//! returns `Result` — the binding layers (rusqlite, and later the loadable
//! extension and WASM) are thin value-mapping shims over these.

pub mod accessors;
pub mod affine;
pub(crate) mod classify;
pub mod compat;
pub mod edit;
pub mod extra;
pub mod geodesic;
#[cfg(feature = "geojson")]
pub mod geojson;
#[cfg(feature = "h3")]
pub mod h3;
pub mod hull;
pub mod io;
pub mod linear;
pub mod manifest;
pub mod measures;
pub mod misc;
#[cfg(feature = "mvt")]
pub mod mvt;
#[cfg(feature = "overlay")]
pub mod overlay;
pub mod predicates;
pub mod processing;
pub mod rtree;
pub mod stubs;
pub mod threed;
#[cfg(feature = "transform")]
pub mod transform;
