//! What happens to a Z when a function *derives* a geometry from a geometry.
//!
//! Before this suite existed, 45 functions answered the question by silently
//! dropping the height: six modules had a private `out()` helper that built a
//! `Geom` with `has_zm: false`, which is the only field the encoder's refusal
//! consults. `UPDATE buildings SET geom = ST_Simplify(geom, 0.1)` therefore
//! flattened a whole table without a word. The audit is `tmp/out-audit.md`.
//!
//! The rule the fix implements, and this suite pins down, is decided by the
//! data rather than by a list of function names:
//!
//! - every output coordinate was a vertex of some input → the Z comes back
//! - some output coordinate was invented → **error**, naming `ST_Force2D`
//! - PostGIS itself answers in 2D → kenro does too, and says so
//!
//! Every expectation below was measured against PostGIS 3.5 in the reference
//! container the golden vectors come from (`postgis/postgis:17-3.5`).

use rusqlite::Connection;

fn conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn
}

/// A 3D column arrives as a WKB blob written by something else — GDAL, QGIS,
/// a CityGML importer. These build the SQL blob literals for one.
fn wkb_literal(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("x'{hex}'")
}

fn point_z(x: f64, y: f64, z: f64) -> String {
    let mut v = vec![0x01u8];
    v.extend_from_slice(&1001u32.to_le_bytes());
    for o in [x, y, z] {
        v.extend_from_slice(&o.to_le_bytes());
    }
    wkb_literal(&v)
}

fn line_z(coords: &[[f64; 3]]) -> String {
    let mut v = vec![0x01u8];
    v.extend_from_slice(&1002u32.to_le_bytes());
    v.extend_from_slice(&(coords.len() as u32).to_le_bytes());
    for c in coords {
        for o in c {
            v.extend_from_slice(&o.to_le_bytes());
        }
    }
    wkb_literal(&v)
}

/// `POLYGON Z ((0 0 1,10 0 2,10 10 3,0 10 4,0 0 1))` — a footprint with a
/// different height at every corner, so a wrong Z is visible rather than lucky.
fn polygon_z() -> String {
    let mut v = vec![0x01u8];
    v.extend_from_slice(&1003u32.to_le_bytes());
    v.extend_from_slice(&1u32.to_le_bytes());
    let ring: [[f64; 3]; 5] = [
        [0., 0., 1.],
        [10., 0., 2.],
        [10., 10., 3.],
        [0., 10., 4.],
        [0., 0., 1.],
    ];
    v.extend_from_slice(&(ring.len() as u32).to_le_bytes());
    for c in ring {
        for o in c {
            v.extend_from_slice(&o.to_le_bytes());
        }
    }
    wkb_literal(&v)
}

fn ndims(conn: &Connection, expr: &str) -> rusqlite::Result<i64> {
    conn.query_row(&format!("SELECT ST_NDims({expr})"), [], |r| r.get(0))
}

#[test]
fn a_derived_geometry_keeps_the_z_of_the_vertices_it_reused() {
    let conn = conn();
    let (p, l) = (
        polygon_z(),
        line_z(&[[0., 0., 1.], [10., 10., 2.], [20., 0., 3.]]),
    );
    // PostGIS answers 3D for every one of these, with the original heights.
    let cases = [
        format!("ST_StartPoint({l})"),
        format!("ST_EndPoint({l})"),
        format!("ST_PointN({l}, 2)"),
        format!("ST_ExteriorRing({p})"),
        format!("ST_Boundary({p})"),
        format!("ST_Reverse({l})"),
        format!("ST_Normalize({p})"),
        format!("ST_ForcePolygonCW({p})"),
        format!("ST_Multi({p})"),
        format!("ST_RemovePoint({l}, 1)"),
        format!("ST_Points({p})"),
        format!("ST_RemoveRepeatedPoints({l})"),
        format!("ST_Simplify({l}, 1)"),
        format!("ST_SimplifyVW({l}, 1)"),
        format!("ST_ConvexHull({p})"),
        format!("ST_LineMerge({l})"),
        format!("ST_UnaryUnion({p})"),
    ];
    for expr in cases {
        assert_eq!(ndims(&conn, &expr).unwrap(), 3, "{expr}");
    }
}

#[test]
fn the_z_that_comes_back_is_the_right_one() {
    let conn = conn();
    let l = line_z(&[[0., 0., 1.], [10., 10., 2.], [20., 0., 3.]]);
    // Not just "3D" — the correct height per vertex, which is what a lookup
    // keyed on (x, y) has to get right.
    let z = |expr: &str| -> f64 {
        conn.query_row(&format!("SELECT ST_Z({expr})"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(z(&format!("ST_StartPoint({l})")), 1.0);
    assert_eq!(z(&format!("ST_PointN({l}, 2)")), 2.0);
    assert_eq!(z(&format!("ST_EndPoint({l})")), 3.0);
    // Reversing swaps which vertex is first, and the height follows it.
    assert_eq!(z(&format!("ST_StartPoint(ST_Reverse({l}))")), 3.0);
}

#[test]
fn a_second_geometrys_vertices_count_too() {
    let conn = conn();
    let l = line_z(&[[0., 0., 1.], [10., 10., 2.]]);
    let p = point_z(5.0, 5.0, 99.0);
    // ST_AddPoint's new vertex comes from the *second* argument, so the index
    // has to be built from every geometry the function was given.
    let expr = format!("ST_AddPoint({l}, {p})");
    assert_eq!(ndims(&conn, &expr).unwrap(), 3);
    let z: f64 = conn
        .query_row(&format!("SELECT ST_Z(ST_EndPoint({expr}))"), [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(z, 99.0);
}

#[test]
fn inventing_a_vertex_is_an_error_not_a_guess() {
    let conn = conn();
    let l = line_z(&[[0., 0., 1.], [10., 10., 2.], [20., 0., 3.]]);
    // Each of these places a vertex where no input had one. PostGIS
    // interpolates a Z; kenro cannot, so it refuses and names the way out
    // rather than handing back a silently flattened geometry.
    let cases = [
        format!("ST_Segmentize({l}, 2)"),
        format!("ST_LineSubstring({l}, 0.2, 0.8)"),
        format!("ST_LineInterpolatePoint({l}, 0.3)"),
        format!("ST_ChaikinSmoothing({l})"),
        format!("ST_LineExtend({l}, 1)"),
    ];
    for expr in cases {
        let err = ndims(&conn, &expr).unwrap_err().to_string();
        assert!(err.contains("invent a Z"), "{expr}: {err}");
        assert!(err.contains("ST_Force2D"), "{expr}: {err}");
        // And ST_Force2D really is the way through.
        let flattened = expr.replacen(&l, &format!("ST_Force2D({l})"), 1);
        assert_eq!(ndims(&conn, &flattened).unwrap(), 2, "{flattened}");
    }
}

#[test]
fn a_union_that_crosses_refuses_and_one_that_does_not_survives() {
    let conn = conn();
    let p = polygon_z();
    // Overlapping polygons put vertices at the crossings: refused.
    let crossing = format!("ST_Union({p}, ST_Translate(ST_Force2D({p}), 5, 5))");
    assert!(ndims(&conn, &crossing).is_err(), "{crossing}");
    // Intersecting a shape with (a 2D copy of) itself reuses only its own
    // vertices, so there is an honest Z for every one of them.
    let identical = format!("ST_Intersection({p}, ST_Force2D({p}))");
    assert_eq!(ndims(&conn, &identical).unwrap(), 3);

    // The aggregate has no source blob to index at finish time, and a union
    // invents crossings anyway, so it refuses as soon as any row carried a Z.
    let agg = format!("(SELECT ST_Union(g) FROM (SELECT {p} AS g))");
    let err = ndims(&conn, &agg).unwrap_err().to_string();
    assert!(err.contains("ST_Force2D"), "{err}");
}

#[test]
fn the_functions_postgis_answers_in_2d_stay_2d() {
    let conn = conn();
    let (p, l) = (polygon_z(), line_z(&[[0., 0., 1.], [10., 10., 2.]]));
    let pt = point_z(1.0, 2.0, 3.0);
    // Measured: PostGIS returns ST_NDims = 2 for every one of these on 3D
    // input. Erroring here would be wrong, not strict.
    let cases = [
        format!("ST_Centroid({p})"),
        format!("ST_PointOnSurface({p})"),
        format!("ST_Envelope({p})"),
        format!("ST_OrientedEnvelope({p})"),
        format!("ST_MinimumBoundingCircle({p})"),
        format!("ST_ClosestPoint({l}, {pt})"),
        format!("ST_ShortestLine({l}, {pt})"),
        format!("ST_LongestLine({l}, {pt})"),
        format!("ST_Buffer({pt}, 1)"),
        format!("ST_ClipByBox2D({p}, ST_MakeEnvelope(0,0,5,5))"),
        format!("ST_AsMVTGeom({p}, ST_MakeEnvelope(0,0,20,20))"),
        format!("ST_Force2D({p})"),
    ];
    for expr in cases {
        assert_eq!(ndims(&conn, &expr).unwrap(), 2, "{expr}");
    }
}

#[test]
fn a_box_never_takes_a_z_from_a_neighbouring_vertex() {
    let conn = conn();
    let p = polygon_z();
    // The trap the (x, y) index would otherwise fall into: a bounding-box
    // corner can share its x and y with an input vertex while needing a
    // completely different Z. `ST_BoundingDiagonal` of this polygon ends at
    // (10 10), which *is* a vertex — at z = 3, while PostGIS's diagonal ends
    // at z = 4 (the box's zmax). Answering 3 there would be confidently wrong,
    // so every bbox-shaped result is 2D on purpose.
    for expr in [
        format!("ST_BoundingDiagonal({p})"),
        format!("ST_Expand({p}, 1)"),
        format!("ST_Envelope({p})"),
        format!("(SELECT ST_Extent(g) FROM (SELECT {p} AS g))"),
    ] {
        assert_eq!(ndims(&conn, &expr).unwrap(), 2, "{expr}");
    }
}

#[test]
fn moving_a_point_keeps_its_elevation() {
    let conn = conn();
    let pt = point_z(1.0, 2.0, 3.0);
    // ST_Project slides a point along the ground; PostGIS keeps the height
    // (measured), and so does kenro — the one place a Z is asserted for a
    // coordinate no input occupied.
    let expr = format!("ST_Project({pt}, 100, 0.5)");
    assert_eq!(ndims(&conn, &expr).unwrap(), 3);
    let z: f64 = conn
        .query_row(&format!("SELECT ST_Z({expr})"), [], |r| r.get(0))
        .unwrap();
    assert_eq!(z, 3.0);
}

#[test]
fn two_heights_at_one_plan_position_are_ambiguous_not_a_coin_flip() {
    let conn = conn();
    // A vertical feature seen from above: the same (x, y) twice, at different
    // heights. There is no single Z to restore, so a function that would have
    // to pick one refuses instead.
    let vertical = line_z(&[[0., 0., 0.], [0., 0., 10.], [5., 5., 10.]]);
    let err = ndims(&conn, &format!("ST_Reverse({vertical})"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("invent a Z"), "{err}");
    // The unambiguous part of the same geometry is unaffected elsewhere.
    let flat = line_z(&[[0., 0., 0.], [1., 1., 10.], [5., 5., 10.]]);
    assert_eq!(ndims(&conn, &format!("ST_Reverse({flat})")).unwrap(), 3);
}

#[test]
fn nothing_changed_for_2d_input() {
    let conn = conn();
    // The whole mechanism is inert without a Z: no index is built, no lookup
    // happens, and the answers are the ones the golden suites already pin.
    let cases = [
        (
            "SELECT ST_AsText(ST_Reverse(ST_GeomFromText('LINESTRING(0 0,1 1)')))",
            "LINESTRING(1 1,0 0)",
        ),
        (
            "SELECT ST_AsText(ST_StartPoint(ST_GeomFromText('LINESTRING(0 0,1 1)')))",
            "POINT(0 0)",
        ),
        (
            "SELECT ST_AsText(ST_Segmentize(ST_GeomFromText('LINESTRING(0 0,4 0)'), 2))",
            "LINESTRING(0 0,2 0,4 0)",
        ),
        (
            "SELECT ST_AsText(ST_Centroid(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')))",
            "POINT(1 1)",
        ),
    ];
    for (sql, want) in cases {
        let got: String = conn.query_row(sql, [], |r| r.get(0)).unwrap();
        assert_eq!(got, want, "{sql}");
    }
}
