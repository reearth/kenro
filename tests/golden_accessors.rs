//! Golden tests for the accessor functions vs the reference PostGIS.
//! Scalars compare by value; geometry-returning functions compare
//! geometrically (coordinate tolerance — centroid math need not be
//! bit-identical between implementations).

mod common;

use common::Vector;
use kenro::functions::{accessors, io};
use rusqlite::Connection;
use rusqlite::types::Value;

const GEOM_TOLERANCE: f64 = 1e-9;

fn returns_geometry(func: &str) -> bool {
    matches!(func, "centroid" | "envelope" | "simplify")
}

fn check(id: &str, func: &str, got: &serde_json::Value, want: &serde_json::Value) {
    match (got, want) {
        (serde_json::Value::Null, serde_json::Value::Null) => {}
        (serde_json::Value::Number(g), serde_json::Value::Number(w)) => {
            common::assert_number(id, g.as_f64().unwrap(), w.as_f64().unwrap())
        }
        (serde_json::Value::Bool(g), serde_json::Value::Bool(w)) => {
            assert_eq!(g, w, "{id}")
        }
        (serde_json::Value::String(g), serde_json::Value::String(w)) if returns_geometry(func) => {
            assert!(
                common::geoms_approx_equal(g, w, GEOM_TOLERANCE),
                "{id}: got {g}, want {w}"
            )
        }
        (serde_json::Value::String(g), serde_json::Value::String(w)) => {
            assert_eq!(g, w, "{id}")
        }
        (got, want) => panic!("{id}: type mismatch: got {got}, want {want}"),
    }
}

fn run_pure(v: &Vector) -> Result<serde_json::Value, kenro::Error> {
    let blob = io::st_geom_from_text(v.a.as_ref().unwrap(), None)?;
    Ok(match v.func.as_str() {
        "area" => accessors::st_area(&blob)?.into(),
        "length" => accessors::st_length(&blob)?.into(),
        "centroid" => io::st_as_text(&accessors::st_centroid(&blob)?)?.into(),
        "envelope" => io::st_as_text(&accessors::st_envelope(&blob)?)?.into(),
        "x" => match accessors::st_x(&blob)? {
            Some(x) => x.into(),
            None => serde_json::Value::Null,
        },
        "y" => match accessors::st_y(&blob)? {
            Some(y) => y.into(),
            None => serde_json::Value::Null,
        },
        "numpoints" => match accessors::st_num_points(&blob)? {
            Some(n) => n.into(),
            None => serde_json::Value::Null,
        },
        "isvalid" => accessors::st_is_valid(&blob)?.into(),
        "simplify" => io::st_as_text(&accessors::st_simplify(&blob, v.arg.unwrap())?)?.into(),
        other => panic!("{}: unknown fn {other}", v.id),
    })
}

#[test]
fn golden_accessors_through_pure_functions() {
    for v in common::load("accessors") {
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
fn golden_accessors_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    for v in common::load("accessors") {
        let sql = match v.func.as_str() {
            "area" => "SELECT ST_Area(ST_GeomFromText(?1))",
            "length" => "SELECT ST_Length(ST_GeomFromText(?1))",
            "centroid" => "SELECT ST_AsText(ST_Centroid(ST_GeomFromText(?1)))",
            "envelope" => "SELECT ST_AsText(ST_Envelope(ST_GeomFromText(?1)))",
            "x" => "SELECT ST_X(ST_GeomFromText(?1))",
            "y" => "SELECT ST_Y(ST_GeomFromText(?1))",
            "numpoints" => "SELECT ST_NumPoints(ST_GeomFromText(?1))",
            "isvalid" => "SELECT ST_IsValid(ST_GeomFromText(?1))",
            "simplify" => "SELECT ST_AsText(ST_Simplify(ST_GeomFromText(?1), ?2))",
            other => panic!("{}: unknown fn {other}", v.id),
        };
        let result: rusqlite::Result<Value> = if v.func == "simplify" {
            conn.query_row(
                sql,
                rusqlite::params![v.a.as_ref().unwrap(), v.arg.unwrap()],
                |r| r.get(0),
            )
        } else {
            conn.query_row(sql, [v.a.as_ref().unwrap()], |r| r.get(0))
        };
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = match result.unwrap_or_else(|e| panic!("{}: {e}", v.id)) {
            Value::Null => serde_json::Value::Null,
            Value::Integer(i) if v.func == "isvalid" => serde_json::Value::Bool(i == 1),
            Value::Integer(i) => i.into(),
            Value::Real(f) => f.into(),
            Value::Text(s) => s.into(),
            other => panic!("{}: unexpected SQL value {other:?}", v.id),
        };
        check(&v.id, &v.func, &got, v.effective());
    }
}
