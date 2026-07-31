//! Dimension classification and geometry-shape helpers shared by the
//! overlay family and the MVT pipeline: operand classes (puntal / lineal /
//! areal), conversions to homogeneous multi-geometries, and the
//! single-member/empty normalization rules (golden-verified against
//! PostGIS's typed empties).

use geo_types::{Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

use crate::error::{Error, Result};
use crate::geom;

/// Dimension class of an operand.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Class {
    Puntal,
    Lineal,
    Areal,
}

pub(crate) fn classify(func: &'static str, g: &Geometry<f64>) -> Result<Class> {
    Ok(match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => Class::Puntal,
        Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_) => Class::Lineal,
        Geometry::Polygon(_)
        | Geometry::MultiPolygon(_)
        | Geometry::Rect(_)
        | Geometry::Triangle(_) => Class::Areal,
        Geometry::GeometryCollection(_) => {
            return Err(Error::Unsupported {
                func,
                reason: "GeometryCollection operands are not supported".into(),
            });
        }
    })
}

/// Reject NaN/Inf coordinates before anything reaches i_overlay (whose
/// robustness contract does not cover non-finite input; on wasm a panic
/// would abort the instance).
pub(crate) fn ensure_finite(func: &'static str, g: &Geometry<f64>) -> Result<()> {
    use geo::CoordsIter;
    if geom::is_empty(g) {
        return Ok(());
    }
    if g.coords_iter()
        .any(|c| !c.x.is_finite() || !c.y.is_finite())
    {
        return Err(Error::Unsupported {
            func,
            reason: "geometry contains non-finite (NaN/Inf) coordinates".into(),
        });
    }
    Ok(())
}

pub(crate) fn points_of(g: &Geometry<f64>) -> Vec<Point<f64>> {
    match g {
        Geometry::Point(p) => vec![*p],
        Geometry::MultiPoint(mp) => mp.0.clone(),
        _ => vec![],
    }
}

pub(crate) fn to_multi_polygon(g: &Geometry<f64>) -> MultiPolygon<f64> {
    match g {
        Geometry::Polygon(p) => MultiPolygon(vec![p.clone()]),
        Geometry::MultiPolygon(mp) => mp.clone(),
        Geometry::Rect(r) => MultiPolygon(vec![r.to_polygon()]),
        Geometry::Triangle(t) => MultiPolygon(vec![t.to_polygon()]),
        _ => MultiPolygon(vec![]),
    }
}

pub(crate) fn to_multi_line(g: &Geometry<f64>) -> MultiLineString<f64> {
    match g {
        Geometry::LineString(ls) => MultiLineString(vec![ls.clone()]),
        Geometry::MultiLineString(mls) => mls.clone(),
        Geometry::Line(l) => MultiLineString(vec![LineString::new(vec![l.start, l.end])]),
        _ => MultiLineString(vec![]),
    }
}

/// Unwrap single-member multi geometries to their singular type and map
/// empty results to the PostGIS-typed empties (golden-verified).
pub(crate) fn normalize_points(points: Vec<Point<f64>>) -> Geometry<f64> {
    match points.len() {
        0 => Geometry::Point(Point::new(f64::NAN, f64::NAN)), // POINT EMPTY
        1 => Geometry::Point(points[0]),
        _ => Geometry::MultiPoint(MultiPoint(points)),
    }
}

pub(crate) fn normalize_lines(lines: MultiLineString<f64>) -> Geometry<f64> {
    let mut non_empty: Vec<LineString<f64>> =
        lines.0.into_iter().filter(|ls| !ls.0.is_empty()).collect();
    match non_empty.len() {
        0 => Geometry::LineString(LineString::new(vec![])), // LINESTRING EMPTY
        1 => Geometry::LineString(non_empty.remove(0)),
        _ => Geometry::MultiLineString(MultiLineString(non_empty)),
    }
}

pub(crate) fn normalize_polygons(polys: MultiPolygon<f64>) -> Geometry<f64> {
    let mut non_empty: Vec<Polygon<f64>> = polys
        .0
        .into_iter()
        .filter(|p| !p.exterior().0.is_empty())
        .collect();
    match non_empty.len() {
        0 => Geometry::Polygon(Polygon::new(LineString::new(vec![]), vec![])), // POLYGON EMPTY
        1 => Geometry::Polygon(non_empty.remove(0)),
        _ => Geometry::MultiPolygon(MultiPolygon(non_empty)),
    }
}
