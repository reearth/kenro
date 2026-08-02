//! Concave hulls and Delaunay triangulation.
//!
//! Both are behind their own cargo feature (`concave-hull`, `delaunay`) and
//! both are in `full`, because they are the two most expensive functions in
//! the catalog by binary size — measured on the wasm standard tier,
//! +41 KB and +81 KB respectively, against ~21 KB for a whole group of
//! ordinary functions. Nothing else here is unusual; the size is the reason
//! they are opt-in rather than default.

#[cfg(feature = "concave-hull")]
use geo::algorithm::{Area, ConcaveHull, ConvexHull};
use geo_types::Geometry;

#[allow(unused_imports)]
use crate::error::{Error, Result};
use crate::geom::{self, Geom};

#[allow(dead_code)]
fn out(geometry: Geometry<f64>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid,
            has_zm: false,
        },
        func,
    )
}

/// `ST_ConcaveHull(geom, target_percent)` — a hull whose area is the given
/// fraction of the convex hull's; 1.0 *is* the convex hull.
///
/// PostGIS's argument is this area ratio, while `geo`'s parameter is a
/// "concavity" length ratio with the opposite sense — pasting PostGIS SQL
/// against geo's parameter would silently return a wildly different shape.
/// kenro therefore keeps **PostGIS's contract** and searches geo's parameter
/// for it, which costs a handful of hull computations per call.
///
/// ⚠️ The hull *family* still differs from GEOS's, so the vertices are not
/// PostGIS's. What holds — and is tested — is the contract: the result never
/// exceeds the convex hull, a target of 1.0 returns the convex hull exactly,
/// and a smaller target never returns a larger hull.
#[cfg(feature = "concave-hull")]
pub fn st_concave_hull(bytes: &[u8], target_percent: f64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_ConcaveHull";
    if !(0.0..=1.0).contains(&target_percent) {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!(
                "target_percent must be between 0 and 1 (got {target_percent}); it is the \
                 fraction of the convex hull's area to aim for, as in PostGIS"
            ),
        });
    }
    let g = geom::decode_auto(bytes)?;
    let convex = convex_hull_of(&g.geometry, FUNC)?;
    let convex_area = convex.unsigned_area();
    if target_percent >= 1.0 || convex_area == 0.0 {
        return out(Geometry::Polygon(convex), g.srid, FUNC);
    }

    let target_area = convex_area * target_percent;
    // Larger concavity → closer to the convex hull → larger area. Bisect for
    // the tightest hull that still meets the target, so kenro never carves
    // away more than was asked for.
    let (mut lo, mut hi) = (0.01_f64, 100.0_f64);
    let mut best = convex.clone();
    for _ in 0..24 {
        let mid = (lo * hi).sqrt(); // geometric mean: the parameter is a ratio
        let hull = concave_hull_of(&g.geometry, mid, FUNC)?;
        if hull.unsigned_area() >= target_area {
            best = hull;
            hi = mid;
        } else {
            lo = mid;
        }
        if hi / lo < 1.001 {
            break;
        }
    }
    out(Geometry::Polygon(best), g.srid, FUNC)
}

#[cfg(feature = "concave-hull")]
fn convex_hull_of(g: &Geometry<f64>, func: &'static str) -> Result<geo_types::Polygon<f64>> {
    Ok(match g {
        Geometry::MultiPoint(mp) => mp.convex_hull(),
        Geometry::LineString(l) => l.convex_hull(),
        Geometry::MultiLineString(mls) => mls.convex_hull(),
        Geometry::Polygon(p) => p.convex_hull(),
        Geometry::MultiPolygon(mp) => mp.convex_hull(),
        Geometry::Point(p) => p.convex_hull(),
        _ => {
            return Err(Error::Unsupported {
                func,
                reason: "unsupported geometry type".into(),
            });
        }
    })
}

#[cfg(feature = "concave-hull")]
fn concave_hull_of(
    g: &Geometry<f64>,
    concavity: f64,
    func: &'static str,
) -> Result<geo_types::Polygon<f64>> {
    let options = geo::algorithm::concave_hull::ConcaveHullOptions {
        concavity,
        length_threshold: 0.0,
    };
    Ok(match g {
        Geometry::MultiPoint(mp) => mp.concave_hull_with_options(options),
        Geometry::LineString(l) => l.concave_hull_with_options(options),
        Geometry::MultiLineString(mls) => mls.concave_hull_with_options(options),
        Geometry::Polygon(p) => p.concave_hull_with_options(options),
        Geometry::MultiPolygon(mp) => mp.concave_hull_with_options(options),
        Geometry::Point(_) => convex_hull_of(g, func)?,
        _ => {
            return Err(Error::Unsupported {
                func,
                reason: "unsupported geometry type".into(),
            });
        }
    })
}

/// `ST_DelaunayTriangles(geom)` — the Delaunay triangulation of the input's
/// vertices.
///
/// ⚠️ PostGIS returns a GEOMETRYCOLLECTION (or, with its `flags` argument, a
/// MULTILINESTRING or TIN); kenro never produces collections, so this is a
/// **MULTIPOLYGON** of triangles. The `tolerance` and `flags` arguments are
/// not implemented — `geo`'s triangulator has no snapping tolerance, and
/// the alternative outputs are derivable (`ST_Boundary` for the edges).
#[cfg(feature = "delaunay")]
pub fn st_delaunay_triangles(bytes: &[u8]) -> Result<Vec<u8>> {
    use geo::algorithm::TriangulateDelaunayUnconstrained;
    const FUNC: &str = "ST_DelaunayTriangles";
    let g = geom::decode_auto(bytes)?;
    let triangles = match &g.geometry {
        Geometry::MultiPoint(mp) => mp.unconstrained_triangulation(),
        Geometry::LineString(l) => l.unconstrained_triangulation(),
        Geometry::MultiLineString(mls) => mls.unconstrained_triangulation(),
        Geometry::Polygon(p) => p.unconstrained_triangulation(),
        Geometry::MultiPolygon(mp) => mp.unconstrained_triangulation(),
        _ => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "unsupported geometry type".into(),
            });
        }
    }
    .map_err(|e| Error::Unsupported {
        func: FUNC,
        reason: format!("triangulation failed: {e:?}"),
    })?;
    let polygons: Vec<geo_types::Polygon<f64>> =
        triangles.into_iter().map(|t| t.to_polygon()).collect();
    out(
        Geometry::MultiPolygon(geo_types::MultiPolygon::new(polygons)),
        g.srid,
        FUNC,
    )
}

/// `ST_TriangulatePolygon(geom)` — triangulate a polygon **respecting its
/// own edges**, so the triangles tile exactly the polygon and nothing else.
///
/// This is the constrained counterpart of `ST_DelaunayTriangles`, which
/// triangulates the convex hull of the vertices and therefore covers holes
/// and concavities as well. The constrained algorithm was previously listed
/// as out of scope; it is in fact `geo`'s `TriangulateDelaunay`, and `spade` is
/// already in the tree because the unconstrained triangulator uses it.
///
/// ⚠️ PostGIS returns a GEOMETRYCOLLECTION of triangle polygons; kenro
/// returns a **MULTIPOLYGON**, as `ST_DelaunayTriangles` already does. The
/// *set* of triangles is not GEOS's — a triangulation is not unique — but the
/// contract is: every triangle lies inside the polygon, and their areas sum
/// to the polygon's. Non-areal input is an error here rather than PostGIS's
/// empty collection.
#[cfg(feature = "delaunay")]
pub fn st_triangulate_polygon(bytes: &[u8]) -> Result<Vec<u8>> {
    use geo::algorithm::TriangulateDelaunay;
    const FUNC: &str = "ST_TriangulatePolygon";
    let g = geom::decode_auto(bytes)?;
    let triangles = match &g.geometry {
        Geometry::Polygon(p) => p.constrained_triangulation(Default::default()),
        Geometry::MultiPolygon(mp) => mp.constrained_triangulation(Default::default()),
        Geometry::Rect(r) => r.to_polygon().constrained_triangulation(Default::default()),
        Geometry::Triangle(t) => t.to_polygon().constrained_triangulation(Default::default()),
        _ => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "argument must be a POLYGON or MULTIPOLYGON (PostGIS answers \
                         GEOMETRYCOLLECTION EMPTY here; kenro will not return a collection, \
                         and an empty result would hide the mistake)"
                    .into(),
            });
        }
    }
    .map_err(|e| Error::Unsupported {
        func: FUNC,
        reason: format!("triangulation failed: {e:?}"),
    })?;
    out(
        Geometry::MultiPolygon(geo_types::MultiPolygon::new(
            triangles.into_iter().map(|t| t.to_polygon()).collect(),
        )),
        g.srid,
        FUNC,
    )
}

#[cfg(test)]
mod tests {
    #[cfg(any(feature = "concave-hull", feature = "delaunay"))]
    use super::*;
    #[cfg(any(feature = "concave-hull", feature = "delaunay"))]
    use crate::functions::io::st_geom_from_text;

    #[cfg(any(feature = "concave-hull", feature = "delaunay"))]
    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    /// A ring of points around a hollow centre: a concave hull can carve the
    /// middle out, a convex one cannot.
    #[cfg(feature = "concave-hull")]
    const RING: &str = "MULTIPOINT(0 0,2 0,4 0,4 2,4 4,2 4,0 4,0 2,\
                        1 1,3 1,3 3,1 3)";

    #[cfg(feature = "concave-hull")]
    #[test]
    fn concave_hull_keeps_postgis_argument_contract() {
        use crate::functions::accessors::st_area;

        let input = g(RING);
        let convex = crate::functions::processing::st_convex_hull(&input).unwrap();
        let convex_area = st_area(&convex).unwrap();

        // 1.0 is the convex hull exactly — the property PostGIS documents.
        let at_one = st_concave_hull(&input, 1.0).unwrap();
        assert!((st_area(&at_one).unwrap() - convex_area).abs() < 1e-9);

        // Nothing ever exceeds the convex hull, and a smaller target never
        // gives a larger hull.
        let mut previous = convex_area;
        for target in [0.9, 0.7, 0.5, 0.2] {
            let area = st_area(&st_concave_hull(&input, target).unwrap()).unwrap();
            assert!(
                area <= convex_area + 1e-9,
                "{target}: {area} > {convex_area}"
            );
            assert!(area <= previous + 1e-9, "{target}: {area} > {previous}");
            previous = area;
        }
    }

    #[cfg(feature = "concave-hull")]
    #[test]
    fn concave_hull_rejects_a_geo_style_concavity() {
        // The trap this guards: geo's parameter is a concavity of ~2, which
        // is out of PostGIS's range and must not be silently accepted.
        let err = st_concave_hull(&g(RING), 2.0).unwrap_err().to_string();
        assert!(err.contains("between 0 and 1"), "{err}");
        assert!(st_concave_hull(&g(RING), -0.1).is_err());
    }

    #[cfg(feature = "delaunay")]
    #[test]
    fn delaunay_triangulates_a_square_like_postgis() {
        use crate::functions::accessors::{st_area, st_num_geometries};

        // PostGIS 3.5: 2 triangles, total area 16 (as a GEOMETRYCOLLECTION;
        // kenro returns the same triangles as a MULTIPOLYGON).
        let triangles = st_delaunay_triangles(&g("MULTIPOINT(0 0,4 0,4 4,0 4)")).unwrap();
        assert_eq!(st_num_geometries(&triangles).unwrap(), 2);
        assert!((st_area(&triangles).unwrap() - 16.0).abs() < 1e-9);
        assert_eq!(
            crate::functions::accessors::st_geometry_type(&triangles).unwrap(),
            "ST_MultiPolygon"
        );
    }

    /// The constrained triangulation's whole point is what it *doesn't*
    /// cover, so the test is the difference from the unconstrained one.
    #[cfg(feature = "delaunay")]
    #[test]
    fn constrained_triangulation_respects_holes_and_concavity() {
        use crate::functions::accessors::{st_area, st_geometry_type, st_num_geometries};

        // A square with a 2×2 hole. PostGIS 3.5 (GEOS 3.11): 8 triangles
        // totalling 96 — the hole is not covered.
        let holed = g("POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,4 2,4 4,2 4,2 2))");
        let t = st_triangulate_polygon(&holed).unwrap();
        assert!((st_area(&t).unwrap() - 96.0).abs() < 1e-9);
        assert_eq!(st_geometry_type(&t).unwrap(), "ST_MultiPolygon");
        // …whereas the unconstrained triangulation spans the convex hull of
        // the vertices, hole included. This is the distinction that makes
        // the second function worth having.
        assert!((st_area(&st_delaunay_triangles(&holed).unwrap()).unwrap() - 100.0).abs() < 1e-9);

        // A concave L keeps its notch.
        let l = g("POLYGON((0 0,10 0,10 4,4 4,4 10,0 10,0 0))");
        assert!((st_area(&st_triangulate_polygon(&l).unwrap()).unwrap() - 64.0).abs() < 1e-9);

        // Disjoint members are triangulated separately, not bridged.
        let two = g("MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))");
        let t = st_triangulate_polygon(&two).unwrap();
        assert_eq!(st_num_geometries(&t).unwrap(), 2);
        assert!((st_area(&t).unwrap() - 1.0).abs() < 1e-9);

        // Non-areal input errors rather than returning PostGIS's empty
        // collection, which would look like a successful triangulation.
        assert!(st_triangulate_polygon(&g("LINESTRING(0 0,1 1)")).is_err());
        assert!(st_triangulate_polygon(&g("POINT(0 0)")).is_err());
    }
}
