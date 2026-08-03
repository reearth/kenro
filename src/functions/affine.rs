//! Affine transformations: ST_Rotate, ST_Translate, ST_Scale.
//!
//! PostGIS rotates and scales about the ORIGIN (0, 0) by default — geo's
//! `Rotate`/`Scale` convenience methods use the centroid / bounding-rect
//! center, so the transforms here are built explicitly as
//! `AffineTransform` matrices.
//!
//! The matrix is built with `geo` but **applied through [`crate::coords`]**,
//! coordinate by coordinate in the encoding, rather than to a decoded
//! geometry. That is what PostGIS does in effect: all three leave Z alone and
//! all three accept a surface collection (measured on 3.5 —
//! `ST_Translate(POINT Z (1 2 3), 10, 20)` is `POINT(11 22 3)`, and
//! `ST_Rotate` on a POLYHEDRALSURFACE returns one). Decoding into
//! `geo_types` would have refused both.

use geo::AffineTransform;

use crate::coords;
use crate::error::Result;

/// Apply a 2D matrix to every coordinate, leaving Z and M untouched.
fn apply(bytes: &[u8], transform: AffineTransform<f64>) -> Result<Vec<u8>> {
    coords::map_coords(bytes, &mut |p| {
        let moved = transform.apply(geo_types::Coord { x: p.x, y: p.y });
        p.x = moved.x;
        p.y = moved.y;
    })
}

/// `ST_Rotate(geom, radians)` — counter-clockwise about the origin (0,0),
/// PostGIS's 2-arg convention.
pub fn st_rotate(bytes: &[u8], radians: f64) -> Result<Vec<u8>> {
    st_rotate_xy(bytes, radians, 0.0, 0.0)
}

/// `ST_Rotate(geom, radians, x0, y0)` — counter-clockwise about (x0, y0).
/// Built directly from the radians (geo's Rotate trait works in degrees;
/// a degree round-trip would lose precision for exact multiples of π).
pub fn st_rotate_xy(bytes: &[u8], radians: f64, x0: f64, y0: f64) -> Result<Vec<u8>> {
    let (sin, cos) = radians.sin_cos();
    let transform = AffineTransform::new(
        cos,
        -sin,
        x0 - x0 * cos + y0 * sin,
        sin,
        cos,
        y0 - x0 * sin - y0 * cos,
    );
    apply(bytes, transform)
}

/// `ST_Translate(geom, dx, dy)`.
pub fn st_translate(bytes: &[u8], dx: f64, dy: f64) -> Result<Vec<u8>> {
    apply(bytes, AffineTransform::translate(dx, dy))
}

/// `ST_Scale(geom, xfactor, yfactor)` — about the origin (0,0), PostGIS's
/// convention.
pub fn st_scale(bytes: &[u8], xfactor: f64, yfactor: f64) -> Result<Vec<u8>> {
    apply(
        bytes,
        AffineTransform::scale(xfactor, yfactor, geo_types::coord! { x: 0.0, y: 0.0 }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    fn text(blob: &[u8]) -> String {
        st_as_text(blob).unwrap()
    }

    #[test]
    fn rotate_about_origin_and_custom_point() {
        // 90° CCW about origin: (1, 0) → (0, 1).
        let rotated = st_rotate(&g("POINT(1 0)"), std::f64::consts::FRAC_PI_2).unwrap();
        let wkt = text(&rotated);
        assert!(wkt.starts_with("POINT(") && wkt.contains(" 1)"), "{wkt}");
        // 180° about (5, 5): (4, 5) → (6, 5), modulo fp noise.
        let rotated = st_rotate_xy(&g("POINT(4 5)"), std::f64::consts::PI, 5.0, 5.0).unwrap();
        let wkt = text(&rotated);
        assert!(
            wkt.starts_with("POINT(5.999") || wkt.starts_with("POINT(6 5"),
            "{wkt}"
        );
    }

    #[test]
    fn translate_and_scale() {
        assert_eq!(
            text(&st_translate(&g("LINESTRING(0 0,1 1)"), 10.0, -2.0).unwrap()),
            "LINESTRING(10 -2,11 -1)"
        );
        // Scale about ORIGIN, not the bbox center: (2, 3) → (4, 9).
        assert_eq!(
            text(&st_scale(&g("POINT(2 3)"), 2.0, 3.0).unwrap()),
            "POINT(4 9)"
        );
    }

    /// ISO WKB `POINT Z (1 2 3)` — how a 3D value actually reaches kenro.
    fn point_z() -> Vec<u8> {
        let mut v = vec![0x01];
        v.extend_from_slice(&1001u32.to_le_bytes());
        for value in [1.0f64, 2.0, 3.0] {
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    }

    /// All three used to refuse 3D input. PostGIS never did, so they now go
    /// through `coords` and the Z rides along. Every expectation measured on
    /// PostGIS 3.5 against `POINT Z (1 2 3)`.
    #[test]
    fn z_rides_through_every_transform() {
        use crate::functions::{rtree, threed};
        let cases: Vec<(&str, Vec<u8>, f64, f64)> = vec![
            // ST_Translate(…, 10, 20) → POINT(11 22 3)
            (
                "translate",
                st_translate(&point_z(), 10.0, 20.0).unwrap(),
                11.0,
                22.0,
            ),
            // ST_Scale(…, 2, 3) → POINT(2 6 3) — Z is NOT scaled by the 2-arg form
            ("scale", st_scale(&point_z(), 2.0, 3.0).unwrap(), 2.0, 6.0),
            // ST_Rotate(…, pi/2) → POINT(-2 1 3)
            (
                "rotate",
                st_rotate(&point_z(), std::f64::consts::FRAC_PI_2).unwrap(),
                -2.0,
                1.0,
            ),
            // ST_Rotate(…, pi/2, 5, 5) → POINT(8 1 3)
            (
                "rotate_xy",
                st_rotate_xy(&point_z(), std::f64::consts::FRAC_PI_2, 5.0, 5.0).unwrap(),
                8.0,
                1.0,
            ),
        ];
        for (name, out, want_x, want_y) in cases {
            let (x, y) = (
                rtree::st_min_x(&out).unwrap().unwrap(),
                rtree::st_min_y(&out).unwrap().unwrap(),
            );
            assert!((x - want_x).abs() < 1e-9, "{name}: x = {x}");
            assert!((y - want_y).abs() < 1e-9, "{name}: y = {y}");
            // The Z is untouched by all four, and still there.
            assert_eq!(threed::st_z(&out).unwrap(), Some(3.0), "{name}");
            assert_eq!(threed::st_coord_dim(&out).unwrap(), 3, "{name}");
        }
    }

    /// PostGIS transforms a POLYHEDRALSURFACE (measured), so kenro must too —
    /// this is the *move a building* case, and it used to be an error.
    #[test]
    fn surface_collections_transform() {
        use crate::functions::{rtree, surface, threed};
        let moved = st_translate(&surface::fixtures::cube(6), 1000.0, 2000.0).unwrap();
        assert_eq!(surface::st_num_patches(&moved).unwrap(), Some(6));
        assert_eq!(surface::is_closed(&moved).unwrap(), Some(true));
        assert_eq!(rtree::st_min_x(&moved).unwrap(), Some(1000.0));
        // Z untouched by a 2D translate, as in PostGIS.
        assert_eq!(threed::st_zmax(&moved).unwrap(), Some(1.0));

        let scaled = st_scale(&surface::fixtures::cube(6), 2.0, 2.0).unwrap();
        assert_eq!(rtree::st_max_x(&scaled).unwrap(), Some(2.0));
        assert_eq!(threed::st_zmax(&scaled).unwrap(), Some(1.0));
    }
}
