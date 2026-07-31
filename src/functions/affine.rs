//! Affine transformations: ST_Rotate, ST_Translate, ST_Scale.
//!
//! PostGIS rotates and scales about the ORIGIN (0, 0) by default — geo's
//! `Rotate`/`Scale` convenience methods use the centroid / bounding-rect
//! center, so the transforms here are built explicitly as
//! `AffineTransform` matrices.

use geo::{AffineOps, AffineTransform};
use geo_types::Geometry;

use crate::error::Result;
use crate::geom::{self, Geom};

fn apply(bytes: &[u8], transform: AffineTransform<f64>, func: &'static str) -> Result<Vec<u8>> {
    let geom = geom::decode_auto(bytes)?;
    let geometry: Geometry<f64> = geom.geometry.affine_transform(&transform);
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid: geom.srid,
            has_zm: false,
        },
        func,
    )
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
    apply(bytes, transform, "ST_Rotate")
}

/// `ST_Translate(geom, dx, dy)`.
pub fn st_translate(bytes: &[u8], dx: f64, dy: f64) -> Result<Vec<u8>> {
    apply(bytes, AffineTransform::translate(dx, dy), "ST_Translate")
}

/// `ST_Scale(geom, xfactor, yfactor)` — about the origin (0,0), PostGIS's
/// convention.
pub fn st_scale(bytes: &[u8], xfactor: f64, yfactor: f64) -> Result<Vec<u8>> {
    apply(
        bytes,
        AffineTransform::scale(xfactor, yfactor, geo_types::coord! { x: 0.0, y: 0.0 }),
        "ST_Scale",
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
}
