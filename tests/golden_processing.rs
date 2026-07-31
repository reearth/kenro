//! Golden tests for the Tier B measure/processing functions vs the
//! reference PostGIS. Scalars compare by value with tolerance;
//! geometry-returning functions compare geometrically.

mod common;

use common::Vector;
use kenro::functions::{io, measures};
use rusqlite::Connection;
use rusqlite::types::Value;

fn check(id: &str, func: &str, got: &serde_json::Value, want: &serde_json::Value) {
    let geometric = matches!(func, "closestpoint" | "lineinterpolate");
    match (got, want) {
        (serde_json::Value::Null, serde_json::Value::Null) => {}
        (serde_json::Value::Number(g), serde_json::Value::Number(w)) => {
            common::assert_number(id, g.as_f64().unwrap(), w.as_f64().unwrap())
        }
        (serde_json::Value::String(g), serde_json::Value::String(w)) if geometric => {
            assert!(
                common::geoms_approx_equal(g, w, 1e-9),
                "{id}: got {g}, want {w}"
            )
        }
        (serde_json::Value::String(g), serde_json::Value::String(w)) => {
            assert_eq!(g, w, "{id}")
        }
        (got, want) => panic!("{id}: type mismatch: got {got}, want {want}"),
    }
}

fn opt_astext(blob: Option<Vec<u8>>) -> Result<serde_json::Value, kenro::Error> {
    Ok(match blob {
        Some(b) => io::st_as_text(&b)?.into(),
        None => serde_json::Value::Null,
    })
}

fn run_pure(v: &Vector) -> Result<serde_json::Value, kenro::Error> {
    let a = io::st_geom_from_text(v.a.as_ref().unwrap(), None)?;
    let b = || io::st_geom_from_text(v.b.as_ref().unwrap(), None);
    Ok(match v.func.as_str() {
        "closestpoint" => opt_astext(measures::st_closest_point(&a, &b()?)?)?,
        "lineinterpolate" => {
            io::st_as_text(&measures::st_line_interpolate_point(&a, v.arg.unwrap())?)?.into()
        }
        "linelocate" => measures::st_line_locate_point(&a, &b()?)?.into(),
        "hausdorff" => measures::st_hausdorff_distance(&a, &b()?)?.into(),
        "frechet" => measures::st_frechet_distance(&a, &b()?)?.into(),
        "azimuth" => match measures::st_azimuth(&a, &b()?)? {
            Some(az) => serde_json::json!(az),
            None => serde_json::Value::Null,
        },
        other => panic!("{}: unknown fn {other}", v.id),
    })
}

#[test]
fn golden_processing_through_pure_functions() {
    for v in common::load("processing") {
        let result = run_pure(&v);
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = result.unwrap_or_else(|e| panic!("{}: {e}", v.id));
        check(&v.id, &v.func, &got, v.effective());
    }
}

#[test]
fn golden_processing_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    for v in common::load("processing") {
        let sql = match v.func.as_str() {
            "closestpoint" => {
                "SELECT ST_AsText(ST_ClosestPoint(ST_GeomFromText(?1), ST_GeomFromText(?2)))"
            }
            "lineinterpolate" => {
                "SELECT ST_AsText(ST_LineInterpolatePoint(ST_GeomFromText(?1), ?2))"
            }
            "linelocate" => "SELECT ST_LineLocatePoint(ST_GeomFromText(?1), ST_GeomFromText(?2))",
            "hausdorff" => "SELECT ST_HausdorffDistance(ST_GeomFromText(?1), ST_GeomFromText(?2))",
            "frechet" => "SELECT ST_FrechetDistance(ST_GeomFromText(?1), ST_GeomFromText(?2))",
            "azimuth" => "SELECT ST_Azimuth(ST_GeomFromText(?1), ST_GeomFromText(?2))",
            other => panic!("{}: unknown fn {other}", v.id),
        };
        let result: rusqlite::Result<Value> = match v.func.as_str() {
            "lineinterpolate" => conn.query_row(
                sql,
                rusqlite::params![v.a.as_ref().unwrap(), v.arg.unwrap()],
                |r| r.get(0),
            ),
            _ => conn.query_row(sql, [v.a.as_ref().unwrap(), v.b.as_ref().unwrap()], |r| {
                r.get(0)
            }),
        };
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = match result.unwrap_or_else(|e| panic!("{}: {e}", v.id)) {
            Value::Null => serde_json::Value::Null,
            Value::Real(f) => f.into(),
            Value::Integer(i) => i.into(),
            Value::Text(s) => s.into(),
            other => panic!("{}: unexpected SQL value {other:?}", v.id),
        };
        check(&v.id, &v.func, &got, v.effective());
    }
}
