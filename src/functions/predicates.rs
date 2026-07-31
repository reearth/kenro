//! Spatial predicates, DE-9IM based via `geo::Relate` so boundary cases come
//! out PostGIS-correct (e.g. a point on a polygon boundary: intersects=true,
//! contains=false).

use geo::{Distance, Euclidean, Relate};
use geo_types::Geometry;

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

pub fn st_intersects(a: &[u8], b: &[u8]) -> Result<bool> {
    relate_predicate("ST_Intersects", a, b, |m| m.is_intersects())
}

pub fn st_contains(a: &[u8], b: &[u8]) -> Result<bool> {
    relate_predicate("ST_Contains", a, b, |m| m.is_contains())
}

pub fn st_within(a: &[u8], b: &[u8]) -> Result<bool> {
    relate_predicate("ST_Within", a, b, |m| m.is_within())
}

/// 2D cartesian minimum distance; `None` (SQL NULL) when either side is
/// empty, matching PostGIS.
pub fn st_distance(a: &[u8], b: &[u8]) -> Result<Option<f64>> {
    let (ga, gb) = decode_pair("ST_Distance", a, b)?;
    if geom::is_empty(&ga.geometry) || geom::is_empty(&gb.geometry) {
        return Ok(None);
    }
    reject_collection("ST_Distance", &ga)?;
    reject_collection("ST_Distance", &gb)?;
    Ok(Some(Euclidean.distance(&ga.geometry, &gb.geometry)))
}

/// `distance <= d` (PostGIS uses `<=` and raises on a negative tolerance).
pub fn st_dwithin(a: &[u8], b: &[u8], d: f64) -> Result<bool> {
    if d < 0.0 {
        return Err(Error::Unsupported {
            func: "ST_DWithin",
            reason: "tolerance cannot be less than zero".into(),
        });
    }
    Ok(st_distance(a, b)?.is_some_and(|dist| dist <= d))
}

fn relate_predicate(
    func: &'static str,
    a: &[u8],
    b: &[u8],
    pred: impl Fn(&geo::relate::IntersectionMatrix) -> bool,
) -> Result<bool> {
    let (ga, gb) = decode_pair(func, a, b)?;
    if geom::is_empty(&ga.geometry) || geom::is_empty(&gb.geometry) {
        return Ok(false);
    }
    reject_collection(func, &ga)?;
    reject_collection(func, &gb)?;
    let matrix = ga.geometry.relate(&gb.geometry);
    Ok(pred(&matrix))
}

fn decode_pair(func: &'static str, a: &[u8], b: &[u8]) -> Result<(Geom, Geom)> {
    let ga = geom::decode_auto(a)?;
    let gb = geom::decode_auto(b)?;
    // Mixed *known* SRIDs error (PostGIS behavior). If either side is
    // unknown (srid <= 0 — plain WKB, or ST_GeomFromText without srid),
    // proceed: the headline rtree+predicate query depends on this leniency.
    if ga.srid > 0 && gb.srid > 0 && ga.srid != gb.srid {
        return Err(Error::MixedSrid {
            func,
            a: ga.srid,
            b: gb.srid,
        });
    }
    Ok((ga, gb))
}

fn reject_collection(func: &'static str, g: &Geom) -> Result<()> {
    if matches!(g.geometry, Geometry::GeometryCollection(_)) {
        return Err(Error::Unsupported {
            func,
            reason: "GeometryCollection operands are not supported".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::st_geom_from_text;

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    fn g_srid(wkt: &str, srid: i32) -> Vec<u8> {
        st_geom_from_text(wkt, Some(srid)).unwrap()
    }

    const SQUARE: &str = "POLYGON((0 0,10 0,10 10,0 10,0 0))";

    #[test]
    fn basic_predicates() {
        assert!(st_intersects(&g(SQUARE), &g("POINT(5 5)")).unwrap());
        assert!(!st_intersects(&g(SQUARE), &g("POINT(20 20)")).unwrap());
        assert!(st_contains(&g(SQUARE), &g("POINT(5 5)")).unwrap());
        assert!(st_within(&g("POINT(5 5)"), &g(SQUARE)).unwrap());
        assert!(!st_within(&g(SQUARE), &g("POINT(5 5)")).unwrap());
    }

    #[test]
    fn boundary_point_intersects_but_not_contained() {
        let pt = g("POINT(10 5)");
        assert!(st_intersects(&g(SQUARE), &pt).unwrap());
        assert!(!st_contains(&g(SQUARE), &pt).unwrap());
        assert!(!st_within(&pt, &g(SQUARE)).unwrap());
    }

    #[test]
    fn distance_and_dwithin() {
        let a = g("POINT(0 0)");
        let b = g("POINT(3 4)");
        assert_eq!(st_distance(&a, &b).unwrap(), Some(5.0));
        assert!(st_dwithin(&a, &b, 5.0).unwrap()); // <= boundary
        assert!(!st_dwithin(&a, &b, 4.999).unwrap());
        assert!(st_dwithin(&a, &b, -1.0).is_err()); // PostGIS raises too
    }

    #[test]
    fn empty_operands() {
        let e = g("LINESTRING EMPTY");
        assert!(!st_intersects(&g(SQUARE), &e).unwrap());
        assert!(!st_contains(&g(SQUARE), &e).unwrap());
        assert_eq!(st_distance(&g(SQUARE), &e).unwrap(), None);
        assert!(!st_dwithin(&g(SQUARE), &e, 100.0).unwrap());
    }

    #[test]
    fn srid_gate() {
        let a = g_srid("POINT(0 0)", 4326);
        let b = g_srid("POINT(0 0)", 6668);
        assert!(matches!(
            st_intersects(&a, &b),
            Err(Error::MixedSrid { .. })
        ));
        // Unknown srid on either side proceeds.
        assert!(st_intersects(&a, &g("POINT(0 0)")).unwrap());
    }

    #[test]
    fn geometry_collection_rejected() {
        let gc = g("GEOMETRYCOLLECTION(POINT(1 1))");
        assert!(matches!(
            st_contains(&g(SQUARE), &gc),
            Err(Error::Unsupported { .. })
        ));
        assert!(matches!(
            st_intersects(&gc, &g(SQUARE)),
            Err(Error::Unsupported { .. })
        ));
    }
}
