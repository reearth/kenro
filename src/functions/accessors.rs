//! Accessors and light processing: ST_Area, ST_Length, ST_Centroid,
//! ST_Envelope, ST_X, ST_Y, ST_NumPoints, ST_IsValid, ST_Simplify.
//! (ST_SRID lives with the transform functions.)

use geo::{Area, Centroid, Euclidean, Length, Simplify};
use geo_types::{Geometry, LineString, Point, Polygon, coord};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

/// `ST_Area(geom)` — planar area; 0 for non-areal or empty geometries.
pub fn st_area(bytes: &[u8]) -> Result<f64> {
    Ok(geom::decode_auto(bytes)?.geometry.unsigned_area())
}

/// `ST_Length(geom)` — 2D cartesian length of linear geometries; 0 for
/// points and polygons (as in PostGIS — polygon boundary length is
/// ST_Perimeter's job).
pub fn st_length(bytes: &[u8]) -> Result<f64> {
    fn length(g: &Geometry<f64>) -> f64 {
        match g {
            Geometry::Line(l) => Euclidean.length(l),
            Geometry::LineString(ls) => Euclidean.length(ls),
            Geometry::MultiLineString(mls) => Euclidean.length(mls),
            Geometry::GeometryCollection(gc) => gc.0.iter().map(length).sum(),
            _ => 0.0,
        }
    }
    Ok(length(&geom::decode_auto(bytes)?.geometry))
}

/// `ST_Centroid(geom)` — POINT (POINT EMPTY for empty input, via kenro's
/// NaN-point convention). SRID preserved.
pub fn st_centroid(bytes: &[u8]) -> Result<Vec<u8>> {
    let geom = geom::decode_auto(bytes)?;
    let centroid = geom
        .geometry
        .centroid()
        .unwrap_or_else(|| Point::new(f64::NAN, f64::NAN));
    geom::encode_canonical_gpb(
        &Geom {
            geometry: Geometry::Point(centroid),
            srid: geom.srid,
            has_zm: false,
        },
        "ST_Centroid",
    )
}

/// `ST_Envelope(geom)` — bounding box as POINT (degenerate to a point),
/// LINESTRING (degenerate to a segment), or POLYGON in PostGIS vertex order
/// (minx miny, minx maxy, maxx maxy, maxx miny, minx miny). SRID preserved.
pub fn st_envelope(bytes: &[u8]) -> Result<Vec<u8>> {
    let geom = geom::decode_auto(bytes)?;
    let geometry = match geom::envelope(&geom.geometry) {
        // Empty input: PostGIS returns the (empty) input unchanged,
        // preserving its type — golden-verified.
        None => geom.geometry.clone(),
        Some(e) => {
            if e.min_x == e.max_x && e.min_y == e.max_y {
                Geometry::Point(Point::new(e.min_x, e.min_y))
            } else if e.min_x == e.max_x || e.min_y == e.max_y {
                Geometry::LineString(LineString::new(vec![
                    coord! { x: e.min_x, y: e.min_y },
                    coord! { x: e.max_x, y: e.max_y },
                ]))
            } else {
                Geometry::Polygon(Polygon::new(
                    LineString::new(vec![
                        coord! { x: e.min_x, y: e.min_y },
                        coord! { x: e.min_x, y: e.max_y },
                        coord! { x: e.max_x, y: e.max_y },
                        coord! { x: e.max_x, y: e.min_y },
                        coord! { x: e.min_x, y: e.min_y },
                    ]),
                    vec![],
                ))
            }
        }
    };
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid: geom.srid,
            has_zm: false,
        },
        "ST_Envelope",
    )
}

/// `ST_X(geom)` / `ST_Y(geom)` — POINT only (PostGIS raises for other
/// types); NULL for POINT EMPTY.
pub fn st_x(bytes: &[u8]) -> Result<Option<f64>> {
    point_ordinate(bytes, "ST_X", |p| p.x())
}

pub fn st_y(bytes: &[u8]) -> Result<Option<f64>> {
    point_ordinate(bytes, "ST_Y", |p| p.y())
}

fn point_ordinate(
    bytes: &[u8],
    func: &'static str,
    pick: impl Fn(&Point<f64>) -> f64,
) -> Result<Option<f64>> {
    let geom = geom::decode_auto(bytes)?;
    let Geometry::Point(p) = &geom.geometry else {
        return Err(Error::Unsupported {
            func,
            reason: format!(
                "argument must be a POINT, got {}",
                geom::wkt_type_name(&geom.geometry)
            ),
        });
    };
    let v = pick(p);
    Ok(if v.is_nan() { None } else { Some(v) })
}

/// `ST_NumPoints(geom)` — vertex count for LINESTRING, NULL for every other
/// type (PostGIS semantics; ST_NPoints is the count-everything function and
/// remains a stub).
pub fn st_num_points(bytes: &[u8]) -> Result<Option<i64>> {
    let geom = geom::decode_auto(bytes)?;
    Ok(match geom.geometry {
        Geometry::LineString(ls) => Some(ls.0.len() as i64),
        Geometry::Line(_) => Some(2),
        _ => None,
    })
}

/// `ST_IsValid(geom)` — georust validation, close to GEOS. Documented gap:
/// interior simple-connectivity is unchecked (see README diff table).
pub fn st_is_valid(bytes: &[u8]) -> Result<bool> {
    use geo::algorithm::Validation;
    Ok(geom::decode_auto(bytes)?.geometry.is_valid())
}

/// `ST_Simplify(geom, tolerance)` — Ramer-Douglas-Peucker, matching the
/// PostGIS two-argument form (preserveCollapsed=false). Points pass through
/// unchanged; like PostGIS, the result can be invalid.
pub fn st_simplify(bytes: &[u8], tolerance: f64) -> Result<Vec<u8>> {
    let geom = geom::decode_auto(bytes)?;
    fn simplify(g: &Geometry<f64>, tol: f64) -> Geometry<f64> {
        match g {
            Geometry::LineString(ls) => Geometry::LineString(ls.simplify(tol)),
            Geometry::MultiLineString(mls) => Geometry::MultiLineString(mls.simplify(tol)),
            Geometry::Polygon(p) => Geometry::Polygon(p.simplify(tol)),
            Geometry::MultiPolygon(mp) => Geometry::MultiPolygon(mp.simplify(tol)),
            Geometry::Rect(r) => Geometry::Polygon(r.to_polygon().simplify(tol)),
            Geometry::Triangle(t) => Geometry::Polygon(t.to_polygon().simplify(tol)),
            Geometry::GeometryCollection(gc) => {
                Geometry::GeometryCollection(gc.0.iter().map(|m| simplify(m, tol)).collect())
            }
            other => other.clone(),
        }
    }
    geom::encode_canonical_gpb(
        &Geom {
            geometry: simplify(&geom.geometry, tolerance),
            srid: geom.srid,
            has_zm: false,
        },
        "ST_Simplify",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    #[test]
    fn area_and_length() {
        assert_eq!(st_area(&g("POLYGON((0 0,4 0,4 4,0 4,0 0))")).unwrap(), 16.0);
        assert_eq!(
            st_area(&g("POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,3 1,3 3,1 3,1 1))")).unwrap(),
            12.0
        );
        assert_eq!(st_area(&g("POINT(1 2)")).unwrap(), 0.0);
        assert_eq!(st_length(&g("LINESTRING(0 0,3 4)")).unwrap(), 5.0);
        assert_eq!(
            st_length(&g("POLYGON((0 0,4 0,4 4,0 4,0 0))")).unwrap(),
            0.0
        );
        assert_eq!(
            st_length(&g("GEOMETRYCOLLECTION(LINESTRING(0 0,3 4),POINT(1 1))")).unwrap(),
            5.0
        );
    }

    #[test]
    fn centroid() {
        let c = st_centroid(&g("POLYGON((0 0,4 0,4 4,0 4,0 0))")).unwrap();
        assert_eq!(st_as_text(&c).unwrap(), "POINT(2 2)");
        let e = st_centroid(&g("LINESTRING EMPTY")).unwrap();
        assert_eq!(st_as_text(&e).unwrap(), "POINT EMPTY");
    }

    #[test]
    fn envelope_shapes() {
        assert_eq!(
            st_as_text(&st_envelope(&g("LINESTRING(1 2,5 8)")).unwrap()).unwrap(),
            "POLYGON((1 2,1 8,5 8,5 2,1 2))"
        );
        assert_eq!(
            st_as_text(&st_envelope(&g("POINT(3 4)")).unwrap()).unwrap(),
            "POINT(3 4)"
        );
        assert_eq!(
            st_as_text(&st_envelope(&g("LINESTRING(1 2,5 2)")).unwrap()).unwrap(),
            "LINESTRING(1 2,5 2)"
        );
    }

    #[test]
    fn x_y_numpoints() {
        assert_eq!(st_x(&g("POINT(3 4)")).unwrap(), Some(3.0));
        assert_eq!(st_y(&g("POINT(3 4)")).unwrap(), Some(4.0));
        assert!(st_x(&g("LINESTRING(0 0,1 1)")).is_err());
        assert_eq!(
            st_num_points(&g("LINESTRING(0 0,1 1,2 2)")).unwrap(),
            Some(3)
        );
        assert_eq!(st_num_points(&g("POINT(0 0)")).unwrap(), None);
        assert_eq!(
            st_num_points(&g("POLYGON((0 0,1 0,1 1,0 1,0 0))")).unwrap(),
            None
        );
    }

    #[test]
    fn is_valid() {
        assert!(st_is_valid(&g("POLYGON((0 0,4 0,4 4,0 4,0 0))")).unwrap());
        // Bowtie: self-intersecting ring.
        assert!(!st_is_valid(&g("POLYGON((0 0,4 4,4 0,0 4,0 0))")).unwrap());
    }

    #[test]
    fn simplify() {
        let simplified = st_simplify(&g("LINESTRING(0 0,1 0.01,2 0,3 0.01,4 0)"), 0.1).unwrap();
        assert_eq!(st_as_text(&simplified).unwrap(), "LINESTRING(0 0,4 0)");
        let point = st_simplify(&g("POINT(1 2)"), 10.0).unwrap();
        assert_eq!(st_as_text(&point).unwrap(), "POINT(1 2)");
    }
}
