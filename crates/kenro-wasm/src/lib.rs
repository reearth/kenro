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

use kenro::functions::{
    accessors, compat, edit, extra, geodesic, io, linear, manifest, misc, predicates, rtree,
    surface, threed, threed_metric, threed_solid,
};
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

// ---- PostGIS compatibility (functions::compat) ----
// Alias spellings (ST_XMin …) reuse the exports above: the manifest maps the
// SQL name, so they add no wasm at all. Only these need code.

#[wasm_bindgen(js_name = stForce2d)]
pub fn st_force_2d(geom: &[u8]) -> R<Vec<u8>> {
    compat::st_force_2d(geom).map_err(err)
}

#[wasm_bindgen(js_name = stAsEwkt)]
pub fn st_as_ewkt(geom: &[u8]) -> R<String> {
    compat::st_as_ewkt(geom).map_err(err)
}

#[wasm_bindgen(js_name = stGeomFromEwkt)]
pub fn st_geom_from_ewkt(text: &str) -> R<Vec<u8>> {
    compat::st_geom_from_ewkt(text).map_err(err)
}

#[wasm_bindgen(js_name = stAsEwkb)]
pub fn st_as_ewkb(geom: &[u8]) -> R<Vec<u8>> {
    compat::st_as_ewkb(geom).map_err(err)
}

#[wasm_bindgen(js_name = stAsHexEwkb)]
pub fn st_as_hex_ewkb(geom: &[u8]) -> R<String> {
    compat::st_as_hex_ewkb(geom).map_err(err)
}

macro_rules! typed_ctor {
    ($fn_text:ident, $export_text:literal, $fn_text_srid:ident, $export_text_srid:literal, $expect:ident) => {
        #[wasm_bindgen(js_name = $export_text)]
        pub fn $fn_text(wkt: &str) -> R<Option<Vec<u8>>> {
            compat::from_text_typed(wkt, None, compat::Expect::$expect).map_err(err)
        }

        #[wasm_bindgen(js_name = $export_text_srid)]
        pub fn $fn_text_srid(wkt: &str, srid: i32) -> R<Option<Vec<u8>>> {
            compat::from_text_typed(wkt, Some(srid), compat::Expect::$expect).map_err(err)
        }
    };
}

typed_ctor!(
    st_point_from_text,
    "stPointFromText",
    st_point_from_text_srid,
    "stPointFromTextSrid",
    Point
);
typed_ctor!(
    st_line_from_text,
    "stLineFromText",
    st_line_from_text_srid,
    "stLineFromTextSrid",
    LineString
);
typed_ctor!(
    st_poly_from_text,
    "stPolyFromText",
    st_poly_from_text_srid,
    "stPolyFromTextSrid",
    Polygon
);
typed_ctor!(
    st_mpoint_from_text,
    "stMPointFromText",
    st_mpoint_from_text_srid,
    "stMPointFromTextSrid",
    MultiPoint
);
typed_ctor!(
    st_mline_from_text,
    "stMLineFromText",
    st_mline_from_text_srid,
    "stMLineFromTextSrid",
    MultiLineString
);
typed_ctor!(
    st_mpoly_from_text,
    "stMPolyFromText",
    st_mpoly_from_text_srid,
    "stMPolyFromTextSrid",
    MultiPolygon
);

macro_rules! typed_ctor_wkb {
    ($fn_wkb:ident, $export_wkb:literal, $fn_wkb_srid:ident, $export_wkb_srid:literal, $expect:ident) => {
        #[wasm_bindgen(js_name = $export_wkb)]
        pub fn $fn_wkb(wkb: &[u8]) -> R<Option<Vec<u8>>> {
            compat::from_wkb_typed(wkb, None, compat::Expect::$expect).map_err(err)
        }

        #[wasm_bindgen(js_name = $export_wkb_srid)]
        pub fn $fn_wkb_srid(wkb: &[u8], srid: i32) -> R<Option<Vec<u8>>> {
            compat::from_wkb_typed(wkb, Some(srid), compat::Expect::$expect).map_err(err)
        }
    };
}

typed_ctor_wkb!(
    st_point_from_wkb,
    "stPointFromWkb",
    st_point_from_wkb_srid,
    "stPointFromWkbSrid",
    Point
);
typed_ctor_wkb!(
    st_line_from_wkb,
    "stLineFromWkb",
    st_line_from_wkb_srid,
    "stLineFromWkbSrid",
    LineString
);
typed_ctor_wkb!(
    st_poly_from_wkb,
    "stPolyFromWkb",
    st_poly_from_wkb_srid,
    "stPolyFromWkbSrid",
    Polygon
);
typed_ctor_wkb!(
    st_mpoint_from_wkb,
    "stMPointFromWkb",
    st_mpoint_from_wkb_srid,
    "stMPointFromWkbSrid",
    MultiPoint
);
typed_ctor_wkb!(
    st_mline_from_wkb,
    "stMLineFromWkb",
    st_mline_from_wkb_srid,
    "stMLineFromWkbSrid",
    MultiLineString
);
typed_ctor_wkb!(
    st_mpoly_from_wkb,
    "stMPolyFromWkb",
    st_mpoly_from_wkb_srid,
    "stMPolyFromWkbSrid",
    MultiPolygon
);

// ---- Structural accessors and editing (functions::edit) ----

#[wasm_bindgen(js_name = stExteriorRing)]
pub fn st_exterior_ring(geom: &[u8]) -> R<Option<Vec<u8>>> {
    edit::st_exterior_ring(geom).map_err(err)
}

#[wasm_bindgen(js_name = stInteriorRingN)]
pub fn st_interior_ring_n(geom: &[u8], n: i32) -> R<Option<Vec<u8>>> {
    edit::st_interior_ring_n(geom, n as i64).map_err(err)
}

#[wasm_bindgen(js_name = stNumInteriorRings)]
pub fn st_num_interior_rings(geom: &[u8]) -> R<Option<i64>> {
    edit::st_num_interior_rings(geom).map_err(err)
}

#[wasm_bindgen(js_name = stNRings)]
pub fn st_nrings(geom: &[u8]) -> R<i64> {
    edit::st_nrings(geom).map_err(err)
}

#[wasm_bindgen(js_name = stBoundary)]
pub fn st_boundary(geom: &[u8]) -> R<Vec<u8>> {
    edit::st_boundary(geom).map_err(err)
}

#[wasm_bindgen(js_name = stIsClosed)]
pub fn st_is_closed(geom: &[u8]) -> R<bool> {
    edit::st_is_closed(geom).map_err(err)
}

#[wasm_bindgen(js_name = stIsRing)]
pub fn st_is_ring(geom: &[u8]) -> R<bool> {
    edit::st_is_ring(geom).map_err(err)
}

#[wasm_bindgen(js_name = stAddPoint)]
pub fn st_add_point(line: &[u8], point: &[u8]) -> R<Option<Vec<u8>>> {
    edit::st_add_point(line, point, None).map_err(err)
}

#[wasm_bindgen(js_name = stAddPointAt)]
pub fn st_add_point_at(line: &[u8], point: &[u8], position: i32) -> R<Option<Vec<u8>>> {
    edit::st_add_point(line, point, Some(position as i64)).map_err(err)
}

#[wasm_bindgen(js_name = stSetPoint)]
pub fn st_set_point(line: &[u8], index: i32, point: &[u8]) -> R<Option<Vec<u8>>> {
    edit::st_set_point(line, index as i64, point).map_err(err)
}

#[wasm_bindgen(js_name = stRemovePoint)]
pub fn st_remove_point(line: &[u8], index: i32) -> R<Option<Vec<u8>>> {
    edit::st_remove_point(line, index as i64).map_err(err)
}

#[wasm_bindgen(js_name = stMakeLine)]
pub fn st_make_line(a: &[u8], b: &[u8]) -> R<Vec<u8>> {
    edit::st_make_line(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stMakePolygon)]
pub fn st_make_polygon(geom: &[u8]) -> R<Vec<u8>> {
    edit::st_make_polygon(geom).map_err(err)
}

#[wasm_bindgen(js_name = stMulti)]
pub fn st_multi(geom: &[u8]) -> R<Vec<u8>> {
    edit::st_multi(geom).map_err(err)
}

#[wasm_bindgen(js_name = stSnapToGrid)]
pub fn st_snap_to_grid(geom: &[u8], size: f64) -> R<Vec<u8>> {
    edit::st_snap_to_grid(geom, size, size).map_err(err)
}

#[wasm_bindgen(js_name = stSnapToGridXy)]
pub fn st_snap_to_grid_xy(geom: &[u8], size_x: f64, size_y: f64) -> R<Vec<u8>> {
    edit::st_snap_to_grid(geom, size_x, size_y).map_err(err)
}

#[wasm_bindgen(js_name = stFlipCoordinates)]
pub fn st_flip_coordinates(geom: &[u8]) -> R<Vec<u8>> {
    edit::st_flip_coordinates(geom).map_err(err)
}

#[wasm_bindgen(js_name = stShiftLongitude)]
pub fn st_shift_longitude(geom: &[u8]) -> R<Vec<u8>> {
    edit::st_shift_longitude(geom).map_err(err)
}

#[wasm_bindgen(js_name = stExpand)]
pub fn st_expand(geom: &[u8], units: f64) -> R<Option<Vec<u8>>> {
    edit::st_expand(geom, units).map_err(err)
}

// ---- Sphere/spheroid measures, dimension, orientation, linear referencing ----

#[wasm_bindgen(js_name = stDistanceSphere)]
pub fn st_distance_sphere(a: &[u8], b: &[u8]) -> R<f64> {
    geodesic::st_distance_sphere(a, b).map_err(err)
}

#[cfg(feature = "spheroid")]
#[wasm_bindgen(js_name = stDistanceSpheroid)]
pub fn st_distance_spheroid(a: &[u8], b: &[u8]) -> R<f64> {
    geodesic::st_distance_spheroid(a, b).map_err(err)
}

#[cfg(feature = "spheroid")]
#[wasm_bindgen(js_name = stDistanceSpheroidOn)]
pub fn st_distance_spheroid_on(a: &[u8], b: &[u8], spheroid: &str) -> R<f64> {
    geodesic::st_distance_spheroid_on(a, b, spheroid).map_err(err)
}

#[cfg(feature = "spheroid")]
#[wasm_bindgen(js_name = stLengthSpheroid)]
pub fn st_length_spheroid(geom: &[u8], spheroid: &str) -> R<f64> {
    geodesic::st_length_spheroid(geom, spheroid).map_err(err)
}

#[wasm_bindgen(js_name = stProject)]
pub fn st_project(geom: &[u8], distance: f64, azimuth: f64) -> R<Vec<u8>> {
    geodesic::st_project(geom, distance, azimuth).map_err(err)
}

#[wasm_bindgen(js_name = stDimension)]
pub fn st_dimension(geom: &[u8]) -> R<i32> {
    accessors::st_dimension(geom).map(|v| v as i32).map_err(err)
}

#[wasm_bindgen(js_name = stCoordDim)]
pub fn st_coord_dim(geom: &[u8]) -> R<i32> {
    threed::st_coord_dim(geom).map(|v| v as i32).map_err(err)
}

#[wasm_bindgen(js_name = stHasZ)]
pub fn st_has_z(geom: &[u8]) -> R<bool> {
    threed::st_has_z(geom).map_err(err)
}

#[wasm_bindgen(js_name = stHasM)]
pub fn st_has_m(geom: &[u8]) -> R<bool> {
    threed::st_has_m(geom).map_err(err)
}

#[wasm_bindgen(js_name = stZ)]
pub fn st_z(geom: &[u8]) -> R<Option<f64>> {
    threed::st_z(geom).map_err(err)
}

#[wasm_bindgen(js_name = stM)]
pub fn st_m(geom: &[u8]) -> R<Option<f64>> {
    threed::st_m(geom).map_err(err)
}

#[wasm_bindgen(js_name = stZMin)]
pub fn st_zmin(geom: &[u8]) -> R<Option<f64>> {
    threed::st_zmin(geom).map_err(err)
}

#[wasm_bindgen(js_name = stZMax)]
pub fn st_zmax(geom: &[u8]) -> R<Option<f64>> {
    threed::st_zmax(geom).map_err(err)
}

#[wasm_bindgen(js_name = stIsValidReason)]
pub fn st_is_valid_reason(geom: &[u8]) -> R<String> {
    accessors::st_is_valid_reason(geom).map_err(err)
}

#[wasm_bindgen(js_name = stForcePolygonCw)]
pub fn st_force_polygon_cw(geom: &[u8]) -> R<Vec<u8>> {
    edit::st_force_polygon_cw(geom).map_err(err)
}

#[wasm_bindgen(js_name = stForcePolygonCcw)]
pub fn st_force_polygon_ccw(geom: &[u8]) -> R<Vec<u8>> {
    edit::st_force_polygon_ccw(geom).map_err(err)
}

#[wasm_bindgen(js_name = stIsPolygonCw)]
pub fn st_is_polygon_cw(geom: &[u8]) -> R<bool> {
    edit::st_is_polygon_cw(geom).map_err(err)
}

#[wasm_bindgen(js_name = stIsPolygonCcw)]
pub fn st_is_polygon_ccw(geom: &[u8]) -> R<bool> {
    edit::st_is_polygon_ccw(geom).map_err(err)
}

#[wasm_bindgen(js_name = stSegmentize)]
pub fn st_segmentize(geom: &[u8], max_length: f64) -> R<Vec<u8>> {
    linear::st_segmentize(geom, max_length).map_err(err)
}

#[wasm_bindgen(js_name = stLineSubstring)]
pub fn st_line_substring(geom: &[u8], from: f64, to: f64) -> R<Option<Vec<u8>>> {
    linear::st_line_substring(geom, from, to).map_err(err)
}

#[wasm_bindgen(js_name = stShortestLine)]
pub fn st_shortest_line(a: &[u8], b: &[u8]) -> R<Option<Vec<u8>>> {
    linear::st_shortest_line(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stLongestLine)]
pub fn st_longest_line(a: &[u8], b: &[u8]) -> R<Option<Vec<u8>>> {
    linear::st_longest_line(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stMaxDistance)]
pub fn st_max_distance(a: &[u8], b: &[u8]) -> R<Option<f64>> {
    linear::st_max_distance(a, b).map_err(err)
}

// ---- Smallest enclosing circle and overlay-powered areal operations ----

#[wasm_bindgen(js_name = stMinimumBoundingRadius)]
pub fn st_minimum_bounding_radius(geom: &[u8]) -> R<Option<f64>> {
    linear::st_minimum_bounding_radius(geom).map_err(err)
}

#[wasm_bindgen(js_name = stMinimumBoundingCircle)]
pub fn st_minimum_bounding_circle(geom: &[u8]) -> R<Option<Vec<u8>>> {
    linear::st_minimum_bounding_circle(geom, 48).map_err(err)
}

#[wasm_bindgen(js_name = stMinimumBoundingCircleSegs)]
pub fn st_minimum_bounding_circle_segs(geom: &[u8], segs: i32) -> R<Option<Vec<u8>>> {
    linear::st_minimum_bounding_circle(geom, segs as i64).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stUnaryUnion)]
pub fn st_unary_union(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::overlay::st_unary_union(geom).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stClipByBox2d)]
pub fn st_clip_by_box_2d(geom: &[u8], box_geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::overlay::st_clip_by_box_2d(geom, box_geom).map_err(err)
}

#[cfg(feature = "overlay")]
#[wasm_bindgen(js_name = stSubdivide)]
pub fn st_subdivide(geom: &[u8], max_vertices: i32) -> R<Vec<u8>> {
    kenro::functions::overlay::st_subdivide(geom, max_vertices as i64).map_err(err)
}

// ---- The rest of the reachable surface (functions::extra) ----

#[wasm_bindgen(js_name = stContainsProperly)]
pub fn st_contains_properly(a: &[u8], b: &[u8]) -> R<bool> {
    extra::st_contains_properly(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stDfullyWithin)]
pub fn st_d_fully_within(a: &[u8], b: &[u8], d: f64) -> R<bool> {
    extra::st_d_fully_within(a, b, d).map_err(err)
}

#[wasm_bindgen(js_name = stRelateMatch)]
pub fn st_relate_match(matrix: &str, pattern: &str) -> R<bool> {
    extra::st_relate_match(matrix, pattern).map_err(err)
}

#[wasm_bindgen(js_name = stAffine)]
pub fn st_affine(geom: &[u8], a: f64, b: f64, d: f64, e: f64, xoff: f64, yoff: f64) -> R<Vec<u8>> {
    extra::st_affine(geom, a, b, d, e, xoff, yoff).map_err(err)
}

/// `ST_Affine`'s 3D form: the upper 3×4 of a 4×4 matrix.
#[wasm_bindgen(js_name = stAffine3d)]
#[allow(clippy::too_many_arguments)]
pub fn st_affine_3d(
    geom: &[u8],
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
    g: f64,
    h: f64,
    i: f64,
    xoff: f64,
    yoff: f64,
    zoff: f64,
) -> R<Vec<u8>> {
    extra::st_affine_3d(geom, a, b, c, d, e, f, g, h, i, xoff, yoff, zoff).map_err(err)
}

// ---- the two SFCGAL measurements (functions::threed_solid) ----

#[wasm_bindgen(js_name = st3dArea)]
pub fn st_3d_area(geom: &[u8]) -> R<f64> {
    threed_solid::st_3d_area(geom).map_err(err)
}

#[wasm_bindgen(js_name = kenroVolume)]
pub fn kenro_volume(geom: &[u8]) -> R<Option<f64>> {
    threed_solid::kenro_volume(geom).map_err(err)
}

// ---- the core-PostGIS 3D metric family (functions::threed_metric) ----

#[wasm_bindgen(js_name = st3dDistance)]
pub fn st_3d_distance(a: &[u8], b: &[u8]) -> R<Option<f64>> {
    threed_metric::st_3d_distance(a, b).map_err(err)
}

#[wasm_bindgen(js_name = st3dMaxDistance)]
pub fn st_3d_max_distance(a: &[u8], b: &[u8]) -> R<Option<f64>> {
    threed_metric::st_3d_max_distance(a, b).map_err(err)
}

#[wasm_bindgen(js_name = st3dIntersects)]
pub fn st_3d_intersects(a: &[u8], b: &[u8]) -> R<bool> {
    threed_metric::st_3d_intersects(a, b).map_err(err)
}

#[wasm_bindgen(js_name = st3dDwithin)]
pub fn st_3d_dwithin(a: &[u8], b: &[u8], d: f64) -> R<bool> {
    threed_metric::st_3d_dwithin(a, b, d).map_err(err)
}

#[wasm_bindgen(js_name = st3dDfullyWithin)]
pub fn st_3d_dfully_within(a: &[u8], b: &[u8], d: f64) -> R<bool> {
    threed_metric::st_3d_dfully_within(a, b, d).map_err(err)
}

#[wasm_bindgen(js_name = st3dClosestPoint)]
pub fn st_3d_closest_point(a: &[u8], b: &[u8]) -> R<Option<Vec<u8>>> {
    threed_metric::st_3d_closest_point(a, b).map_err(err)
}

#[wasm_bindgen(js_name = st3dShortestLine)]
pub fn st_3d_shortest_line(a: &[u8], b: &[u8]) -> R<Option<Vec<u8>>> {
    threed_metric::st_3d_shortest_line(a, b).map_err(err)
}

#[wasm_bindgen(js_name = st3dLongestLine)]
pub fn st_3d_longest_line(a: &[u8], b: &[u8]) -> R<Option<Vec<u8>>> {
    threed_metric::st_3d_longest_line(a, b).map_err(err)
}

#[wasm_bindgen(js_name = st3dLineInterpolatePoint)]
pub fn st_3d_line_interpolate_point(geom: &[u8], fraction: f64) -> R<Vec<u8>> {
    threed_metric::st_3d_line_interpolate_point(geom, fraction).map_err(err)
}

/// `ST_Force3D(geom)` / `ST_Force3DZ(geom)` — zvalue defaults to 0.
#[wasm_bindgen(js_name = stForce3d)]
pub fn st_force_3d(geom: &[u8]) -> R<Vec<u8>> {
    compat::st_force_3d(geom, 0.0).map_err(err)
}

/// `ST_Force3D(geom, zvalue)` / `ST_Force3DZ(geom, zvalue)`.
#[wasm_bindgen(js_name = stForce3dZ)]
pub fn st_force_3d_z(geom: &[u8], z: f64) -> R<Vec<u8>> {
    compat::st_force_3d(geom, z).map_err(err)
}

/// `ST_MakePoint(x, y, z)`.
#[wasm_bindgen(js_name = stMakePointZ)]
pub fn st_make_point_z(x: f64, y: f64, z: f64) -> R<Vec<u8>> {
    io::st_make_point_z(x, y, z).map_err(err)
}

#[wasm_bindgen(js_name = stTransScale)]
pub fn st_trans_scale(geom: &[u8], dx: f64, dy: f64, x_factor: f64, y_factor: f64) -> R<Vec<u8>> {
    extra::st_trans_scale(geom, dx, dy, x_factor, y_factor).map_err(err)
}

#[wasm_bindgen(js_name = stReducePrecision)]
pub fn st_reduce_precision(geom: &[u8], gridsize: f64) -> R<Vec<u8>> {
    extra::st_reduce_precision(geom, gridsize).map_err(err)
}

#[wasm_bindgen(js_name = stAngle3)]
pub fn st_angle_3(p1: &[u8], p2: &[u8], p3: &[u8]) -> R<Option<f64>> {
    extra::st_angle_3(p1, p2, p3).map_err(err)
}

#[wasm_bindgen(js_name = stAngle4)]
pub fn st_angle_4(p1: &[u8], p2: &[u8], p3: &[u8], p4: &[u8]) -> R<Option<f64>> {
    extra::st_angle_4(p1, p2, p3, p4).map_err(err)
}

#[wasm_bindgen(js_name = stLineInterpolatePoints)]
pub fn st_line_interpolate_points(geom: &[u8], fraction: f64) -> R<Option<Vec<u8>>> {
    extra::st_line_interpolate_points(geom, fraction).map_err(err)
}

#[wasm_bindgen(js_name = stPoints)]
pub fn st_points(geom: &[u8]) -> R<Vec<u8>> {
    extra::st_points(geom).map_err(err)
}

#[wasm_bindgen(js_name = stBoundingDiagonal)]
pub fn st_bounding_diagonal(geom: &[u8]) -> R<Option<Vec<u8>>> {
    extra::st_bounding_diagonal(geom).map_err(err)
}

#[wasm_bindgen(js_name = stOrderingEquals)]
pub fn st_ordering_equals(a: &[u8], b: &[u8]) -> R<bool> {
    extra::st_ordering_equals(a, b).map_err(err)
}

#[wasm_bindgen(js_name = stGeohash)]
pub fn st_geohash(geom: &[u8]) -> R<Option<String>> {
    extra::st_geohash(geom, None).map_err(err)
}

#[wasm_bindgen(js_name = stGeohashChars)]
pub fn st_geohash_chars(geom: &[u8], maxchars: i32) -> R<Option<String>> {
    extra::st_geohash(geom, Some(maxchars as i64)).map_err(err)
}

/// `ST_Extent` accumulator — the bounding box of every stepped row.
#[wasm_bindgen]
pub struct ExtentAgg {
    inner: Option<extra::ExtentAggregate>,
}

#[wasm_bindgen]
impl ExtentAgg {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> ExtentAgg {
        ExtentAgg {
            inner: Some(extra::ExtentAggregate::new()),
        }
    }

    pub fn step(&mut self, geom: &[u8]) -> Result<(), JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("kenro: ST_Extent accumulator already finished"))?
            .step(geom)
            .map_err(err)
    }

    /// `undefined` = SQL NULL (zero rows aggregated).
    pub fn finish(&mut self) -> Result<Option<Vec<u8>>, JsError> {
        self.inner
            .take()
            .ok_or_else(|| JsError::new("kenro: ST_Extent accumulator already finished"))?
            .finish()
            .map_err(err)
    }
}

/// `ST_3DExtent` accumulator. ⚠️ `finish` returns **text** —
/// `BOX3D(minx miny minz,maxx maxy maxz)` — because SQLite has no box3d type
/// and kenro cannot write a 3D geometry to stand in for one.
#[wasm_bindgen]
pub struct Extent3DAgg {
    inner: Option<extra::Extent3DAggregate>,
}

#[wasm_bindgen]
impl Extent3DAgg {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Extent3DAgg {
        Extent3DAgg {
            inner: Some(extra::Extent3DAggregate::new()),
        }
    }

    pub fn step(&mut self, geom: &[u8]) -> Result<(), JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("kenro: ST_3DExtent accumulator already finished"))?
            .step(geom)
            .map_err(err)
    }

    /// `undefined` = SQL NULL (zero rows aggregated).
    pub fn finish(&mut self) -> Result<Option<String>, JsError> {
        self.inner
            .take()
            .ok_or_else(|| JsError::new("kenro: ST_3DExtent accumulator already finished"))?
            .finish()
            .map_err(err)
    }
}

// ---- Size-gated algorithms (functions::hull) ----

#[cfg(feature = "concave-hull")]
#[wasm_bindgen(js_name = stConcaveHull)]
pub fn st_concave_hull(geom: &[u8], target_percent: f64) -> R<Vec<u8>> {
    kenro::functions::hull::st_concave_hull(geom, target_percent).map_err(err)
}

#[cfg(feature = "delaunay")]
#[wasm_bindgen(js_name = stDelaunayTriangles)]
pub fn st_delaunay_triangles(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::hull::st_delaunay_triangles(geom).map_err(err)
}

#[cfg(feature = "delaunay")]
#[wasm_bindgen(js_name = stTriangulatePolygon)]
pub fn st_triangulate_polygon(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::hull::st_triangulate_polygon(geom).map_err(err)
}

#[cfg(feature = "voronoi")]
#[wasm_bindgen(js_name = stVoronoiPolygons)]
pub fn st_voronoi_polygons(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::hull::st_voronoi_polygons(geom, None, None).map_err(err)
}

#[cfg(feature = "voronoi")]
#[wasm_bindgen(js_name = stVoronoiPolygonsTol)]
pub fn st_voronoi_polygons_tol(geom: &[u8], tolerance: f64) -> R<Vec<u8>> {
    kenro::functions::hull::st_voronoi_polygons(geom, Some(tolerance), None).map_err(err)
}

#[cfg(feature = "voronoi")]
#[wasm_bindgen(js_name = stVoronoiPolygonsExtend)]
pub fn st_voronoi_polygons_extend(geom: &[u8], tolerance: f64, extend_to: &[u8]) -> R<Vec<u8>> {
    kenro::functions::hull::st_voronoi_polygons(geom, Some(tolerance), Some(extend_to)).map_err(err)
}

#[cfg(feature = "voronoi")]
#[wasm_bindgen(js_name = stVoronoiLines)]
pub fn st_voronoi_lines(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::hull::st_voronoi_lines(geom, None, None).map_err(err)
}

#[cfg(feature = "voronoi")]
#[wasm_bindgen(js_name = stVoronoiLinesTol)]
pub fn st_voronoi_lines_tol(geom: &[u8], tolerance: f64) -> R<Vec<u8>> {
    kenro::functions::hull::st_voronoi_lines(geom, Some(tolerance), None).map_err(err)
}

#[cfg(feature = "voronoi")]
#[wasm_bindgen(js_name = stVoronoiLinesExtend)]
pub fn st_voronoi_lines_extend(geom: &[u8], tolerance: f64, extend_to: &[u8]) -> R<Vec<u8>> {
    kenro::functions::hull::st_voronoi_lines(geom, Some(tolerance), Some(extend_to)).map_err(err)
}

// ---- Line structure (functions::lines) ----

#[wasm_bindgen(js_name = stSquareGrid)]
pub fn st_square_grid(size: f64, bounds: &[u8]) -> R<Vec<u8>> {
    kenro::functions::grid::st_square_grid(size, bounds).map_err(err)
}

#[wasm_bindgen(js_name = stHexagonGrid)]
pub fn st_hexagon_grid(size: f64, bounds: &[u8]) -> R<Vec<u8>> {
    kenro::functions::grid::st_hexagon_grid(size, bounds).map_err(err)
}

#[wasm_bindgen(js_name = stIsSimple)]
pub fn st_is_simple(geom: &[u8]) -> R<bool> {
    kenro::functions::lines::st_is_simple(geom).map_err(err)
}

#[wasm_bindgen(js_name = stLineMerge)]
pub fn st_line_merge(geom: &[u8]) -> R<Vec<u8>> {
    kenro::functions::lines::st_line_merge(geom).map_err(err)
}

#[wasm_bindgen(js_name = stLineMergeDirected)]
pub fn st_line_merge_directed(geom: &[u8], directed: bool) -> R<Vec<u8>> {
    kenro::functions::lines::st_line_merge_directed(geom, directed).map_err(err)
}

// ---- The tail (functions::misc) ----

#[wasm_bindgen(js_name = stPolygon)]
pub fn st_polygon(geom: &[u8], srid: i32) -> R<Vec<u8>> {
    misc::st_polygon(geom, srid).map_err(err)
}

#[wasm_bindgen(js_name = stLineFromMultipoint)]
pub fn st_line_from_multipoint(geom: &[u8]) -> R<Option<Vec<u8>>> {
    misc::st_line_from_multipoint(geom).map_err(err)
}

#[wasm_bindgen(js_name = stLineExtend)]
pub fn st_line_extend(geom: &[u8], forward: f64) -> R<Option<Vec<u8>>> {
    misc::st_line_extend(geom, forward, 0.0).map_err(err)
}

#[wasm_bindgen(js_name = stLineExtendBoth)]
pub fn st_line_extend_both(geom: &[u8], forward: f64, backward: f64) -> R<Option<Vec<u8>>> {
    misc::st_line_extend(geom, forward, backward).map_err(err)
}

#[wasm_bindgen(js_name = stPointInsideCircle)]
pub fn st_point_inside_circle(geom: &[u8], cx: f64, cy: f64, radius: f64) -> R<bool> {
    misc::st_point_inside_circle(geom, cx, cy, radius).map_err(err)
}

#[wasm_bindgen(js_name = stWrapX)]
pub fn st_wrap_x(geom: &[u8], wrap: f64, amount: f64) -> R<Vec<u8>> {
    misc::st_wrap_x(geom, wrap, amount).map_err(err)
}

#[wasm_bindgen(js_name = stMakeBox2d)]
pub fn st_make_box_2d(low: &[u8], high: &[u8]) -> R<Vec<u8>> {
    misc::st_make_box_2d(low, high).map_err(err)
}

#[wasm_bindgen(js_name = stGeomFromGeohash)]
pub fn st_geom_from_geohash(hash: &str) -> R<Vec<u8>> {
    misc::st_geom_from_geohash(hash, None).map_err(err)
}

#[wasm_bindgen(js_name = stGeomFromGeohashPrec)]
pub fn st_geom_from_geohash_prec(hash: &str, precision: i32) -> R<Vec<u8>> {
    misc::st_geom_from_geohash(hash, Some(precision as i64)).map_err(err)
}

#[wasm_bindgen(js_name = stPointFromGeohash)]
pub fn st_point_from_geohash(hash: &str) -> R<Vec<u8>> {
    misc::st_point_from_geohash(hash, None).map_err(err)
}

#[wasm_bindgen(js_name = stPointFromGeohashPrec)]
pub fn st_point_from_geohash_prec(hash: &str, precision: i32) -> R<Vec<u8>> {
    misc::st_point_from_geohash(hash, Some(precision as i64)).map_err(err)
}

#[wasm_bindgen(js_name = stGeometricMedian)]
pub fn st_geometric_median(geom: &[u8]) -> R<Option<Vec<u8>>> {
    misc::st_geometric_median(geom, None).map_err(err)
}

#[wasm_bindgen(js_name = stGeometricMedianTol)]
pub fn st_geometric_median_tol(geom: &[u8], tolerance: f64) -> R<Option<Vec<u8>>> {
    misc::st_geometric_median(geom, Some(tolerance)).map_err(err)
}

#[wasm_bindgen(js_name = stLineCrossingDirection)]
pub fn st_line_crossing_direction(a: &[u8], b: &[u8]) -> R<i32> {
    misc::st_line_crossing_direction(a, b)
        .map(|v| v as i32)
        .map_err(err)
}

#[wasm_bindgen(js_name = stSummary)]
pub fn st_summary(geom: &[u8]) -> R<String> {
    misc::st_summary(geom).map_err(err)
}

#[wasm_bindgen(js_name = stMemSize)]
pub fn st_mem_size(geom: &[u8]) -> R<i32> {
    misc::st_mem_size(geom).map(|v| v as i32).map_err(err)
}

#[wasm_bindgen(js_name = stNormalize)]
pub fn st_normalize(geom: &[u8]) -> R<Vec<u8>> {
    misc::st_normalize(geom).map_err(err)
}

// ---- GML 2/3 I/O (functions::gml) ----

#[cfg(feature = "gml")]
#[wasm_bindgen(js_name = stAsGml)]
pub fn st_as_gml(geom: &[u8]) -> R<String> {
    kenro::functions::gml::st_as_gml(geom, 2, None).map_err(err)
}

#[cfg(feature = "gml")]
#[wasm_bindgen(js_name = stAsGmlVersion)]
pub fn st_as_gml_version(version: i32, geom: &[u8]) -> R<String> {
    kenro::functions::gml::st_as_gml(geom, version as i64, None).map_err(err)
}

#[cfg(feature = "text-encodings")]
#[wasm_bindgen(js_name = stAsKml)]
pub fn st_as_kml(geom: &[u8]) -> R<String> {
    kenro::functions::kml::st_as_kml(geom, None, None).map_err(err)
}

#[cfg(feature = "text-encodings")]
#[wasm_bindgen(js_name = stAsKmlDigits)]
pub fn st_as_kml_digits(geom: &[u8], digits: i32) -> R<String> {
    kenro::functions::kml::st_as_kml(geom, Some(digits as i64), None).map_err(err)
}

#[cfg(feature = "text-encodings")]
#[wasm_bindgen(js_name = stAsKmlPrefix)]
pub fn st_as_kml_prefix(geom: &[u8], digits: i32, prefix: &str) -> R<String> {
    kenro::functions::kml::st_as_kml(geom, Some(digits as i64), Some(prefix)).map_err(err)
}

#[cfg(feature = "text-encodings")]
#[wasm_bindgen(js_name = stAsSvg)]
pub fn st_as_svg(geom: &[u8]) -> R<String> {
    kenro::functions::svg::st_as_svg(geom, None, None).map_err(err)
}

#[cfg(feature = "text-encodings")]
#[wasm_bindgen(js_name = stAsSvgRel)]
pub fn st_as_svg_rel(geom: &[u8], rel: i32) -> R<String> {
    kenro::functions::svg::st_as_svg(geom, Some(rel as i64), None).map_err(err)
}

#[cfg(feature = "text-encodings")]
#[wasm_bindgen(js_name = stAsSvgDigits)]
pub fn st_as_svg_digits(geom: &[u8], rel: i32, digits: i32) -> R<String> {
    kenro::functions::svg::st_as_svg(geom, Some(rel as i64), Some(digits as i64)).map_err(err)
}

#[cfg(feature = "gml")]
#[wasm_bindgen(js_name = stAsGmlDigits)]
pub fn st_as_gml_digits(version: i32, geom: &[u8], digits: i32) -> R<String> {
    kenro::functions::gml::st_as_gml(geom, version as i64, Some(digits as i64)).map_err(err)
}

#[cfg(feature = "gml")]
#[wasm_bindgen(js_name = stGeomFromGml)]
pub fn st_geom_from_gml(text: &str) -> R<Vec<u8>> {
    kenro::functions::gml::st_geom_from_gml(text, None).map_err(err)
}

#[cfg(feature = "gml")]
#[wasm_bindgen(js_name = stGeomFromGmlSrid)]
pub fn st_geom_from_gml_srid(text: &str, srid: i32) -> R<Vec<u8>> {
    kenro::functions::gml::st_geom_from_gml(text, Some(srid)).map_err(err)
}

// ---- Surface collections (functions::surface) ----

#[wasm_bindgen(js_name = stNumPatches)]
pub fn st_num_patches(geom: &[u8]) -> R<Option<i64>> {
    surface::st_num_patches(geom).map_err(err)
}

#[wasm_bindgen(js_name = stPatchN)]
pub fn st_patch_n(geom: &[u8], n: i32) -> R<Option<Vec<u8>>> {
    surface::st_patch_n(geom, n as i64).map_err(err)
}

#[wasm_bindgen(js_name = kenroGpkgExtensionRequired)]
pub fn kenro_gpkg_extension_required(geom: &[u8]) -> R<Option<String>> {
    surface::extension_required(geom).map_err(err)
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
#[wasm_bindgen(js_name = stSplit)]
pub fn st_split(input: &[u8], blade: &[u8]) -> R<Vec<u8>> {
    kenro::functions::lines::st_split(input, blade).map_err(err)
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

/// Accumulator for `kenro_dijkstra(id, source, target, cost, start_vid,
/// end_vid [, reverse_cost])`. `reverse_cost` is last precisely so that the
/// 6-argument call can leave it `undefined` here — wasm-bindgen only makes
/// trailing arguments optional.
#[cfg(feature = "routing")]
#[wasm_bindgen]
pub struct DijkstraAgg {
    inner: Option<kenro::functions::routing::DijkstraAggregate>,
}

#[cfg(feature = "routing")]
#[wasm_bindgen]
impl DijkstraAgg {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> DijkstraAgg {
        DijkstraAgg {
            inner: Some(kenro::functions::routing::DijkstraAggregate::new()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        id: i32,
        source: i32,
        target: i32,
        cost: f64,
        start_vid: i32,
        end_vid: i32,
        reverse_cost: Option<f64>,
    ) -> Result<(), JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("kenro: kenro_dijkstra accumulator already finished"))?
            .step(id, source, target, cost, start_vid, end_vid, reverse_cost)
            .map_err(err)
    }

    /// `undefined` = SQL NULL (zero rows, or no path).
    pub fn finish(&mut self) -> Result<Option<String>, JsError> {
        self.inner
            .take()
            .ok_or_else(|| JsError::new("kenro: kenro_dijkstra accumulator already finished"))?
            .finish()
            .map_err(err)
    }
}

/// Accumulator for `kenro_dijkstra_cost(source, target, cost, start_vid,
/// end_vid [, reverse_cost])` — the same search, total cost only.
#[cfg(feature = "routing")]
#[wasm_bindgen]
pub struct DijkstraCostAgg {
    inner: Option<kenro::functions::routing::DijkstraCostAggregate>,
}

#[cfg(feature = "routing")]
#[wasm_bindgen]
impl DijkstraCostAgg {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> DijkstraCostAgg {
        DijkstraCostAgg {
            inner: Some(kenro::functions::routing::DijkstraCostAggregate::new()),
        }
    }

    pub fn step(
        &mut self,
        source: i32,
        target: i32,
        cost: f64,
        start_vid: i32,
        end_vid: i32,
        reverse_cost: Option<f64>,
    ) -> Result<(), JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("kenro: kenro_dijkstra_cost accumulator already finished"))?
            .step(source, target, cost, start_vid, end_vid, reverse_cost)
            .map_err(err)
    }

    /// `undefined` = SQL NULL (zero rows, or no path).
    pub fn finish(&mut self) -> Result<Option<f64>, JsError> {
        self.inner
            .take()
            .ok_or_else(|| JsError::new("kenro: kenro_dijkstra_cost accumulator already finished"))?
            .finish()
            .map_err(err)
    }
}

/// Accumulator for `kenro_drivingdistance(id, source, target, cost,
/// start_vid, limit [, reverse_cost])`.
#[cfg(feature = "routing")]
#[wasm_bindgen]
pub struct DrivingDistAgg {
    inner: Option<kenro::functions::routing::DrivingDistanceAggregate>,
}

#[cfg(feature = "routing")]
#[wasm_bindgen]
impl DrivingDistAgg {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> DrivingDistAgg {
        DrivingDistAgg {
            inner: Some(kenro::functions::routing::DrivingDistanceAggregate::new()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        id: i32,
        source: i32,
        target: i32,
        cost: f64,
        start_vid: i32,
        limit: f64,
        reverse_cost: Option<f64>,
    ) -> Result<(), JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| {
                JsError::new("kenro: kenro_drivingdistance accumulator already finished")
            })?
            .step(id, source, target, cost, start_vid, limit, reverse_cost)
            .map_err(err)
    }

    /// `undefined` = SQL NULL (zero rows, or a negative limit).
    pub fn finish(&mut self) -> Result<Option<String>, JsError> {
        self.inner
            .take()
            .ok_or_else(|| {
                JsError::new("kenro: kenro_drivingdistance accumulator already finished")
            })?
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
        Kind::OptInt => "opt_int",
        Kind::OptText => "opt_text",
        Kind::OptBlob => "opt_blob",
        Kind::TextOrInt => "text_or_int",
        Kind::BlobOrText => "blob_or_text",
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
            "stForce2d",
            "stAsEwkt",
            "stGeomFromEwkt",
            "stAsEwkb",
            "stAsHexEwkb",
            "stPointFromText",
            "stPointFromTextSrid",
            "stLineFromText",
            "stLineFromTextSrid",
            "stPolyFromText",
            "stPolyFromTextSrid",
            "stMPointFromText",
            "stMPointFromTextSrid",
            "stMLineFromText",
            "stMLineFromTextSrid",
            "stMPolyFromText",
            "stMPolyFromTextSrid",
            "stPointFromWkb",
            "stPointFromWkbSrid",
            "stLineFromWkb",
            "stLineFromWkbSrid",
            "stPolyFromWkb",
            "stPolyFromWkbSrid",
            "stMPointFromWkb",
            "stMPointFromWkbSrid",
            "stMLineFromWkb",
            "stMLineFromWkbSrid",
            "stMPolyFromWkb",
            "stMPolyFromWkbSrid",
            "stExteriorRing",
            "stInteriorRingN",
            "stNumInteriorRings",
            "stNRings",
            "stBoundary",
            "stIsClosed",
            "stIsRing",
            "stAddPoint",
            "stAddPointAt",
            "stSetPoint",
            "stRemovePoint",
            "stMakeLine",
            "stMakePolygon",
            "stMulti",
            "stSnapToGrid",
            "stSnapToGridXy",
            "stFlipCoordinates",
            "stShiftLongitude",
            "stExpand",
            "stDistanceSphere",
            "stDistanceSpheroid",
            "stDistanceSpheroidOn",
            "stLengthSpheroid",
            "stProject",
            "stDimension",
            "stCoordDim",
            "stIsValidReason",
            "stForcePolygonCw",
            "stForcePolygonCcw",
            "stIsPolygonCw",
            "stIsPolygonCcw",
            "stSegmentize",
            "stLineSubstring",
            "stShortestLine",
            "stLongestLine",
            "stMaxDistance",
            "stMinimumBoundingRadius",
            "stMinimumBoundingCircle",
            "stMinimumBoundingCircleSegs",
            "stUnaryUnion",
            "stClipByBox2d",
            "stSubdivide",
            "stContainsProperly",
            "stDfullyWithin",
            "stRelateMatch",
            "stAffine",
            "stTransScale",
            "stReducePrecision",
            "stAngle3",
            "stAngle4",
            "stLineInterpolatePoints",
            "stPoints",
            "stBoundingDiagonal",
            "stOrderingEquals",
            "stGeohash",
            "stGeohashChars",
            "stConcaveHull",
            "stDelaunayTriangles",
            "stTriangulatePolygon",
            "stVoronoiPolygons",
            "stVoronoiPolygonsTol",
            "stVoronoiPolygonsExtend",
            "stVoronoiLines",
            "stVoronoiLinesTol",
            "stVoronoiLinesExtend",
            "stSquareGrid",
            "stHexagonGrid",
            "stIsSimple",
            "stLineMerge",
            "stLineMergeDirected",
            "stSplit",
            "stPolygon",
            "stLineFromMultipoint",
            "stLineExtend",
            "stLineExtendBoth",
            "stPointInsideCircle",
            "stWrapX",
            "stMakeBox2d",
            "stGeomFromGeohash",
            "stGeomFromGeohashPrec",
            "stPointFromGeohash",
            "stPointFromGeohashPrec",
            "stGeometricMedian",
            "stGeometricMedianTol",
            "stLineCrossingDirection",
            "stSummary",
            "stMemSize",
            "stNormalize",
            "stHasZ",
            "stHasM",
            "stZ",
            "stM",
            "stZMin",
            "stZMax",
            "stAsGml",
            "stAsGmlVersion",
            "stAsGmlDigits",
            "stAsKml",
            "stAsKmlDigits",
            "stAsKmlPrefix",
            "stAsSvg",
            "stAsSvgRel",
            "stAsSvgDigits",
            "stGeomFromGml",
            "stGeomFromGmlSrid",
            "stNumPatches",
            "stPatchN",
            "kenroGpkgExtensionRequired",
            "stAffine3d",
            "stForce3d",
            "stForce3dZ",
            "stMakePointZ",
            "st3dDistance",
            "st3dMaxDistance",
            "st3dIntersects",
            "st3dDwithin",
            "st3dDfullyWithin",
            "st3dClosestPoint",
            "st3dShortestLine",
            "st3dLongestLine",
            "st3dLineInterpolatePoint",
            "st3dArea",
            "kenroVolume",
        ];
        for entry in kenro::functions::manifest::active_functions() {
            assert!(
                known.contains(&entry.export),
                "manifest export {} has no wasm-bindgen counterpart",
                entry.export
            );
        }
        let known_aggregates = [
            "UnionAgg",
            "MvtAgg",
            "ExtentAgg",
            "Extent3DAgg",
            "DijkstraAgg",
            "DijkstraCostAgg",
            "DrivingDistAgg",
        ];
        for entry in kenro::functions::manifest::active_aggregates() {
            assert!(
                known_aggregates.contains(&entry.ctor_export),
                "manifest aggregate ctor {} has no wasm-bindgen counterpart",
                entry.ctor_export
            );
        }
    }
}
