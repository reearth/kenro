//! Pure counterparts of the SQL functions. No SQLite types appear anywhere
//! in this module tree: every function takes `&[u8]` / `&str` / scalars and
//! returns `Result` — the binding layers (rusqlite, and later the loadable
//! extension and WASM) are thin value-mapping shims over these.

/// A coordinate as PostGIS prints it in a text encoding: fixed to `digits`
/// decimals, then trailing zeros and a bare decimal point trimmed. Shared by
/// the GML, KML and SVG writers, which agree on this and differ on
/// everything else.
///
/// Negative zero prints as `0`. It arrives two ways — SVG negates every Y, so
/// `y = 0` becomes `-0.0`, and any small negative rounds to it — and PostGIS
/// prints `0` for both, in all three encodings. Verified live.
#[cfg(any(feature = "gml", feature = "text-encodings"))]
pub(crate) fn num(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    match s.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s.to_string(),
    }
}

pub mod accessors;
pub mod affine;
// Operand-class helpers shared by the overlay engine and the MVT encoder,
// and used by nothing else — a minimal build carries neither.
#[cfg(any(feature = "overlay", feature = "mvt"))]
pub(crate) mod classify;
pub mod compat;
pub mod edit;
pub mod extra;
pub mod geodesic;
#[cfg(feature = "geojson")]
pub mod geojson;
#[cfg(feature = "gml")]
pub mod gml;
pub mod grid;
#[cfg(feature = "h3")]
pub mod h3;
pub mod hull;
pub mod io;
#[cfg(feature = "text-encodings")]
pub mod kml;
pub mod linear;
pub mod lines;
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
pub mod surface;
#[cfg(feature = "text-encodings")]
pub mod svg;
pub mod threed;
pub mod threed_metric;
#[cfg(feature = "transform")]
pub mod transform;
