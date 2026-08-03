//! # kenro
//!
//! SpatiaLite-style spatial SQL for SQLite in pure Rust — PostGIS-compatible
//! `ST_` functions that work with rusqlite, as a loadable extension, and in
//! WASM.
//!
//! kenro is a spatial SQL engine for SQLite in pure Rust, with zero C
//! dependencies: WKB/WKT/GeoPackage-blob I/O, the DE-9IM predicate family,
//! pure-Rust overlay/repair/buffer, SQL aggregates (`ST_Union`, `ST_AsMVT`),
//! and the helper functions the GeoPackage spatial index triggers require —
//! so a plain SQLite build can maintain a GeoPackage spatial index
//! correctly.
//!
//! Function names and signatures follow PostGIS (SQL/MM `ST_` prefix);
//! PostGIS is the declared reference for semantics. Where kenro differs, it
//! errs loudly — never silently.
//!
//! ```no_run
//! # #[cfg(feature = "rusqlite")]
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let conn = rusqlite::Connection::open("parks.gpkg")?;
//! kenro::register(&conn)?;
//! let wkt: String = conn.query_row(
//!     "SELECT ST_AsText(ST_GeomFromGPB(geom)) FROM parks LIMIT 1",
//!     [],
//!     |r| r.get(0),
//! )?;
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "rusqlite"))]
//! # fn main() {}
//! ```
//!
//! All functions are pure (no I/O, no clock, no randomness): identical input
//! always produces identical output.

pub mod coords;
#[cfg(feature = "transform")]
pub mod crs;
pub mod error;
pub mod functions;
pub mod geom;
pub mod gpb;
#[cfg(feature = "mvt")]
pub mod mvt;

#[cfg(feature = "rusqlite")]
mod sqlite;

pub use error::{Error, Result};
pub use geom::Geom;

#[cfg(feature = "rusqlite")]
pub use sqlite::rusqlite_ext::register;
