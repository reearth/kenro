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
        "SELECT ST_SRID(ST_SetSRID(ST_GeomFromText('POINT(1 2)'), 4326))",
        "SELECT ST_AsText(ST_Transform(ST_GeomFromText('POINT(139.7 35.7)', 4326), 6677))",
        "SELECT h3_latlng_to_cell(ST_GeomFromText('POINT(139.7 35.7)', 4326), 9)",
        "SELECT h3_cell_to_parent(h3_latlng_to_cell(ST_GeomFromText('POINT(139.7 35.7)', 4326), 9), 5)",
        "SELECT h3_string_to_cell(h3_cell_to_string(h3_latlng_to_cell(ST_GeomFromText('POINT(139.7 35.7)', 4326), 9)))",
        "SELECT ST_AsGeoJSON(ST_GeomFromText('POINT(1 2)'))",
        "SELECT ST_AsGeoJSON(ST_GeomFromText('POINT(1 2)'), 3)",
        "SELECT ST_AsText(ST_GeomFromGeoJSON('{\"type\":\"Point\",\"coordinates\":[1,2]}'))",
        "SELECT ST_Area(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
        "SELECT ST_Length(ST_GeomFromText('LINESTRING(0 0,3 4)'))",
        "SELECT ST_AsText(ST_Centroid(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')))",
        "SELECT ST_AsText(ST_Envelope(ST_GeomFromText('LINESTRING(0 0,3 4)')))",
        "SELECT ST_X(ST_GeomFromText('POINT(3 4)'))",
        "SELECT ST_Y(ST_GeomFromText('POINT(3 4)'))",
        "SELECT ST_NumPoints(ST_GeomFromText('LINESTRING(0 0,1 1)'))",
        "SELECT ST_IsValid(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
        "SELECT ST_AsText(ST_Simplify(ST_GeomFromText('LINESTRING(0 0,1 0.001,2 0)'), 0.1))",
        "SELECT ST_Disjoint(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(9 9)'))",
        "SELECT ST_Relate(ST_GeomFromText('POINT(1 1)'), ST_GeomFromText('POINT(1 1)'))",
        "SELECT ST_Relate(ST_GeomFromText('POINT(1 1)'), ST_GeomFromText('POINT(1 1)'), '0FFFFFFF2')",
        "SELECT ST_NPoints(ST_GeomFromText('LINESTRING(0 0,1 1)'))",
        "SELECT ST_Perimeter(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
        "SELECT ST_GeometryType(ST_GeomFromText('POINT(1 2)'))",
        "SELECT ST_NumGeometries(ST_GeomFromText('MULTIPOINT(1 2,3 4)'))",
        "SELECT ST_AsText(ST_GeometryN(ST_GeomFromText('MULTIPOINT(1 2,3 4)'), 1))",
        "SELECT ST_AsText(ST_StartPoint(ST_GeomFromText('LINESTRING(1 2,3 4)')))",
        "SELECT ST_AsText(ST_EndPoint(ST_GeomFromText('LINESTRING(1 2,3 4)')))",
        "SELECT ST_AsText(ST_PointN(ST_GeomFromText('LINESTRING(1 2,3 4)'), 2))",
        "SELECT ST_AsText(ST_Reverse(ST_GeomFromText('LINESTRING(1 2,3 4)')))",
        "SELECT ST_AsText(ST_MakePoint(1.5, 2.5))",
        "SELECT ST_AsText(ST_Point(1, 2))",
        "SELECT ST_SRID(ST_Point(1, 2, 4326))",
        "SELECT ST_AsText(ST_MakeEnvelope(0, 0, 2, 3))",
        "SELECT ST_SRID(ST_MakeEnvelope(0, 0, 2, 3, 4326))",
        "SELECT GPKG_IsAssignable('GEOMETRY', 'POINT')",
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
        "SELECT ST_SetSRID(NULL, 4326)",
        "SELECT ST_SetSRID(ST_GeomFromText('POINT(1 2)'), NULL)",
        "SELECT ST_SRID(NULL)",
        "SELECT ST_Transform(NULL, 4326)",
        "SELECT ST_Transform(ST_GeomFromText('POINT(1 2)', 4326), NULL)",
        "SELECT h3_latlng_to_cell(NULL, 9)",
        "SELECT h3_cell_to_parent(NULL, 5)",
        "SELECT h3_cell_to_string(NULL)",
        "SELECT h3_string_to_cell(NULL)",
        "SELECT ST_AsGeoJSON(NULL)",
        "SELECT ST_GeomFromGeoJSON(NULL)",
        "SELECT ST_Area(NULL)",
        "SELECT ST_Length(NULL)",
        "SELECT ST_Centroid(NULL)",
        "SELECT ST_Envelope(NULL)",
        "SELECT ST_X(NULL)",
        "SELECT ST_Y(NULL)",
        "SELECT ST_NumPoints(NULL)",
        "SELECT ST_IsValid(NULL)",
        "SELECT ST_Simplify(NULL, 0.1)",
        "SELECT ST_Disjoint(NULL, NULL)",
        "SELECT ST_Relate(NULL, NULL)",
        "SELECT ST_Relate(NULL, NULL, 'T*F**F***')",
        "SELECT ST_NPoints(NULL)",
        "SELECT ST_Perimeter(NULL)",
        "SELECT ST_GeometryType(NULL)",
        "SELECT ST_NumGeometries(NULL)",
        "SELECT ST_GeometryN(NULL, 1)",
        "SELECT ST_StartPoint(NULL)",
        "SELECT ST_EndPoint(NULL)",
        "SELECT ST_PointN(NULL, 1)",
        "SELECT ST_Reverse(NULL)",
        "SELECT ST_MakePoint(NULL, 2)",
        "SELECT ST_Point(1, NULL)",
        "SELECT ST_MakeEnvelope(NULL, 0, 2, 3)",
        "SELECT GPKG_IsAssignable(NULL, 'POINT')",
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
    let err = query_value(&conn, "SELECT ST_MakeValid(ST_GeomFromText('POINT(0 0)'))")
        .unwrap_err()
        .to_string();
    assert!(err.contains("ST_MakeValid is not implemented"), "{err}");
    assert!(err.contains("ST_IsValid"), "{err}");

    // Stubs are loud for any arity, including NULL args.
    assert!(query_value(&conn, "SELECT ST_AsMVTGeom(NULL, NULL)").is_err());
    assert!(query_value(&conn, "SELECT ST_MakeValid(NULL)").is_err());

    let err = query_value(&conn, "SELECT ST_AsMVT(ST_GeomFromText('POINT(0 0)'))")
        .unwrap_err()
        .to_string();
    assert!(err.contains("tippecanoe"), "{err}");
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
fn union_aggregate_dissolves_per_group() {
    let conn = conn();
    conn.execute_batch(
        "CREATE TABLE zones (grp TEXT, geom BLOB);
         INSERT INTO zones VALUES
           ('a', ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))')),
           ('a', ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')),
           ('b', ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')),
           ('b', NULL);",
    )
    .unwrap();
    let mut stmt = conn
        .prepare("SELECT grp, ST_Area(ST_Union(geom)) FROM zones GROUP BY grp ORDER BY grp")
        .unwrap();
    let rows: Vec<(String, f64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!((rows[0].1 - 175.0).abs() < 1e-6, "{}", rows[0].1);
    assert!((rows[1].1 - 4.0).abs() < 1e-6, "{}", rows[1].1); // NULL row skipped
    // Zero rows aggregated → SQL NULL.
    let empty: Value = conn
        .query_row(
            "SELECT ST_Union(geom) FROM zones WHERE grp = 'nope'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(matches!(empty, Value::Null));
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
