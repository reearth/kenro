//! The tail of the PostGIS surface: alternative spellings, small
//! constructors, geohash decoding, and a handful of one-off computations.
//!
//! Nothing here needed a new dependency or a new idea — these are the
//! functions that were simply never written, collected in one place.
//! Conventions verified against a live PostGIS 3.5.

use geo_types::{Coord, Geometry, LineString, MultiLineString, Point, Polygon};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

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

/// Encode a derived geometry as 2D **on purpose**: PostGIS answers in 2D here
/// too (measured), so there is no Z to preserve and nothing to refuse.
fn out_2d(geometry: Geometry<f64>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid,
            has_zm: false,
        },
        func,
    )
}

/// `ST_Polygon(linestring, srid)` — a polygon from a closed shell, labelled
/// with the SRID. (`ST_MakePolygon` is the same thing without the SRID.)
pub fn st_polygon(bytes: &[u8], srid: i32) -> Result<Vec<u8>> {
    let poly = crate::functions::edit::st_make_polygon(bytes)?;
    crate::functions::io::st_set_srid(&poly, srid)
}

/// `ST_LineFromMultiPoint(multipoint)` — the points in order, as a line.
/// NULL for any other type, as in PostGIS.
pub fn st_line_from_multipoint(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let g = geom::decode_auto(bytes)?;
    let Geometry::MultiPoint(mp) = &g.geometry else {
        return Ok(None);
    };
    out(
        Geometry::LineString(LineString::new(mp.iter().map(|p| p.0).collect())),
        g.srid,
        "ST_LineFromMultiPoint",
        &[bytes],
    )
    .map(Some)
}

/// `ST_LineExtend(line, forward [, backward])` — extend the line along its
/// end segments' directions. The new vertices are prepended and appended, so
/// the original ones survive (PostGIS behavior, verified live).
pub fn st_line_extend(bytes: &[u8], forward: f64, backward: f64) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_LineExtend";
    if forward < 0.0 || backward < 0.0 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "distances cannot be negative".into(),
        });
    }
    let g = geom::decode_auto(bytes)?;
    let Geometry::LineString(line) = &g.geometry else {
        return Ok(None);
    };
    let mut coords = line.0.clone();
    if coords.len() < 2 {
        return out(g.geometry.clone(), g.srid, FUNC, &[bytes]).map(Some);
    }
    if backward > 0.0 {
        let (a, b) = (coords[1], coords[0]);
        if let Some(p) = extend_from(b, a, backward) {
            coords.insert(0, p);
        }
    }
    if forward > 0.0 {
        let n = coords.len();
        let (a, b) = (coords[n - 2], coords[n - 1]);
        if let Some(p) = extend_from(b, a, forward) {
            coords.push(p);
        }
    }
    out(
        Geometry::LineString(LineString::new(coords)),
        g.srid,
        FUNC,
        &[bytes],
    )
    .map(Some)
}

/// A point `distance` beyond `tip`, continuing away from `from`.
fn extend_from(tip: Coord<f64>, from: Coord<f64>, distance: f64) -> Option<Coord<f64>> {
    let (dx, dy) = (tip.x - from.x, tip.y - from.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return None;
    }
    Some(Coord {
        x: tip.x + dx / len * distance,
        y: tip.y + dy / len * distance,
    })
}

/// `ST_PointInsideCircle(point, cx, cy, radius)` — planar containment.
pub fn st_point_inside_circle(bytes: &[u8], cx: f64, cy: f64, radius: f64) -> Result<bool> {
    let g = geom::decode_auto(bytes)?;
    let Geometry::Point(p) = g.geometry else {
        return Err(Error::Unsupported {
            func: "ST_PointInsideCircle",
            reason: "the first argument must be a POINT".into(),
        });
    };
    Ok(((p.x() - cx).powi(2) + (p.y() - cy).powi(2)).sqrt() <= radius)
}

/// `ST_WrapX(geom, wrap, move)` — shift the parts on one side of the `wrap`
/// meridian by `move`, splitting anything that crosses it.
///
/// ⚠️ kenro splits **at vertices only**: a segment that spans the meridian is
/// assigned to the side its first vertex is on, where PostGIS cuts the
/// segment. Densify first (`ST_Segmentize`) if the difference matters.
///
/// Because kenro's version *is* coordinate-wise, it goes through `coords` and
/// so keeps Z and M (measured on PostGIS 3.5:
/// `ST_WrapX(POINT Z (-170 2 3), 0, 360)` is `POINT(190 2 3)`). Surface
/// collections are refused rather than wrapped, which is PostGIS's answer too
/// — "Wrapping of PolyhedralSurface geometries is unsupported".
pub fn st_wrap_x(bytes: &[u8], wrap: f64, amount: f64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_WrapX";
    if let Some(kind) = geom::surface_kind(bytes) {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!("wrapping a {} is unsupported, as in PostGIS", kind.name()),
        });
    }
    crate::coords::map_coords(bytes, &mut |p| {
        if (amount > 0.0 && p.x < wrap) || (amount < 0.0 && p.x > wrap) {
            p.x += amount;
        }
    })
}

/// `ST_MakeBox2D(low, high)` — the rectangle between two corner points.
///
/// ⚠️ returns a POLYGON: PostGIS returns its `box2d` type, which SQLite has
/// no equivalent for (the same divergence as `ST_Extent` and `ST_Expand`).
pub fn st_make_box_2d(low: &[u8], high: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_MakeBox2D";
    let (a, b) = (geom::decode_auto(low)?, geom::decode_auto(high)?);
    let (Geometry::Point(p1), Geometry::Point(p2)) = (&a.geometry, &b.geometry) else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "both arguments must be POINTs".into(),
        });
    };
    let srid = if a.srid > 0 { a.srid } else { b.srid };
    out_2d(rect(p1.x(), p1.y(), p2.x(), p2.y()), srid, FUNC)
}

fn rect(minx: f64, miny: f64, maxx: f64, maxy: f64) -> Geometry<f64> {
    let (minx, maxx) = if minx <= maxx {
        (minx, maxx)
    } else {
        (maxx, minx)
    };
    let (miny, maxy) = if miny <= maxy {
        (miny, maxy)
    } else {
        (maxy, miny)
    };
    Geometry::Polygon(Polygon::new(
        LineString::new(vec![
            Coord { x: minx, y: miny },
            Coord { x: minx, y: maxy },
            Coord { x: maxx, y: maxy },
            Coord { x: maxx, y: miny },
            Coord { x: minx, y: miny },
        ]),
        vec![],
    ))
}

/// `ST_GeomFromGeoHash(hash [, precision])` — the cell a geohash names, as a
/// POLYGON. `ST_Box2dFromGeoHash` is the same function in PostGIS.
pub fn st_geom_from_geohash(hash: &str, precision: Option<i64>) -> Result<Vec<u8>> {
    let (minx, miny, maxx, maxy) = decode_geohash(hash, precision)?;
    out_2d(rect(minx, miny, maxx, maxy), 4326, "ST_GeomFromGeoHash")
}

/// `ST_PointFromGeoHash(hash [, precision])` — the centre of that cell.
pub fn st_point_from_geohash(hash: &str, precision: Option<i64>) -> Result<Vec<u8>> {
    let (minx, miny, maxx, maxy) = decode_geohash(hash, precision)?;
    out_2d(
        Geometry::Point(Point::new((minx + maxx) / 2.0, (miny + maxy) / 2.0)),
        4326,
        "ST_PointFromGeoHash",
    )
}

/// The lon/lat bounds a geohash prefix denotes.
fn decode_geohash(hash: &str, precision: Option<i64>) -> Result<(f64, f64, f64, f64)> {
    const FUNC: &str = "ST_GeomFromGeoHash";
    const BASE32: &[u8] = b"0123456789bcdefghjkmnpqrstuvwxyz";
    let take = match precision {
        Some(n) if n < 1 => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "precision must be positive".into(),
            });
        }
        Some(n) => (n as usize).min(hash.len()),
        None => hash.len(),
    };
    if hash.is_empty() {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "geohash is empty".into(),
        });
    }
    let (mut lon, mut lat) = ((-180.0f64, 180.0f64), (-90.0f64, 90.0f64));
    let mut even = true;
    for ch in hash[..take].bytes() {
        let Some(value) = BASE32.iter().position(|c| *c == ch.to_ascii_lowercase()) else {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: format!("{:?} is not a base32 geohash character", ch as char),
            });
        };
        for bit in (0..5).rev() {
            let set = value & (1 << bit) != 0;
            let range = if even { &mut lon } else { &mut lat };
            let mid = (range.0 + range.1) / 2.0;
            if set {
                range.0 = mid;
            } else {
                range.1 = mid;
            }
            even = !even;
        }
    }
    Ok((lon.0, lat.0, lon.1, lat.1))
}

/// `ST_GeometricMedian(geom [, tolerance])` — the point minimizing the sum
/// of distances to every input vertex (Weiszfeld's algorithm).
pub fn st_geometric_median(bytes: &[u8], tolerance: Option<f64>) -> Result<Option<Vec<u8>>> {
    use geo::algorithm::CoordsIter;
    const FUNC: &str = "ST_GeometricMedian";
    let g = geom::decode_auto(bytes)?;
    let points: Vec<Coord<f64>> = g.geometry.coords_iter().collect();
    if points.is_empty() {
        return Ok(None);
    }
    let tol = tolerance.unwrap_or(1e-8).max(1e-12);
    // Start at the centroid, then iterate the weighted average of directions.
    let mut current = Coord {
        x: points.iter().map(|p| p.x).sum::<f64>() / points.len() as f64,
        y: points.iter().map(|p| p.y).sum::<f64>() / points.len() as f64,
    };
    for _ in 0..1000 {
        let (mut nx, mut ny, mut weight) = (0.0, 0.0, 0.0);
        let mut on_vertex = false;
        for p in &points {
            let d = ((p.x - current.x).powi(2) + (p.y - current.y).powi(2)).sqrt();
            if d < tol {
                on_vertex = true;
                continue;
            }
            nx += p.x / d;
            ny += p.y / d;
            weight += 1.0 / d;
        }
        if weight == 0.0 || on_vertex && weight == 0.0 {
            break;
        }
        let next = Coord {
            x: nx / weight,
            y: ny / weight,
        };
        let moved = ((next.x - current.x).powi(2) + (next.y - current.y).powi(2)).sqrt();
        current = next;
        if moved < tol {
            break;
        }
    }
    out(
        Geometry::Point(Point::from(current)),
        g.srid,
        FUNC,
        &[bytes],
    )
    .map(Some)
}

/// `ST_LineCrossingDirection(a, b)` — how line `b` crosses line `a`, in
/// PostGIS's codes: 0 none, 1 left, -1 right, 2 multiple left, -2 multiple
/// right, 3 multiple crossings ending left, -3 ending right.
pub fn st_line_crossing_direction(a: &[u8], b: &[u8]) -> Result<i64> {
    const FUNC: &str = "ST_LineCrossingDirection";
    let (ga, gb) = (geom::decode_auto(a)?, geom::decode_auto(b)?);
    if ga.srid > 0 && gb.srid > 0 && ga.srid != gb.srid {
        return Err(Error::MixedSrid {
            func: FUNC,
            a: ga.srid,
            b: gb.srid,
        });
    }
    let (Geometry::LineString(la), Geometry::LineString(lb)) = (&ga.geometry, &gb.geometry) else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "both arguments must be LINESTRINGs".into(),
        });
    };
    let (mut left, mut right, mut last) = (0i64, 0i64, 0i64);
    for sa in la.lines() {
        for sb in lb.lines() {
            let Some(side) = crossing_side(sa, sb) else {
                continue;
            };
            if side > 0 {
                left += 1;
            } else {
                right += 1;
            }
            last = side;
        }
    }
    let total = left + right;
    Ok(match (total, last) {
        (0, _) => 0,
        (1, s) => s,
        (_, s) if left > 0 && right > 0 => 3 * s,
        (_, s) => 2 * s,
    })
}

/// +1 when `b` crosses `a` left to right, -1 right to left, `None` when the
/// segments do not properly cross.
fn crossing_side(a: geo_types::Line<f64>, b: geo_types::Line<f64>) -> Option<i64> {
    let side = |p: Coord<f64>| {
        (a.end.x - a.start.x) * (p.y - a.start.y) - (a.end.y - a.start.y) * (p.x - a.start.x)
    };
    let (s1, s2) = (side(b.start), side(b.end));
    let cross = |l: geo_types::Line<f64>, p: Coord<f64>| {
        (l.end.x - l.start.x) * (p.y - l.start.y) - (l.end.y - l.start.y) * (p.x - l.start.x)
    };
    let (t1, t2) = (cross(b, a.start), cross(b, a.end));
    if s1 * s2 < 0.0 && t1 * t2 < 0.0 {
        Some(if s1 > 0.0 { 1 } else { -1 })
    } else {
        None
    }
}

/// `ST_Summary(geom)` — a one-line description: type, flags and vertex count.
///
/// ⚠️ PostGIS prints a tree with byte offsets; kenro prints the same leading
/// token (`Point[S]` — S for "has SRID") plus its own vertex count, which is
/// the part that is actually portable.
pub fn st_summary(bytes: &[u8]) -> Result<String> {
    use geo::algorithm::CoordsIter;
    let g = geom::decode_auto(bytes)?;
    let mut flags = String::new();
    if g.srid > 0 {
        flags.push('S');
    }
    if g.has_zm {
        flags.push('Z');
    }
    let name = geom::wkt_type_name(&g.geometry);
    let pretty = name
        .split('_')
        .next_back()
        .unwrap_or(name)
        .to_ascii_lowercase();
    let mut chars = pretty.chars();
    let cased = match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => pretty.clone(),
    };
    Ok(format!(
        "{cased}[{flags}] with {} vertices",
        g.geometry.coords_count()
    ))
}

/// `ST_MemSize(geom)` — the size of the stored value in bytes.
///
/// ⚠️ PostGIS reports its own in-memory representation; kenro reports the
/// length of the GeoPackage blob it would write, which is the number that
/// means something for a SQLite column.
pub fn st_mem_size(bytes: &[u8]) -> Result<i64> {
    let g = geom::decode_auto(bytes)?;
    Ok(geom::encode_storage_gpb(&g, "ST_MemSize")?.len() as i64)
}

/// `ST_Normalize(geom)` — a canonical form: rings oriented (exterior
/// clockwise, interiors counter-clockwise) and multi-parts ordered.
///
/// PostGIS's ordering is by its own internal comparison; kenro orders by
/// bounding box, which is stable and reproducible but not byte-identical to
/// PostGIS for every input.
pub fn st_normalize(bytes: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Normalize";
    let g = geom::decode_auto(bytes)?;
    let oriented = crate::functions::edit::st_force_polygon_cw(bytes)?;
    let mut og = geom::decode_auto(&oriented)?;
    let key = |g: &Geometry<f64>| {
        geom::envelope(g)
            .map(|e| (e.min_x, e.min_y, e.max_x, e.max_y))
            .unwrap_or((f64::MAX, f64::MAX, f64::MAX, f64::MAX))
    };
    let cmp = |a: &(f64, f64, f64, f64), b: &(f64, f64, f64, f64)| {
        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
    };
    match &mut og.geometry {
        Geometry::MultiPolygon(mp) => mp.0.sort_by(|a, b| {
            cmp(
                &key(&Geometry::Polygon(a.clone())),
                &key(&Geometry::Polygon(b.clone())),
            )
        }),
        Geometry::MultiLineString(mls) => mls.0.sort_by(|a, b| {
            cmp(
                &key(&Geometry::LineString(a.clone())),
                &key(&Geometry::LineString(b.clone())),
            )
        }),
        Geometry::MultiPoint(mp) => {
            mp.0.sort_by(|a, b| cmp(&(a.x(), a.y(), a.x(), a.y()), &(b.x(), b.y(), b.x(), b.y())))
        }
        _ => {}
    }
    let _ = MultiLineString::<f64>::new(vec![]); // keep the import honest
    out(og.geometry, g.srid, FUNC, &[bytes])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text, st_srid};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }
    fn wkt(b: &[u8]) -> String {
        st_as_text(b).unwrap()
    }

    #[test]
    fn small_constructors_match_postgis() {
        // PostGIS 3.5, one by one.
        assert_eq!(
            wkt(&st_polygon(&g("LINESTRING(0 0,1 0,1 1,0 0)"), 4326).unwrap()),
            "POLYGON((0 0,1 0,1 1,0 0))"
        );
        assert_eq!(
            st_srid(&st_polygon(&g("LINESTRING(0 0,1 0,1 1,0 0)"), 4326).unwrap()).unwrap(),
            4326
        );
        assert_eq!(
            wkt(
                &st_line_from_multipoint(&g("MULTIPOINT((0 0),(1 1),(2 2))"))
                    .unwrap()
                    .unwrap()
            ),
            "LINESTRING(0 0,1 1,2 2)"
        );
        assert!(st_line_from_multipoint(&g("POINT(0 0)")).unwrap().is_none());
        // PostGIS: LINESTRING(-0.5 0,0 0,1 0,2 0)
        assert_eq!(
            wkt(&st_line_extend(&g("LINESTRING(0 0,1 0)"), 1.0, 0.5)
                .unwrap()
                .unwrap()),
            "LINESTRING(-0.5 0,0 0,1 0,2 0)"
        );
        // PostGIS: POLYGON((0 0,0 4,3 4,3 0,0 0))
        assert_eq!(
            wkt(&st_make_box_2d(&g("POINT(0 0)"), &g("POINT(3 4)")).unwrap()),
            "POLYGON((0 0,0 4,3 4,3 0,0 0))"
        );
    }

    #[test]
    fn geohash_decodes_to_postgis_cells() {
        // PostGIS: ST_PointFromGeoHash('xn76f') → POINT(139.68017578125 35.66162109375)
        assert_eq!(
            wkt(&st_point_from_geohash("xn76f", None).unwrap()),
            "POINT(139.68017578125 35.66162109375)"
        );
        // …and the cell it centres, whose corners PostGIS prints as
        // 139.658203125 / 35.6396484375 → 139.7021484375 / 35.68359375.
        let cell = wkt(&st_geom_from_geohash("xn76f", None).unwrap());
        assert!(
            cell.starts_with("POLYGON((139.658203125 35.6396484375"),
            "{cell}"
        );
        // Round-trips against kenro's own encoder.
        let hash = crate::functions::extra::st_geohash(
            &st_point_from_geohash("xn76f", None).unwrap(),
            Some(5),
        )
        .unwrap();
        assert_eq!(hash.as_deref(), Some("xn76f"));
        assert!(st_geom_from_geohash("not-base32!", None).is_err());
    }

    #[test]
    fn wrap_x_moves_one_side_of_the_meridian() {
        // kenro splits at vertices, so a vertex at -170 moves to 190.
        assert_eq!(
            wkt(&st_wrap_x(&g("LINESTRING(-170 0,170 0)"), 0.0, 360.0).unwrap()),
            "LINESTRING(190 0,170 0)"
        );
    }

    #[test]
    fn point_inside_circle_and_geometric_median() {
        assert!(st_point_inside_circle(&g("POINT(1 1)"), 0.0, 0.0, 2.0).unwrap());
        assert!(!st_point_inside_circle(&g("POINT(3 3)"), 0.0, 0.0, 2.0).unwrap());
        // PostGIS: the median of a square's corners is its centre.
        let median = st_geometric_median(&g("MULTIPOINT((0 0),(4 0),(0 4),(4 4))"), None)
            .unwrap()
            .unwrap();
        let (x, y) = (
            crate::functions::accessors::st_x(&median).unwrap().unwrap(),
            crate::functions::accessors::st_y(&median).unwrap().unwrap(),
        );
        assert!((x - 2.0).abs() < 1e-6 && (y - 2.0).abs() < 1e-6, "{x} {y}");
    }

    #[test]
    fn line_crossing_direction_uses_postgis_codes() {
        // PostGIS: 1 for a single left-to-right crossing.
        assert_eq!(
            st_line_crossing_direction(&g("LINESTRING(0 0,2 2)"), &g("LINESTRING(0 2,2 0)"))
                .unwrap(),
            1
        );
        // Parallel lines never cross.
        assert_eq!(
            st_line_crossing_direction(&g("LINESTRING(0 0,2 0)"), &g("LINESTRING(0 1,2 1)"))
                .unwrap(),
            0
        );
    }

    #[test]
    fn summary_and_mem_size_report_kenros_own_view() {
        // PostGIS prints "Point[S]" first; kenro keeps that token.
        let s = st_summary(&st_geom_from_text("POINT(1 2)", Some(4326)).unwrap()).unwrap();
        assert!(s.starts_with("Point[S]"), "{s}");
        assert!(s.contains("1 vertices"), "{s}");
        // The stored blob's length, not PostGIS's in-memory size.
        assert!(st_mem_size(&g("POINT(1 2)")).unwrap() > 0);
    }

    #[test]
    fn normalize_orients_rings_and_orders_parts() {
        let normalized = st_normalize(&g("POLYGON((0 0,0 2,2 2,2 0,0 0))")).unwrap();
        assert!(crate::functions::edit::st_is_polygon_cw(&normalized).unwrap());
        // Parts come out in a stable order regardless of input order.
        let a = st_normalize(&g("MULTIPOINT((5 5),(1 1))")).unwrap();
        let b = st_normalize(&g("MULTIPOINT((1 1),(5 5))")).unwrap();
        assert_eq!(wkt(&a), wkt(&b));
    }
}
