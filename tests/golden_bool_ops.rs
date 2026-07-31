//! Golden tests for the overlay functions vs the reference PostGIS.
//! Areal results compare by symmetric-difference area ratio (i_overlay and
//! GEOS compute the same arrangement with different vertex chains); point
//! and empty results compare exactly; line clips compare geometrically.

mod common;

use common::Vector;
use geo::BooleanOps;
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

/// Symmetric-difference area ratio between two areal WKTs.
fn areal_close(id: &str, got: &str, want: &str) {
    use geo::Area;
    if got == want {
        return;
    }
    let (Some(g), Some(w)) = (as_multi_polygon(got), as_multi_polygon(want)) else {
        panic!("{id}: non-areal comparison operands: got {got}, want {want}");
    };
    let denom = w.unsigned_area().max(1e-12);
    let ratio = g.xor(&w).unsigned_area() / denom;
    // i_overlay snaps coordinates onto an internal integer grid
    // (~1e-8 relative coordinate error vs GEOS's fp vertices), so the areal
    // agreement bound is 1e-6 — still a 0.0001% area difference ceiling.
    assert!(
        ratio <= 1e-6,
        "{id}: symmetric-difference ratio {ratio}: got {got}, want {want}"
    );
}

fn check(v: &Vector, got: &serde_json::Value) {
    let want = v.effective();
    match (v.mode.as_deref(), got, want) {
        (Some("areal"), serde_json::Value::String(g), serde_json::Value::String(w)) => {
            areal_close(&v.id, g, w)
        }
        (Some("geometric"), serde_json::Value::String(g), serde_json::Value::String(w)) => {
            assert!(
                common::geoms_approx_equal(g, w, 1e-9),
                "{}: got {g}, want {w}",
                v.id
            )
        }
        (_, serde_json::Value::String(g), serde_json::Value::String(w)) => {
            assert_eq!(g, w, "{}", v.id)
        }
        (_, got, want) => panic!("{}: type mismatch: got {got}, want {want}", v.id),
    }
}

fn run_pure(v: &Vector) -> Result<String, kenro::Error> {
    let a = io::st_geom_from_text(v.a.as_ref().unwrap(), None)?;
    if v.func == "makevalid" {
        return io::st_as_text(&overlay::st_make_valid(&a)?);
    }
    let b = io::st_geom_from_text(v.b.as_ref().unwrap(), None)?;
    let blob = match v.func.as_str() {
        "intersection" => overlay::st_intersection(&a, &b)?,
        "difference" => overlay::st_difference(&a, &b)?,
        "symdifference" => overlay::st_sym_difference(&a, &b)?,
        "union" => overlay::st_union(&a, &b)?,
        other => panic!("{}: unknown fn {other}", v.id),
    };
    io::st_as_text(&blob)
}

#[test]
fn golden_bool_ops_through_pure_functions() {
    for v in common::load("bool_ops") {
        let result = run_pure(&v);
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = result.unwrap_or_else(|e| panic!("{}: {e}", v.id));
        check(&v, &serde_json::Value::String(got));
    }
}

#[test]
fn golden_bool_ops_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    for v in common::load("bool_ops") {
        if v.func == "makevalid" {
            let got: String = conn
                .query_row(
                    "SELECT ST_AsText(ST_MakeValid(ST_GeomFromText(?1)))",
                    [v.a.as_ref().unwrap()],
                    |r| r.get(0),
                )
                .unwrap_or_else(|e| panic!("{}: {e}", v.id));
            check(&v, &serde_json::Value::String(got));
            continue;
        }
        let sql = match v.func.as_str() {
            "intersection" => {
                "SELECT ST_AsText(ST_Intersection(ST_GeomFromText(?1), ST_GeomFromText(?2)))"
            }
            "difference" => {
                "SELECT ST_AsText(ST_Difference(ST_GeomFromText(?1), ST_GeomFromText(?2)))"
            }
            "symdifference" => {
                "SELECT ST_AsText(ST_SymDifference(ST_GeomFromText(?1), ST_GeomFromText(?2)))"
            }
            "union" => "SELECT ST_AsText(ST_Union(ST_GeomFromText(?1), ST_GeomFromText(?2)))",
            other => panic!("{}: unknown fn {other}", v.id),
        };
        let result: rusqlite::Result<Value> =
            conn.query_row(sql, [v.a.as_ref().unwrap(), v.b.as_ref().unwrap()], |r| {
                r.get(0)
            });
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        match result.unwrap_or_else(|e| panic!("{}: {e}", v.id)) {
            Value::Text(s) => check(&v, &serde_json::Value::String(s)),
            other => panic!("{}: unexpected SQL value {other:?}", v.id),
        }
    }
}
