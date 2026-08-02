//! PostGIS name compatibility, exercised through SQL.
//!
//! Every expectation here was read off a live PostGIS 3.5 session
//! (`postgis/postgis:17-3.5`) rather than from the docs — including the ones
//! that are easy to guess wrong: the typed constructors return NULL on a type
//! mismatch instead of erroring, and `ST_AsEWKT` omits the `SRID=` prefix
//! when the SRID is unknown.

use rusqlite::Connection;

fn conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn
}

fn text(conn: &Connection, sql: &str) -> Option<String> {
    conn.query_row(sql, [], |r| r.get::<_, Option<String>>(0))
        .unwrap()
}

fn real(conn: &Connection, sql: &str) -> f64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn bbox_accessors_answer_to_both_spellings() {
    let conn = conn();
    // kenro uses the GeoPackage trigger names; PostGIS spells them ST_XMin.
    for (kenro_name, postgis_name) in [
        ("ST_MinX", "ST_XMin"),
        ("ST_MaxX", "ST_XMax"),
        ("ST_MinY", "ST_YMin"),
        ("ST_MaxY", "ST_YMax"),
    ] {
        let g = "ST_GeomFromText('LINESTRING(1 2,5 9)')";
        assert_eq!(
            real(&conn, &format!("SELECT {kenro_name}({g})")),
            real(&conn, &format!("SELECT {postgis_name}({g})")),
            "{kenro_name} vs {postgis_name}",
        );
    }
    // PostGIS 3.5: ST_XMin(ST_GeomFromText('LINESTRING(1 2,5 9)')) = 1
    assert_eq!(
        real(
            &conn,
            "SELECT ST_XMin(ST_GeomFromText('LINESTRING(1 2,5 9)'))"
        ),
        1.0
    );
}

#[test]
fn measure_and_constructor_aliases_share_their_originals() {
    let conn = conn();
    let poly = "ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))')";
    assert_eq!(
        real(&conn, &format!("SELECT ST_Area2D({poly})")),
        real(&conn, &format!("SELECT ST_Area({poly})"))
    );
    assert_eq!(
        real(&conn, &format!("SELECT ST_Perimeter2D({poly})")),
        real(&conn, &format!("SELECT ST_Perimeter({poly})"))
    );
    assert_eq!(
        real(
            &conn,
            "SELECT ST_Length2D(ST_GeomFromText('LINESTRING(0 0,3 4)'))"
        ),
        5.0
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_GeometryFromText('POINT(1 2)', 4326))"
        )
        .as_deref(),
        Some("POINT(1 2)")
    );
}

#[test]
fn symmetric_difference_accepts_both_postgis_spellings() {
    let conn = conn();
    let a = "ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')";
    let b = "ST_GeomFromText('POLYGON((1 1,3 1,3 3,1 3,1 1))')";
    let long = text(
        &conn,
        &format!("SELECT ST_AsText(ST_SymmetricDifference({a}, {b}))"),
    );
    let short = text(
        &conn,
        &format!("SELECT ST_AsText(ST_SymDifference({a}, {b}))"),
    );
    assert_eq!(long, short);
    assert!(long.unwrap().starts_with("MULTIPOLYGON"));
}

#[test]
fn ewkt_carries_the_srid_and_round_trips() {
    let conn = conn();
    // PostGIS 3.5: ST_AsEWKT(ST_GeomFromText('POINT(1 2)',4326))
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsEWKT(ST_GeomFromText('POINT(1 2)', 4326))"
        )
        .as_deref(),
        Some("SRID=4326;POINT(1 2)")
    );
    // …and with an unknown SRID, no prefix at all.
    assert_eq!(
        text(&conn, "SELECT ST_AsEWKT(ST_GeomFromText('POINT(1 2)'))").as_deref(),
        Some("POINT(1 2)")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsEWKT(ST_GeomFromEWKT('SRID=3857;POINT(1 2)'))"
        )
        .as_deref(),
        Some("SRID=3857;POINT(1 2)")
    );
    assert_eq!(
        conn.query_row(
            "SELECT ST_SRID(ST_GeomFromEWKT('SRID=3857;POINT(1 2)'))",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3857
    );
}

#[test]
fn hex_ewkb_is_byte_identical_to_postgis() {
    let conn = conn();
    // PostGIS 3.5: ST_AsHexEWKB(ST_GeomFromText('POINT(1 2)',4326))
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsHexEWKB(ST_GeomFromText('POINT(1 2)', 4326))"
        )
        .as_deref(),
        Some("0101000020E6100000000000000000F03F0000000000000040")
    );
    // ST_GeomFromEWKB reads back what PostGIS wrote, SRID included.
    assert_eq!(
        conn.query_row(
            "SELECT ST_SRID(ST_GeomFromEWKB(ST_AsEWKB(ST_GeomFromText('POINT(1 2)', 4326))))",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        4326
    );
}

#[test]
fn typed_constructors_return_null_on_a_type_mismatch() {
    let conn = conn();
    // PostGIS 3.5 returns NULL here rather than raising — verified live.
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_PointFromText('LINESTRING(0 0,1 1)'))"
        ),
        None
    );
    assert_eq!(
        text(&conn, "SELECT ST_AsText(ST_PointFromText('POINT(1 2)'))").as_deref(),
        Some("POINT(1 2)")
    );
    for (func, wkt) in [
        ("ST_LineFromText", "LINESTRING(0 0,1 1)"),
        ("ST_LineStringFromText", "LINESTRING(0 0,1 1)"),
        ("ST_PolyFromText", "POLYGON((0 0,1 0,1 1,0 1,0 0))"),
        ("ST_PolygonFromText", "POLYGON((0 0,1 0,1 1,0 1,0 0))"),
        ("ST_MPointFromText", "MULTIPOINT((1 2),(3 4))"),
        ("ST_MLineFromText", "MULTILINESTRING((0 0,1 1))"),
        ("ST_MPolyFromText", "MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))"),
    ] {
        assert!(
            text(&conn, &format!("SELECT ST_AsText({func}('{wkt}'))")).is_some(),
            "{func} rejected its own type"
        );
        assert!(
            text(&conn, &format!("SELECT ST_AsText({func}('POINT(9 9)'))")).is_none()
                || func == "ST_PointFromText",
            "{func} accepted a POINT"
        );
    }
    // The srid arity is applied, not ignored.
    assert_eq!(
        conn.query_row(
            "SELECT ST_SRID(ST_PointFromText('POINT(1 2)', 3857))",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3857
    );
}

#[test]
fn typed_wkb_constructors_match_the_text_ones() {
    let conn = conn();
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_PointFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)'))))"
        )
        .as_deref(),
        Some("POINT(1 2)")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_PointFromWKB(ST_AsBinary(ST_GeomFromText('LINESTRING(0 0,1 1)'))))"
        ),
        None
    );
}

#[test]
fn force_2d_is_the_only_way_a_3d_payload_reaches_an_encoder() {
    let conn = conn();
    // ISO WKB POINT Z (1 2 3) — kenro decodes 3D but refuses to encode it.
    let wkb_z: Vec<u8> = {
        let mut v = vec![0x01];
        v.extend_from_slice(&1001u32.to_le_bytes());
        for value in [1.0f64, 2.0, 3.0] {
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    };
    let err = conn
        .query_row("SELECT ST_AsText(?1)", [&wkb_z], |r| {
            r.get::<_, Option<String>>(0)
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("kenro:"), "{err}");

    let flat: String = conn
        .query_row("SELECT ST_AsText(ST_Force2D(?1))", [&wkb_z], |r| r.get(0))
        .unwrap();
    assert_eq!(flat, "POINT(1 2)");
}

#[test]
fn compat_functions_are_null_strict_like_everything_else() {
    let conn = conn();
    for sql in [
        "SELECT ST_XMin(NULL)",
        "SELECT ST_AsEWKT(NULL)",
        "SELECT ST_AsHexEWKB(NULL)",
        "SELECT ST_GeomFromEWKT(NULL)",
        "SELECT ST_Force2D(NULL)",
        "SELECT ST_PointFromText(NULL)",
        "SELECT ST_PointFromText(NULL, 4326)",
        "SELECT ST_PointFromWKB(NULL)",
    ] {
        let v: Option<String> = conn.query_row(sql, [], |r| r.get(0)).unwrap();
        assert!(v.is_none(), "{sql} was not NULL-strict");
    }
}

// ---- Structural accessors and editing (functions::edit) ----

fn int(conn: &Connection, sql: &str) -> Option<i64> {
    conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0))
        .unwrap()
}

#[test]
fn ring_accessors_use_postgis_indexing_and_null_rules() {
    let conn = conn();
    let holed = "ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))')";
    assert_eq!(
        text(
            &conn,
            &format!("SELECT ST_AsText(ST_ExteriorRing({holed}))")
        )
        .as_deref(),
        Some("LINESTRING(0 0,4 0,4 4,0 4,0 0)")
    );
    // Rings are 1-based; out of range is NULL, not an error.
    assert_eq!(
        text(
            &conn,
            &format!("SELECT ST_AsText(ST_InteriorRingN({holed}, 1))")
        )
        .as_deref(),
        Some("LINESTRING(1 1,2 1,2 2,1 2,1 1)")
    );
    assert_eq!(
        text(
            &conn,
            &format!("SELECT ST_AsText(ST_InteriorRingN({holed}, 2))")
        ),
        None
    );
    assert_eq!(
        int(&conn, &format!("SELECT ST_NumInteriorRings({holed})")),
        Some(1)
    );
    assert_eq!(
        int(&conn, &format!("SELECT ST_NumInteriorRing({holed})")),
        Some(1)
    );
    assert_eq!(int(&conn, &format!("SELECT ST_NRings({holed})")), Some(2));
    // Wrong type → NULL (PostGIS 3.5, verified live).
    assert_eq!(
        int(
            &conn,
            "SELECT ST_NumInteriorRings(ST_GeomFromText('POINT(0 0)'))"
        ),
        None
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_ExteriorRing(ST_GeomFromText('LINESTRING(0 0,1 1)')))"
        ),
        None
    );
}

#[test]
fn boundary_matches_postgis_including_the_empty_cases() {
    let conn = conn();
    for (input, expected) in [
        (
            "POLYGON((0 0,4 0,4 4,0 4,0 0))",
            "LINESTRING(0 0,4 0,4 4,0 4,0 0)",
        ),
        (
            "POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))",
            "MULTILINESTRING((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))",
        ),
        ("LINESTRING(0 0,1 1,2 0)", "MULTIPOINT((0 0),(2 0))"),
        ("LINESTRING(0 0,1 1,1 0,0 0)", "MULTIPOINT EMPTY"),
        ("POINT(1 1)", "POINT EMPTY"),
    ] {
        assert_eq!(
            text(
                &conn,
                &format!("SELECT ST_AsText(ST_Boundary(ST_GeomFromText('{input}')))")
            )
            .as_deref(),
            Some(expected),
            "ST_Boundary({input})"
        );
    }
}

#[test]
fn is_ring_raises_on_non_linear_input_like_postgis() {
    let conn = conn();
    assert_eq!(
        int(
            &conn,
            "SELECT ST_IsRing(ST_GeomFromText('LINESTRING(0 0,1 1,1 0,0 0)'))"
        ),
        Some(1)
    );
    assert_eq!(
        int(
            &conn,
            "SELECT ST_IsClosed(ST_GeomFromText('LINESTRING(0 0,1 1)'))"
        ),
        Some(0)
    );
    // The one function in this group that errors rather than returning NULL.
    let err = conn
        .query_row("SELECT ST_IsRing(ST_GeomFromText('POINT(0 0)'))", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("linear feature"), "{err}");
}

#[test]
fn vertex_surgery_through_sql() {
    let conn = conn();
    let line = "ST_GeomFromText('LINESTRING(0 0,1 1)')";
    assert_eq!(
        text(
            &conn,
            &format!("SELECT ST_AsText(ST_AddPoint({line}, ST_GeomFromText('POINT(2 2)')))")
        )
        .as_deref(),
        Some("LINESTRING(0 0,1 1,2 2)")
    );
    assert_eq!(
        text(
            &conn,
            &format!("SELECT ST_AsText(ST_AddPoint({line}, ST_GeomFromText('POINT(9 9)'), 0))")
        )
        .as_deref(),
        Some("LINESTRING(9 9,0 0,1 1)")
    );
    assert_eq!(
        text(
            &conn,
            &format!("SELECT ST_AsText(ST_SetPoint({line}, 0, ST_GeomFromText('POINT(9 9)')))")
        )
        .as_deref(),
        Some("LINESTRING(9 9,1 1)")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_RemovePoint(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'), 0))"
        )
        .as_deref(),
        Some("LINESTRING(1 1,2 2)")
    );
}

#[test]
fn constructors_and_coordinate_ops_through_sql() {
    let conn = conn();
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_MakeLine(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(1 1)')))"
        )
        .as_deref(),
        Some("LINESTRING(0 0,1 1)")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_MakePolygon(ST_GeomFromText('LINESTRING(0 0,1 0,1 1,0 0)')))"
        )
        .as_deref(),
        Some("POLYGON((0 0,1 0,1 1,0 0))")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_Multi(ST_GeomFromText('POINT(1 2)')))"
        )
        .as_deref(),
        Some("MULTIPOINT((1 2))")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_SnapToGrid(ST_GeomFromText('POINT(1.23 4.57)'), 0.5))"
        )
        .as_deref(),
        Some("POINT(1 4.5)")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_FlipCoordinates(ST_GeomFromText('POINT(1 2)')))"
        )
        .as_deref(),
        Some("POINT(2 1)")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_Expand(ST_GeomFromText('POINT(1 1)'), 2))"
        )
        .as_deref(),
        Some("POLYGON((-1 -1,-1 3,3 3,3 -1,-1 -1))")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_ShiftLongitude(ST_GeomFromText('POINT(-10 5)')))"
        )
        .as_deref(),
        Some("POINT(350 5)")
    );
}

#[test]
fn edit_functions_are_null_strict() {
    let conn = conn();
    for sql in [
        "SELECT ST_ExteriorRing(NULL)",
        "SELECT ST_InteriorRingN(NULL, 1)",
        "SELECT ST_Boundary(NULL)",
        "SELECT ST_IsClosed(NULL)",
        "SELECT ST_AddPoint(NULL, NULL)",
        "SELECT ST_SnapToGrid(NULL, 1.0)",
        "SELECT ST_Expand(NULL, 1.0)",
    ] {
        let v: Option<Vec<u8>> = conn.query_row(sql, [], |r| r.get(0)).unwrap();
        assert!(v.is_none(), "{sql} was not NULL-strict");
    }
}
