//! Linear referencing and distance geometry: densification, substrings, and
//! the segments that realize the minimum and maximum distance between two
//! geometries.

use geo::algorithm::line_measures::{Densify, Euclidean};
use geo_types::{Coord, Geometry, LineString, Point};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

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

/// `ST_Segmentize(geom, max_length)` — insert vertices so no segment is
/// longer than `max_length`, in the geometry's own units.
///
/// Each segment is split into equal parts, as PostGIS does: a 10-unit line
/// with a maximum of 4 becomes three 3⅓-unit segments, not 4+4+2.
pub fn st_segmentize(bytes: &[u8], max_length: f64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Segmentize";
    if max_length.is_nan() || max_length <= 0.0 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "maximum segment length must be positive".into(),
        });
    }
    let g = geom::decode_auto(bytes)?;
    let dense = match &g.geometry {
        Geometry::LineString(l) => Geometry::LineString(Euclidean.densify(l, max_length)),
        Geometry::MultiLineString(mls) => {
            Geometry::MultiLineString(Euclidean.densify(mls, max_length))
        }
        Geometry::Polygon(p) => Geometry::Polygon(Euclidean.densify(p, max_length)),
        Geometry::MultiPolygon(mp) => Geometry::MultiPolygon(Euclidean.densify(mp, max_length)),
        // Puntal input has no segments to split.
        other => other.clone(),
    };
    out(dense, g.srid, FUNC)
}

/// `ST_LineSubstring(line, from, to)` — the piece between two fractions of
/// the line's length, both in [0,1].
pub fn st_line_substring(bytes: &[u8], from: f64, to: f64) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_LineSubstring";
    if !(0.0..=1.0).contains(&from) || !(0.0..=1.0).contains(&to) || from > to {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "fractions must satisfy 0 <= from <= to <= 1".into(),
        });
    }
    let g = geom::decode_auto(bytes)?;
    let Geometry::LineString(line) = &g.geometry else {
        return Ok(None);
    };
    let total = line_length(line);
    if total == 0.0 {
        return out(g.geometry.clone(), g.srid, FUNC).map(Some);
    }
    let (start, end) = (total * from, total * to);

    let mut coords: Vec<Coord<f64>> = Vec::new();
    let mut walked = 0.0;
    for segment in line.lines() {
        let seg_len = segment.start.euclidean_distance_to(&segment.end);
        if seg_len == 0.0 {
            continue;
        }
        let (seg_start, seg_end) = (walked, walked + seg_len);
        if seg_end >= start && seg_start <= end {
            let enter = ((start - seg_start) / seg_len).clamp(0.0, 1.0);
            let leave = ((end - seg_start) / seg_len).clamp(0.0, 1.0);
            let a = lerp(segment.start, segment.end, enter);
            let b = lerp(segment.start, segment.end, leave);
            if coords.last() != Some(&a) {
                coords.push(a);
            }
            if coords.last() != Some(&b) {
                coords.push(b);
            }
        }
        walked = seg_end;
    }
    if coords.len() == 1 {
        // A zero-length request collapses to a point, as in PostGIS.
        return out(Geometry::Point(Point::from(coords[0])), g.srid, FUNC).map(Some);
    }
    out(Geometry::LineString(LineString::new(coords)), g.srid, FUNC).map(Some)
}

fn lerp(a: Coord<f64>, b: Coord<f64>, t: f64) -> Coord<f64> {
    Coord {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    }
}

trait CoordDistance {
    fn euclidean_distance_to(&self, other: &Coord<f64>) -> f64;
}

impl CoordDistance for Coord<f64> {
    fn euclidean_distance_to(&self, other: &Coord<f64>) -> f64 {
        ((other.x - self.x).powi(2) + (other.y - self.y).powi(2)).sqrt()
    }
}

fn line_length(line: &LineString<f64>) -> f64 {
    line.lines()
        .map(|l| l.start.euclidean_distance_to(&l.end))
        .sum()
}

/// `ST_ShortestLine(a, b)` — the two-point line realizing the minimum
/// distance.
///
/// The endpoints are searched vertex-against-segment in both directions,
/// which is exact whenever the geometries are disjoint. When they intersect
/// the distance is zero and PostGIS may pick a different (equally valid)
/// zero-length line.
pub fn st_shortest_line(a: &[u8], b: &[u8]) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_ShortestLine";
    let (ga, gb) = pair(FUNC, a, b)?;
    let Some((from, to)) = closest_pair(&ga.geometry, &gb.geometry) else {
        return Ok(None);
    };
    out(
        Geometry::LineString(LineString::new(vec![from, to])),
        srid_of(&ga, &gb),
        FUNC,
    )
    .map(Some)
}

/// `ST_LongestLine(a, b)` — the two-point line realizing the maximum
/// distance, which is always attained at a pair of vertices.
pub fn st_longest_line(a: &[u8], b: &[u8]) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_LongestLine";
    let (ga, gb) = pair(FUNC, a, b)?;
    let Some((from, to)) = farthest_pair(&ga.geometry, &gb.geometry) else {
        return Ok(None);
    };
    out(
        Geometry::LineString(LineString::new(vec![from, to])),
        srid_of(&ga, &gb),
        FUNC,
    )
    .map(Some)
}

/// `ST_MaxDistance(a, b)` — the length of [`st_longest_line`].
pub fn st_max_distance(a: &[u8], b: &[u8]) -> Result<Option<f64>> {
    let (ga, gb) = pair("ST_MaxDistance", a, b)?;
    Ok(farthest_pair(&ga.geometry, &gb.geometry).map(|(from, to)| from.euclidean_distance_to(&to)))
}

fn pair(func: &'static str, a: &[u8], b: &[u8]) -> Result<(Geom, Geom)> {
    let (ga, gb) = (geom::decode_auto(a)?, geom::decode_auto(b)?);
    if ga.srid > 0 && gb.srid > 0 && ga.srid != gb.srid {
        return Err(Error::MixedSrid {
            func,
            a: ga.srid,
            b: gb.srid,
        });
    }
    Ok((ga, gb))
}

fn srid_of(a: &Geom, b: &Geom) -> i32 {
    if a.srid > 0 { a.srid } else { b.srid }
}

fn vertices(g: &Geometry<f64>) -> Vec<Coord<f64>> {
    use geo::algorithm::CoordsIter;
    g.coords_iter().collect()
}

/// Every segment of a geometry, for vertex-against-segment search.
fn segments(g: &Geometry<f64>) -> Vec<(Coord<f64>, Coord<f64>)> {
    let mut out = Vec::new();
    let mut push_line = |l: &LineString<f64>| {
        for seg in l.lines() {
            out.push((seg.start, seg.end));
        }
    };
    match g {
        Geometry::LineString(l) => push_line(l),
        Geometry::MultiLineString(mls) => mls.iter().for_each(&mut push_line),
        Geometry::Polygon(p) => {
            push_line(p.exterior());
            p.interiors().iter().for_each(&mut push_line);
        }
        Geometry::MultiPolygon(mp) => {
            for p in mp {
                push_line(p.exterior());
                p.interiors().iter().for_each(&mut push_line);
            }
        }
        _ => {}
    }
    out
}

fn closest_pair(a: &Geometry<f64>, b: &Geometry<f64>) -> Option<(Coord<f64>, Coord<f64>)> {
    let (va, vb) = (vertices(a), vertices(b));
    if va.is_empty() || vb.is_empty() {
        return None;
    }
    let mut best: Option<(f64, Coord<f64>, Coord<f64>)> = None;
    let mut consider = |p: Coord<f64>, q: Coord<f64>| {
        let d = p.euclidean_distance_to(&q);
        if best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
            best = Some((d, p, q));
        }
    };
    for p in &va {
        for q in &vb {
            consider(*p, *q);
        }
        for (s, e) in segments(b) {
            consider(*p, project_onto(*p, s, e));
        }
    }
    for q in &vb {
        for (s, e) in segments(a) {
            consider(project_onto(*q, s, e), *q);
        }
    }
    best.map(|(_, p, q)| (p, q))
}

fn farthest_pair(a: &Geometry<f64>, b: &Geometry<f64>) -> Option<(Coord<f64>, Coord<f64>)> {
    let (va, vb) = (vertices(a), vertices(b));
    let mut best: Option<(f64, Coord<f64>, Coord<f64>)> = None;
    for p in &va {
        for q in &vb {
            let d = p.euclidean_distance_to(q);
            if best.as_ref().is_none_or(|(bd, _, _)| d > *bd) {
                best = Some((d, *p, *q));
            }
        }
    }
    best.map(|(_, p, q)| (p, q))
}

/// The point on segment `s..e` closest to `p`.
fn project_onto(p: Coord<f64>, s: Coord<f64>, e: Coord<f64>) -> Coord<f64> {
    let (dx, dy) = (e.x - s.x, e.y - s.y);
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return s;
    }
    let t = (((p.x - s.x) * dx + (p.y - s.y) * dy) / len_sq).clamp(0.0, 1.0);
    Coord {
        x: s.x + dx * t,
        y: s.y + dy * t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }
    fn wkt(b: &[u8]) -> String {
        st_as_text(b).unwrap()
    }

    #[test]
    fn segmentize_splits_into_equal_parts_like_postgis() {
        // PostGIS 3.5: LINESTRING(0 0,3.333333333333334 0,6.666666666666667 0,10 0)
        let out = st_segmentize(&g("LINESTRING(0 0,10 0)"), 4.0).unwrap();
        let text = wkt(&out);
        assert!(text.starts_with("LINESTRING(0 0,3.33"), "{text}");
        assert_eq!(text.matches(',').count(), 3);
        assert!(st_segmentize(&g("LINESTRING(0 0,10 0)"), 0.0).is_err());
    }

    #[test]
    fn line_substring_matches_postgis() {
        // PostGIS 3.5: ST_LineSubstring(LINESTRING(0 0,10 0), 0.3, 0.7) = LINESTRING(3 0,7 0)
        assert_eq!(
            wkt(&st_line_substring(&g("LINESTRING(0 0,10 0)"), 0.3, 0.7)
                .unwrap()
                .unwrap()),
            "LINESTRING(3 0,7 0)"
        );
        assert!(st_line_substring(&g("LINESTRING(0 0,10 0)"), 0.7, 0.3).is_err());
        assert!(
            st_line_substring(&g("POINT(0 0)"), 0.0, 1.0)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn shortest_and_longest_lines_match_postgis() {
        let p = g("POINT(0 0)");
        let l = g("LINESTRING(2 -1,2 1)");
        // PostGIS 3.5: ST_ShortestLine → LINESTRING(0 0,2 0)
        assert_eq!(
            wkt(&st_shortest_line(&p, &l).unwrap().unwrap()),
            "LINESTRING(0 0,2 0)"
        );
        // PostGIS 3.5: ST_MaxDistance → 2.23606797749979
        let d = st_max_distance(&p, &l).unwrap().unwrap();
        assert!((d - 2.236_067_977_499_79).abs() < 1e-12, "{d}");
        let longest = wkt(&st_longest_line(&p, &l).unwrap().unwrap());
        assert!(longest.starts_with("LINESTRING(0 0,2 "), "{longest}");
    }
}
