//! Accessors and light processing: ST_Area, ST_Length, ST_Centroid,
//! ST_Envelope, ST_X, ST_Y, ST_NumPoints, ST_IsValid, ST_Simplify.
//! (ST_SRID lives with the transform functions.)

use geo::{Area, Centroid, Euclidean, Length, Simplify};
use geo_types::{Geometry, LineString, Point, Polygon, coord};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

/// `ST_Area(geom)` — planar area; 0 for non-areal or empty geometries.
pub fn st_area(bytes: &[u8]) -> Result<f64> {
    // A surface collection is summed patch by patch, which is the planar sum
    // PostGIS reports (measured) — not a 3D surface area.
    if let Some(area) = crate::functions::surface::area(bytes)? {
        return Ok(area);
    }
    Ok(geom::decode_auto(bytes)?.geometry.unsigned_area())
}

/// `ST_NPoints(geom)` — all vertices of any type, including ring-closure
/// duplicates; 0 for empty geometries (incl. the NaN-coordinate POINT
/// EMPTY, which `coords_count` would count as one).
pub fn st_npoints(bytes: &[u8]) -> Result<i64> {
    use geo::CoordsIter;
    let geom = geom::decode_auto(bytes)?;
    if geom::is_empty(&geom.geometry) {
        return Ok(0);
    }
    Ok(geom.geometry.coords_count() as i64)
}

/// `ST_Perimeter(geom)` — boundary length of areal geometries; 0 for
/// points and linestrings (as in PostGIS — use ST_Length for those).
pub fn st_perimeter(bytes: &[u8]) -> Result<f64> {
    if let Some(p) = crate::functions::surface::perimeter(bytes)? {
        return Ok(p);
    }
    fn perimeter(g: &Geometry<f64>) -> f64 {
        fn polygon_perimeter(p: &Polygon<f64>) -> f64 {
            std::iter::once(p.exterior())
                .chain(p.interiors().iter())
                .map(|ring| Euclidean.length(ring))
                .sum()
        }
        match g {
            Geometry::Polygon(p) => polygon_perimeter(p),
            Geometry::MultiPolygon(mp) => mp.0.iter().map(polygon_perimeter).sum(),
            Geometry::Rect(r) => polygon_perimeter(&r.to_polygon()),
            Geometry::Triangle(t) => polygon_perimeter(&t.to_polygon()),
            Geometry::GeometryCollection(gc) => gc.0.iter().map(perimeter).sum(),
            _ => 0.0,
        }
    }
    Ok(perimeter(&geom::decode_auto(bytes)?.geometry))
}

/// `ST_GeometryType(geom)` — PostGIS-style prefixed type names
/// (`ST_Point`, …). GeoPackage triggers pair this with GPKG_IsAssignable,
/// which normalizes both spellings.
pub fn st_geometry_type(bytes: &[u8]) -> Result<String> {
    if let Some(kind) = geom::surface_kind(bytes) {
        return Ok(kind.type_name().to_string());
    }
    let geom = geom::decode_auto(bytes)?;
    Ok(match geom.geometry {
        Geometry::Point(_) => "ST_Point",
        Geometry::Line(_) | Geometry::LineString(_) => "ST_LineString",
        Geometry::Polygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => "ST_Polygon",
        Geometry::MultiPoint(_) => "ST_MultiPoint",
        Geometry::MultiLineString(_) => "ST_MultiLineString",
        Geometry::MultiPolygon(_) => "ST_MultiPolygon",
        Geometry::GeometryCollection(_) => "ST_GeometryCollection",
    }
    .to_string())
}

/// `ST_NumGeometries(geom)` — element count for collections, 1 for
/// non-empty single geometries (PostGIS ≥ 2.0), 0 for empty.
pub fn st_num_geometries(bytes: &[u8]) -> Result<i64> {
    // PostGIS counts a surface collection's patches as its members.
    if let Some(n) = crate::functions::surface::st_num_patches(bytes)? {
        return Ok(n);
    }
    let geom = geom::decode_auto(bytes)?;
    Ok(match &geom.geometry {
        Geometry::GeometryCollection(gc) => gc.0.len() as i64,
        Geometry::MultiPoint(m) => m.0.len() as i64,
        Geometry::MultiLineString(m) => m.0.len() as i64,
        Geometry::MultiPolygon(m) => m.0.len() as i64,
        single => {
            if geom::is_empty(single) {
                0
            } else {
                1
            }
        }
    })
}

/// `ST_GeometryN(geom, n)` — 1-based element access; NULL when out of
/// range; a non-empty single geometry is its own element 1 (PostGIS ≥ 2.0).
pub fn st_geometry_n(bytes: &[u8], n: i64) -> Result<Option<Vec<u8>>> {
    let geom = geom::decode_auto(bytes)?;
    if n < 1 {
        return Ok(None);
    }
    let idx = (n - 1) as usize;
    let element: Option<Geometry<f64>> = match &geom.geometry {
        Geometry::GeometryCollection(gc) => gc.0.get(idx).cloned(),
        Geometry::MultiPoint(m) => m.0.get(idx).map(|p| Geometry::Point(*p)),
        Geometry::MultiLineString(m) => m.0.get(idx).cloned().map(Geometry::LineString),
        Geometry::MultiPolygon(m) => m.0.get(idx).cloned().map(Geometry::Polygon),
        single => (idx == 0 && !geom::is_empty(single)).then(|| single.clone()),
    };
    element
        .map(|geometry| geom::encode_derived(geometry, geom.srid, "ST_GeometryN", &[bytes]))
        .transpose()
}

/// `ST_StartPoint(geom)` / `ST_EndPoint(geom)` — PostGIS ≥ 3.2 semantics,
/// golden-verified: points return themselves, linestrings their first/last
/// vertex, multilinestrings the first point of the first member (resp.
/// last of the last); areal and collection input → NULL.
pub fn st_start_point(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    line_endpoint(bytes, "ST_StartPoint", false)
}

pub fn st_end_point(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    line_endpoint(bytes, "ST_EndPoint", true)
}

fn line_endpoint(bytes: &[u8], func: &'static str, end: bool) -> Result<Option<Vec<u8>>> {
    let geom = geom::decode_auto(bytes)?;
    let pick = |ls: &LineString<f64>| {
        if end {
            ls.0.last().copied()
        } else {
            ls.0.first().copied()
        }
    };
    let coord = match &geom.geometry {
        Geometry::Point(p) => (!geom::is_empty(&geom.geometry)).then_some(p.0),
        Geometry::LineString(ls) => pick(ls),
        Geometry::Line(l) => Some(if end { l.end } else { l.start }),
        Geometry::MultiLineString(mls) => {
            let member = if end { mls.0.last() } else { mls.0.first() };
            member.and_then(pick)
        }
        _ => None,
    };
    coord
        .map(|c| geom::encode_derived(Geometry::Point(Point::from(c)), geom.srid, func, &[bytes]))
        .transpose()
}

/// `ST_PointN(linestring, n)` — 1-based vertex access with negative
/// indexing (-1 = last, PostGIS ≥ 2.3); NULL for non-LINESTRING input or
/// out-of-range n.
pub fn st_point_n(bytes: &[u8], n: i64) -> Result<Option<Vec<u8>>> {
    let geom = geom::decode_auto(bytes)?;
    let Geometry::LineString(ls) = &geom.geometry else {
        return Ok(None);
    };
    let len = ls.0.len() as i64;
    let resolved = if n < 0 { len + n + 1 } else { n };
    if resolved < 1 || resolved > len {
        return Ok(None);
    }
    let coord = ls.0[(resolved - 1) as usize];
    Some(geom::encode_derived(
        Geometry::Point(Point::from(coord)),
        geom.srid,
        "ST_PointN",
        &[bytes],
    ))
    .transpose()
}

/// `ST_Reverse(geom)` — reverse vertex order per component (member order
/// of multi-geometries is preserved, matching PostGIS).
pub fn st_reverse(bytes: &[u8]) -> Result<Vec<u8>> {
    fn rev_ls(ls: &LineString<f64>) -> LineString<f64> {
        LineString::new(ls.0.iter().rev().copied().collect())
    }
    fn rev_poly(p: &Polygon<f64>) -> Polygon<f64> {
        Polygon::new(
            rev_ls(p.exterior()),
            p.interiors().iter().map(rev_ls).collect(),
        )
    }
    fn rev(g: &Geometry<f64>) -> Geometry<f64> {
        match g {
            Geometry::LineString(ls) => Geometry::LineString(rev_ls(ls)),
            Geometry::Line(l) => Geometry::Line(geo_types::Line::new(l.end, l.start)),
            Geometry::Polygon(p) => Geometry::Polygon(rev_poly(p)),
            Geometry::MultiLineString(m) => Geometry::MultiLineString(geo_types::MultiLineString(
                m.0.iter().map(rev_ls).collect(),
            )),
            Geometry::MultiPolygon(m) => {
                Geometry::MultiPolygon(geo_types::MultiPolygon(m.0.iter().map(rev_poly).collect()))
            }
            Geometry::Rect(r) => Geometry::Polygon(rev_poly(&r.to_polygon())),
            Geometry::Triangle(t) => Geometry::Polygon(rev_poly(&t.to_polygon())),
            Geometry::GeometryCollection(gc) => Geometry::GeometryCollection(
                geo_types::GeometryCollection(gc.0.iter().map(rev).collect()),
            ),
            point => point.clone(),
        }
    }
    let geom = geom::decode_auto(bytes)?;
    geom::encode_derived(rev(&geom.geometry), geom.srid, "ST_Reverse", &[bytes])
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
    // 2D on purpose: PostGIS also answers 2D here — measured on 3.5,
    // ST_Centroid of a POLYGON Z is POINT(5 5), not a 3D point.
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
    // 2D on purpose: PostGIS answers 2D here (measured), and a box corner is
    // generally not an input vertex — looking one up by (x, y) could find a
    // neighbouring vertex's Z and be confidently wrong.
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
    geom::encode_derived(
        simplify(&geom.geometry, tolerance),
        geom.srid,
        "ST_Simplify",
        &[bytes],
    )
}

/// `ST_Dimension(geom)` — 0 for puntal, 1 for lineal, 2 for areal.
pub fn st_dimension(bytes: &[u8]) -> Result<i64> {
    // A surface collection is areal; PostGIS agrees (measured: 2).
    if geom::surface_kind(bytes).is_some() {
        return Ok(2);
    }
    let g = geom::decode_auto(bytes)?;
    Ok(match g.geometry {
        Geometry::Point(_) | Geometry::MultiPoint(_) => 0,
        Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_) => 1,
        Geometry::Polygon(_)
        | Geometry::MultiPolygon(_)
        | Geometry::Rect(_)
        | Geometry::Triangle(_) => 2,
        // PostGIS returns the largest member's dimension; kenro never
        // produces collections, so this only comes from foreign input.
        Geometry::GeometryCollection(ref gc) => gc
            .iter()
            .map(|g| match g {
                Geometry::Point(_) | Geometry::MultiPoint(_) => 0,
                Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_) => 1,
                _ => 2,
            })
            .max()
            .unwrap_or(0),
    })
}

/// `ST_CoordDim(geom)` / `ST_NDims(geom)` — always 2: kenro is 2D, and 3D
/// input has its Z/M dropped on decode (see `ST_Force2D`).
pub fn st_coord_dim(bytes: &[u8]) -> Result<i64> {
    geom::decode_auto(bytes)?;
    Ok(2)
}

/// `ST_IsValidReason(geom)` — `"Valid Geometry"`, or a description of the
/// first problem found.
///
/// ⚠️ The wording is geo's, not PostGIS's: PostGIS says
/// `Self-intersection[1 1]` with the offending coordinate, while geo reports
/// the ring and the kind of defect. Use it as a diagnostic, not as a string
/// to match on.
pub fn st_is_valid_reason(bytes: &[u8]) -> Result<String> {
    use geo::algorithm::Validation;
    let g = geom::decode_auto(bytes)?;
    Ok(match g.geometry.validation_errors().into_iter().next() {
        None => "Valid Geometry".to_string(),
        Some(e) => e.to_string(),
    })
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
