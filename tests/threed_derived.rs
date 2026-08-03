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

/// `POLYHEDRALSURFACE Z` with one triangle near Tokyo, in EPSG:4326 — the
/// shape a CityGML building arrives as.
fn surface_z() -> String {
    let mut v = vec![0x01u8];
    v.extend_from_slice(&1015u32.to_le_bytes());
    v.extend_from_slice(&1u32.to_le_bytes()); // one patch
    v.push(0x01);
    v.extend_from_slice(&1003u32.to_le_bytes()); // Polygon Z
    v.extend_from_slice(&1u32.to_le_bytes()); // one ring
    v.extend_from_slice(&4u32.to_le_bytes());
    for c in [
        [139.7f64, 35.7, 0.],
        [139.7, 35.71, 0.],
        [139.71, 35.71, 10.],
        [139.7, 35.7, 0.],
    ] {
        for o in c {
            v.extend_from_slice(&o.to_le_bytes());
        }
    }
    wkb_literal(&v)
}

/// Reprojecting is the operation a 3D city model needs most, and it used to be
/// the one function that refused 3D outright. PostGIS 3.5, measured:
/// `ST_Transform(POINT Z (139.7 35.7 100), 32654)` is
/// `POINT(382388.69405900664 3951453.5737444377 100)` — x and y move, the
/// height does not.
#[test]
fn reprojection_keeps_the_height() {
    let conn = conn();
    let pt = point_z(139.7, 35.7, 100.0);
    let t = format!("ST_Transform(ST_SetSRID({pt}, 4326), 32654)");
    assert_eq!(ndims(&conn, &t).unwrap(), 3);
    let (x, z, srid): (f64, f64, i64) = conn
        .query_row(
            &format!("SELECT ST_MinX({t}), ST_Z({t}), ST_SRID({t})"),
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    // kenro's projection math is gridless; docs/accuracy.md documents the
    // tolerance. The height is exact because nothing touches it here.
    assert!((x - 382_388.694_059_006_64).abs() < 1e-5, "x = {x}");
    assert_eq!(z, 100.0);
    assert_eq!(srid, 32654);
}

/// The other half of the CityGML case: a building is a surface collection, and
/// reprojecting one has to keep it a surface collection. PostGIS returns a
/// `POLYHEDRALSURFACE` here; kenro used to refuse the input.
#[test]
fn reprojection_moves_a_building_and_it_stays_storable() {
    let conn = conn();
    let s = surface_z();
    let t = format!("ST_Transform(ST_SetSRID({s}, 4326), 32654)");
    let ty: String = conn
        .query_row(&format!("SELECT ST_GeometryType({t})"), [], |r| r.get(0))
        .unwrap();
    assert_eq!(ty, "ST_PolyhedralSurface");
    let (patches, minx, zmax, srid): (i64, f64, f64, i64) = conn
        .query_row(
            &format!("SELECT ST_NumPatches({t}), ST_MinX({t}), ST_ZMax({t}), ST_SRID({t})"),
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(patches, 1);
    assert!(
        (minx - 382_388.694_059_006_64).abs() < 1e-5,
        "minx = {minx}"
    );
    assert_eq!(zmax, 10.0); // the roof height rides through untouched
    assert_eq!(srid, 32654);

    // And the result can be written back to a GeoPackage column: ST_AsGPB
    // takes the byte-level route for a surface, and names the extension row
    // the file needs before it may hold one.
    let ext: String = conn
        .query_row(
            &format!("SELECT kenro_gpkg_extension_required(ST_AsGPB({t}))"),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ext, "gpkg_geom_POLYHEDRALSURFACE");
}

/// The byte-level I/O functions promise to carry a payload across untouched.
/// They validated by decoding, which refused surface collections — breaking the
/// promise for exactly the geometries it was written for.
#[test]
fn the_byte_level_io_functions_accept_a_surface() {
    let conn = conn();
    let s = surface_z();
    for expr in [
        format!("ST_NumPatches(ST_SetSRID({s}, 4326))"),
        format!("ST_NumPatches(ST_GeomFromGPB(ST_SetSRID({s}, 4326)))"),
        format!("ST_NumPatches(ST_AsGPB(ST_SetSRID({s}, 4326)))"),
    ] {
        let n: i64 = conn
            .query_row(&format!("SELECT {expr}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "{expr}");
    }
    // ST_SRID reads the header, so it answers for a surface too.
    let srid: i64 = conn
        .query_row(&format!("SELECT ST_SRID(ST_SetSRID({s}, 6697))"), [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(srid, 6697);
}

#[test]
fn reprojection_refuses_an_unlabelled_geometry_as_postgis_does() {
    let conn = conn();
    let pt = point_z(139.7, 35.7, 100.0);
    let err = conn
        .query_row(&format!("SELECT ST_Transform({pt}, 32654)"), [], |r| {
            r.get::<_, Vec<u8>>(0)
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown (0) SRID"), "{err}");
    // Transforming to the SRID it already has is a normalization, not a move.
    let same = format!("ST_Transform(ST_SetSRID({pt}, 4326), 4326)");
    assert_eq!(ndims(&conn, &same).unwrap(), 3);
    let z: f64 = conn
        .query_row(&format!("SELECT ST_Z({same})"), [], |r| r.get(0))
        .unwrap();
    assert_eq!(z, 100.0);
}

/// Creating a Z, which the design note called the wall. It turned out the
/// writer built for *carrying* heights across a derived geometry already emits
/// ISO XYZ type codes, so a constant Z source is the whole of it — no decoded
/// 3D geometry model involved. Every expectation measured on PostGIS 3.5.
#[test]
fn force_3d_creates_a_z_where_there_was_none() {
    let conn = conn();
    let z = |expr: &str| -> f64 {
        conn.query_row(&format!("SELECT ST_Z({expr})"), [], |r| r.get(0))
            .unwrap()
    };
    // ST_Force3D(POINT(1 2)) → POINT(1 2 0); with a zvalue → POINT(1 2 7).
    assert_eq!(z("ST_Force3D(ST_GeomFromText('POINT(1 2)'))"), 0.0);
    assert_eq!(z("ST_Force3D(ST_GeomFromText('POINT(1 2)'), 7)"), 7.0);
    // ST_Force3DZ is PostGIS's alias for the same thing.
    assert_eq!(z("ST_Force3DZ(ST_GeomFromText('POINT(1 2)'))"), 0.0);
    assert_eq!(z("ST_Force3DZ(ST_GeomFromText('POINT(1 2)'), 7)"), 7.0);
    // The x and y are untouched, and the SRID survives.
    let (x, srid): (f64, i64) = conn
        .query_row(
            "SELECT ST_MinX(g), ST_SRID(g) FROM
               (SELECT ST_Force3D(ST_GeomFromText('POINT(1 2)', 4326)) AS g)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((x, srid), (1.0, 4326));

    // Every geometry type, not just points.
    for wkt in [
        "LINESTRING(0 0,1 1)",
        "POLYGON((0 0,1 0,1 1,0 0))",
        "MULTIPOINT((1 2),(3 4))",
        "MULTILINESTRING((0 0,1 1))",
        "MULTIPOLYGON(((0 0,1 0,1 1,0 0)))",
        "GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))",
    ] {
        let expr = format!("ST_Force3D(ST_GeomFromText('{wkt}'))");
        assert_eq!(ndims(&conn, &expr).unwrap(), 3, "{wkt}");
    }

    // An existing Z is never overwritten — the argument fills gaps, it does not
    // set heights. Measured: ST_Force3D(POINT Z (1 2 3), 7) is POINT(1 2 3).
    let pz = point_z(1.0, 2.0, 3.0);
    assert_eq!(z(&format!("ST_Force3D({pz})")), 3.0);
    assert_eq!(z(&format!("ST_Force3D({pz}, 7)")), 3.0);

    // An empty geometry has no ordinates to raise, so it comes back unchanged.
    assert_eq!(
        ndims(&conn, "ST_Force3D(ST_GeomFromText('LINESTRING EMPTY'))").unwrap(),
        2
    );

    // ST_MakePoint's three-argument form: the smallest use of the same writer.
    assert_eq!(ndims(&conn, "ST_MakePoint(1, 2, 3)").unwrap(), 3);
    assert_eq!(z("ST_MakePoint(1, 2, 3)"), 3.0);
}

/// XYM in, XYZ out — the M is dropped rather than kept alongside, because kenro
/// has no way to write one. Measured: `ST_Force3D(POINT M (1 2 99))` is
/// `POINT(1 2 0)` in PostGIS too.
#[test]
fn force_3d_on_an_m_geometry_drops_the_m() {
    let conn = conn();
    let mut v = vec![0x01u8];
    v.extend_from_slice(&2001u32.to_le_bytes()); // POINT M
    for o in [1.0f64, 2.0, 99.0] {
        v.extend_from_slice(&o.to_le_bytes());
    }
    let pm = wkb_literal(&v);
    let expr = format!("ST_Force3D({pm})");
    assert_eq!(ndims(&conn, &expr).unwrap(), 3);
    let (z, has_m): (f64, i64) = conn
        .query_row(&format!("SELECT ST_Z({expr}), ST_HasM({expr})"), [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(z, 0.0);
    assert_eq!(has_m, 0);
}

/// A surface collection that has no Z in its type code cannot be raised: doing
/// so would mean rebuilding the nested patch encoding, which the XYZ writer
/// does not do. Refused with a message that says which geometry and why.
#[test]
fn force_3d_refuses_a_2d_surface_rather_than_guessing() {
    let conn = conn();
    // POLYHEDRALSURFACE (no Z): type 15, patches of type 3.
    let mut v = vec![0x01u8];
    v.extend_from_slice(&15u32.to_le_bytes());
    v.extend_from_slice(&1u32.to_le_bytes());
    v.push(0x01);
    v.extend_from_slice(&3u32.to_le_bytes());
    v.extend_from_slice(&1u32.to_le_bytes());
    v.extend_from_slice(&4u32.to_le_bytes());
    for c in [[0.0f64, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]] {
        for o in c {
            v.extend_from_slice(&o.to_le_bytes());
        }
    }
    let flat_surface = wkb_literal(&v);
    let err = ndims(&conn, &format!("ST_Force3D({flat_surface})"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("POLYHEDRALSURFACE"), "{err}");
    // A surface that already has a Z passes straight through.
    let s = surface_z();
    assert_eq!(
        conn.query_row(&format!("SELECT ST_NumPatches(ST_Force3D({s}))"), [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
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
