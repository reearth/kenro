//! Measurements on a sphere and on the WGS84 ellipsoid.
//!
//! This closes kenro's sharpest edge: `ST_Distance` and `ST_Length` are
//! planar, so on EPSG:4326 data — the common case for a GeoPackage — they
//! answer in **degrees**. PostGIS users reach for `geography` or these
//! functions instead, and kenro has no geography type, so these are the
//! answer here.
//!
//! `geo`'s metric spaces do the work: `Haversine` carries the same
//! 6 371 008.7714 m mean radius that PostGIS's `ST_DistanceSphere` implies,
//! and `Geodesic` is geographiclib, the same algorithm family behind
//! `ST_DistanceSpheroid`.

use geo::Haversine;
use geo::algorithm::line_measures::Distance;
#[cfg(feature = "spheroid")]
use geo::algorithm::line_measures::Length;
#[cfg(feature = "spheroid")]
use geo::{Geodesic, GeodesicMeasure};
use geo_types::{Geometry, Point};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

fn as_point(g: &Geom, func: &'static str) -> Result<Point<f64>> {
    match g.geometry {
        Geometry::Point(p) => Ok(p),
        _ => Err(Error::Unsupported {
            func,
            reason: "only POINT arguments are supported in kenro (PostGIS accepts any geometry)"
                .into(),
        }),
    }
}

fn decode_pair(func: &'static str, a: &[u8], b: &[u8]) -> Result<(Point<f64>, Point<f64>)> {
    let (ga, gb) = (geom::decode_auto(a)?, geom::decode_auto(b)?);
    if ga.srid > 0 && gb.srid > 0 && ga.srid != gb.srid {
        return Err(Error::MixedSrid {
            func,
            a: ga.srid,
            b: gb.srid,
        });
    }
    Ok((as_point(&ga, func)?, as_point(&gb, func)?))
}

/// `ST_DistanceSphere(a, b)` — great-circle metres on a sphere of radius
/// 6 371 008.7714 m, the radius PostGIS uses.
///
/// kenro accepts POINT arguments only; PostGIS takes any geometry pair.
pub fn st_distance_sphere(a: &[u8], b: &[u8]) -> Result<f64> {
    let (pa, pb) = decode_pair("ST_DistanceSphere", a, b)?;
    Ok(Haversine.distance(pa, pb))
}

#[cfg(feature = "spheroid")]
/// `ST_DistanceSpheroid(a, b)` — metres on the WGS84 ellipsoid
/// (geographiclib). PostGIS's optional third `SPHEROID[…]` argument is
/// accepted by [`st_distance_spheroid_on`].
pub fn st_distance_spheroid(a: &[u8], b: &[u8]) -> Result<f64> {
    let (pa, pb) = decode_pair("ST_DistanceSpheroid", a, b)?;
    Ok(Geodesic.distance(pa, pb))
}

#[cfg(feature = "spheroid")]
/// The three-argument form, taking PostGIS's `SPHEROID["name",a,1/f]` text.
pub fn st_distance_spheroid_on(a: &[u8], b: &[u8], spheroid: &str) -> Result<f64> {
    let (pa, pb) = decode_pair("ST_DistanceSpheroid", a, b)?;
    Ok(measure_from(parse_spheroid("ST_DistanceSpheroid", spheroid)?).distance(pa, pb))
}

#[cfg(feature = "spheroid")]
/// `ST_LengthSpheroid(geom, spheroid)` — geodesic length in metres.
/// PostGIS has no one-argument form, so neither does kenro.
pub fn st_length_spheroid(bytes: &[u8], spheroid: &str) -> Result<f64> {
    const FUNC: &str = "ST_LengthSpheroid";
    let g = geom::decode_auto(bytes)?;
    let measure = measure_from(parse_spheroid(FUNC, spheroid)?);
    // geo reads `metric_space.length(&geometry)`, not the other way round.
    let rings = |p: &geo_types::Polygon<f64>| {
        measure.length(p.exterior()) + p.interiors().iter().map(|r| measure.length(r)).sum::<f64>()
    };
    Ok(match &g.geometry {
        Geometry::LineString(l) => measure.length(l),
        Geometry::MultiLineString(mls) => mls.iter().map(|l| measure.length(l)).sum(),
        Geometry::Polygon(p) => rings(p),
        Geometry::MultiPolygon(mp) => mp.iter().map(rings).sum(),
        Geometry::Point(_) | Geometry::MultiPoint(_) => 0.0,
        _ => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "unsupported geometry type".into(),
            });
        }
    })
}

#[cfg(feature = "spheroid")]
/// Build the measure from a parsed spheroid.
///
/// `GeodesicMeasure::new`'s second parameter is named `inverse_flattening`
/// but is handed straight to geographiclib, which wants the **flattening**
/// itself — passing 298.257… through unconverted yields nonsense (a negative
/// length, in the test that caught this). So invert it here.
fn measure_from(
    (semi_major, inv_flattening): (f64, f64),
) -> impl Distance<f64, Point<f64>, Point<f64>> {
    GeodesicMeasure::new(semi_major, 1.0 / inv_flattening)
}

/// `SPHEROID["WGS 84",6378137,298.257223563]` → (semi-major, 1/f).
///
/// Only the two numbers matter; the name is ignored, as in PostGIS.
#[cfg(feature = "spheroid")]
fn parse_spheroid(func: &'static str, text: &str) -> Result<(f64, f64)> {
    let inner = text
        .trim()
        .strip_prefix("SPHEROID")
        .and_then(|s| s.trim().strip_prefix('['))
        .and_then(|s| s.trim_end().strip_suffix(']'))
        .ok_or_else(|| Error::Unsupported {
            func,
            reason: format!("expected SPHEROID[\"name\",a,1/f], got {text:?}"),
        })?;
    let mut parts = inner.rsplit(',');
    let inv_f = parse_number(func, text, parts.next())?;
    let semi_major = parse_number(func, text, parts.next())?;
    if semi_major <= 0.0 || inv_f <= 0.0 {
        return Err(Error::Unsupported {
            func,
            reason: format!("spheroid parameters must be positive, got {text:?}"),
        });
    }
    Ok((semi_major, inv_f))
}

#[cfg(feature = "spheroid")]
fn parse_number(func: &'static str, whole: &str, part: Option<&str>) -> Result<f64> {
    part.and_then(|p| p.trim().parse::<f64>().ok())
        .ok_or_else(|| Error::Unsupported {
            func,
            reason: format!("expected SPHEROID[\"name\",a,1/f], got {whole:?}"),
        })
}

/// `ST_Project(point, distance, azimuth)` — the point `distance` away along
/// `azimuth` (radians, clockwise from north).
///
/// PostGIS's *geometry* overload is planar — verified live, and kenro matches
/// it. (PostGIS's geodesic behavior lives on the `geography` overload, which
/// kenro has no type for; use `ST_Transform` to a projected CRS first.)
pub fn st_project(bytes: &[u8], distance: f64, azimuth: f64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Project";
    let g = geom::decode_auto(bytes)?;
    let p = as_point(&g, FUNC)?;
    let moved = Point::new(
        p.x() + distance * azimuth.sin(),
        p.y() + distance * azimuth.cos(),
    );
    // The projected point sits where no input vertex did, so the (x, y) index
    // the other derived functions use cannot help. Its height is not in
    // question though — sliding a point along the ground does not change its
    // elevation, and PostGIS agrees (measured on 3.5:
    // `ST_Project(POINT Z (1 2 3), 100, 0.5)` keeps the 3). So assert that one
    // Z rather than refuse.
    if let Some(z) = crate::functions::threed::st_z(bytes)? {
        let index = crate::coords::ZIndex::at(moved.x(), moved.y(), z);
        let wkb = crate::coords::write_wkb_z(&Geometry::Point(moved), &index, FUNC)?;
        return Ok(crate::gpb::write_gpb(&wkb, g.srid, None, false));
    }
    geom::encode_canonical_gpb(
        &Geom {
            geometry: Geometry::Point(moved),
            srid: g.srid,
            has_zm: false,
        },
        FUNC,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, Some(4326)).unwrap()
    }

    #[cfg(feature = "spheroid")]
    const WGS84: &str = "SPHEROID[\"WGS 84\",6378137,298.257223563]";

    #[test]
    #[cfg(feature = "spheroid")]
    fn sphere_and_spheroid_match_postgis() {
        let (a, b) = (g("POINT(0 0)"), g("POINT(1 0)"));
        // PostGIS 3.5: ST_DistanceSphere = 111195.07973463
        assert!(
            (st_distance_sphere(&a, &b).unwrap() - 111_195.079_734_63).abs() < 1e-3,
            "{}",
            st_distance_sphere(&a, &b).unwrap()
        );
        // PostGIS 3.5: ST_DistanceSpheroid = 111319.49079327357
        assert!(
            (st_distance_spheroid(&a, &b).unwrap() - 111_319.490_793_273_57).abs() < 1e-3,
            "{}",
            st_distance_spheroid(&a, &b).unwrap()
        );
        // The explicit-spheroid form agrees with the default one.
        assert!(
            (st_distance_spheroid_on(&a, &b, WGS84).unwrap()
                - st_distance_spheroid(&a, &b).unwrap())
            .abs()
                < 1e-9
        );
    }

    #[test]
    fn the_planar_functions_answer_in_degrees_which_is_the_point() {
        let (a, b) = (g("POINT(0 0)"), g("POINT(1 0)"));
        let planar = crate::functions::predicates::st_distance(&a, &b)
            .unwrap()
            .unwrap();
        assert_eq!(planar, 1.0); // degrees
        assert!(st_distance_sphere(&a, &b).unwrap() > 100_000.0); // metres
    }

    #[test]
    #[cfg(feature = "spheroid")]
    fn length_spheroid_measures_a_degree_of_equator() {
        let line = g("LINESTRING(0 0,1 0)");
        let len = st_length_spheroid(&line, WGS84).unwrap();
        assert!((len - 111_319.490_793_273_57).abs() < 1e-3, "{len}");
        // Points have no length, as in PostGIS.
        assert_eq!(st_length_spheroid(&g("POINT(0 0)"), WGS84).unwrap(), 0.0);
    }

    #[test]
    #[cfg(feature = "spheroid")]
    fn spheroid_text_is_parsed_strictly() {
        let (a, b) = (g("POINT(0 0)"), g("POINT(1 0)"));
        assert!(st_distance_spheroid_on(&a, &b, "WGS 84").is_err());
        assert!(st_distance_spheroid_on(&a, &b, "SPHEROID[\"x\",abc,1]").is_err());
        assert!(st_distance_spheroid_on(&a, &b, "SPHEROID[\"x\",-1,298]").is_err());
    }

    #[test]
    fn project_is_planar_like_postgis_geometry_overload() {
        // PostGIS 3.5: ST_Project(POINT(0 0), 100000, radians(90))
        //              → POINT(100000 -2.449e-11)
        let moved = st_project(&g("POINT(0 0)"), 100_000.0, std::f64::consts::FRAC_PI_2).unwrap();
        let wkt = st_as_text(&moved).unwrap();
        assert!(wkt.starts_with("POINT(100000 "), "{wkt}");
    }

    #[test]
    fn non_point_arguments_are_a_loud_error() {
        let err = st_distance_sphere(&g("LINESTRING(0 0,1 1)"), &g("POINT(0 0)")).unwrap_err();
        assert!(err.to_string().contains("POINT"), "{err}");
    }
}
