//! Golden tests: every vector in tests/golden/predicates.jsonl (generated
//! from the reference PostGIS by scripts/golden/generate.sh and committed)
//! is run twice — once through SQL on a registered connection (exercising
//! binding + core together) and once through the pure functions (localizing
//! failures).
//!
//! A vector carrying `kenro_expected` is a documented divergence: kenro's
//! value is asserted against it, and the `note` explains why. That file is
//! mechanically the source of truth for the README's PostGIS diff table.

use rusqlite::Connection;
use rusqlite::types::Value;
use serde::Deserialize;

use kenro::functions::{io, predicates};

#[derive(Deserialize)]
struct Vector {
    id: String,
    a: String,
    #[serde(default)]
    b: Option<String>,
    #[serde(rename = "fn")]
    func: String,
    #[serde(default)]
    arg: Option<f64>,
    expected: serde_json::Value,
    #[serde(default)]
    kenro_expected: Option<serde_json::Value>,
    #[serde(default)]
    #[allow(dead_code)]
    note: Option<String>,
}

fn load_vectors() -> Vec<Vector> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/predicates.jsonl");
    let content = std::fs::read_to_string(path).expect(
        "tests/golden/predicates.jsonl missing — run scripts/golden/generate.sh to create it",
    );
    let mut vectors = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line).expect(line);
        if value.get("fn").is_none() {
            continue; // provenance header record
        }
        vectors.push(serde_json::from_value(value).expect(line));
    }
    assert!(vectors.len() >= 100, "suspiciously few vectors");
    vectors
}

/// The value kenro must produce: the documented-divergence override if
/// present, otherwise the PostGIS value.
fn effective(v: &Vector) -> &serde_json::Value {
    v.kenro_expected.as_ref().unwrap_or(&v.expected)
}

fn assert_number(id: &str, got: f64, want: f64) {
    let tol = 1e-12 * want.abs().max(1.0);
    assert!(
        (got - want).abs() <= tol,
        "{id}: got {got}, want {want} (tolerance {tol})"
    );
}

#[test]
fn golden_vectors_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();

    for v in load_vectors() {
        let want = effective(&v).clone();
        let sql = match v.func.as_str() {
            "intersects" => "SELECT ST_Intersects(ST_GeomFromText(?1), ST_GeomFromText(?2))",
            "contains" => "SELECT ST_Contains(ST_GeomFromText(?1), ST_GeomFromText(?2))",
            "within" => "SELECT ST_Within(ST_GeomFromText(?1), ST_GeomFromText(?2))",
            "distance" => "SELECT ST_Distance(ST_GeomFromText(?1), ST_GeomFromText(?2))",
            "dwithin" => "SELECT ST_DWithin(ST_GeomFromText(?1), ST_GeomFromText(?2), ?3)",
            "astext" => "SELECT ST_AsText(ST_GeomFromText(?1))",
            other => panic!("{}: unknown fn {other}", v.id),
        };
        let result: rusqlite::Result<Value> = match v.func.as_str() {
            "astext" => conn.query_row(sql, [&v.a], |r| r.get(0)),
            "dwithin" => conn.query_row(
                sql,
                rusqlite::params![&v.a, v.b.as_ref().unwrap(), v.arg.unwrap()],
                |r| r.get(0),
            ),
            _ => conn.query_row(sql, [&v.a, v.b.as_ref().unwrap()], |r| r.get(0)),
        };

        if want.get("error").is_some() {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = result.unwrap_or_else(|e| panic!("{}: {e}", v.id));
        match (&got, &want) {
            (Value::Null, serde_json::Value::Null) => {}
            (Value::Integer(i), serde_json::Value::Bool(b)) => {
                assert_eq!(*i == 1, *b, "{}", v.id)
            }
            (Value::Real(f), serde_json::Value::Number(n)) => {
                assert_number(&v.id, *f, n.as_f64().unwrap())
            }
            (Value::Integer(i), serde_json::Value::Number(n)) => {
                assert_number(&v.id, *i as f64, n.as_f64().unwrap())
            }
            (Value::Text(s), serde_json::Value::String(w)) => assert_eq!(s, w, "{}", v.id),
            (got, want) => panic!("{}: type mismatch: got {got:?}, want {want}", v.id),
        }
    }
}

#[test]
fn golden_vectors_through_pure_functions() {
    for v in load_vectors() {
        let want = effective(&v).clone();
        let expect_error = want.get("error").is_some();

        let result: Result<serde_json::Value, kenro::Error> = (|| {
            let ga = io::st_geom_from_text(&v.a, None)?;
            Ok(match v.func.as_str() {
                "astext" => serde_json::Value::String(io::st_as_text(&ga)?),
                "intersects" | "contains" | "within" | "distance" | "dwithin" => {
                    let gb = io::st_geom_from_text(v.b.as_ref().unwrap(), None)?;
                    match v.func.as_str() {
                        "intersects" => predicates::st_intersects(&ga, &gb)?.into(),
                        "contains" => predicates::st_contains(&ga, &gb)?.into(),
                        "within" => predicates::st_within(&ga, &gb)?.into(),
                        "dwithin" => predicates::st_dwithin(&ga, &gb, v.arg.unwrap())?.into(),
                        _ => match predicates::st_distance(&ga, &gb)? {
                            Some(d) => serde_json::json!(d),
                            None => serde_json::Value::Null,
                        },
                    }
                }
                other => panic!("{}: unknown fn {other}", v.id),
            })
        })();

        if expect_error {
            assert!(result.is_err(), "{}: expected an error", v.id);
            continue;
        }
        let got = result.unwrap_or_else(|e| panic!("{}: {e}", v.id));
        match (&got, &want) {
            (serde_json::Value::Number(g), serde_json::Value::Number(w)) => {
                assert_number(&v.id, g.as_f64().unwrap(), w.as_f64().unwrap())
            }
            _ => assert_eq!(got, want, "{}", v.id),
        }
    }
}
