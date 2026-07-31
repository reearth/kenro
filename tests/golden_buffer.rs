//! Golden tests for ST_Buffer vs the reference PostGIS. Round-style
//! results compare by symmetric-difference area ratio (arc tessellation
//! differs between geo and GEOS); degenerate cases compare exactly.

mod common;

use geo::{Area, BooleanOps};
use geo_types::{Geometry, MultiPolygon};
use kenro::functions::{io, overlay};
use rusqlite::Connection;
use rusqlite::types::Value;

fn as_multi_polygon(wkt: &str) -> Option<MultiPolygon<f64>> {
    let geom = kenro::geom::decode_wkt(wkt, 0).ok()?;
    Some(match geom.geometry {
        Geometry::Polygon(p) => MultiPolygon(vec![p]),
        Geometry::MultiPolygon(mp) => mp,
        _ => return None,
    })
}

fn check(id: &str, mode: Option<&str>, got: &str, want: &str) {
    if got == want {
        return;
    }
    match mode {
        Some("buffer") => {
            let (Some(g), Some(w)) = (as_multi_polygon(got), as_multi_polygon(want)) else {
                panic!("{id}: non-areal buffer comparison: got {got}, want {want}");
            };
            let denom = w.unsigned_area().max(1e-12);
            let ratio = g.xor(&w).unsigned_area() / denom;
            // Different arc tessellations: bound the area disagreement at 2%.
            assert!(
                ratio <= 0.02,
                "{id}: symmetric-difference ratio {ratio}: got {got}, want {want}"
            );
        }
        _ => assert_eq!(got, want, "{id}"),
    }
}

#[test]
fn golden_buffer_through_pure_functions() {
    for v in common::load("buffer") {
        let a = io::st_geom_from_text(v.a.as_ref().unwrap(), None).unwrap();
        let result = overlay::st_buffer(&a, v.arg.unwrap(), v.arg_text.as_deref());
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = io::st_as_text(&result.unwrap_or_else(|e| panic!("{}: {e}", v.id))).unwrap();
        check(
            &v.id,
            v.mode.as_deref(),
            &got,
            v.effective().as_str().unwrap(),
        );
    }
}

#[test]
fn golden_buffer_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    for v in common::load("buffer") {
        let result: rusqlite::Result<Value> = match &v.arg_text {
            Some(opts) => conn.query_row(
                "SELECT ST_AsText(ST_Buffer(ST_GeomFromText(?1), ?2, ?3))",
                rusqlite::params![v.a.as_ref().unwrap(), v.arg.unwrap(), opts],
                |r| r.get(0),
            ),
            None => conn.query_row(
                "SELECT ST_AsText(ST_Buffer(ST_GeomFromText(?1), ?2))",
                rusqlite::params![v.a.as_ref().unwrap(), v.arg.unwrap()],
                |r| r.get(0),
            ),
        };
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        match result.unwrap_or_else(|e| panic!("{}: {e}", v.id)) {
            Value::Text(s) => check(
                &v.id,
                v.mode.as_deref(),
                &s,
                v.effective().as_str().unwrap(),
            ),
            other => panic!("{}: unexpected SQL value {other:?}", v.id),
        }
    }
}
