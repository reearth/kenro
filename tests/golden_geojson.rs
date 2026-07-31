//! Golden tests for ST_AsGeoJSON (byte-for-byte string equality against
//! PostGIS) and ST_GeomFromGeoJSON.

mod common;

use kenro::functions::{geojson, io};
use rusqlite::Connection;

#[test]
fn golden_geojson_through_pure_functions() {
    for v in common::load("geojson") {
        match v.func.as_str() {
            "asgeojson" => {
                let blob = io::st_geom_from_text(v.a.as_ref().unwrap(), v.srid.filter(|s| *s != 0))
                    .unwrap();
                let result = geojson::st_as_geojson(&blob, v.arg.map(|d| d as i64));
                if v.expects_error() {
                    assert!(result.is_err(), "{}: expected an error", v.id);
                    continue;
                }
                let got = result.unwrap_or_else(|e| panic!("{}: {e}", v.id));
                assert_eq!(&got, v.effective().as_str().unwrap(), "{}", v.id);
            }
            "fromgeojson" => {
                let result = geojson::st_geom_from_geojson(v.a.as_ref().unwrap());
                if v.expects_error() {
                    assert!(result.is_err(), "{}: expected an error", v.id);
                    continue;
                }
                let blob = result.unwrap_or_else(|e| panic!("{}: {e}", v.id));
                let got = io::st_as_text(&blob).unwrap();
                let want = v.effective().as_str().unwrap();
                assert!(
                    common::geoms_approx_equal(&got, want, 1e-12),
                    "{}: got {got}, want {want}",
                    v.id
                );
                assert_eq!(
                    io::st_srid(&blob).unwrap(),
                    v.expected_srid.unwrap(),
                    "{}",
                    v.id
                );
            }
            other => panic!("{}: unknown fn {other}", v.id),
        }
    }
}

#[test]
fn golden_geojson_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    for v in common::load("geojson") {
        let result: rusqlite::Result<String> = match v.func.as_str() {
            "asgeojson" => match v.arg {
                Some(digits) => conn.query_row(
                    "SELECT ST_AsGeoJSON(ST_SetSRID(ST_GeomFromText(?1), ?2), ?3)",
                    rusqlite::params![v.a.as_ref().unwrap(), v.srid.unwrap_or(0), digits as i64],
                    |r| r.get(0),
                ),
                None => conn.query_row(
                    "SELECT ST_AsGeoJSON(ST_SetSRID(ST_GeomFromText(?1), ?2))",
                    rusqlite::params![v.a.as_ref().unwrap(), v.srid.unwrap_or(0)],
                    |r| r.get(0),
                ),
            },
            "fromgeojson" => conn.query_row(
                "SELECT ST_AsText(ST_GeomFromGeoJSON(?1))",
                [v.a.as_ref().unwrap()],
                |r| r.get(0),
            ),
            other => panic!("{}: unknown fn {other}", v.id),
        };
        if v.expects_error() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = result.unwrap_or_else(|e| panic!("{}: {e}", v.id));
        let want = v.effective().as_str().unwrap();
        if v.func == "asgeojson" {
            assert_eq!(&got, want, "{}", v.id);
        } else {
            assert!(
                common::geoms_approx_equal(&got, want, 1e-12),
                "{}: got {got}, want {want}",
                v.id
            );
        }
    }
}
