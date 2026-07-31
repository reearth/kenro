//! Two-geometry measures: ST_ClosestPoint, ST_LineInterpolatePoint,
//! ST_LineLocatePoint, ST_HausdorffDistance, ST_FrechetDistance,
//! ST_Azimuth.

use geo::line_measures::{FrechetDistance, InterpolatableLine};
use geo::{ClosestPoint, Euclidean, HausdorffDistance, LineLocatePoint};
use geo_types::Geometry;

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

fn decode_pair(func: &'static str, a: &[u8], b: &[u8]) -> Result<(Geom, Geom)> {
    let ga = geom::decode_auto(a)?;
    let gb = geom::decode_auto(b)?;
    if ga.srid > 0 && gb.srid > 0 && ga.srid != gb.srid {
        return Err(Error::MixedSrid {
            func,
            a: ga.srid,
            b: gb.srid,
        });
    }
    Ok((ga, gb))
}

fn point_geom(geometry: Geometry<f64>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid,
            has_zm: false,
        },
        func,
    )
}

/// `ST_ClosestPoint(g1, g2)` — the point on g1 closest to g2. geo's
/// `ClosestPoint` only accepts a Point on the right-hand side, so g2 must
/// be a POINT (documented divergence; other types raise an error).
pub fn st_closest_point(a: &[u8], b: &[u8]) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_ClosestPoint";
    let (ga, gb) = decode_pair(FUNC, a, b)?;
    if geom::is_empty(&ga.geometry) || geom::is_empty(&gb.geometry) {
        return Ok(None);
    }
    let Geometry::Point(p) = gb.geometry else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!(
                "the second argument must be a POINT in kenro (got {}); PostGIS accepts any \
                 geometry",
                geom::wkt_type_name(&gb.geometry)
            ),
        });
    };
    match ga.geometry.closest_point(&p) {
        geo::Closest::Intersection(c) | geo::Closest::SinglePoint(c) => {
            Some(point_geom(Geometry::Point(c), ga.srid, FUNC)).transpose()
        }
        geo::Closest::Indeterminate => Ok(None),
    }
}

/// `ST_LineInterpolatePoint(line, fraction)` — PostGIS raises when the
/// fraction is outside [0, 1] (geo would silently clamp) or the input is
/// not a LINESTRING.
pub fn st_line_interpolate_point(a: &[u8], fraction: f64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_LineInterpolatePoint";
    if !(0.0..=1.0).contains(&fraction) {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!("fraction must be between 0 and 1, got {fraction}"),
        });
    }
    let geom = geom::decode_auto(a)?;
    let Geometry::LineString(ls) = &geom.geometry else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!(
                "argument must be a LINESTRING, got {}",
                geom::wkt_type_name(&geom.geometry)
            ),
        });
    };
    let point = ls
        .point_at_ratio_from_start(&Euclidean, fraction)
        .ok_or_else(|| Error::Unsupported {
            func: FUNC,
            reason: "cannot interpolate on an empty or degenerate linestring".into(),
        })?;
    point_geom(Geometry::Point(point), geom.srid, FUNC)
}

/// `ST_LineLocatePoint(line, point)` — fraction of the line's length at
/// the closest point to the given point.
pub fn st_line_locate_point(a: &[u8], b: &[u8]) -> Result<f64> {
    const FUNC: &str = "ST_LineLocatePoint";
    let (ga, gb) = decode_pair(FUNC, a, b)?;
    let Geometry::LineString(ls) = &ga.geometry else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!(
                "first argument must be a LINESTRING, got {}",
                geom::wkt_type_name(&ga.geometry)
            ),
        });
    };
    let Geometry::Point(p) = gb.geometry else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!(
                "second argument must be a POINT, got {}",
                geom::wkt_type_name(&gb.geometry)
            ),
        });
    };
    ls.line_locate_point(&p).ok_or_else(|| Error::Unsupported {
        func: FUNC,
        reason: "cannot locate on an empty or degenerate linestring".into(),
    })
}

/// `ST_HausdorffDistance(g1, g2)` — discrete Hausdorff distance. geo
/// computes vertex-to-vertex; GEOS computes vertex-to-segment, so results
/// can differ when the nearest point lies mid-segment (documented
/// divergence, golden-pinned).
pub fn st_hausdorff_distance(a: &[u8], b: &[u8]) -> Result<f64> {
    const FUNC: &str = "ST_HausdorffDistance";
    let (ga, gb) = decode_pair(FUNC, a, b)?;
    if geom::is_empty(&ga.geometry) || geom::is_empty(&gb.geometry) {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "empty geometry operands are not supported".into(),
        });
    }
    Ok(ga.geometry.hausdorff_distance(&gb.geometry))
}

/// `ST_FrechetDistance(g1, g2)` — discrete Fréchet distance over vertices
/// (the PostGIS 2-arg form, densifyFrac = -1). LINESTRING × LINESTRING
/// only, matching geo's implementation.
pub fn st_frechet_distance(a: &[u8], b: &[u8]) -> Result<f64> {
    const FUNC: &str = "ST_FrechetDistance";
    let (ga, gb) = decode_pair(FUNC, a, b)?;
    let (Geometry::LineString(la), Geometry::LineString(lb)) = (&ga.geometry, &gb.geometry) else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "both arguments must be LINESTRINGs in kenro".into(),
        });
    };
    if la.0.is_empty() || lb.0.is_empty() {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "empty geometry operands are not supported".into(),
        });
    }
    Ok(Euclidean.frechet_distance(la, lb))
}

/// `ST_Azimuth(pointA, pointB)` — radians clockwise from north; NULL for
/// coincident points.
pub fn st_azimuth(a: &[u8], b: &[u8]) -> Result<Option<f64>> {
    const FUNC: &str = "ST_Azimuth";
    let (ga, gb) = decode_pair(FUNC, a, b)?;
    let (Geometry::Point(pa), Geometry::Point(pb)) = (&ga.geometry, &gb.geometry) else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "both arguments must be POINTs".into(),
        });
    };
    if geom::is_empty(&ga.geometry) || geom::is_empty(&gb.geometry) {
        return Ok(None);
    }
    if pa == pb {
        return Ok(None);
    }
    let azimuth = (pb.x() - pa.x()).atan2(pb.y() - pa.y());
    Ok(Some(if azimuth < 0.0 {
        azimuth + 2.0 * std::f64::consts::PI
    } else {
        azimuth
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    #[test]
    fn closest_point() {
        let line = g("LINESTRING(0 0,10 0)");
        let blob = st_closest_point(&line, &g("POINT(5 3)")).unwrap().unwrap();
        assert_eq!(st_as_text(&blob).unwrap(), "POINT(5 0)");
        assert!(st_closest_point(&line, &g("LINESTRING(0 1,1 1)")).is_err());
        assert_eq!(
            st_closest_point(&g("LINESTRING EMPTY"), &g("POINT(0 0)")).unwrap(),
            None
        );
    }

    #[test]
    fn line_interpolate_and_locate() {
        let line = g("LINESTRING(0 0,10 0)");
        let mid = st_line_interpolate_point(&line, 0.5).unwrap();
        assert_eq!(st_as_text(&mid).unwrap(), "POINT(5 0)");
        assert!(st_line_interpolate_point(&line, 1.5).is_err()); // PostGIS raises
        assert!(st_line_interpolate_point(&g("POINT(0 0)"), 0.5).is_err());
        assert_eq!(
            st_line_locate_point(&line, &g("POINT(2.5 4)")).unwrap(),
            0.25
        );
        assert!(st_line_locate_point(&g("POINT(0 0)"), &g("POINT(1 1)")).is_err());
    }

    #[test]
    fn hausdorff_and_frechet() {
        let a = g("LINESTRING(0 0,10 0)");
        let b = g("LINESTRING(0 3,10 3)");
        assert_eq!(st_hausdorff_distance(&a, &b).unwrap(), 3.0);
        assert_eq!(st_frechet_distance(&a, &b).unwrap(), 3.0);
        // Reversed direction matters for Fréchet, not Hausdorff.
        let rev = g("LINESTRING(10 3,0 3)");
        assert_eq!(st_hausdorff_distance(&a, &rev).unwrap(), 3.0);
        assert!(st_frechet_distance(&a, &rev).unwrap() > 3.0);
        assert!(st_frechet_distance(&a, &g("POINT(0 0)")).is_err());
    }

    #[test]
    fn azimuth() {
        use std::f64::consts::PI;
        let o = g("POINT(0 0)");
        let north = st_azimuth(&o, &g("POINT(0 5)")).unwrap().unwrap();
        assert!((north - 0.0).abs() < 1e-12);
        let east = st_azimuth(&o, &g("POINT(5 0)")).unwrap().unwrap();
        assert!((east - PI / 2.0).abs() < 1e-12);
        let southwest = st_azimuth(&o, &g("POINT(-1 -1)")).unwrap().unwrap();
        assert!((southwest - 5.0 * PI / 4.0).abs() < 1e-12);
        assert_eq!(st_azimuth(&o, &g("POINT(0 0)")).unwrap(), None);
        assert!(st_azimuth(&o, &g("LINESTRING(0 0,1 1)")).is_err());
    }
}
