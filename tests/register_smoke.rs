//! Every registered function is callable through SQL; stubs error with their
//! hint text; NULL in → NULL out for every implemented function.

use rusqlite::Connection;
use rusqlite::types::Value;

fn conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn
}

fn query_value(conn: &Connection, sql: &str) -> rusqlite::Result<Value> {
    conn.query_row(sql, [], |r| r.get::<_, Value>(0))
}

#[test]
fn every_implemented_function_is_callable() {
    let conn = conn();
    let cases = [
        "SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))",
        "SELECT ST_AsText(ST_GeomFromText('POINT(1 2)', 4326))",
        "SELECT ST_AsText(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)'))))",
        "SELECT ST_AsText(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)')), 4326))",
        "SELECT ST_AsText(ST_GeomFromGPB(ST_AsGPB(ST_GeomFromText('POINT(1 2)'))))",
        "SELECT ST_Intersects(ST_GeomFromText('POINT(1 1)'), ST_GeomFromText('POINT(1 1)'))",
        "SELECT ST_Contains(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), ST_GeomFromText('POINT(1 1)'))",
        "SELECT ST_Within(ST_GeomFromText('POINT(1 1)'), ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))",
        "SELECT ST_Distance(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)'))",
        "SELECT ST_DWithin(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)'), 5.0)",
        "SELECT ST_MinX(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
        "SELECT ST_MaxX(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
        "SELECT ST_MinY(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
        "SELECT ST_MaxY(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
        "SELECT ST_IsEmpty(ST_GeomFromText('LINESTRING(1 2,3 4)'))",
    ];
    for sql in cases {
        assert!(
            !matches!(query_value(&conn, sql).unwrap(), Value::Null),
            "{sql}"
        );
    }
}

#[test]
fn results_are_correct_through_sql() {
    let conn = conn();
    let wkt: String = conn
        .query_row("SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(wkt, "POINT(1 2)");
    let d: f64 = conn
        .query_row(
            "SELECT ST_Distance(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)'))",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(d, 5.0);
    let hit: i64 = conn
        .query_row(
            "SELECT ST_Intersects(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'),
                                  ST_GeomFromText('POINT(10 5)'))",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hit, 1);
}

#[test]
fn null_in_null_out_for_every_function() {
    let conn = conn();
    let cases = [
        "SELECT ST_GeomFromText(NULL)",
        "SELECT ST_GeomFromText(NULL, 4326)",
        "SELECT ST_GeomFromText('POINT(1 2)', NULL)",
        "SELECT ST_GeomFromWKB(NULL)",
        "SELECT ST_GeomFromGPB(NULL)",
        "SELECT ST_AsText(NULL)",
        "SELECT ST_AsBinary(NULL)",
        "SELECT ST_AsGPB(NULL)",
        "SELECT ST_Intersects(NULL, ST_GeomFromText('POINT(1 1)'))",
        "SELECT ST_Intersects(ST_GeomFromText('POINT(1 1)'), NULL)",
        "SELECT ST_Contains(NULL, NULL)",
        "SELECT ST_Within(NULL, NULL)",
        "SELECT ST_Distance(NULL, NULL)",
        "SELECT ST_DWithin(NULL, NULL, 1.0)",
        "SELECT ST_DWithin(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(0 0)'), NULL)",
        "SELECT ST_MinX(NULL)",
        "SELECT ST_MaxX(NULL)",
        "SELECT ST_MinY(NULL)",
        "SELECT ST_MaxY(NULL)",
        "SELECT ST_IsEmpty(NULL)",
    ];
    for sql in cases {
        assert!(
            matches!(query_value(&conn, sql).unwrap(), Value::Null),
            "{sql}"
        );
    }
}

#[test]
fn stubs_error_with_helpful_hints() {
    let conn = conn();
    let err = query_value(
        &conn,
        "SELECT ST_Buffer(ST_GeomFromText('POINT(0 0)'), 1.0)",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("ST_Buffer is not implemented"), "{err}");
    assert!(err.contains("SpatiaLite or DuckDB spatial"), "{err}");

    let err = query_value(
        &conn,
        "SELECT ST_Transform(ST_GeomFromText('POINT(0 0)'), 4326)",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("Planned for kenro 0.2"), "{err}");

    // Stubs are loud for any arity, including NULL args.
    assert!(query_value(&conn, "SELECT ST_Area(NULL)").is_err());
    assert!(query_value(&conn, "SELECT ST_Centroid()").is_err());
}

#[test]
fn errors_are_attributable_and_actionable() {
    let conn = conn();
    // TEXT where a geometry BLOB is expected → hint toward ST_GeomFromText.
    let err = query_value(&conn, "SELECT ST_AsText('POINT(1 2)')")
        .unwrap_err()
        .to_string();
    assert!(err.contains("kenro:"), "{err}");
    assert!(err.contains("ST_GeomFromText"), "{err}");

    // Garbage blob → invalid, not NULL.
    let err = query_value(&conn, "SELECT ST_AsText(x'DEADBEEF')")
        .unwrap_err()
        .to_string();
    assert!(err.contains("kenro:"), "{err}");

    // Mixed SRIDs → explicit error naming both.
    let err = query_value(
        &conn,
        "SELECT ST_Intersects(ST_GeomFromText('POINT(0 0)', 4326),
                              ST_GeomFromText('POINT(0 0)', 6668))",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("mixed SRIDs 4326 and 6668"), "{err}");
}

#[test]
fn functions_run_under_trusted_schema_off() {
    // The gpkg rtree triggers run kenro functions from schema context;
    // SQLITE_INNOCUOUS must make that legal under trusted_schema=off.
    let conn = conn();
    conn.pragma_update(None, "trusted_schema", false).unwrap();
    conn.execute_batch(
        "CREATE TABLE t(id INTEGER PRIMARY KEY, geom BLOB);
         CREATE TRIGGER trg AFTER INSERT ON t
         BEGIN
           SELECT RAISE(ABORT, 'geometry is empty') WHERE ST_IsEmpty(NEW.geom);
         END;",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO t(geom) VALUES (ST_AsGPB(ST_GeomFromText('POINT(1 2)')))",
        [],
    )
    .unwrap();
    let err = conn
        .execute(
            "INSERT INTO t(geom) VALUES (ST_AsGPB(ST_GeomFromText('POLYGON EMPTY')))",
            [],
        )
        .unwrap_err();
    assert!(err.to_string().contains("geometry is empty"), "{err}");
}
