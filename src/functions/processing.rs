//! Geometry processing: ST_ConvexHull, ST_PointOnSurface, ST_SimplifyVW,
//! ST_ChaikinSmoothing, ST_RemoveRepeatedPoints, ST_OrientedEnvelope.

use geo::{ConvexHull, InteriorPoint, MinimumRotatedRect, RemoveRepeatedPoints, SimplifyVw};
use geo_types::{Geometry, LineString, Point, Polygon};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

/// Encode a derived geometry, restoring Z from the inputs it came from.
/// See [`geom::encode_derived`] for why every call site has to name them.
fn encode(
    geometry: Geometry<f64>,
    srid: i32,
    func: &'static str,
    sources: &[&[u8]],
) -> Result<Vec<u8>> {
    geom::encode_derived(geometry, srid, func, sources)
}

/// Encode a derived geometry as 2D **on purpose**: either PostGIS answers in
/// 2D here too (measured), or there was no geometry input to carry a Z.
fn encode_2d(geometry: Geometry<f64>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid,
            has_zm: false,
        },
        func,
    )
}

/// Collapse a possibly-degenerate ring polygon to POINT / LINESTRING /
/// POLYGON based on its distinct vertices (used by convex hull and
/// oriented envelope, whose outputs degenerate exactly like PostGIS's).
fn collapse_ring_polygon(poly: Polygon<f64>) -> Geometry<f64> {
    let mut distinct: Vec<geo_types::Coord<f64>> = Vec::new();
    for c in poly.exterior().0.iter() {
        if !distinct.contains(c) {
            distinct.push(*c);
        }
    }
    match distinct.len() {
        0 => Geometry::Polygon(poly),
        1 => Geometry::Point(Point::from(distinct[0])),
        2 => Geometry::LineString(LineString::new(distinct)),
        _ => Geometry::Polygon(poly),
    }
}

/// `ST_ConvexHull(geom)` — degenerate hulls collapse to POINT/LINESTRING
/// like PostGIS. Empty input passes through.
pub fn st_convex_hull(bytes: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_ConvexHull";
    let geom = geom::decode_auto(bytes)?;
    if geom::is_empty(&geom.geometry) {
        return encode(geom.geometry.clone(), geom.srid, FUNC, &[bytes]);
    }
    let hull = geom.geometry.convex_hull();
    encode(collapse_ring_polygon(hull), geom.srid, FUNC, &[bytes])
}

/// `ST_PointOnSurface(geom)` — a point guaranteed on the geometry;
/// POINT EMPTY for empty input. geo's interior point need not coincide
/// with GEOS's choice (documented divergence, golden-pinned on symmetric
/// shapes).
pub fn st_point_on_surface(bytes: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_PointOnSurface";
    let geom = geom::decode_auto(bytes)?;
    let point = geom
        .geometry
        .interior_point()
        .unwrap_or_else(|| Point::new(f64::NAN, f64::NAN));
    encode_2d(Geometry::Point(point), geom.srid, FUNC)
}

/// `ST_SimplifyVW(geom, tolerance)` — Visvalingam-Whyatt; the tolerance is
/// a triangle area in CRS units, same semantics as PostGIS.
pub fn st_simplify_vw(bytes: &[u8], tolerance: f64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_SimplifyVW";
    let geom = geom::decode_auto(bytes)?;
    fn simplify(g: &Geometry<f64>, tol: f64) -> Geometry<f64> {
        match g {
            Geometry::LineString(ls) => Geometry::LineString(ls.simplify_vw(tol)),
            Geometry::MultiLineString(mls) => Geometry::MultiLineString(mls.simplify_vw(tol)),
            Geometry::Polygon(p) => Geometry::Polygon(p.simplify_vw(tol)),
            Geometry::MultiPolygon(mp) => Geometry::MultiPolygon(mp.simplify_vw(tol)),
            Geometry::Rect(r) => Geometry::Polygon(r.to_polygon().simplify_vw(tol)),
            Geometry::Triangle(t) => Geometry::Polygon(t.to_polygon().simplify_vw(tol)),
            Geometry::GeometryCollection(gc) => Geometry::GeometryCollection(
                geo_types::GeometryCollection(gc.0.iter().map(|m| simplify(m, tol)).collect()),
            ),
            other => other.clone(),
        }
    }
    encode(
        simplify(&geom.geometry, tolerance),
        geom.srid,
        FUNC,
        &[bytes],
    )
}

/// `ST_ChaikinSmoothing(geom [, nIterations])` — PostGIS caps iterations
/// at 5 (and errors above). Hand-rolled to match PostGIS's variant exactly
/// (geo's ChaikinSmoothing also subdivides the end segments of open lines;
/// PostGIS keeps them, golden-verified):
/// open lines keep their endpoints and replace each interior vertex with
/// the quarter-points of its adjacent segments; rings get the classic
/// per-edge Q/R subdivision.
pub fn st_chaikin_smoothing(bytes: &[u8], iterations: i64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_ChaikinSmoothing";
    if !(0..=5).contains(&iterations) {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!("number of iterations must be between 0 and 5, got {iterations}"),
        });
    }
    use geo_types::Coord;
    fn lerp(a: Coord<f64>, b: Coord<f64>, t: f64) -> Coord<f64> {
        Coord {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }
    fn smooth_open(ls: &LineString<f64>) -> LineString<f64> {
        let pts = &ls.0;
        if pts.len() < 3 {
            return ls.clone();
        }
        let mut out = Vec::with_capacity(2 * pts.len());
        out.push(pts[0]);
        for i in 1..pts.len() - 1 {
            out.push(lerp(pts[i], pts[i - 1], 0.25));
            out.push(lerp(pts[i], pts[i + 1], 0.25));
        }
        out.push(*pts.last().unwrap());
        LineString::new(out)
    }
    fn smooth_ring(ls: &LineString<f64>) -> LineString<f64> {
        let pts = &ls.0;
        if pts.len() < 4 {
            return ls.clone();
        }
        // Closed ring: last point duplicates the first; subdivide each edge.
        let mut out = Vec::with_capacity(2 * pts.len());
        for w in pts.windows(2) {
            out.push(lerp(w[0], w[1], 0.25));
            out.push(lerp(w[0], w[1], 0.75));
        }
        out.push(out[0]);
        LineString::new(out)
    }
    fn smooth(g: &Geometry<f64>, iterations: i64) -> Geometry<f64> {
        fn once(g: &Geometry<f64>) -> Geometry<f64> {
            match g {
                Geometry::LineString(ls) => Geometry::LineString(smooth_open(ls)),
                Geometry::Polygon(p) => Geometry::Polygon(Polygon::new(
                    smooth_ring(p.exterior()),
                    p.interiors().iter().map(smooth_ring).collect(),
                )),
                Geometry::MultiLineString(m) => Geometry::MultiLineString(
                    geo_types::MultiLineString(m.0.iter().map(smooth_open).collect()),
                ),
                Geometry::MultiPolygon(m) => Geometry::MultiPolygon(geo_types::MultiPolygon(
                    m.0.iter()
                        .map(|p| {
                            Polygon::new(
                                smooth_ring(p.exterior()),
                                p.interiors().iter().map(smooth_ring).collect(),
                            )
                        })
                        .collect(),
                )),
                Geometry::GeometryCollection(gc) => Geometry::GeometryCollection(
                    geo_types::GeometryCollection(gc.0.iter().map(once).collect()),
                ),
                other => other.clone(),
            }
        }
        let mut current = g.clone();
        for _ in 0..iterations {
            current = once(&current);
        }
        current
    }
    let geom = geom::decode_auto(bytes)?;
    let smoothed = smooth(&geom.geometry, iterations);
    encode(smoothed, geom.srid, FUNC, &[bytes])
}

/// `ST_RemoveRepeatedPoints(geom)` — exact consecutive-duplicate removal
/// (the PostGIS tolerance argument is not supported by geo, so only the
/// tolerance-0 form is registered).
pub fn st_remove_repeated_points(bytes: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_RemoveRepeatedPoints";
    let geom = geom::decode_auto(bytes)?;
    let cleaned = geom.geometry.remove_repeated_points();
    encode(cleaned, geom.srid, FUNC, &[bytes])
}

/// `ST_OrientedEnvelope(geom)` — minimum rotated rectangle; degenerates to
/// POINT/LINESTRING like PostGIS. Ties between equal-area rectangles may
/// pick a different (equally valid) answer than GEOS — golden vectors use
/// a rotation-insensitive comparison.
pub fn st_oriented_envelope(bytes: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_OrientedEnvelope";
    let geom = geom::decode_auto(bytes)?;
    if geom::is_empty(&geom.geometry) {
        return encode_2d(geom.geometry.clone(), geom.srid, FUNC);
    }
    match MinimumRotatedRect::minimum_rotated_rect(&geom.geometry) {
        Some(rect) => encode_2d(collapse_ring_polygon(rect), geom.srid, FUNC),
        None => {
            // Degenerate input (single point, collinear): fall back to the
            // axis-aligned collapse, which is exact for those cases.
            let hull = geom.geometry.convex_hull();
            encode_2d(collapse_ring_polygon(hull), geom.srid, FUNC)
        }
    }
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
    fn convex_hull_degenerate_collapse() {
        // Ring start vertex is implementation-defined; check the vertex set.
        let hull = text(&st_convex_hull(&g("MULTIPOINT(0 0,4 0,4 4,0 4,2 2)")).unwrap());
        assert!(hull.starts_with("POLYGON"), "{hull}");
        for v in ["0 0", "4 0", "4 4", "0 4"] {
            assert!(hull.contains(v), "{hull} missing {v}");
        }
        assert!(!hull.contains("2 2"), "{hull}");
        assert_eq!(
            text(&st_convex_hull(&g("MULTIPOINT(1 2,1 2)")).unwrap()),
            "POINT(1 2)"
        );
        let collinear = text(&st_convex_hull(&g("MULTIPOINT(0 0,2 2,1 1)")).unwrap());
        assert!(collinear.starts_with("LINESTRING"), "{collinear}");
    }

    #[test]
    fn point_on_surface_lands_inside() {
        let blob = st_point_on_surface(&g("POLYGON((0 0,4 0,4 4,0 4,0 0))")).unwrap();
        assert_eq!(text(&blob), "POINT(2 2)");
        let empty = st_point_on_surface(&g("POLYGON EMPTY")).unwrap();
        assert_eq!(text(&empty), "POINT EMPTY");
    }

    #[test]
    fn simplify_vw_and_chaikin_and_dedup() {
        let simplified = st_simplify_vw(&g("LINESTRING(0 0,1 0.01,2 0,3 0.01,4 0)"), 1.0).unwrap();
        assert_eq!(text(&simplified), "LINESTRING(0 0,4 0)");

        let smoothed = st_chaikin_smoothing(&g("LINESTRING(0 0,4 4,8 0)"), 1).unwrap();
        assert!(text(&smoothed).starts_with("LINESTRING(0 0,"));
        assert!(st_chaikin_smoothing(&g("LINESTRING(0 0,1 1)"), 6).is_err());

        let cleaned = st_remove_repeated_points(&g("LINESTRING(0 0,0 0,1 1,1 1,2 2)")).unwrap();
        assert_eq!(text(&cleaned), "LINESTRING(0 0,1 1,2 2)");
    }

    #[test]
    fn oriented_envelope() {
        // A rotated rectangle's oriented envelope is (approximately) itself.
        let blob = st_oriented_envelope(&g("MULTIPOINT(0 0,2 2,3 1,1 -1)")).unwrap();
        let wkt = text(&blob);
        assert!(wkt.starts_with("POLYGON"), "{wkt}");
        assert_eq!(
            text(&st_oriented_envelope(&g("POINT(3 4)")).unwrap()),
            "POINT(3 4)"
        );
        // Collinear input degenerates to a LINESTRING (fp noise tolerated).
        let line = text(&st_oriented_envelope(&g("LINESTRING(0 0,2 2)")).unwrap());
        assert!(line.starts_with("LINESTRING(0 0,"), "{line}");
        assert!(line.contains("1.999") || line.contains("2 2"), "{line}");
    }
}
