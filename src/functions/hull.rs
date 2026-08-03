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
use crate::geom;

#[allow(dead_code)]
/// Encode a derived geometry, restoring Z from the inputs it came from.
/// See [`geom::encode_derived`] for why every call site has to name them.
fn out(
    geometry: Geometry<f64>,
    srid: i32,
    func: &'static str,
    sources: &[&[u8]],
) -> Result<Vec<u8>> {
    geom::encode_derived(geometry, srid, func, sources)
}

/// Encode a derived geometry as 2D **on purpose**, for the Voronoi pair: a
/// cell corner is a circumcentre, never an input vertex, so there is no Z to
/// carry — and PostGIS answers in 2D here too (`ST_Zmflag` = 0, measured,
/// unlike its `ST_DelaunayTriangles`, which is 3).
#[cfg(feature = "voronoi")]
fn out_2d(geometry: Geometry<f64>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &crate::geom::Geom {
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
        return out(Geometry::Polygon(convex), g.srid, FUNC, &[bytes]);
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
    out(Geometry::Polygon(best), g.srid, FUNC, &[bytes])
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
        &[bytes],
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
        &[bytes],
    )
}

// ---------------------------------------------------------------------------
// Voronoi — the dual of the Delaunay triangulation above
// ---------------------------------------------------------------------------

/// PostGIS's default clip box, measured rather than taken from a doc comment.
///
/// GEOS expands the sites' envelope by **`max(width, height)` on all four
/// sides** — not by a percentage, and not per-axis. Measured on PostGIS 3.5:
/// sites spanning `0..10 × 0..2` come back clipped to `-10..20 × -10..12`,
/// so the *height* is padded by the *width*. `geo`'s `VoronoiClip::Padded`
/// documents itself as "matching PostGIS" at 50% padding; it does not, which
/// is why kenro computes the box itself and passes `VoronoiClip::Polygon`.
///
/// `extend_to` is **unioned** with that box rather than replacing it (also
/// measured: an `extend_to` smaller than the default leaves the result
/// unchanged), and only its envelope matters, not its shape.
#[cfg(feature = "voronoi")]
fn voronoi_clip(
    geometry: &Geometry<f64>,
    extend_to: Option<&Geometry<f64>>,
) -> Option<geo_types::Polygon<f64>> {
    use geo::BoundingRect;
    let rect = geometry.bounding_rect()?;
    let pad = rect.width().max(rect.height());
    let (mut min_x, mut min_y) = (rect.min().x - pad, rect.min().y - pad);
    let (mut max_x, mut max_y) = (rect.max().x + pad, rect.max().y + pad);
    if let Some(e) = extend_to.and_then(|e| e.bounding_rect()) {
        min_x = min_x.min(e.min().x);
        min_y = min_y.min(e.min().y);
        max_x = max_x.max(e.max().x);
        max_y = max_y.max(e.max().y);
    }
    Some(
        geo_types::Rect::new(
            geo_types::coord! { x: min_x, y: min_y },
            geo_types::coord! { x: max_x, y: max_y },
        )
        .to_polygon(),
    )
}

/// `ST_VoronoiPolygons(geom [, tolerance [, extend_to]])` — one cell per input
/// vertex.
///
/// Unlike `ST_Dump` and the grid generators, this one is **not** set-returning
/// in PostGIS either — it returns a single geometry — so kenro can use the
/// real name rather than inventing one.
///
/// ⚠️ **Divergences.** PostGIS returns a GEOMETRYCOLLECTION; kenro returns a
/// **MULTIPOLYGON**, as `ST_DelaunayTriangles` and `ST_Subdivide` already do.
/// The output is 2D even for 3D input — PostGIS's is too (`ST_Zmflag` = 0,
/// measured), because a cell's corners are circumcentres and never input
/// vertices, so there is no Z to carry. **Collinear input is an error here**
/// where PostGIS produces cells: `geo` reports `CollinearInput` and offers
/// the perpendicular bisectors instead, which is `ST_VoronoiLines`.
#[cfg(feature = "voronoi")]
pub fn st_voronoi_polygons(
    bytes: &[u8],
    tolerance: Option<f64>,
    extend_to: Option<&[u8]>,
) -> Result<Vec<u8>> {
    use geo::algorithm::voronoi::{Voronoi, VoronoiClip, VoronoiError, VoronoiParams};
    const FUNC: &str = "ST_VoronoiPolygons";

    let g = geom::decode_auto(bytes)?;
    let extend = extend_to.map(geom::decode_auto).transpose()?;
    let empty = || {
        out_2d(
            Geometry::MultiPolygon(geo_types::MultiPolygon::new(vec![])),
            g.srid,
            FUNC,
        )
    };
    let Some(clip) = voronoi_clip(&g.geometry, extend.as_ref().map(|e| &e.geometry)) else {
        return empty();
    };
    let params = VoronoiParams::new()
        .tolerance(tolerance.unwrap_or(0.0))
        .clip(VoronoiClip::Polygon(&clip));

    let cells = match &g.geometry {
        Geometry::MultiPoint(mp) => mp.voronoi_cells_with_params(params),
        Geometry::Point(p) => p.voronoi_cells_with_params(params),
        Geometry::LineString(l) => l.voronoi_cells_with_params(params),
        Geometry::MultiLineString(mls) => mls.voronoi_cells_with_params(params),
        Geometry::Polygon(p) => p.voronoi_cells_with_params(params),
        Geometry::MultiPolygon(mp) => mp.voronoi_cells_with_params(params),
        _ => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "unsupported geometry type".into(),
            });
        }
    };
    let cells = match cells {
        Ok(c) => c,
        // PostGIS answers GEOMETRYCOLLECTION EMPTY for one point or none;
        // kenro answers an empty MULTIPOLYGON, which is the same statement.
        Err(VoronoiError::InsufficientVertices) => return empty(),
        Err(VoronoiError::CollinearInput) => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "the input vertices are collinear, so they bound no cells; PostGIS \
                         returns degenerate ones here, kenro does not — use ST_VoronoiLines \
                         for the perpendicular bisectors"
                    .into(),
            });
        }
        Err(e) => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: format!("voronoi failed: {e:?}"),
            });
        }
    };
    out_2d(
        Geometry::MultiPolygon(geo_types::MultiPolygon::new(cells)),
        g.srid,
        FUNC,
    )
}

/// `ST_VoronoiLines(geom [, tolerance [, extend_to]])` — the cell boundaries.
///
/// PostGIS returns a MULTILINESTRING here, so this is the one function in the
/// pair with **no return-type divergence at all**. It also works on collinear
/// input, where `ST_VoronoiPolygons` cannot.
#[cfg(feature = "voronoi")]
pub fn st_voronoi_lines(
    bytes: &[u8],
    tolerance: Option<f64>,
    extend_to: Option<&[u8]>,
) -> Result<Vec<u8>> {
    use geo::algorithm::voronoi::{Voronoi, VoronoiClip, VoronoiError, VoronoiParams};
    const FUNC: &str = "ST_VoronoiLines";

    let g = geom::decode_auto(bytes)?;
    let extend = extend_to.map(geom::decode_auto).transpose()?;
    let empty = || {
        out_2d(
            Geometry::MultiLineString(geo_types::MultiLineString::new(vec![])),
            g.srid,
            FUNC,
        )
    };
    let Some(clip) = voronoi_clip(&g.geometry, extend.as_ref().map(|e| &e.geometry)) else {
        return empty();
    };
    let params = VoronoiParams::new()
        .tolerance(tolerance.unwrap_or(0.0))
        .clip(VoronoiClip::Polygon(&clip));

    let edges = match &g.geometry {
        Geometry::MultiPoint(mp) => mp.voronoi_edges_with_params(params),
        Geometry::Point(p) => p.voronoi_edges_with_params(params),
        Geometry::LineString(l) => l.voronoi_edges_with_params(params),
        Geometry::MultiLineString(mls) => mls.voronoi_edges_with_params(params),
        Geometry::Polygon(p) => p.voronoi_edges_with_params(params),
        Geometry::MultiPolygon(mp) => mp.voronoi_edges_with_params(params),
        _ => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "unsupported geometry type".into(),
            });
        }
    };
    let edges = match edges {
        Ok(e) => e,
        Err(VoronoiError::InsufficientVertices) => return empty(),
        Err(e) => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: format!("voronoi failed: {e:?}"),
            });
        }
    };
    out_2d(
        Geometry::MultiLineString(geo_types::MultiLineString::new(
            edges
                .into_iter()
                .map(|l| geo_types::LineString::new(vec![l.start, l.end]))
                .collect(),
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

    /// The clip box is the part worth pinning: `geo`'s own `Padded` mode
    /// claims to match PostGIS and does not, so these numbers come from a
    /// live PostGIS 3.5 session.
    #[cfg(feature = "voronoi")]
    #[test]
    fn voronoi_clips_the_way_postgis_does() {
        use crate::functions::accessors::{st_area, st_geometry_type, st_num_geometries};
        use crate::functions::io::st_as_text;

        let square = g("MULTIPOINT(0 0,4 0,4 4,0 4)");
        let v = st_voronoi_polygons(&square, None, None).unwrap();
        // PostGIS: 4 cells, total area 144 — the sites' 4×4 envelope padded
        // by 4 on every side, so 12×12.
        assert_eq!(st_num_geometries(&v).unwrap(), 4);
        assert!(
            (st_area(&v).unwrap() - 144.0).abs() < 1e-9,
            "{}",
            st_area(&v).unwrap()
        );
        // kenro returns a MULTIPOLYGON where PostGIS returns a collection.
        assert_eq!(st_geometry_type(&v).unwrap(), "ST_MultiPolygon");

        // The padding is max(width, height) on all four sides, not per-axis:
        // sites over 0..10 × 0..2 clip to -10..20 × -10..12 in PostGIS, so
        // the *height* is padded by the *width*. This is the assertion that
        // fails if anyone swaps in geo's `Padded`.
        let wide = g("MULTIPOINT(0 0,10 0,10 2,0 2)");
        let v = st_voronoi_polygons(&wide, None, None).unwrap();
        assert!(
            (st_area(&v).unwrap() - 30.0 * 22.0).abs() < 1e-9,
            "{}",
            st_area(&v).unwrap()
        );

        // extend_to is unioned with that box and only its envelope counts —
        // one smaller than the default changes nothing.
        let big = st_voronoi_polygons(
            &square,
            Some(0.0),
            Some(&g("POLYGON((-10 -10,10 -10,10 10,-10 10,-10 -10))")),
        )
        .unwrap();
        assert!(
            (st_area(&big).unwrap() - 400.0).abs() < 1e-9,
            "{}",
            st_area(&big).unwrap()
        );
        let small = st_voronoi_polygons(
            &square,
            Some(0.0),
            Some(&g("POLYGON((1 1,3 1,3 3,1 3,1 1))")),
        )
        .unwrap();
        assert!(
            (st_area(&small).unwrap() - 144.0).abs() < 1e-9,
            "{}",
            st_area(&small).unwrap()
        );

        // One vertex bounds nothing: PostGIS says GEOMETRYCOLLECTION EMPTY,
        // kenro says MULTIPOLYGON EMPTY.
        assert_eq!(
            st_as_text(&st_voronoi_polygons(&g("POINT(0 0)"), None, None).unwrap()).unwrap(),
            "MULTIPOLYGON EMPTY"
        );

        // The lines form is the one with no divergence at all — PostGIS
        // returns a MULTILINESTRING here too.
        let l = st_voronoi_lines(&square, None, None).unwrap();
        assert_eq!(st_geometry_type(&l).unwrap(), "ST_MultiLineString");
        assert!(st_num_geometries(&l).unwrap() >= 4);

        // Collinear sites: PostGIS returns degenerate cells, kenro refuses
        // and names the alternative — which does work.
        let line = g("MULTIPOINT(0 0,1 1,2 2)");
        let err = st_voronoi_polygons(&line, None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("ST_VoronoiLines"), "{err}");
        assert!(st_voronoi_lines(&line, None, None).is_ok());

        // 2D out for 3D in, as PostGIS does here — and the contrast with
        // ST_DelaunayTriangles, whose vertices *are* input vertices and so
        // keeps Z, is the whole reason this pair uses a different encoder.
        // Built as WKB because ST_GeomFromText refuses 3D by design.
        let mut z = vec![0x01];
        z.extend_from_slice(&1004u32.to_le_bytes()); // MultiPoint Z
        z.extend_from_slice(&4u32.to_le_bytes());
        for xyz in [
            [0.0_f64, 0.0, 5.0],
            [4.0, 0.0, 6.0],
            [4.0, 4.0, 7.0],
            [0.0, 4.0, 8.0],
        ] {
            z.push(0x01);
            z.extend_from_slice(&1001u32.to_le_bytes()); // Point Z
            for v in xyz {
                z.extend_from_slice(&v.to_le_bytes());
            }
        }
        use crate::functions::threed::st_has_z;
        assert!(st_has_z(&z).unwrap(), "the fixture itself must be 3D");
        assert!(!st_has_z(&st_voronoi_polygons(&z, None, None).unwrap()).unwrap());
        assert!(!st_has_z(&st_voronoi_lines(&z, None, None).unwrap()).unwrap());
        assert!(st_has_z(&st_delaunay_triangles(&z).unwrap()).unwrap());
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
