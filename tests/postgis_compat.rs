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
#[cfg(feature = "overlay")]
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

// ---- Sphere/spheroid, dimension, orientation, linear referencing ----

const WGS84_SPHEROID: &str = "SPHEROID[\"WGS 84\",6378137,298.257223563]";

#[test]
fn geodesic_measures_answer_in_metres_where_the_planar_ones_answer_in_degrees() {
    let conn = conn();
    let a = "ST_GeomFromText('POINT(0 0)', 4326)";
    let b = "ST_GeomFromText('POINT(1 0)', 4326)";
    // The trap this group exists to fix: ST_Distance on 4326 is degrees.
    assert_eq!(real(&conn, &format!("SELECT ST_Distance({a}, {b})")), 1.0);
    // PostGIS 3.5: 111195.07973463 — spherical, available in every build.
    assert!(
        (real(&conn, &format!("SELECT ST_DistanceSphere({a}, {b})")) - 111_195.079_734_63).abs()
            < 1e-3
    );
    spheroid_measures(&conn, a, b);
}

/// The ellipsoidal half sits behind the `spheroid` feature (it pulls
/// geographiclib). Both states are asserted, so a build without the feature
/// has to fail loudly rather than quietly answer with the sphere.
#[cfg(feature = "spheroid")]
fn spheroid_measures(conn: &Connection, a: &str, b: &str) {
    // PostGIS 3.5: 111319.49079327357
    assert!(
        (real(conn, &format!("SELECT ST_DistanceSpheroid({a}, {b})")) - 111_319.490_793_273_57)
            .abs()
            < 1e-3
    );
    assert!(
        (real(
            conn,
            &format!("SELECT ST_DistanceSpheroid({a}, {b}, '{WGS84_SPHEROID}')")
        ) - 111_319.490_793_273_57)
            .abs()
            < 1e-3
    );
    assert!(
        (real(
            conn,
            &format!("SELECT ST_LengthSpheroid(ST_GeomFromText('LINESTRING(0 0,1 0)', 4326), '{WGS84_SPHEROID}')")
        ) - 111_319.490_793_273_57)
            .abs()
            < 1e-3
    );
    // A malformed spheroid is a loud error, not a silent default.
    assert!(
        conn.query_row(
            "SELECT ST_LengthSpheroid(ST_GeomFromText('LINESTRING(0 0,1 0)', 4326), 'WGS 84')",
            [],
            |r| r.get::<_, f64>(0)
        )
        .is_err()
    );
}

#[cfg(not(feature = "spheroid"))]
fn spheroid_measures(conn: &Connection, a: &str, b: &str) {
    let err = conn
        .query_row(&format!("SELECT ST_DistanceSpheroid({a}, {b})"), [], |r| {
            r.get::<_, f64>(0)
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("spheroid"), "{err}");
}

#[test]
fn dimension_and_validity_reporting() {
    let conn = conn();
    assert_eq!(
        int(&conn, "SELECT ST_Dimension(ST_GeomFromText('POINT(0 0)'))"),
        Some(0)
    );
    assert_eq!(
        int(
            &conn,
            "SELECT ST_Dimension(ST_GeomFromText('LINESTRING(0 0,1 1)'))"
        ),
        Some(1)
    );
    assert_eq!(
        int(
            &conn,
            "SELECT ST_Dimension(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 0))'))"
        ),
        Some(2)
    );
    assert_eq!(
        int(&conn, "SELECT ST_CoordDim(ST_GeomFromText('POINT(1 2)'))"),
        Some(2)
    );
    assert_eq!(
        int(&conn, "SELECT ST_NDims(ST_GeomFromText('POINT(1 2)'))"),
        Some(2)
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_IsValidReason(ST_GeomFromText('POINT(1 1)'))"
        )
        .as_deref(),
        Some("Valid Geometry")
    );
    // Wording is geo's, not PostGIS's — documented divergence; what matters
    // is that a bowtie is reported at all.
    let reason = text(
        &conn,
        "SELECT ST_IsValidReason(ST_GeomFromText('POLYGON((0 0,2 2,2 0,0 2,0 0))'))",
    )
    .unwrap();
    assert_ne!(reason, "Valid Geometry");
    assert!(reason.to_lowercase().contains("intersection"), "{reason}");
}

#[test]
fn ring_orientation_round_trips() {
    let conn = conn();
    let ccw = "ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')";
    assert_eq!(
        int(&conn, &format!("SELECT ST_IsPolygonCCW({ccw})")),
        Some(1)
    );
    assert_eq!(
        int(&conn, &format!("SELECT ST_IsPolygonCW({ccw})")),
        Some(0)
    );
    assert_eq!(
        int(
            &conn,
            &format!("SELECT ST_IsPolygonCW(ST_ForcePolygonCW({ccw}))")
        ),
        Some(1)
    );
    // ST_ForceRHR is PostGIS's older spelling of ST_ForcePolygonCW.
    assert_eq!(
        text(&conn, &format!("SELECT ST_AsText(ST_ForceRHR({ccw}))")),
        text(
            &conn,
            &format!("SELECT ST_AsText(ST_ForcePolygonCW({ccw}))")
        )
    );
    assert_eq!(
        int(
            &conn,
            &format!("SELECT ST_IsPolygonCCW(ST_ForcePolygonCCW(ST_ForcePolygonCW({ccw})))")
        ),
        Some(1)
    );
}

#[test]
fn linear_referencing_matches_postgis() {
    let conn = conn();
    // PostGIS 3.5: LINESTRING(0 0,3.333333333333334 0,6.666666666666667 0,10 0)
    let segmentized = text(
        &conn,
        "SELECT ST_AsText(ST_Segmentize(ST_GeomFromText('LINESTRING(0 0,10 0)'), 4))",
    )
    .unwrap();
    assert!(
        segmentized.starts_with("LINESTRING(0 0,3.33"),
        "{segmentized}"
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_LineSubstring(ST_GeomFromText('LINESTRING(0 0,10 0)'), 0.3, 0.7))"
        )
        .as_deref(),
        Some("LINESTRING(3 0,7 0)")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_ShortestLine(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)')))"
        )
        .as_deref(),
        Some("LINESTRING(0 0,2 0)")
    );
    assert!(
        (real(
            &conn,
            "SELECT ST_MaxDistance(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)'))"
        ) - 2.236_067_977_499_79)
            .abs()
            < 1e-12
    );
}

#[test]
fn phase3_functions_are_null_strict() {
    let conn = conn();
    for sql in [
        "SELECT ST_DistanceSphere(NULL, NULL)",
        "SELECT ST_Project(NULL, 1.0, 1.0)",
        "SELECT ST_Dimension(NULL)",
        "SELECT ST_IsValidReason(NULL)",
        "SELECT ST_ForcePolygonCW(NULL)",
        "SELECT ST_Segmentize(NULL, 1.0)",
        "SELECT ST_LineSubstring(NULL, 0.0, 1.0)",
        "SELECT ST_ShortestLine(NULL, NULL)",
        "SELECT ST_MaxDistance(NULL, NULL)",
    ] {
        let v: Option<Vec<u8>> = conn.query_row(sql, [], |r| r.get(0)).unwrap();
        assert!(v.is_none(), "{sql} was not NULL-strict");
    }
}

// ---- Smallest enclosing circle, and the overlay-powered areal operations ----

#[test]
fn minimum_bounding_circle_and_radius() {
    let conn = conn();
    // PostGIS 3.5: ST_MinimumBoundingRadius(LINESTRING(0 0,4 0)) → radius 2
    assert!(
        (real(
            &conn,
            "SELECT ST_MinimumBoundingRadius(ST_GeomFromText('LINESTRING(0 0,4 0)'))"
        ) - 2.0)
            .abs()
            < 1e-9
    );
    // The circle covers the far corner of the square it was built from.
    assert_eq!(
        int(
            &conn,
            "SELECT ST_Covers(ST_MinimumBoundingCircle(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))')), ST_GeomFromText('POINT(4 4)'))"
        ),
        Some(1)
    );
    // The segments-per-quarter arity controls the vertex count.
    assert_eq!(
        int(
            &conn,
            "SELECT ST_NPoints(ST_MinimumBoundingCircle(ST_GeomFromText('LINESTRING(0 0,4 0)'), 2))"
        ),
        Some(9) // 4 quarters x 2 segments, plus the closing vertex
    );
}

#[test]
#[cfg(feature = "overlay")]
fn areal_operations_through_the_overlay_mesh() {
    let conn = conn();
    // Two overlapping members dissolve into one polygon, as in PostGIS.
    let dissolved = text(
        &conn,
        "SELECT ST_GeometryType(ST_UnaryUnion(ST_GeomFromText('MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((1 1,3 1,3 3,1 3,1 1)))')))",
    );
    assert_eq!(dissolved.as_deref(), Some("ST_Polygon"));
    assert!(
        (real(
            &conn,
            "SELECT ST_Area(ST_UnaryUnion(ST_GeomFromText('MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((1 1,3 1,3 3,1 3,1 1)))')))"
        ) - 7.0)
            .abs()
            < 1e-9
    );

    // PostGIS 3.5: ST_ClipByBox2D(10x10 square, box 2..5) → POLYGON((2 2,2 5,5 5,5 2,2 2))
    assert!(
        (real(
            &conn,
            "SELECT ST_Area(ST_ClipByBox2D(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_MakeEnvelope(2,2,5,5)))"
        ) - 9.0)
            .abs()
            < 1e-9
    );

    // Subdivision preserves area and respects the vertex budget. A plain
    // square already fits in 5 vertices — PostGIS leaves it alone too — so
    // densify it first to have something to split.
    let dense = "ST_Segmentize(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), 2)";
    assert!(
        (real(&conn, &format!("SELECT ST_Area(ST_Subdivide({dense}, 8))")) - 100.0).abs() < 1e-9
    );
    assert!(
        int(
            &conn,
            &format!("SELECT ST_NumGeometries(ST_Subdivide({dense}, 8))")
        )
        .unwrap()
            > 1,
        "a 20-vertex square with a budget of 8 must split"
    );
    // A budget too small to hold a rectangle is a loud error.
    assert!(
        conn.query_row(
            "SELECT ST_Subdivide(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), 3)",
            [],
            |r| r.get::<_, Vec<u8>>(0)
        )
        .is_err()
    );
}

#[test]
fn phase4_functions_are_null_strict() {
    let conn = conn();
    for sql in [
        "SELECT ST_MinimumBoundingRadius(NULL)",
        "SELECT ST_MinimumBoundingCircle(NULL)",
        "SELECT ST_MinimumBoundingCircle(NULL, 8)",
    ] {
        let v: Option<Vec<u8>> = conn.query_row(sql, [], |r| r.get(0)).unwrap();
        assert!(v.is_none(), "{sql} was not NULL-strict");
    }
}

// ---- The rest of the reachable surface (functions::extra) ----

#[test]
fn extra_predicates_and_transforms_match_postgis() {
    let conn = conn();
    let poly = "ST_GeomFromText('POLYGON((0 0,3 0,3 3,0 3,0 0))')";
    // PostGIS 3.5: interior point true, corner false.
    assert_eq!(
        int(
            &conn,
            &format!("SELECT ST_ContainsProperly({poly}, ST_GeomFromText('POINT(1 1)'))")
        ),
        Some(1)
    );
    assert_eq!(
        int(
            &conn,
            &format!("SELECT ST_ContainsProperly({poly}, ST_GeomFromText('POINT(0 0)'))")
        ),
        Some(0)
    );
    // PostGIS 3.5: true at 3, false at 2.
    let (p, l) = (
        "ST_GeomFromText('POINT(0 0)')",
        "ST_GeomFromText('LINESTRING(2 -1,2 1)')",
    );
    assert_eq!(
        int(&conn, &format!("SELECT ST_DFullyWithin({p}, {l}, 3)")),
        Some(1)
    );
    assert_eq!(
        int(&conn, &format!("SELECT ST_DFullyWithin({p}, {l}, 2)")),
        Some(0)
    );
    assert_eq!(
        int(&conn, "SELECT ST_RelateMatch('101202FFF', 'TTTTTTFFF')"),
        Some(1)
    );
    // PostGIS 3.5: ST_Affine(LINESTRING(1 2,3 4),2,0,0,2,10,20)
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_Affine(ST_GeomFromText('LINESTRING(1 2,3 4)'), 2,0,0,2,10,20))"
        )
        .as_deref(),
        Some("LINESTRING(12 24,16 28)")
    );
    // PostGIS 3.5: translate first, then scale.
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_TransScale(ST_GeomFromText('POINT(1 2)'), 1, 2, 3, 4))"
        )
        .as_deref(),
        Some("POINT(6 16)")
    );
}

#[test]
fn angles_are_clockwise_and_vertex_accessors_match_postgis() {
    let conn = conn();
    // PostGIS 3.5: 270 degrees, not 90 — the angle is measured clockwise.
    let degrees = real(
        &conn,
        "SELECT ST_Angle(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(1 0)'), ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(0 1)')) * 180.0 / 3.14159265358979",
    );
    assert!((degrees - 270.0).abs() < 1e-6, "{degrees}");
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_LineInterpolatePoints(ST_GeomFromText('LINESTRING(0 0,10 0)'), 0.25))"
        )
        .as_deref(),
        Some("MULTIPOINT((2.5 0),(5 0),(7.5 0),(10 0))")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_Points(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 0))')))"
        )
        .as_deref(),
        Some("MULTIPOINT((0 0),(1 0),(1 1),(0 0))")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_BoundingDiagonal(ST_GeomFromText('LINESTRING(1 2,5 9)')))"
        )
        .as_deref(),
        Some("LINESTRING(1 2,5 9)")
    );
    // Ordering, unlike ST_Equals, is not topological.
    assert_eq!(
        int(
            &conn,
            "SELECT ST_OrderingEquals(ST_GeomFromText('LINESTRING(0 0,1 1)'), ST_GeomFromText('LINESTRING(1 1,0 0)'))"
        ),
        Some(0)
    );
    assert_eq!(
        int(
            &conn,
            "SELECT ST_Equals(ST_GeomFromText('LINESTRING(0 0,1 1)'), ST_GeomFromText('LINESTRING(1 1,0 0)'))"
        ),
        Some(1)
    );
}

#[test]
fn geohash_matches_postgis_through_sql() {
    let conn = conn();
    // PostGIS 3.5: 'xn76fzq7jfn42q30gmb9', and 'xn76f' at 5 characters.
    assert_eq!(
        text(
            &conn,
            "SELECT ST_GeoHash(ST_GeomFromText('POINT(139.7 35.68)', 4326))"
        )
        .as_deref(),
        Some("xn76fzq7jfn42q30gmb9")
    );
    assert_eq!(
        text(
            &conn,
            "SELECT ST_GeoHash(ST_GeomFromText('POINT(139.7 35.68)', 4326), 5)"
        )
        .as_deref(),
        Some("xn76f")
    );
}

#[test]
fn extent_aggregates_the_bounding_box_over_rows() {
    let conn = conn();
    conn.execute_batch(
        "CREATE TABLE p (g BLOB);
         INSERT INTO p VALUES (ST_GeomFromText('POINT(1 2)')), (ST_GeomFromText('POINT(5 0)')), (NULL);",
    )
    .unwrap();
    // PostGIS 3.5: BOX(1 0,5 2). kenro returns the same box as a POLYGON,
    // and skips the NULL row.
    assert_eq!(
        text(&conn, "SELECT ST_AsText(ST_Extent(g)) FROM p").as_deref(),
        Some("POLYGON((1 0,1 2,5 2,5 0,1 0))")
    );
    // An all-NULL group is NULL, not an empty polygon.
    assert_eq!(
        text(
            &conn,
            "SELECT ST_AsText(ST_Extent(g)) FROM p WHERE g IS NULL"
        ),
        None
    );
}

#[test]
fn extra_functions_are_null_strict() {
    let conn = conn();
    for sql in [
        "SELECT ST_ContainsProperly(NULL, NULL)",
        "SELECT ST_DFullyWithin(NULL, NULL, 1.0)",
        "SELECT ST_RelateMatch(NULL, NULL)",
        "SELECT ST_Affine(NULL, 1,0,0,1,0,0)",
        "SELECT ST_TransScale(NULL, 1,1,1,1)",
        "SELECT ST_ReducePrecision(NULL, 0.1)",
        "SELECT ST_Angle(NULL, NULL, NULL)",
        "SELECT ST_LineInterpolatePoints(NULL, 0.5)",
        "SELECT ST_Points(NULL)",
        "SELECT ST_BoundingDiagonal(NULL)",
        "SELECT ST_GeoHash(NULL)",
    ] {
        let v: Option<Vec<u8>> = conn.query_row(sql, [], |r| r.get(0)).unwrap();
        assert!(v.is_none(), "{sql} was not NULL-strict");
    }
}

// ---- The two size-gated algorithms (functions::hull) ----

#[test]
#[cfg(feature = "concave-hull")]
fn concave_hull_keeps_postgis_argument_contract() {
    let conn = conn();
    let ring = "ST_GeomFromText('MULTIPOINT(0 0,2 0,4 0,4 2,4 4,2 4,0 4,0 2,1 1,3 1,3 3,1 3)')";
    let convex = real(&conn, &format!("SELECT ST_Area(ST_ConvexHull({ring}))"));
    // PostGIS's argument is the fraction of the convex hull's area; 1.0 is
    // the convex hull itself.
    assert!(
        (real(
            &conn,
            &format!("SELECT ST_Area(ST_ConcaveHull({ring}, 1.0))")
        ) - convex)
            .abs()
            < 1e-9
    );
    // Never larger than the convex hull, and monotone in the target.
    let mut previous = convex;
    for target in ["0.9", "0.5", "0.2"] {
        let area = real(
            &conn,
            &format!("SELECT ST_Area(ST_ConcaveHull({ring}, {target}))"),
        );
        assert!(area <= previous + 1e-9, "{target}: {area} > {previous}");
        previous = area;
    }
    // geo's own parameter is a concavity of ~2, out of PostGIS's range: the
    // paste must fail loudly rather than return a different shape.
    let err = conn
        .query_row(&format!("SELECT ST_ConcaveHull({ring}, 2.0)"), [], |r| {
            r.get::<_, Vec<u8>>(0)
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("between 0 and 1"), "{err}");
}

#[test]
#[cfg(feature = "delaunay")]
fn delaunay_triangulates_like_postgis_but_as_a_multipolygon() {
    let conn = conn();
    let square = "ST_GeomFromText('MULTIPOINT(0 0,4 0,4 4,0 4)')";
    // PostGIS 3.5: 2 triangles totalling area 16, in a GEOMETRYCOLLECTION.
    assert_eq!(
        int(
            &conn,
            &format!("SELECT ST_NumGeometries(ST_DelaunayTriangles({square}))")
        ),
        Some(2)
    );
    assert!(
        (real(
            &conn,
            &format!("SELECT ST_Area(ST_DelaunayTriangles({square}))")
        ) - 16.0)
            .abs()
            < 1e-9
    );
    // kenro never produces collections, so it is a MULTIPOLYGON.
    assert_eq!(
        text(
            &conn,
            &format!("SELECT ST_GeometryType(ST_DelaunayTriangles({square}))")
        )
        .as_deref(),
        Some("ST_MultiPolygon")
    );
}

#[test]
#[cfg(not(any(feature = "concave-hull", feature = "delaunay")))]
fn the_size_gated_algorithms_name_their_feature_when_off() {
    let conn = conn();
    for (sql, feature) in [
        (
            "SELECT ST_ConcaveHull(ST_GeomFromText('MULTIPOINT(0 0,1 1)'), 0.5)",
            "concave-hull",
        ),
        (
            "SELECT ST_DelaunayTriangles(ST_GeomFromText('MULTIPOINT(0 0,1 1,2 0)'))",
            "delaunay",
        ),
    ] {
        let err = conn
            .query_row(sql, [], |r| r.get::<_, Vec<u8>>(0))
            .unwrap_err()
            .to_string();
        assert!(err.contains(feature), "{err}");
    }
}

#[test]
#[cfg(feature = "crs-full")]
fn full_builds_transform_to_national_grids() {
    let conn = conn();
    // The point of putting crs-full in `full`: a national or local system
    // works without a rebuild. EPSG:6677 is Japan's plane rectangular CS IX,
    // EPSG:27700 the British National Grid — neither is in kenro's curated
    // table (WGS84, Web Mercator, UTM).
    for (srid, x_range) in [(6677i64, (-30000.0, 30000.0)), (27700, (0.0, 700_000.0))] {
        let wkt = if srid == 6677 {
            "POINT(139.7454 35.6586)" // Tokyo Tower
        } else {
            "POINT(-0.1246 51.5007)" // Big Ben
        };
        let x: f64 = conn
            .query_row(
                &format!("SELECT ST_X(ST_Transform(ST_GeomFromText('{wkt}', 4326), {srid}))"),
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|e| panic!("EPSG:{srid}: {e}"));
        assert!(
            x > x_range.0 && x < x_range.1,
            "EPSG:{srid}: x = {x} outside {x_range:?}"
        );
        // And it round-trips back to where it started.
        let lon: f64 = conn
            .query_row(
                &format!(
                    "SELECT ST_X(ST_Transform(ST_Transform(ST_GeomFromText('{wkt}', 4326), {srid}), 4326))"
                ),
                [],
                |r| r.get(0),
            )
            .unwrap();
        let expected: f64 = conn
            .query_row(
                &format!("SELECT ST_X(ST_GeomFromText('{wkt}', 4326))"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            (lon - expected).abs() < 1e-6,
            "EPSG:{srid}: {lon} vs {expected}"
        );
    }
}

#[test]
#[cfg(all(feature = "transform", not(feature = "crs-full")))]
fn builds_without_crs_full_name_the_feature() {
    let conn = conn();
    let err = conn
        .query_row(
            "SELECT ST_Transform(ST_GeomFromText('POINT(139.7 35.6)', 4326), 6677)",
            [],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("6677") && err.contains("crs-full"), "{err}");
}
