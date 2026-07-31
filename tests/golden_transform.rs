//! Golden tests for ST_Transform: kenro's result vs the reference PostGIS,
//! compared with a metric tolerance (proj4rs is not bit-identical to PROJ;
//! docs/accuracy.md quantifies the difference — these vectors assert it
//! stays inside the documented envelope).

mod common;

use common::Vector;
use kenro::functions::{io, transform};
use rusqlite::Connection;

/// Default per-coordinate tolerance for transform vectors, in meters.
const DEFAULT_TOLERANCE_M: f64 = 0.01;

fn geographic(srid: i32) -> bool {
    matches!(srid, 4326 | 4612 | 6668)
}

/// Compare two WKT geometries coordinate-wise, with deltas converted to
/// meters (degree deltas are scaled by the local metric factors).
fn within_tolerance_m(id: &str, got: &str, want: &str, to_srid: i32, tol_m: f64) {
    use geo::CoordsIter;
    let ga = kenro::geom::decode_wkt(got, 0).unwrap();
    let gb = kenro::geom::decode_wkt(want, 0).unwrap();
    let ca: Vec<_> = ga.geometry.coords_iter().collect();
    let cb: Vec<_> = gb.geometry.coords_iter().collect();
    assert_eq!(ca.len(), cb.len(), "{id}: vertex count");
    for (p, q) in ca.iter().zip(&cb) {
        let (dx_m, dy_m) = if geographic(to_srid) {
            let lat = q.y.to_radians();
            ((p.x - q.x) * 111_320.0 * lat.cos(), (p.y - q.y) * 110_540.0)
        } else {
            (p.x - q.x, p.y - q.y)
        };
        let err = (dx_m * dx_m + dy_m * dy_m).sqrt();
        assert!(
            err <= tol_m,
            "{id}: {err:.6} m off at ({}, {}) vs ({}, {})",
            p.x,
            p.y,
            q.x,
            q.y
        );
    }
}

fn input_blob(v: &Vector) -> Vec<u8> {
    let src = v.src_srid.unwrap();
    io::st_geom_from_text(v.a.as_ref().unwrap(), (src > 0).then_some(src)).unwrap()
}

/// The unknown-EPSG vector asserts the curated-table error; with `crs-full`
/// the code resolves from the full registry instead.
fn skipped(v: &Vector) -> bool {
    cfg!(feature = "crs-full") && v.id.starts_with("unknown_epsg")
}

#[test]
fn golden_transform_through_pure_functions() {
    for v in common::load("transform") {
        if skipped(&v) {
            continue;
        }
        let blob = input_blob(&v);
        let result = transform::st_transform(&blob, v.to_srid.unwrap());
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = io::st_as_text(&result.unwrap_or_else(|e| panic!("{}: {e}", v.id))).unwrap();
        let want = v.effective().as_str().unwrap();
        within_tolerance_m(&v.id, &got, want, v.to_srid.unwrap(), DEFAULT_TOLERANCE_M);
    }
}

#[test]
fn golden_transform_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    for v in common::load("transform") {
        if skipped(&v) {
            continue;
        }
        let result: rusqlite::Result<String> = conn.query_row(
            "SELECT ST_AsText(ST_Transform(ST_SetSRID(ST_GeomFromText(?1), ?2), ?3))",
            rusqlite::params![
                v.a.as_ref().unwrap(),
                v.src_srid.unwrap(),
                v.to_srid.unwrap()
            ],
            |r| r.get(0),
        );
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = result.unwrap_or_else(|e| panic!("{}: {e}", v.id));
        let want = v.effective().as_str().unwrap();
        within_tolerance_m(&v.id, &got, want, v.to_srid.unwrap(), DEFAULT_TOLERANCE_M);
    }
}
