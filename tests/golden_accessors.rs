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

fn opt_astext(blob: Option<Vec<u8>>) -> Result<serde_json::Value, kenro::Error> {
    Ok(match blob {
        Some(b) => io::st_as_text(&b)?.into(),
        None => serde_json::Value::Null,
    })
}

fn run_pure(v: &Vector) -> Result<serde_json::Value, kenro::Error> {
    // Constructors take numeric args instead of an input geometry.
    let args = v.args.clone().unwrap_or_default();
    match v.func.as_str() {
        "makepoint" => return Ok(io::st_as_text(&io::st_make_point(args[0], args[1])?)?.into()),
        "point" => return Ok(io::st_as_text(&io::st_point(args[0], args[1], None)?)?.into()),
        "point_srid" => {
            let blob = io::st_point(args[0], args[1], v.srid)?;
            return Ok(io::st_srid(&blob)?.into());
        }
        "makeenvelope" => {
            let blob = io::st_make_envelope(args[0], args[1], args[2], args[3], None)?;
            return Ok(io::st_as_text(&blob)?.into());
        }
        "makeenvelope_srid" => {
            let blob = io::st_make_envelope(args[0], args[1], args[2], args[3], v.srid)?;
            return Ok(io::st_srid(&blob)?.into());
        }
        _ => {}
    }
    let blob = io::st_geom_from_text(v.a.as_ref().unwrap(), None)?;
    Ok(match v.func.as_str() {
        "area" => accessors::st_area(&blob)?.into(),
        "npoints" => accessors::st_npoints(&blob)?.into(),
        "perimeter" => accessors::st_perimeter(&blob)?.into(),
        "geomtype" => accessors::st_geometry_type(&blob)?.into(),
        "numgeoms" => accessors::st_num_geometries(&blob)?.into(),
        "geometryn" => opt_astext(accessors::st_geometry_n(&blob, v.arg.unwrap() as i64)?)?,
        "startpoint" => opt_astext(accessors::st_start_point(&blob)?)?,
        "endpoint" => opt_astext(accessors::st_end_point(&blob)?)?,
        "pointn" => opt_astext(accessors::st_point_n(&blob, v.arg.unwrap() as i64)?)?,
        "reverse" => io::st_as_text(&accessors::st_reverse(&blob)?)?.into(),
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
            "npoints" => "SELECT ST_NPoints(ST_GeomFromText(?1))",
            "perimeter" => "SELECT ST_Perimeter(ST_GeomFromText(?1))",
            "geomtype" => "SELECT ST_GeometryType(ST_GeomFromText(?1))",
            "numgeoms" => "SELECT ST_NumGeometries(ST_GeomFromText(?1))",
            "geometryn" => "SELECT ST_AsText(ST_GeometryN(ST_GeomFromText(?1), ?2))",
            "startpoint" => "SELECT ST_AsText(ST_StartPoint(ST_GeomFromText(?1)))",
            "endpoint" => "SELECT ST_AsText(ST_EndPoint(ST_GeomFromText(?1)))",
            "pointn" => "SELECT ST_AsText(ST_PointN(ST_GeomFromText(?1), ?2))",
            "reverse" => "SELECT ST_AsText(ST_Reverse(ST_GeomFromText(?1)))",
            "makepoint" => "SELECT ST_AsText(ST_MakePoint(?1, ?2))",
            "point" => "SELECT ST_AsText(ST_Point(?1, ?2))",
            "point_srid" => "SELECT ST_SRID(ST_Point(?1, ?2, ?3))",
            "makeenvelope" => "SELECT ST_AsText(ST_MakeEnvelope(?1, ?2, ?3, ?4))",
            "makeenvelope_srid" => "SELECT ST_SRID(ST_MakeEnvelope(?1, ?2, ?3, ?4, ?5))",
            other => panic!("{}: unknown fn {other}", v.id),
        };
        let args = v.args.clone().unwrap_or_default();
        let result: rusqlite::Result<Value> = match v.func.as_str() {
            "simplify" => conn.query_row(
                sql,
                rusqlite::params![v.a.as_ref().unwrap(), v.arg.unwrap()],
                |r| r.get(0),
            ),
            "geometryn" | "pointn" => conn.query_row(
                sql,
                rusqlite::params![v.a.as_ref().unwrap(), v.arg.unwrap() as i64],
                |r| r.get(0),
            ),
            "makepoint" | "point" => {
                conn.query_row(sql, rusqlite::params![args[0], args[1]], |r| r.get(0))
            }
            "point_srid" => conn.query_row(
                sql,
                rusqlite::params![args[0], args[1], v.srid.unwrap()],
                |r| r.get(0),
            ),
            "makeenvelope" => conn.query_row(
                sql,
                rusqlite::params![args[0], args[1], args[2], args[3]],
                |r| r.get(0),
            ),
            "makeenvelope_srid" => conn.query_row(
                sql,
                rusqlite::params![args[0], args[1], args[2], args[3], v.srid.unwrap()],
                |r| r.get(0),
            ),
            _ => conn.query_row(sql, [v.a.as_ref().unwrap()], |r| r.get(0)),
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
