//! # kenro (間縄)
//!
//! Spatial functions for SQLite in pure Rust — works with rusqlite, as a
//! loadable extension, and in WASM.
//!
//! kenro provides the "20% of SpatiaLite everyone actually uses" with zero C
//! dependencies: WKB/WKT/GeoPackage-blob I/O, DE-9IM predicates, and the
//! helper functions the GeoPackage R-tree spatial index triggers require —
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

pub mod error;
pub mod functions;
pub mod geom;
pub mod gpb;

#[cfg(feature = "rusqlite")]
mod sqlite;

pub use error::{Error, Result};
pub use geom::Geom;

#[cfg(feature = "rusqlite")]
pub use sqlite::rusqlite_ext::register;
