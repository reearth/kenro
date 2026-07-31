//! kenro's pure core behind a plain C ABI, for wasm hosts that have no JS
//! glue layer — the Go binding runs this module in [wazero] and registers the
//! exports as `modernc.org/sqlite` user-defined functions, the same shape as
//! the browser adapters in `kenro-wasm`.
//!
//! No SQLite is compiled here. The host owns the database; this module is a
//! pure function library over geometry bytes.
//!
//! # Calling convention
//!
//! One export per (SQL function, arity), named `k_` + the manifest's `export`
//! column, so the host needs no name mangling of its own — it reads
//! [`k_manifest`] and derives everything else.
//!
//! Arguments map by [`kenro::functions::manifest::Kind`]:
//!
//! | kind | wasm params |
//! |---|---|
//! | `Blob` / `Text` / `TextOrInt` | `ptr: u32, len: u32` |
//! | `Int` | `i32` |
//! | `I64` | `i64` |
//! | `Real` | `f64` |
//!
//! Every export returns an `i32` status: `0` ok, `1` error, `2` SQL NULL.
//! The payload is read out afterwards, so the host never has to decode a
//! packed return value:
//!
//! - `Blob` / `OptBlob` / `Text` results, and error messages, land in the OUT
//!   buffer ([`kenro_out_ptr`] / [`kenro_out_len`])
//! - `Int` / `I64` / `OptI64` / `Bool` land in [`kenro_ret_i64`]
//! - `Real` / `OptReal` land in [`kenro_ret_f64`]
//!
//! NULL-strictness (any SQL NULL argument → NULL result) lives in the host
//! adapter, exactly as it does for rusqlite and the JS hosts: SQL NULL never
//! reaches this crate.
//!
//! [wazero]: https://wazero.io

// Exports mirror the manifest's camelCase `export` column verbatim so the two
// cannot drift; that means non-snake-case symbol names.
#![allow(non_snake_case)]
// Every export takes host-owned `(ptr, len)` argument blocks. Their safety
// contract is the calling convention documented above — it belongs to the
// host, not to a Rust caller, and marking the thunks `unsafe` would not
// change the wasm ABI they are compiled to.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::cell::UnsafeCell;

use kenro::functions::{accessors, affine, io, manifest, measures, predicates, processing, rtree};

// ---------------------------------------------------------------- status

/// Call succeeded; payload in the OUT buffer or a return slot.
const OK: i32 = 0;
/// Call failed; the `kenro: `-prefixed message is in the OUT buffer.
const ERR: i32 = 1;
/// Call succeeded with a SQL NULL result.
const NULL: i32 = 2;

// ---------------------------------------------------------------- memory

/// A wasm module instance is single-threaded and the host holds one instance
/// per connection, so the return slots below are plain module-level state.
/// (The native test build is multi-threaded and serializes access itself.)
struct Slot<T>(UnsafeCell<T>);

// SAFETY: upheld by the host, which must not call one instance concurrently.
unsafe impl<T> Sync for Slot<T> {}

impl<T> Slot<T> {
    const fn new(v: T) -> Self {
        Slot(UnsafeCell::new(v))
    }

    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}

/// Result bytes / error message from the last call.
static OUT: Slot<Vec<u8>> = Slot::new(Vec::new());
/// Integer or boolean result from the last call.
static RET_I64: Slot<i64> = Slot::new(0);
/// Floating-point result from the last call.
static RET_F64: Slot<f64> = Slot::new(0.0);

fn set_out(v: Vec<u8>) {
    *OUT.get() = v;
}

/// Allocate `len` bytes for the host to write arguments into.
///
/// The host owns the block until it passes it to [`kenro_free`]; this module
/// never frees it on its behalf.
#[unsafe(no_mangle)]
pub extern "C" fn kenro_alloc(len: u32) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr
}

/// Free a block from [`kenro_alloc`]. `len` must be the allocated length.
#[unsafe(no_mangle)]
pub extern "C" fn kenro_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() {
        return;
    }
    unsafe { drop(Vec::from_raw_parts(ptr, 0, len as usize)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn kenro_out_ptr() -> *const u8 {
    OUT.get().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn kenro_out_len() -> u32 {
    OUT.get().len() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn kenro_ret_i64() -> i64 {
    *RET_I64.get()
}

#[unsafe(no_mangle)]
pub extern "C" fn kenro_ret_f64() -> f64 {
    *RET_F64.get()
}

// ---------------------------------------------------------------- helpers

/// Borrow a host-provided argument block. Safe as long as the host keeps it
/// alive for the duration of the call, which the calling convention requires.
fn s<'a>(ptr: *const u8, len: u32) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(ptr, len as usize) }
}

/// TEXT arguments arrive as UTF-8 bytes; the host validated them already, but
/// invalid input must still fail loudly rather than panic.
fn t<'a>(ptr: *const u8, len: u32) -> Result<&'a str, i32> {
    core::str::from_utf8(s(ptr, len)).map_err(|e| {
        set_out(format!("kenro: invalid UTF-8 in TEXT argument: {e}").into_bytes());
        ERR
    })
}

fn fail(e: kenro::Error) -> i32 {
    set_out(e.to_string().into_bytes());
    ERR
}

fn blob(r: kenro::Result<Vec<u8>>) -> i32 {
    match r {
        Ok(v) => {
            set_out(v);
            OK
        }
        Err(e) => fail(e),
    }
}

fn opt_blob(r: kenro::Result<Option<Vec<u8>>>) -> i32 {
    match r {
        Ok(Some(v)) => {
            set_out(v);
            OK
        }
        Ok(None) => NULL,
        Err(e) => fail(e),
    }
}

fn text(r: kenro::Result<String>) -> i32 {
    blob(r.map(String::into_bytes))
}

fn int(r: kenro::Result<impl Into<i64>>) -> i32 {
    match r {
        Ok(v) => {
            *RET_I64.get() = v.into();
            OK
        }
        Err(e) => fail(e),
    }
}

fn opt_int(r: kenro::Result<Option<i64>>) -> i32 {
    match r {
        Ok(Some(v)) => {
            *RET_I64.get() = v;
            OK
        }
        Ok(None) => NULL,
        Err(e) => fail(e),
    }
}

fn boolean(r: kenro::Result<bool>) -> i32 {
    int(r.map(|v| v as i64))
}

fn real(r: kenro::Result<f64>) -> i32 {
    match r {
        Ok(v) => {
            *RET_F64.get() = v;
            OK
        }
        Err(e) => fail(e),
    }
}

fn opt_real(r: kenro::Result<Option<f64>>) -> i32 {
    match r {
        Ok(Some(v)) => {
            *RET_F64.get() = v;
            OK
        }
        Ok(None) => NULL,
        Err(e) => fail(e),
    }
}

/// `Ok`-short-circuit for TEXT arguments: `let x = try_str!(p, l);`
macro_rules! try_str {
    ($p:expr, $l:expr) => {
        match t($p, $l) {
            Ok(v) => v,
            Err(status) => return status,
        }
    };
}

// ================================================================ exports
//
// One per manifest entry, in manifest order. Each is a single expression so
// the mapping stays auditable against `manifest::FUNCTIONS`.

// ---- Geometry I/O ----

#[unsafe(no_mangle)]
pub extern "C" fn k_stGeomFromText(p: *const u8, l: u32) -> i32 {
    blob(io::st_geom_from_text(try_str!(p, l), None))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stGeomFromTextSrid(p: *const u8, l: u32, srid: i32) -> i32 {
    blob(io::st_geom_from_text(try_str!(p, l), Some(srid)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stGeomFromWkb(p: *const u8, l: u32) -> i32 {
    blob(io::st_geom_from_wkb(s(p, l), None))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stGeomFromWkbSrid(p: *const u8, l: u32, srid: i32) -> i32 {
    blob(io::st_geom_from_wkb(s(p, l), Some(srid)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stGeomFromGpb(p: *const u8, l: u32) -> i32 {
    blob(io::st_geom_from_gpb(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stAsText(p: *const u8, l: u32) -> i32 {
    text(io::st_as_text(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stAsBinary(p: *const u8, l: u32) -> i32 {
    blob(io::st_as_binary(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stAsGpb(p: *const u8, l: u32) -> i32 {
    blob(io::st_as_gpb(s(p, l)))
}

// ---- SRID ----

#[unsafe(no_mangle)]
pub extern "C" fn k_stSetSrid(p: *const u8, l: u32, srid: i32) -> i32 {
    blob(io::st_set_srid(s(p, l), srid))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stSrid(p: *const u8, l: u32) -> i32 {
    int(io::st_srid(s(p, l)))
}

// ---- Predicates & measures ----

macro_rules! predicate {
    ($export:ident, $f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $export(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
            boolean($f(s(ap, al), s(bp, bl)))
        }
    };
}

predicate!(k_stIntersects, predicates::st_intersects);
predicate!(k_stContains, predicates::st_contains);
predicate!(k_stWithin, predicates::st_within);
predicate!(k_stDisjoint, predicates::st_disjoint);
predicate!(k_stTouches, predicates::st_touches);
predicate!(k_stCrosses, predicates::st_crosses);
predicate!(k_stOverlaps, predicates::st_overlaps);
predicate!(k_stEquals, predicates::st_equals);
predicate!(k_stCovers, predicates::st_covers);
predicate!(k_stCoveredBy, predicates::st_covered_by);

#[unsafe(no_mangle)]
pub extern "C" fn k_stRelate(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
    text(predicates::st_relate(s(ap, al), s(bp, bl)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stRelatePattern(
    ap: *const u8,
    al: u32,
    bp: *const u8,
    bl: u32,
    pp: *const u8,
    pl: u32,
) -> i32 {
    boolean(predicates::st_relate_pattern(
        s(ap, al),
        s(bp, bl),
        try_str!(pp, pl),
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stDistance(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
    opt_real(predicates::st_distance(s(ap, al), s(bp, bl)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stDwithin(ap: *const u8, al: u32, bp: *const u8, bl: u32, d: f64) -> i32 {
    boolean(predicates::st_dwithin(s(ap, al), s(bp, bl), d))
}

// ---- GeoPackage R-tree ----

#[unsafe(no_mangle)]
pub extern "C" fn k_stMinX(p: *const u8, l: u32) -> i32 {
    opt_real(rtree::st_min_x(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stMaxX(p: *const u8, l: u32) -> i32 {
    opt_real(rtree::st_max_x(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stMinY(p: *const u8, l: u32) -> i32 {
    opt_real(rtree::st_min_y(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stMaxY(p: *const u8, l: u32) -> i32 {
    opt_real(rtree::st_max_y(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stIsEmpty(p: *const u8, l: u32) -> i32 {
    boolean(rtree::st_is_empty(s(p, l)))
}

// ---- CRS transform ----

#[cfg(feature = "transform")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stTransform(p: *const u8, l: u32, srid: i32) -> i32 {
    blob(kenro::functions::transform::st_transform(s(p, l), srid))
}

// ---- GeoJSON ----

#[cfg(feature = "geojson")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stAsGeojson(p: *const u8, l: u32) -> i32 {
    text(kenro::functions::geojson::st_as_geojson(s(p, l), None))
}

#[cfg(feature = "geojson")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stAsGeojsonDigits(p: *const u8, l: u32, digits: i32) -> i32 {
    text(kenro::functions::geojson::st_as_geojson(
        s(p, l),
        Some(digits as i64),
    ))
}

#[cfg(feature = "geojson")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stGeomFromGeojson(p: *const u8, l: u32) -> i32 {
    blob(kenro::functions::geojson::st_geom_from_geojson(try_str!(
        p, l
    )))
}

// ---- H3 cells ----

#[cfg(feature = "h3")]
#[unsafe(no_mangle)]
pub extern "C" fn k_h3LatlngToCell(p: *const u8, l: u32, res: i32) -> i32 {
    int(kenro::functions::h3::h3_latlng_to_cell(s(p, l), res as i64))
}

#[cfg(feature = "h3")]
#[unsafe(no_mangle)]
pub extern "C" fn k_h3CellToParent(cell: i64, res: i32) -> i32 {
    int(kenro::functions::h3::h3_cell_to_parent(cell, res as i64))
}

#[cfg(feature = "h3")]
#[unsafe(no_mangle)]
pub extern "C" fn k_h3CellToString(cell: i64) -> i32 {
    text(kenro::functions::h3::h3_cell_to_string(cell))
}

#[cfg(feature = "h3")]
#[unsafe(no_mangle)]
pub extern "C" fn k_h3StringToCell(p: *const u8, l: u32) -> i32 {
    int(kenro::functions::h3::h3_string_to_cell(try_str!(p, l)))
}

// ---- Constructors ----

#[unsafe(no_mangle)]
pub extern "C" fn k_stMakePoint(x: f64, y: f64) -> i32 {
    blob(io::st_make_point(x, y))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stPoint(x: f64, y: f64) -> i32 {
    blob(io::st_point(x, y, None))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stPointSrid(x: f64, y: f64, srid: i32) -> i32 {
    blob(io::st_point(x, y, Some(srid)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stMakeEnvelope(xmin: f64, ymin: f64, xmax: f64, ymax: f64) -> i32 {
    blob(io::st_make_envelope(xmin, ymin, xmax, ymax, None))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stMakeEnvelopeSrid(
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
    srid: i32,
) -> i32 {
    blob(io::st_make_envelope(xmin, ymin, xmax, ymax, Some(srid)))
}

// ---- GeoPackage geometry-type triggers (extension F.4) ----

#[unsafe(no_mangle)]
pub extern "C" fn k_gpkgIsAssignable(ep: *const u8, el: u32, ap: *const u8, al: u32) -> i32 {
    boolean(rtree::gpkg_is_assignable(
        try_str!(ep, el),
        try_str!(ap, al),
    ))
}

// ---- Measures ----

#[unsafe(no_mangle)]
pub extern "C" fn k_stClosestPoint(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
    opt_blob(measures::st_closest_point(s(ap, al), s(bp, bl)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stLineInterpolatePoint(p: *const u8, l: u32, fraction: f64) -> i32 {
    blob(measures::st_line_interpolate_point(s(p, l), fraction))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stLineLocatePoint(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
    real(measures::st_line_locate_point(s(ap, al), s(bp, bl)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stHausdorffDistance(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
    real(measures::st_hausdorff_distance(s(ap, al), s(bp, bl)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stFrechetDistance(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
    real(measures::st_frechet_distance(s(ap, al), s(bp, bl)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stAzimuth(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
    opt_real(measures::st_azimuth(s(ap, al), s(bp, bl)))
}

// ---- Overlay ----

#[cfg(feature = "overlay")]
macro_rules! overlay2 {
    ($export:ident, $f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $export(ap: *const u8, al: u32, bp: *const u8, bl: u32) -> i32 {
            blob($f(s(ap, al), s(bp, bl)))
        }
    };
}

#[cfg(feature = "overlay")]
overlay2!(k_stIntersection, kenro::functions::overlay::st_intersection);
#[cfg(feature = "overlay")]
overlay2!(k_stDifference, kenro::functions::overlay::st_difference);
#[cfg(feature = "overlay")]
overlay2!(
    k_stSymDifference,
    kenro::functions::overlay::st_sym_difference
);
#[cfg(feature = "overlay")]
overlay2!(k_stUnion, kenro::functions::overlay::st_union);

#[cfg(feature = "overlay")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stBuffer(p: *const u8, l: u32, distance: f64) -> i32 {
    blob(kenro::functions::overlay::st_buffer(
        s(p, l),
        distance,
        None,
    ))
}

/// `Kind::TextOrInt`: the host normalizes an INTEGER `n` to `quad_segs=n`
/// before calling, so this side only ever sees TEXT.
#[cfg(feature = "overlay")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stBufferOpts(
    p: *const u8,
    l: u32,
    distance: f64,
    op: *const u8,
    ol: u32,
) -> i32 {
    blob(kenro::functions::overlay::st_buffer(
        s(p, l),
        distance,
        Some(try_str!(op, ol)),
    ))
}

#[cfg(feature = "overlay")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stMakeValid(p: *const u8, l: u32) -> i32 {
    blob(kenro::functions::overlay::st_make_valid(s(p, l)))
}

// ---- MVT ----

#[cfg(feature = "mvt")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stAsMvtGeom(gp: *const u8, gl: u32, bp: *const u8, bl: u32) -> i32 {
    opt_blob(kenro::functions::mvt::st_as_mvt_geom(
        s(gp, gl),
        s(bp, bl),
        None,
        None,
        None,
    ))
}

#[cfg(feature = "mvt")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stAsMvtGeomExtent(
    gp: *const u8,
    gl: u32,
    bp: *const u8,
    bl: u32,
    extent: i32,
) -> i32 {
    opt_blob(kenro::functions::mvt::st_as_mvt_geom(
        s(gp, gl),
        s(bp, bl),
        Some(extent),
        None,
        None,
    ))
}

#[cfg(feature = "mvt")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stAsMvtGeomBuffer(
    gp: *const u8,
    gl: u32,
    bp: *const u8,
    bl: u32,
    extent: i32,
    buffer: i32,
) -> i32 {
    opt_blob(kenro::functions::mvt::st_as_mvt_geom(
        s(gp, gl),
        s(bp, bl),
        Some(extent),
        Some(buffer),
        None,
    ))
}

#[cfg(feature = "mvt")]
#[unsafe(no_mangle)]
pub extern "C" fn k_stAsMvtGeomClip(
    gp: *const u8,
    gl: u32,
    bp: *const u8,
    bl: u32,
    extent: i32,
    buffer: i32,
    clip: i32,
) -> i32 {
    opt_blob(kenro::functions::mvt::st_as_mvt_geom(
        s(gp, gl),
        s(bp, bl),
        Some(extent),
        Some(buffer),
        Some(clip),
    ))
}

// ---- Processing ----

#[unsafe(no_mangle)]
pub extern "C" fn k_stConvexHull(p: *const u8, l: u32) -> i32 {
    blob(processing::st_convex_hull(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stPointOnSurface(p: *const u8, l: u32) -> i32 {
    blob(processing::st_point_on_surface(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stSimplifyVw(p: *const u8, l: u32, tolerance: f64) -> i32 {
    blob(processing::st_simplify_vw(s(p, l), tolerance))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stChaikinSmoothing(p: *const u8, l: u32) -> i32 {
    blob(processing::st_chaikin_smoothing(s(p, l), 1))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stChaikinSmoothingN(p: *const u8, l: u32, iterations: i32) -> i32 {
    blob(processing::st_chaikin_smoothing(s(p, l), iterations as i64))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stRemoveRepeatedPoints(p: *const u8, l: u32) -> i32 {
    blob(processing::st_remove_repeated_points(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stOrientedEnvelope(p: *const u8, l: u32) -> i32 {
    blob(processing::st_oriented_envelope(s(p, l)))
}

// ---- Affine ----

#[unsafe(no_mangle)]
pub extern "C" fn k_stRotate(p: *const u8, l: u32, radians: f64) -> i32 {
    blob(affine::st_rotate(s(p, l), radians))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stRotateXY(p: *const u8, l: u32, radians: f64, x0: f64, y0: f64) -> i32 {
    blob(affine::st_rotate_xy(s(p, l), radians, x0, y0))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stTranslate(p: *const u8, l: u32, dx: f64, dy: f64) -> i32 {
    blob(affine::st_translate(s(p, l), dx, dy))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stScale(p: *const u8, l: u32, xf: f64, yf: f64) -> i32 {
    blob(affine::st_scale(s(p, l), xf, yf))
}

// ---- Accessors ----

#[unsafe(no_mangle)]
pub extern "C" fn k_stArea(p: *const u8, l: u32) -> i32 {
    real(accessors::st_area(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stNPoints(p: *const u8, l: u32) -> i32 {
    int(accessors::st_npoints(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stPerimeter(p: *const u8, l: u32) -> i32 {
    real(accessors::st_perimeter(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stGeometryType(p: *const u8, l: u32) -> i32 {
    text(accessors::st_geometry_type(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stNumGeometries(p: *const u8, l: u32) -> i32 {
    int(accessors::st_num_geometries(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stGeometryN(p: *const u8, l: u32, n: i32) -> i32 {
    opt_blob(accessors::st_geometry_n(s(p, l), n as i64))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stStartPoint(p: *const u8, l: u32) -> i32 {
    opt_blob(accessors::st_start_point(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stEndPoint(p: *const u8, l: u32) -> i32 {
    opt_blob(accessors::st_end_point(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stPointN(p: *const u8, l: u32, n: i32) -> i32 {
    opt_blob(accessors::st_point_n(s(p, l), n as i64))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stReverse(p: *const u8, l: u32) -> i32 {
    blob(accessors::st_reverse(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stLength(p: *const u8, l: u32) -> i32 {
    real(accessors::st_length(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stCentroid(p: *const u8, l: u32) -> i32 {
    blob(accessors::st_centroid(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stEnvelope(p: *const u8, l: u32) -> i32 {
    blob(accessors::st_envelope(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stX(p: *const u8, l: u32) -> i32 {
    opt_real(accessors::st_x(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stY(p: *const u8, l: u32) -> i32 {
    opt_real(accessors::st_y(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stNumPoints(p: *const u8, l: u32) -> i32 {
    opt_int(accessors::st_num_points(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stIsValid(p: *const u8, l: u32) -> i32 {
    boolean(accessors::st_is_valid(s(p, l)))
}

#[unsafe(no_mangle)]
pub extern "C" fn k_stSimplify(p: *const u8, l: u32, tolerance: f64) -> i32 {
    blob(accessors::st_simplify(s(p, l), tolerance))
}

// ============================================================= aggregates
//
// Accumulators live in this module and are addressed by an opaque handle, so
// the host can key them off whatever aggregate context it has. The browser
// adapters hold a JS object instead; the shape is otherwise identical.

/// Aggregate kind, as passed to [`k_agg_new`].
const AGG_UNION: i32 = 0;
const AGG_MVT: i32 = 1;

enum Agg {
    #[cfg(feature = "overlay")]
    Union(kenro::functions::overlay::UnionAggregate),
    #[cfg(feature = "mvt")]
    Mvt(kenro::functions::mvt::MvtAggregate),
}

static AGGS: Slot<Vec<Option<Agg>>> = Slot::new(Vec::new());

fn aggs() -> &'static mut Vec<Option<Agg>> {
    AGGS.get()
}

/// Create an accumulator. Returns a handle, or `-1` for an unknown kind (a
/// feature compiled out).
#[unsafe(no_mangle)]
pub extern "C" fn k_agg_new(kind: i32) -> i32 {
    let agg = match kind {
        #[cfg(feature = "overlay")]
        AGG_UNION => Agg::Union(kenro::functions::overlay::UnionAggregate::new()),
        #[cfg(feature = "mvt")]
        AGG_MVT => Agg::Mvt(kenro::functions::mvt::MvtAggregate::new()),
        _ => return -1,
    };
    let slot = aggs().iter().position(Option::is_none);
    match slot {
        Some(i) => {
            aggs()[i] = Some(agg);
            i as i32
        }
        None => {
            aggs().push(Some(agg));
            (aggs().len() - 1) as i32
        }
    }
}

fn agg_gone() -> i32 {
    set_out(b"kenro: aggregate accumulator already finished".to_vec());
    ERR
}

#[cfg(feature = "overlay")]
#[unsafe(no_mangle)]
pub extern "C" fn k_agg_union_step(h: i32, p: *const u8, l: u32) -> i32 {
    match aggs().get_mut(h as usize).and_then(Option::as_mut) {
        Some(Agg::Union(a)) => match a.step(s(p, l)) {
            Ok(()) => OK,
            Err(e) => fail(e),
        },
        _ => agg_gone(),
    }
}

/// `ST_AsMVT(geom [, name [, extent [, props_json]]])`. Trailing arguments the
/// SQL call omits are marked absent with a `has_*` flag, so one export serves
/// all four arities.
#[cfg(feature = "mvt")]
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn k_agg_mvt_step(
    h: i32,
    gp: *const u8,
    gl: u32,
    has_name: i32,
    np: *const u8,
    nl: u32,
    has_extent: i32,
    extent: i32,
    has_props: i32,
    pp: *const u8,
    pl: u32,
) -> i32 {
    let name = if has_name != 0 {
        Some(try_str!(np, nl))
    } else {
        None
    };
    let props = if has_props != 0 {
        Some(try_str!(pp, pl))
    } else {
        None
    };
    let extent = if has_extent != 0 { Some(extent) } else { None };
    match aggs().get_mut(h as usize).and_then(Option::as_mut) {
        Some(Agg::Mvt(a)) => match a.step(s(gp, gl), name, extent, props) {
            Ok(()) => OK,
            Err(e) => fail(e),
        },
        _ => agg_gone(),
    }
}

/// Finish an accumulator and release its handle. `NULL` = zero rows
/// aggregated.
#[unsafe(no_mangle)]
pub extern "C" fn k_agg_finish(h: i32) -> i32 {
    let Some(slot) = aggs().get_mut(h as usize) else {
        return agg_gone();
    };
    match slot.take() {
        #[cfg(feature = "overlay")]
        Some(Agg::Union(a)) => opt_blob(a.finish()),
        #[cfg(feature = "mvt")]
        Some(Agg::Mvt(a)) => opt_blob(a.finish()),
        None => agg_gone(),
    }
}

/// Release a handle without finishing it (aborted statement).
#[unsafe(no_mangle)]
pub extern "C" fn k_agg_drop(h: i32) {
    if let Some(slot) = aggs().get_mut(h as usize) {
        *slot = None;
    }
}

// =============================================================== manifest

fn kind_str(k: manifest::Kind) -> &'static str {
    use manifest::Kind;
    match k {
        Kind::Blob => "blob",
        Kind::Text => "text",
        Kind::Int => "int",
        Kind::I64 => "i64",
        Kind::Real => "real",
        Kind::Bool => "bool",
        Kind::OptReal => "opt_real",
        Kind::OptI64 => "opt_i64",
        Kind::OptBlob => "opt_blob",
        Kind::TextOrInt => "text_or_int",
    }
}

fn json_str(out: &mut String, v: &str) {
    out.push('"');
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_kinds(out: &mut String, kinds: &[manifest::Kind]) {
    out.push('[');
    for (i, k) in kinds.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(out, kind_str(*k));
    }
    out.push(']');
}

/// The function catalog as JSON, resolved for the compiled feature set: the
/// host derives every registration from this and hard-codes no names of its
/// own. Same schema as `kenro_wasm::manifest`, plus an `agg_kind` column for
/// [`k_agg_new`].
///
/// Result lands in the OUT buffer like any other TEXT return.
#[unsafe(no_mangle)]
pub extern "C" fn k_manifest() -> i32 {
    let mut j = String::with_capacity(8192);
    j.push_str("{\"functions\":[");
    for (i, e) in manifest::active_functions().enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push_str("{\"sql_name\":");
        json_str(&mut j, e.sql_name);
        j.push_str(",\"export\":");
        json_str(&mut j, &format!("k_{}", e.export));
        j.push_str(",\"args\":");
        json_kinds(&mut j, e.args);
        j.push_str(",\"ret\":");
        json_str(&mut j, kind_str(e.ret));
        j.push('}');
    }
    j.push_str("],\"aggregates\":[");
    for (i, e) in manifest::active_aggregates().enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push_str("{\"sql_name\":");
        json_str(&mut j, e.sql_name);
        j.push_str(",\"agg_kind\":");
        j.push_str(match e.ctor_export {
            "UnionAgg" => "0",
            _ => "1",
        });
        j.push_str(",\"args\":");
        json_kinds(&mut j, e.args);
        j.push('}');
    }
    j.push_str("],\"stubs\":[");
    for (i, stub) in manifest::active_stubs().iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push_str("{\"name\":");
        json_str(&mut j, stub.name);
        j.push_str(",\"hint\":");
        json_str(&mut j, stub.hint);
        j.push_str(",\"arities\":[");
        for (n, a) in manifest::stub_arities(stub.name).iter().enumerate() {
            if n > 0 {
                j.push(',');
            }
            j.push_str(&a.to_string());
        }
        j.push_str("]}");
    }
    j.push_str("]}");
    set_out(j.into_bytes());
    OK
}

#[cfg(test)]
mod tests;
