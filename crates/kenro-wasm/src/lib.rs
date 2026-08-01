//! WASM bindings over kenro's pure core. No SQLite is compiled here:
//! browser/Node SQLite hosts (the official SQLite WASM build, sql.js,
//! wa-sqlite) register these exports as JS-level user-defined functions via
//! the adapters in `js/src/`.
//!
//! One export per (SQL function, arity) — wasm-bindgen has no overloading;
//! the mapping lives in `kenro::functions::manifest` and is served to JS by
//! [`manifest`]. NULL-strictness lives in the JS adapters: SQL NULL never
//! reaches this crate.

use wasm_bindgen::prelude::*;

use kenro::functions::{accessors, io, manifest, predicates, rtree};
use kenro::geom;

/// kenro::Error → JS exception. The `kenro: `-prefixed message is preserved
/// so SQL error text matches the rusqlite binding exactly.
fn err(e: kenro::Error) -> JsError {
    JsError::new(&e.to_string())
}

type R<T> = Result<T, JsError>;

// ---- Decoded handle ----

/// A geometry decoded once and kept in wasm memory.
///
/// Every function in this module takes a blob and decodes it, because that is
/// what a SQLite UDF receives — bytes, per row, with nowhere to cache. A JS
/// host is not bound by that: a window query decodes one search geometry and
/// tests it against thousands of candidates, and only the candidates are
/// genuinely new each time.
///
/// The predicates below are the same code paths as their blob counterparts
/// (`kenro::functions::predicates::decoded`), so results are identical by
/// construction — this trades JS-side memory management for the decode.
///
/// **The caller must call `free()`**: wasm-bindgen cannot drop this for you,
/// and a leaked handle keeps its geometry alive in the wasm heap for the life
/// of the isolate. Prefer `try { … } finally { g.free() }`.
#[wasm_bindgen]
pub struct Prepared {
    inner: geom::Geom,
}

#[wasm_bindgen]
impl Prepared {
    /// Decode any geometry blob kenro produces (internal, WKB or GeoPackage).
    #[wasm_bindgen(js_name = fromBlob)]
    pub fn from_blob(blob: &[u8]) -> R<Prepared> {
        Ok(Prepared {
            inner: geom::decode_auto(blob).map_err(err)?,
        })
    }

    #[wasm_bindgen(js_name = fromText)]
    pub fn from_text(wkt: &str, srid: i32) -> R<Prepared> {
        Ok(Prepared {
            inner: geom::decode_wkt(wkt, srid).map_err(err)?,
        })
    }

    #[wasm_bindgen(js_name = stIntersects)]
    pub fn st_intersects(&self, other: &Prepared) -> R<bool> {
        predicates::decoded::st_intersects(&self.inner, &other.inner).map_err(err)
    }

    #[wasm_bindgen(js_name = stContains)]
    pub fn st_contains(&self, other: &Prepared) -> R<bool> {
        predicates::decoded::st_contains(&self.inner, &other.inner).map_err(err)
    }

    #[wasm_bindgen(js_name = stWithin)]
    pub fn st_within(&self, other: &Prepared) -> R<bool> {
        predicates::decoded::st_within(&self.inner, &other.inner).map_err(err)
    }

    #[wasm_bindgen(js_name = stCovers)]
    pub fn st_covers(&self, other: &Prepared) -> R<bool> {
        predicates::decoded::st_covers(&self.inner, &other.inner).map_err(err)
    }

    #[wasm_bindgen(js_name = stDistance)]
    pub fn st_distance(&self, other: &Prepared) -> R<Option<f64>> {
        predicates::decoded::st_distance(&self.inner, &other.inner).map_err(err)
    }

    #[wasm_bindgen(js_name = stDwithin)]
    pub fn st_dwithin(&self, other: &Prepared, d: f64) -> R<bool> {
        predicates::decoded::st_dwithin(&self.inner, &other.inner, d).map_err(err)
    }

    // ---- Output, without a round trip through a blob ----

    #[wasm_bindgen(js_name = stSrid)]
    pub fn st_srid(&self) -> i32 {
        self.inner.srid
    }

    #[cfg(feature = "geojson")]
    #[wasm_bindgen(js_name = stAsGeojson)]
    pub fn st_as_geojson(&self) -> R<String> {
        kenro::functions::geojson::decoded::st_as_geojson(&self.inner, None).map_err(err)
    }

    #[cfg(feature = "geojson")]
    #[wasm_bindgen(js_name = stAsGeojsonDigits)]
    pub fn st_as_geojson_digits(&self, digits: i32) -> R<String> {
        kenro::functions::geojson::decoded::st_as_geojson(&self.inner, Some(digits as i64))
            .map_err(err)
    }

    #[wasm_bindgen(js_name = stAsText)]
    pub fn st_as_text(&self) -> R<String> {
        io::decoded::st_as_text(&self.inner).map_err(err)
    }

    #[wasm_bindgen(js_name = stAsBinary)]
    pub fn st_as_binary(&self) -> R<Vec<u8>> {
        io::decoded::st_as_binary(&self.inner).map_err(err)
    }

    /// Storage-grade GeoPackage blob — what to write back to SQLite.
    #[wasm_bindgen(js_name = stAsGpb)]
    pub fn st_as_gpb(&self) -> R<Vec<u8>> {
        io::decoded::st_as_gpb(&self.inner).map_err(err)
    }

    /// Reproject into a **new handle**, which the caller must also free.
    /// (`self` is shared, so this cannot reproject in place.)
    #[cfg(feature = "transform")]
    #[wasm_bindgen(js_name = stTransform)]
    pub fn st_transform(&self, to_srid: i32) -> R<Prepared> {
        let mut out = self.inner.clone();
        kenro::functions::transform::decoded::st_transform_in_place(&mut out, to_srid)
            .map_err(err)?;
        Ok(Prepared { inner: out })
    }
}

// ---- Geometry I/O ----

#[wasm_bindgen(js_name = stGeomFromText)]
pub fn st_geom_from_text(wkt: &str) -> R<Vec<u8>> {
    io::st_geom_from_text(wkt, None).map_err(err)
}

#[wasm_bindgen(js_name = stGeomFromTextSrid)]
pub fn st_geom_from_text_srid(wkt: &str, srid: i32) -> R<Vec<u8>> {
    io::st_geom_from_text(wkt, Some(srid)).map_err(err)
}

#[wasm_bindgen(js_name = stGeomFromWkb)]
pub fn st_geom_from_wkb(wkb: &[u8]) -> R<Vec<u8>> {
    io::st_geom_from_wkb(wkb, None).map_err(err)
}

#[wasm_bindgen(js_name = stGeomFromWkbSrid)]
pub fn st_geom_from_wkb_srid(wkb: &[u8], srid: i32) -> R<Vec<u8>> {
    io::st_geom_from_wkb(wkb, Some(srid)).map_err(err)
}

#[wasm_bindgen(js_name = stGeomFromGpb)]
pub fn st_geom_from_gpb(gpb: &[u8]) -> R<Vec<u8>> {
    io::st_geom_from_gpb(gpb).map_err(err)
}

#[wasm_bindgen(js_name = stAsText)]
pub fn st_as_text(geom: &[u8]) -> R<String> {
    io::st_as_text(geom).map_err(err)
}

#[wasm_bindgen(js_name = stAsBinary)]
pub fn st_as_binary(geom: &[u8]) -> R<Vec<u8>> {
    io::st_as_binary(geom).map_err(err)
}

#[wasm_bindgen(js_name = stAsGpb)]
pub fn st_as_gpb(geom: &[u8]) -> R<Vec<u8>> {
    io::st_as_gpb(geom).map_err(err)
}

// ---- SRID ----

#[wasm_bindgen(js_name = stSetSrid)]
pub fn st_set_srid(geom: &[u8], srid: i32) -> R<Vec<u8>> {
    io::st_set_srid(geom, srid).map_err(err)
}

#[wasm_bindgen(js_name = stSrid)]
pub fn st_srid(geom: &[u8]) -> R<i32> {
    io::st_srid(geom).map_err(err)
}

// ---- Predicates & measures ----

#[wasm_bindgen(js_name = stIntersects)]
pub fn st_intersects(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_intersects(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stContains)]
pub fn st_contains(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_contains(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stWithin)]
pub fn st_within(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_within(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stDisjoint)]
pub fn st_disjoint(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_disjoint(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stTouches)]
pub fn st_touches(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_touches(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stCrosses)]
pub fn st_crosses(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_crosses(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stOverlaps)]
pub fn st_overlaps(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_overlaps(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stEquals)]
pub fn st_equals(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_equals(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stCovers)]
pub fn st_covers(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_covers(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stCoveredBy)]
pub fn st_covered_by(a: &[u8], b: &[u8]) -> R<bool> {
    predicates::st_covered_by(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stRelate)]
pub fn st_relate(a: &[u8], b: &[u8]) -> R<String> {
    predicates::st_relate(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stRelatePattern)]
pub fn st_relate_pattern(a: &[u8], b: &[u8], pattern: &str) -> R<bool> {
    predicates::st_relate_pattern(a, b, pattern).map_err(err)
}

#[wasm_bindgen(js_name = stDistance)]
pub fn st_distance(a: &[u8], b: &[u8]) -> R<Option<f64>> {
    predicates::st_distance(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stDwithin)]
pub fn st_dwithin(a: &[u8], b: &[u8], d: f64) -> R<bool> {
    predicates::st_dwithin(a, b, d).map_err(err)
}

// ---- Measures ----

#[wasm_bindgen(js_name = stClosestPoint)]
pub fn st_closest_point(a: &[u8], b: &[u8]) -> R<Option<Vec<u8>>> {
    kenro::functions::measures::st_closest_point(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stLineInterpolatePoint)]
pub fn st_line_interpolate_point(a: &[u8], fraction: f64) -> R<Vec<u8>> {
    kenro::functions::measures::st_line_interpolate_point(a, fraction).map_err(err)
}

#[wasm_bindgen(js_name = stLineLocatePoint)]
pub fn st_line_locate_point(a: &[u8], b: &[u8]) -> R<f64> {
    kenro::functions::measures::st_line_locate_point(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stHausdorffDistance)]
pub fn st_hausdorff_distance(a: &[u8], b: &[u8]) -> R<f64> {
    kenro::functions::measures::st_hausdorff_distance(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stFrechetDistance)]
pub fn st_frechet_distance(a: &[u8], b: &[u8]) -> R<f64> {
    kenro::functions::measures::st_frechet_distance(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stAzimuth)]
pub fn st_azimuth(a: &[u8], b: &[u8]) -> R<Option<f64>> {
    kenro::functions::measures::st_azimuth(a, b).map_err(err)
}

// ---- GeoPackage R-tree ----

#[wasm_bindgen(js_name = stMinX)]
pub fn st_min_x(geom: &[u8]) -> R<Option<f64>> {
    rtree::st_min_x(geom).map_err(err)
}

#[wasm_bindgen(js_name = stMaxX)]
pub fn st_max_x(geom: &[u8]) -> R<Option<f64>> {
    rtree::st_max_x(geom).map_err(err)
}

#[wasm_bindgen(js_name = stMinY)]
pub fn st_min_y(geom: &[u8]) -> R<Option<f64>> {
    rtree::st_min_y(geom).map_err(err)
}

#[wasm_bindgen(js_name = stMaxY)]
pub fn st_max_y(geom: &[u8]) -> R<Option<f64>> {
    rtree::st_max_y(geom).map_err(err)
}

#[wasm_bindgen(js_name = stIsEmpty)]
pub fn st_is_empty(geom: &[u8]) -> R<bool> {
    rtree::st_is_empty(geom).map_err(err)
}

// ---- CRS transform ----

#[cfg(feature = "transform")]
#[wasm_bindgen(js_name = stTransform)]
pub fn st_transform(geom: &[u8], to_srid: i32) -> R<Vec<u8>> {
    kenro::functions::transform::st_transform(geom, to_srid).map_err(err)
}

// ---- GeoJSON ----

#[cfg(feature = "geojson")]
#[wasm_bindgen(js_name = stAsGeojson)]
pub fn st_as_geojson(geom: &[u8]) -> R<String> {
    kenro::functions::geojson::st_as_geojson(geom, None).map_err(err)
}

#[cfg(feature = "geojson")]
#[wasm_bindgen(js_name = stAsGeojsonDigits)]
pub fn st_as_geojson_digits(geom: &[u8], digits: i32) -> R<String> {
    kenro::functions::geojson::st_as_geojson(geom, Some(digits as i64)).map_err(err)
}

#[cfg(feature = "geojson")]
#[wasm_bindgen(js_name = stGeomFromGeojson)]
pub fn st_geom_from_geojson(text: &str) -> R<Vec<u8>> {
    kenro::functions::geojson::st_geom_from_geojson(text).map_err(err)
}

// ---- H3 (BigInt on the JS side) ----

#[cfg(feature = "h3")]
#[wasm_bindgen(js_name = h3LatlngToCell)]
pub fn h3_latlng_to_cell(geom: &[u8], res: i32) -> R<i64> {
    kenro::functions::h3::h3_latlng_to_cell(geom, res as i64).map_err(err)
}

#[cfg(feature = "h3")]
#[wasm_bindgen(js_name = h3CellToParent)]
pub fn h3_cell_to_parent(cell: i64, res: i32) -> R<i64> {
    kenro::functions::h3::h3_cell_to_parent(cell, res as i64).map_err(err)
}

#[cfg(feature = "h3")]
#[wasm_bindgen(js_name = h3CellToString)]
pub fn h3_cell_to_string(cell: i64) -> R<String> {
    kenro::functions::h3::h3_cell_to_string(cell).map_err(err)
}

#[cfg(feature = "h3")]
#[wasm_bindgen(js_name = h3StringToCell)]
pub fn h3_string_to_cell(s: &str) -> R<i64> {
    kenro::functions::h3::h3_string_to_cell(s).map_err(err)
}

// ---- Overlay ----

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stIntersection)]
pub fn st_intersection(a: &[u8], b: &[u8]) -> R<Vec<u8>> {
    kenro::functions::overlay::st_intersection(a, b).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stDifference)]
pub fn st_difference(a: &[u8], b: &[u8]) -> R<Vec<u8>> {
    kenro::functions::overlay::st_difference(a, b).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stSymDifference)]
pub fn st_sym_difference(a: &[u8], b: &[u8]) -> R<Vec<u8>> {
    kenro::functions::overlay::st_sym_difference(a, b).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stUnion)]
pub fn st_union(a: &[u8], b: &[u8]) -> R<Vec<u8>> {
    kenro::functions::overlay::st_union(a, b).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stBuffer)]
pub fn st_buffer(geom: &[u8], distance: f64) -> R<Vec<u8>> {
    kenro::functions::overlay::st_buffer(geom, distance, None).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stMakeValid)]
pub fn st_make_valid(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::overlay::st_make_valid(geom).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stBufferOpts)]
pub fn st_buffer_opts(geom: &[u8], distance: f64, options: &str) -> R<Vec<u8>> {
    kenro::functions::overlay::st_buffer(geom, distance, Some(options)).map_err(err)
}

// ---- Processing + affine ----

#[wasm_bindgen(js_name = stConvexHull)]
pub fn st_convex_hull(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::processing::st_convex_hull(geom).map_err(err)
}

#[wasm_bindgen(js_name = stPointOnSurface)]
pub fn st_point_on_surface(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::processing::st_point_on_surface(geom).map_err(err)
}

#[wasm_bindgen(js_name = stSimplifyVw)]
pub fn st_simplify_vw(geom: &[u8], tolerance: f64) -> R<Vec<u8>> {
    kenro::functions::processing::st_simplify_vw(geom, tolerance).map_err(err)
}

#[wasm_bindgen(js_name = stChaikinSmoothing)]
pub fn st_chaikin_smoothing(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::processing::st_chaikin_smoothing(geom, 1).map_err(err)
}

#[wasm_bindgen(js_name = stChaikinSmoothingN)]
pub fn st_chaikin_smoothing_n(geom: &[u8], iterations: i32) -> R<Vec<u8>> {
    kenro::functions::processing::st_chaikin_smoothing(geom, iterations as i64).map_err(err)
}

#[wasm_bindgen(js_name = stRemoveRepeatedPoints)]
pub fn st_remove_repeated_points(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::processing::st_remove_repeated_points(geom).map_err(err)
}

#[wasm_bindgen(js_name = stOrientedEnvelope)]
pub fn st_oriented_envelope(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::processing::st_oriented_envelope(geom).map_err(err)
}

#[wasm_bindgen(js_name = stRotate)]
pub fn st_rotate(geom: &[u8], radians: f64) -> R<Vec<u8>> {
    kenro::functions::affine::st_rotate(geom, radians).map_err(err)
}

#[wasm_bindgen(js_name = stRotateXY)]
pub fn st_rotate_xy(geom: &[u8], radians: f64, x0: f64, y0: f64) -> R<Vec<u8>> {
    kenro::functions::affine::st_rotate_xy(geom, radians, x0, y0).map_err(err)
}

#[wasm_bindgen(js_name = stTranslate)]
pub fn st_translate(geom: &[u8], dx: f64, dy: f64) -> R<Vec<u8>> {
    kenro::functions::affine::st_translate(geom, dx, dy).map_err(err)
}

#[wasm_bindgen(js_name = stScale)]
pub fn st_scale(geom: &[u8], xfactor: f64, yfactor: f64) -> R<Vec<u8>> {
    kenro::functions::affine::st_scale(geom, xfactor, yfactor).map_err(err)
}

// ---- Constructors ----

#[wasm_bindgen(js_name = stMakePoint)]
pub fn st_make_point(x: f64, y: f64) -> R<Vec<u8>> {
    io::st_make_point(x, y).map_err(err)
}

#[wasm_bindgen(js_name = stPoint)]
pub fn st_point(x: f64, y: f64) -> R<Vec<u8>> {
    io::st_point(x, y, None).map_err(err)
}

#[wasm_bindgen(js_name = stPointSrid)]
pub fn st_point_srid(x: f64, y: f64, srid: i32) -> R<Vec<u8>> {
    io::st_point(x, y, Some(srid)).map_err(err)
}

#[wasm_bindgen(js_name = stMakeEnvelope)]
pub fn st_make_envelope(xmin: f64, ymin: f64, xmax: f64, ymax: f64) -> R<Vec<u8>> {
    io::st_make_envelope(xmin, ymin, xmax, ymax, None).map_err(err)
}

#[wasm_bindgen(js_name = stMakeEnvelopeSrid)]
pub fn st_make_envelope_srid(xmin: f64, ymin: f64, xmax: f64, ymax: f64, srid: i32) -> R<Vec<u8>> {
    io::st_make_envelope(xmin, ymin, xmax, ymax, Some(srid)).map_err(err)
}

#[wasm_bindgen(js_name = gpkgIsAssignable)]
pub fn gpkg_is_assignable(expected: &str, actual: &str) -> R<bool> {
    rtree::gpkg_is_assignable(expected, actual).map_err(err)
}

// ---- Accessors ----

#[wasm_bindgen(js_name = stArea)]
pub fn st_area(geom: &[u8]) -> R<f64> {
    accessors::st_area(geom).map_err(err)
}

#[wasm_bindgen(js_name = stNPoints)]
pub fn st_npoints(geom: &[u8]) -> R<i64> {
    accessors::st_npoints(geom).map_err(err)
}

#[wasm_bindgen(js_name = stPerimeter)]
pub fn st_perimeter(geom: &[u8]) -> R<f64> {
    accessors::st_perimeter(geom).map_err(err)
}

#[wasm_bindgen(js_name = stGeometryType)]
pub fn st_geometry_type(geom: &[u8]) -> R<String> {
    accessors::st_geometry_type(geom).map_err(err)
}

#[wasm_bindgen(js_name = stNumGeometries)]
pub fn st_num_geometries(geom: &[u8]) -> R<i64> {
    accessors::st_num_geometries(geom).map_err(err)
}

#[wasm_bindgen(js_name = stGeometryN)]
pub fn st_geometry_n(geom: &[u8], n: i32) -> R<Option<Vec<u8>>> {
    accessors::st_geometry_n(geom, n as i64).map_err(err)
}

#[wasm_bindgen(js_name = stStartPoint)]
pub fn st_start_point(geom: &[u8]) -> R<Option<Vec<u8>>> {
    accessors::st_start_point(geom).map_err(err)
}

#[wasm_bindgen(js_name = stEndPoint)]
pub fn st_end_point(geom: &[u8]) -> R<Option<Vec<u8>>> {
    accessors::st_end_point(geom).map_err(err)
}

#[wasm_bindgen(js_name = stPointN)]
pub fn st_point_n(geom: &[u8], n: i32) -> R<Option<Vec<u8>>> {
    accessors::st_point_n(geom, n as i64).map_err(err)
}

#[wasm_bindgen(js_name = stReverse)]
pub fn st_reverse(geom: &[u8]) -> R<Vec<u8>> {
    accessors::st_reverse(geom).map_err(err)
}

#[wasm_bindgen(js_name = stLength)]
pub fn st_length(geom: &[u8]) -> R<f64> {
    accessors::st_length(geom).map_err(err)
}

#[wasm_bindgen(js_name = stCentroid)]
pub fn st_centroid(geom: &[u8]) -> R<Vec<u8>> {
    accessors::st_centroid(geom).map_err(err)
}

#[wasm_bindgen(js_name = stEnvelope)]
pub fn st_envelope(geom: &[u8]) -> R<Vec<u8>> {
    accessors::st_envelope(geom).map_err(err)
}

#[wasm_bindgen(js_name = stX)]
pub fn st_x(geom: &[u8]) -> R<Option<f64>> {
    accessors::st_x(geom).map_err(err)
}

#[wasm_bindgen(js_name = stY)]
pub fn st_y(geom: &[u8]) -> R<Option<f64>> {
    accessors::st_y(geom).map_err(err)
}

#[wasm_bindgen(js_name = stNumPoints)]
pub fn st_num_points(geom: &[u8]) -> R<Option<i64>> {
    accessors::st_num_points(geom).map_err(err)
}

#[wasm_bindgen(js_name = stIsValid)]
pub fn st_is_valid(geom: &[u8]) -> R<bool> {
    accessors::st_is_valid(geom).map_err(err)
}

#[wasm_bindgen(js_name = stSimplify)]
pub fn st_simplify(geom: &[u8], tolerance: f64) -> R<Vec<u8>> {
    accessors::st_simplify(geom, tolerance).map_err(err)
}

// ---- MVT ----

#[cfg(feature = "mvt")]
#[wasm_bindgen(js_name = stAsMvtGeom)]
pub fn st_as_mvt_geom(geom: &[u8], bounds: &[u8]) -> R<Option<Vec<u8>>> {
    kenro::functions::mvt::st_as_mvt_geom(geom, bounds, None, None, None).map_err(err)
}

#[cfg(feature = "mvt")]
#[wasm_bindgen(js_name = stAsMvtGeomExtent)]
pub fn st_as_mvt_geom_extent(geom: &[u8], bounds: &[u8], extent: i32) -> R<Option<Vec<u8>>> {
    kenro::functions::mvt::st_as_mvt_geom(geom, bounds, Some(extent), None, None).map_err(err)
}

#[cfg(feature = "mvt")]
#[wasm_bindgen(js_name = stAsMvtGeomBuffer)]
pub fn st_as_mvt_geom_buffer(
    geom: &[u8],
    bounds: &[u8],
    extent: i32,
    buffer: i32,
) -> R<Option<Vec<u8>>> {
    kenro::functions::mvt::st_as_mvt_geom(geom, bounds, Some(extent), Some(buffer), None)
        .map_err(err)
}

#[cfg(feature = "mvt")]
#[wasm_bindgen(js_name = stAsMvtGeomClip)]
pub fn st_as_mvt_geom_clip(
    geom: &[u8],
    bounds: &[u8],
    extent: i32,
    buffer: i32,
    clip: i32,
) -> R<Option<Vec<u8>>> {
    kenro::functions::mvt::st_as_mvt_geom(geom, bounds, Some(extent), Some(buffer), Some(clip))
        .map_err(err)
}

// ---- Aggregates (accumulator classes; JS adapters drive step/finish) ----

/// Accumulator for the `ST_Union(geom)` aggregate.
#[cfg(feature = "overlay")]
#[wasm_bindgen]
pub struct UnionAgg {
    inner: Option<kenro::functions::overlay::UnionAggregate>,
}

#[cfg(feature = "overlay")]
#[wasm_bindgen]
impl UnionAgg {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> UnionAgg {
        UnionAgg {
            inner: Some(kenro::functions::overlay::UnionAggregate::new()),
        }
    }

    pub fn step(&mut self, geom: &[u8]) -> Result<(), JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("kenro: ST_Union accumulator already finished"))?
            .step(geom)
            .map_err(err)
    }

    /// `undefined` = SQL NULL (zero rows aggregated).
    pub fn finish(&mut self) -> Result<Option<Vec<u8>>, JsError> {
        self.inner
            .take()
            .ok_or_else(|| JsError::new("kenro: ST_Union accumulator already finished"))?
            .finish()
            .map_err(err)
    }
}

/// Accumulator for the `ST_AsMVT(geom [, name [, extent [, props_json]]])`
/// aggregate. Trailing arguments the SQL call omits arrive as `undefined`
/// (→ `None`), so one class serves all four arities.
#[cfg(feature = "mvt")]
#[wasm_bindgen]
pub struct MvtAgg {
    inner: Option<kenro::functions::mvt::MvtAggregate>,
}

#[cfg(feature = "mvt")]
#[wasm_bindgen]
impl MvtAgg {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> MvtAgg {
        MvtAgg {
            inner: Some(kenro::functions::mvt::MvtAggregate::new()),
        }
    }

    pub fn step(
        &mut self,
        geom: &[u8],
        name: Option<String>,
        extent: Option<i32>,
        props: Option<String>,
    ) -> Result<(), JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("kenro: ST_AsMVT accumulator already finished"))?
            .step(geom, name.as_deref(), extent, props.as_deref())
            .map_err(err)
    }

    /// `undefined` = SQL NULL (zero rows aggregated).
    pub fn finish(&mut self) -> Result<Option<Vec<u8>>, JsError> {
        self.inner
            .take()
            .ok_or_else(|| JsError::new("kenro: ST_AsMVT accumulator already finished"))?
            .finish()
            .map_err(err)
    }
}

// ---- Manifest ----

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

/// The function catalog as JSON, resolved for the compiled feature set. The
/// JS adapters iterate this — they contain no function names of their own.
#[wasm_bindgen]
pub fn manifest() -> String {
    let functions: Vec<serde_json::Value> = manifest::active_functions()
        .map(|e| {
            serde_json::json!({
                "sql_name": e.sql_name,
                "export": e.export,
                "args": e.args.iter().map(|k| kind_str(*k)).collect::<Vec<_>>(),
                "ret": kind_str(e.ret),
                "uses_i64": e.args.contains(&manifest::Kind::I64)
                    || matches!(e.ret, manifest::Kind::I64),
            })
        })
        .collect();
    let aggregates: Vec<serde_json::Value> = manifest::active_aggregates()
        .map(|e| {
            serde_json::json!({
                "sql_name": e.sql_name,
                "ctor_export": e.ctor_export,
                "args": e.args.iter().map(|k| kind_str(*k)).collect::<Vec<_>>(),
                "uses_i64": e.args.contains(&manifest::Kind::I64),
            })
        })
        .collect();
    let stubs: Vec<serde_json::Value> = manifest::active_stubs()
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "hint": s.hint,
                "arities": manifest::stub_arities(s.name),
            })
        })
        .collect();
    serde_json::json!({ "functions": functions, "aggregates": aggregates, "stubs": stubs })
        .to_string()
}

#[cfg(test)]
mod tests {
    /// Every manifest export name must correspond to a real export above.
    /// (Native-target test; the list is the manifest's `export` column.)
    #[test]
    fn manifest_exports_resolve() {
        let known = [
            "stGeomFromText",
            "stGeomFromTextSrid",
            "stGeomFromWkb",
            "stGeomFromWkbSrid",
            "stGeomFromGpb",
            "stAsText",
            "stAsBinary",
            "stAsGpb",
            "stSetSrid",
            "stSrid",
            "stIntersects",
            "stContains",
            "stWithin",
            "stDisjoint",
            "stTouches",
            "stCrosses",
            "stOverlaps",
            "stEquals",
            "stCovers",
            "stCoveredBy",
            "stRelate",
            "stRelatePattern",
            "stDistance",
            "stDwithin",
            "stMinX",
            "stMaxX",
            "stMinY",
            "stMaxY",
            "stIsEmpty",
            "stTransform",
            "stAsGeojson",
            "stAsGeojsonDigits",
            "stGeomFromGeojson",
            "h3LatlngToCell",
            "h3CellToParent",
            "h3CellToString",
            "h3StringToCell",
            "stIntersection",
            "stDifference",
            "stSymDifference",
            "stUnion",
            "stBuffer",
            "stBufferOpts",
            "stConvexHull",
            "stPointOnSurface",
            "stSimplifyVw",
            "stChaikinSmoothing",
            "stChaikinSmoothingN",
            "stRemoveRepeatedPoints",
            "stOrientedEnvelope",
            "stRotate",
            "stRotateXY",
            "stTranslate",
            "stScale",
            "stClosestPoint",
            "stLineInterpolatePoint",
            "stLineLocatePoint",
            "stHausdorffDistance",
            "stFrechetDistance",
            "stAzimuth",
            "stMakePoint",
            "stPoint",
            "stPointSrid",
            "stMakeEnvelope",
            "stMakeEnvelopeSrid",
            "gpkgIsAssignable",
            "stNPoints",
            "stPerimeter",
            "stGeometryType",
            "stNumGeometries",
            "stGeometryN",
            "stStartPoint",
            "stEndPoint",
            "stPointN",
            "stReverse",
            "stArea",
            "stLength",
            "stCentroid",
            "stEnvelope",
            "stX",
            "stY",
            "stNumPoints",
            "stIsValid",
            "stSimplify",
            "stMakeValid",
            "stAsMvtGeom",
            "stAsMvtGeomExtent",
            "stAsMvtGeomBuffer",
            "stAsMvtGeomClip",
        ];
        for entry in kenro::functions::manifest::active_functions() {
            assert!(
                known.contains(&entry.export),
                "manifest export {} has no wasm-bindgen counterpart",
                entry.export
            );
        }
        let known_aggregates = ["UnionAgg", "MvtAgg"];
        for entry in kenro::functions::manifest::active_aggregates() {
            assert!(
                known_aggregates.contains(&entry.ctor_export),
                "manifest aggregate ctor {} has no wasm-bindgen counterpart",
                entry.ctor_export
            );
        }
    }
}
