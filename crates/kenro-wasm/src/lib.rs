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

/// kenro::Error → JS exception. The `kenro: `-prefixed message is preserved
/// so SQL error text matches the rusqlite binding exactly.
fn err(e: kenro::Error) -> JsError {
    JsError::new(&e.to_string())
}

type R<T> = Result<T, JsError>;

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
    serde_json::json!({ "functions": functions, "stubs": stubs }).to_string()
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
        ];
        for entry in kenro::functions::manifest::active_functions() {
            assert!(
                known.contains(&entry.export),
                "manifest export {} has no wasm-bindgen counterpart",
                entry.export
            );
        }
    }
}
