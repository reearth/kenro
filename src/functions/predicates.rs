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

/// The only predicate that is TRUE when an operand is empty (the DE-9IM
/// matrix degenerates to `FF*FF****`).
pub fn st_disjoint(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb) = decode_pair("ST_Disjoint", a, b)?;
    if geom::is_empty(&ga.geometry) || geom::is_empty(&gb.geometry) {
        return Ok(true);
    }
    reject_collection("ST_Disjoint", &ga)?;
    reject_collection("ST_Disjoint", &gb)?;
    Ok(ga.geometry.relate(&gb.geometry).is_disjoint())
}

pub fn st_touches(a: &[u8], b: &[u8]) -> Result<bool> {
    relate_predicate("ST_Touches", a, b, |m| m.is_touches())
}

pub fn st_crosses(a: &[u8], b: &[u8]) -> Result<bool> {
    relate_predicate("ST_Crosses", a, b, |m| m.is_crosses())
}

pub fn st_overlaps(a: &[u8], b: &[u8]) -> Result<bool> {
    relate_predicate("ST_Overlaps", a, b, |m| m.is_overlaps())
}

/// Two empty geometries are equal in PostGIS (golden-verified), so the
/// empty short-circuit differs from the other predicates.
pub fn st_equals(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb) = decode_pair("ST_Equals", a, b)?;
    let (ea, eb) = (geom::is_empty(&ga.geometry), geom::is_empty(&gb.geometry));
    if ea || eb {
        return Ok(ea && eb);
    }
    reject_collection("ST_Equals", &ga)?;
    reject_collection("ST_Equals", &gb)?;
    Ok(ga.geometry.relate(&gb.geometry).is_equal_topo())
}

pub fn st_covers(a: &[u8], b: &[u8]) -> Result<bool> {
    relate_predicate("ST_Covers", a, b, |m| m.is_covers())
}

pub fn st_covered_by(a: &[u8], b: &[u8]) -> Result<bool> {
    relate_predicate("ST_CoveredBy", a, b, |m| m.is_coveredby())
}

/// `ST_Relate(a, b)` → the 9-character DE-9IM matrix string.
pub fn st_relate(a: &[u8], b: &[u8]) -> Result<String> {
    de9im_string("ST_Relate", a, b)
}

/// `ST_Relate(a, b, pattern)` → whether the DE-9IM matrix matches the
/// pattern (`T` = any intersection, `F` = none, `0`/`1`/`2` = exact
/// dimension, `*` = anything).
pub fn st_relate_pattern(a: &[u8], b: &[u8], pattern: &str) -> Result<bool> {
    let matrix = de9im_string("ST_Relate", a, b)?;
    de9im_matches("ST_Relate", &matrix, pattern)
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

/// The 9-character DE-9IM string in row order (Interior, Boundary,
/// Exterior of a) × (Interior, Boundary, Exterior of b). Empty operands
/// bypass `relate` (their matrix is fully determined by the other side's
/// dimensions), everything else reads the computed matrix cell by cell.
fn de9im_string(func: &'static str, a: &[u8], b: &[u8]) -> Result<String> {
    use geo::HasDimensions;
    use geo::algorithm::dimensions::Dimensions;
    use geo::coordinate_position::CoordPos;

    let (ga, gb) = decode_pair(func, a, b)?;
    reject_collection(func, &ga)?;
    reject_collection(func, &gb)?;

    let dim_char = |d: Dimensions| match d {
        Dimensions::Empty => 'F',
        Dimensions::ZeroDimensional => '0',
        Dimensions::OneDimensional => '1',
        Dimensions::TwoDimensional => '2',
    };

    let a_empty = geom::is_empty(&ga.geometry);
    let b_empty = geom::is_empty(&gb.geometry);
    if a_empty || b_empty {
        // An empty side's interior/boundary rows and columns are all F; its
        // exterior is everything, so it meets the other side's interior and
        // boundary at their full dimensions. Exterior × exterior is always 2.
        let (ai, ab) = if a_empty {
            (Dimensions::Empty, Dimensions::Empty)
        } else {
            (ga.geometry.dimensions(), ga.geometry.boundary_dimensions())
        };
        let (bi, bb) = if b_empty {
            (Dimensions::Empty, Dimensions::Empty)
        } else {
            (gb.geometry.dimensions(), gb.geometry.boundary_dimensions())
        };
        let cells = [
            ['F', 'F', dim_char(ai)],
            ['F', 'F', dim_char(ab)],
            [dim_char(bi), dim_char(bb), '2'],
        ];
        return Ok(cells.iter().flatten().collect());
    }

    let matrix = ga.geometry.relate(&gb.geometry);
    let positions = [CoordPos::Inside, CoordPos::OnBoundary, CoordPos::Outside];
    Ok(positions
        .iter()
        .flat_map(|pa| positions.iter().map(|pb| dim_char(matrix.get(*pa, *pb))))
        .collect())
}

/// Match a computed DE-9IM string against a pattern.
fn de9im_matches(func: &'static str, matrix: &str, pattern: &str) -> Result<bool> {
    if pattern.len() != 9 {
        return Err(Error::Unsupported {
            func,
            reason: format!("DE-9IM pattern must be 9 characters, got {:?}", pattern),
        });
    }
    let mut result = true;
    for (m, p) in matrix.chars().zip(pattern.chars()) {
        let ok = match p.to_ascii_uppercase() {
            '*' => true,
            'T' => m != 'F',
            'F' => m == 'F',
            '0' | '1' | '2' => m == p,
            other => {
                return Err(Error::Unsupported {
                    func,
                    reason: format!("invalid DE-9IM pattern character {other:?}"),
                });
            }
        };
        result &= ok;
    }
    Ok(result)
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
    fn predicate_family() {
        let square = g(SQUARE);
        let inner = g("POLYGON((2 2,8 2,8 8,2 8,2 2))");
        let adjacent = g("POLYGON((10 0,20 0,20 10,10 10,10 0))");
        let far = g("POLYGON((100 100,110 100,110 110,100 110,100 100))");
        let overlapping = g("POLYGON((5 5,15 5,15 15,5 15,5 5))");
        let crossing_line = g("LINESTRING(-5 5,15 5)");
        let boundary_pt = g("POINT(10 5)");

        assert!(st_disjoint(&square, &far).unwrap());
        assert!(!st_disjoint(&square, &inner).unwrap());
        assert!(st_touches(&square, &adjacent).unwrap());
        assert!(!st_touches(&square, &inner).unwrap());
        assert!(st_crosses(&square, &crossing_line).unwrap());
        assert!(!st_crosses(&square, &inner).unwrap());
        assert!(st_overlaps(&square, &overlapping).unwrap());
        assert!(!st_overlaps(&square, &inner).unwrap()); // containment is not overlap
        assert!(st_equals(&square, &g(SQUARE)).unwrap());
        assert!(!st_equals(&square, &inner).unwrap());
        // Covers vs Contains: a boundary point is covered but not contained.
        assert!(st_covers(&square, &boundary_pt).unwrap());
        assert!(!st_contains(&square, &boundary_pt).unwrap());
        assert!(st_covered_by(&boundary_pt, &square).unwrap());
    }

    #[test]
    fn disjoint_is_true_for_empty_operands() {
        let e = g("LINESTRING EMPTY");
        assert!(st_disjoint(&g(SQUARE), &e).unwrap());
        assert!(st_disjoint(&e, &e).unwrap());
        assert!(!st_touches(&g(SQUARE), &e).unwrap());
        assert!(st_equals(&e, &e).unwrap()); // PostGIS: two empties are equal
        assert!(!st_equals(&g(SQUARE), &e).unwrap());
    }

    #[test]
    fn relate_strings() {
        // Point strictly inside a polygon: the classic 0FFFFF212.
        assert_eq!(st_relate(&g(SQUARE), &g("POINT(5 5)")).unwrap().len(), 9);
        assert_eq!(
            st_relate(&g("POINT(5 5)"), &g(SQUARE)).unwrap(),
            "0FFFFF212"
        );
        // Empty vs polygon: manual matrix.
        assert_eq!(
            st_relate(&g("LINESTRING EMPTY"), &g(SQUARE)).unwrap(),
            "FFFFFF212"
        );
        assert_eq!(
            st_relate(&g(SQUARE), &g("POLYGON EMPTY")).unwrap(),
            "FF2FF1FF2"
        );
    }

    #[test]
    fn relate_pattern_matching() {
        let within_pattern = "T*F**F***";
        assert!(st_relate_pattern(&g("POINT(5 5)"), &g(SQUARE), within_pattern).unwrap());
        assert!(!st_relate_pattern(&g("POINT(50 50)"), &g(SQUARE), within_pattern).unwrap());
        // Case-insensitive pattern letters; invalid patterns error.
        assert!(st_relate_pattern(&g("POINT(5 5)"), &g(SQUARE), "t*f**f***").unwrap());
        assert!(st_relate_pattern(&g(SQUARE), &g(SQUARE), "*********").unwrap());
        assert!(st_relate_pattern(&g(SQUARE), &g(SQUARE), "TOOSHORT").is_err());
        assert!(st_relate_pattern(&g(SQUARE), &g(SQUARE), "X********").is_err());
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
