//! Line-level structure: simplicity, merging and splitting.
//!
//! These three sat in "out of scope" for the same stated reason — that they
//! need a noding engine kenro does not carry. Re-checking that claim against
//! the crates already in the tree, two thirds of it turned out to be wrong:
//!
//! - **`ST_IsSimple`** was ruled out because `geo`'s `Validation` has no
//!   self-intersection variant for `LineString` (it has one for `Polygon`,
//!   which is how `ST_IsRing` works). True, but `geo::sweep::Intersections`
//!   enumerates every intersecting segment pair in O(n log n) — the same
//!   sweep the validator uses, just reached directly.
//! - **`ST_LineMerge`** needs no noding at all. GEOS's `LineMerger` doesn't
//!   node either: it joins chains at nodes of degree exactly 2 and leaves
//!   everything else alone. Two lines that cross in their interiors come back
//!   unmerged from PostGIS too — measured, and asserted below.
//! - **`ST_Split`** does need the segment arithmetic, but for areal input
//!   `i_overlay` already has it (`slice_by`), and for lineal input splitting
//!   at known points is ordinary vertex work.
//!
//! `ST_Node` — noding an arbitrary line soup against itself — remains out:
//! that is the piece none of these three actually required.
//!
//! **Return type.** PostGIS returns a GEOMETRYCOLLECTION from `ST_Split` and
//! from `ST_LineMerge`'s failure cases. kenro never produces one, and here it
//! doesn't have to: splitting cannot change dimension, so the pieces of a
//! polygon are polygons and the pieces of a line are lines. Both come back as
//! a MULTI\*, the same accommodation `ST_Subdivide` already makes.

use geo::algorithm::line_intersection::{LineIntersection, line_intersection};
#[cfg(feature = "overlay")]
use geo_types::MultiPolygon;
use geo_types::{Coord, Geometry, Line, LineString, MultiLineString, Polygon};

use crate::error::{Error, Result};
use crate::geom;

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

/// Coordinates are compared by bit pattern, not by epsilon.
///
/// Every point this module matches on is either a vertex copied verbatim from
/// the input or one computed once and then reused, so equal points are equal
/// bit-for-bit. A tolerance would only invent joins the input didn't have.
fn key(c: Coord<f64>) -> (u64, u64) {
    (c.x.to_bits(), c.y.to_bits())
}

/// Drop repeated consecutive coordinates.
///
/// They are zero-length segments, and PostGIS treats them as absent:
/// `ST_IsSimple('LINESTRING(0 0,0 0,1 1)')` is true. Collapsing first means
/// the rest of this module never has to special-case them.
fn collapse(coords: &[Coord<f64>]) -> Vec<Coord<f64>> {
    let mut out: Vec<Coord<f64>> = Vec::with_capacity(coords.len());
    for &c in coords {
        if out.last().map(|&p| key(p) != key(c)).unwrap_or(true) {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// ST_IsSimple
// ---------------------------------------------------------------------------

/// `ST_IsSimple(geom)` — has the geometry no anomalous self-intersection?
///
/// The OGC definition is "the only self-intersections are at boundary
/// points", which for a line means: the curve may close on itself, and
/// distinct curves may meet, but only ever end-to-end. kenro tests exactly
/// that, in two passes that between them cover every case PostGIS was
/// measured on:
///
/// 1. **point multiplicity.** A coordinate may appear more than once only if
///    *every* appearance is at the end of its component. This is what
///    separates a closed ring (simple) from a ring with a tail hanging off
///    the same vertex (not simple) — the tail makes one appearance interior.
/// 2. **segment pairs.** Any collinear overlap fails; any proper crossing
///    fails; a touch at a single point is allowed only where that point ends
///    both segments. This catches what pass 1 cannot see, namely a segment
///    ending part-way along another with no vertex there.
///
/// Areal input is tested **ring by ring, independently**: a bow-tie ring is
/// not simple, but two overlapping members of a MULTIPOLYGON are (verified
/// against PostGIS — polygon simplicity says nothing about how the pieces sit
/// relative to each other). Points are simple unless a MULTIPOINT repeats
/// one.
pub fn st_is_simple(bytes: &[u8]) -> Result<bool> {
    const FUNC: &str = "ST_IsSimple";
    let g = geom::decode_auto(bytes)?;
    Ok(match &g.geometry {
        Geometry::Point(_) => true,
        Geometry::MultiPoint(mp) => {
            let mut seen = std::collections::HashSet::new();
            mp.0.iter().all(|p| seen.insert(key(p.0)))
        }
        Geometry::Line(_) => true,
        Geometry::LineString(ls) => lineal_is_simple(&[ls.0.as_slice()]),
        Geometry::MultiLineString(mls) => {
            lineal_is_simple(&mls.0.iter().map(|l| l.0.as_slice()).collect::<Vec<_>>())
        }
        Geometry::Polygon(p) => rings_are_simple(p),
        Geometry::MultiPolygon(mp) => mp.0.iter().all(rings_are_simple),
        Geometry::Rect(_) | Geometry::Triangle(_) => true,
        Geometry::GeometryCollection(_) => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "GeometryCollection is not supported".into(),
            });
        }
    })
}

fn rings_are_simple(p: &Polygon<f64>) -> bool {
    std::iter::once(p.exterior())
        .chain(p.interiors())
        .all(|r| lineal_is_simple(&[r.0.as_slice()]))
}

fn lineal_is_simple(parts: &[&[Coord<f64>]]) -> bool {
    let parts: Vec<Vec<Coord<f64>>> = parts
        .iter()
        .map(|p| collapse(p))
        .filter(|p| p.len() >= 2)
        .collect();
    if parts.is_empty() {
        return true; // empty is simple
    }

    // Pass 1 — a repeated point must be an endpoint everywhere it appears.
    let mut interior_seen = std::collections::HashSet::new();
    let mut count: std::collections::HashMap<(u64, u64), usize> = std::collections::HashMap::new();
    for part in &parts {
        for (i, &c) in part.iter().enumerate() {
            *count.entry(key(c)).or_default() += 1;
            if i != 0 && i != part.len() - 1 {
                interior_seen.insert(key(c));
            }
        }
    }
    if count
        .iter()
        .any(|(k, &n)| n > 1 && interior_seen.contains(k))
    {
        return false;
    }

    // Pass 2 — segment against segment.
    let segments: Vec<Line<f64>> = parts
        .iter()
        .flat_map(|p| p.windows(2).map(|w| Line::new(w[0], w[1])))
        .collect();
    for (i, a) in segments.iter().enumerate() {
        for b in &segments[i + 1..] {
            match line_intersection(*a, *b) {
                None => {}
                Some(LineIntersection::Collinear { .. }) => return false,
                Some(LineIntersection::SinglePoint { intersection, .. }) => {
                    let ends = |l: &Line<f64>| {
                        key(l.start) == key(intersection) || key(l.end) == key(intersection)
                    };
                    if !(ends(a) && ends(b)) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// ST_LineMerge
// ---------------------------------------------------------------------------

/// `ST_LineMerge(geom)` / `ST_LineMerge(geom, directed)` — sew lines together
/// where they meet end to end.
///
/// A node joining exactly two line ends is dissolved; anything else is left
/// alone. That is GEOS's rule, and it is why a Y junction comes back with all
/// three arms intact and why two lines that cross in their interiors are not
/// merged at all — there is no vertex where they cross, and this function
/// does not create one (`ST_Node` would, and kenro has no `ST_Node`).
///
/// With `directed` false (the default) a line is reversed when that lets it
/// join; with `directed` true the original directions are honoured, so
/// `(0 0,1 1)` and `(2 2,1 1)` stay apart.
///
/// A single resulting chain comes back as a LINESTRING and several as a
/// MULTILINESTRING, matching PostGIS.
///
/// ⚠️ **Divergences.** PostGIS answers `GEOMETRYCOLLECTION EMPTY` for
/// non-lineal input; kenro raises an error instead, because a silent empty is
/// the worst possible answer to "merge this polygon". The order of the
/// components in a multi-line result is unspecified and differs from GEOS's,
/// and so is the *direction* of a chain assembled from parts that had to be
/// reversed: kenro grows outward from the first part it is given, GEOS from
/// whichever end its own edge ordering reaches first. Both are correct — an
/// undirected merge has no preferred direction — but `ST_AsText` of the
/// result can read backwards from PostGIS's. Use `directed`, or `ST_Reverse`,
/// when the direction is load-bearing.
pub fn st_line_merge(bytes: &[u8]) -> Result<Vec<u8>> {
    line_merge(bytes, false)
}

pub fn st_line_merge_directed(bytes: &[u8], directed: bool) -> Result<Vec<u8>> {
    line_merge(bytes, directed)
}

fn line_merge(bytes: &[u8], directed: bool) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_LineMerge";
    let g = geom::decode_auto(bytes)?;
    let parts: Vec<Vec<Coord<f64>>> = match &g.geometry {
        Geometry::LineString(ls) => vec![ls.0.clone()],
        Geometry::MultiLineString(mls) => mls.0.iter().map(|l| l.0.clone()).collect(),
        Geometry::Line(l) => vec![vec![l.start, l.end]],
        _ => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "argument must be a LINESTRING or MULTILINESTRING (PostGIS answers \
                         GEOMETRYCOLLECTION EMPTY here; kenro will not return a collection, \
                         and an empty result would hide the mistake)"
                    .into(),
            });
        }
    };
    let merged = merge_chains(parts, directed);
    let geometry = match merged.len() {
        1 => Geometry::LineString(LineString::new(merged.into_iter().next().unwrap())),
        _ => Geometry::MultiLineString(MultiLineString::new(
            merged.into_iter().map(LineString::new).collect(),
        )),
    };
    out(geometry, g.srid, FUNC, &[bytes])
}

/// Join the parts at every node where exactly two line ends meet.
fn merge_chains(parts: Vec<Vec<Coord<f64>>>, directed: bool) -> Vec<Vec<Coord<f64>>> {
    let mut parts: Vec<Vec<Coord<f64>>> = parts
        .into_iter()
        .map(|p| collapse(&p))
        .filter(|p| p.len() >= 2)
        .collect();
    if parts.len() < 2 {
        return parts;
    }

    // Degree of each node, counting only line *ends*. A node touched by
    // anything other than exactly two ends is a junction and stays put.
    let mut degree: std::collections::HashMap<(u64, u64), usize> = std::collections::HashMap::new();
    for p in &parts {
        *degree.entry(key(p[0])).or_default() += 1;
        *degree.entry(key(*p.last().unwrap())).or_default() += 1;
    }

    let mut used = vec![false; parts.len()];
    let mut result = Vec::new();
    for i in 0..parts.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut chain = std::mem::take(&mut parts[i]);
        // Grow at both ends until no unique partner remains.
        loop {
            let mut grew = false;
            for side in [true, false] {
                let node = if side {
                    *chain.last().unwrap()
                } else {
                    chain[0]
                };
                if degree.get(&key(node)).copied().unwrap_or(0) != 2 {
                    continue;
                }
                let Some(j) = (0..parts.len()).find(|&j| {
                    !used[j] && {
                        let p = &parts[j];
                        let head = key(p[0]) == key(node);
                        let tail = key(*p.last().unwrap()) == key(node);
                        // Directed merging only ever appends tail-to-head.
                        if directed {
                            if side { head } else { tail }
                        } else {
                            head || tail
                        }
                    }
                }) else {
                    continue;
                };
                // A chain that has closed on itself must not keep growing.
                if key(chain[0]) == key(*chain.last().unwrap()) {
                    continue;
                }
                used[j] = true;
                let mut piece = std::mem::take(&mut parts[j]);
                if side {
                    if key(piece[0]) != key(node) {
                        piece.reverse();
                    }
                    chain.extend_from_slice(&piece[1..]);
                } else {
                    if key(*piece.last().unwrap()) != key(node) {
                        piece.reverse();
                    }
                    piece.pop();
                    piece.extend_from_slice(&chain);
                    chain = piece;
                }
                grew = true;
            }
            if !grew {
                break;
            }
        }
        result.push(chain);
    }
    result
}

// ---------------------------------------------------------------------------
// ST_Split
// ---------------------------------------------------------------------------

/// `ST_Split(input, blade)` — cut a geometry with another.
///
/// Lineal input is cut wherever the blade meets it: at the blade's own points
/// if the blade is puntal, or at the crossing points if it is lineal. Areal
/// input is cut by a lineal blade only, using `i_overlay`'s slice — the same
/// engine behind `ST_Intersection`, reached directly because `geo` does not
/// re-export it.
///
/// A blade that fails to cut is not an error: the input comes back as a
/// single-element multi, as PostGIS's collection does.
///
/// ⚠️ **Divergences.** The result is a MULTIPOLYGON or MULTILINESTRING rather
/// than PostGIS's GEOMETRYCOLLECTION (see the module note). Splitting a
/// polygon by a point is an error in both. Where PostGIS's slice records the
/// blade's touch as a new vertex on an uncut boundary, kenro's may not — the
/// pieces are the same shape either way.
#[cfg(feature = "overlay")]
pub fn st_split(input: &[u8], blade: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Split";
    let g = geom::decode_auto(input)?;
    let b = geom::decode_auto(blade)?;
    if g.srid > 0 && b.srid > 0 && g.srid != b.srid {
        return Err(Error::MixedSrid {
            func: FUNC,
            a: g.srid,
            b: b.srid,
        });
    }
    let geometry = match &g.geometry {
        Geometry::LineString(_) | Geometry::MultiLineString(_) | Geometry::Line(_) => {
            let parts = lineal_parts(FUNC, &g.geometry)?;
            let cuts = cut_points(FUNC, &b.geometry)?;
            Geometry::MultiLineString(MultiLineString::new(
                split_lines(parts, &cuts)
                    .into_iter()
                    .map(LineString::new)
                    .collect(),
            ))
        }
        Geometry::Polygon(_)
        | Geometry::MultiPolygon(_)
        | Geometry::Rect(_)
        | Geometry::Triangle(_) => {
            Geometry::MultiPolygon(slice_areal(FUNC, &g.geometry, &b.geometry)?)
        }
        _ => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "input must be lineal or areal".into(),
            });
        }
    };
    out(geometry, g.srid, FUNC, &[input, blade])
}

#[cfg(feature = "overlay")]
fn lineal_parts(func: &'static str, g: &Geometry<f64>) -> Result<Vec<Vec<Coord<f64>>>> {
    Ok(match g {
        Geometry::LineString(ls) => vec![collapse(&ls.0)],
        Geometry::MultiLineString(mls) => mls.0.iter().map(|l| collapse(&l.0)).collect(),
        Geometry::Line(l) => vec![vec![l.start, l.end]],
        _ => {
            return Err(Error::Unsupported {
                func,
                reason: "input must be lineal or areal".into(),
            });
        }
    })
}

/// The points at which a blade cuts — the blade's own vertices when it is
/// puntal, and nothing otherwise (lineal blades are intersected per segment).
#[cfg(feature = "overlay")]
enum Blade {
    Points(Vec<Coord<f64>>),
    Lines(Vec<Line<f64>>),
}

#[cfg(feature = "overlay")]
fn cut_points(func: &'static str, blade: &Geometry<f64>) -> Result<Blade> {
    Ok(match blade {
        Geometry::Point(p) => Blade::Points(vec![p.0]),
        Geometry::MultiPoint(mp) => Blade::Points(mp.0.iter().map(|p| p.0).collect()),
        Geometry::LineString(ls) => Blade::Lines(ls.lines().collect()),
        Geometry::MultiLineString(mls) => {
            Blade::Lines(mls.iter().flat_map(|l| l.lines()).collect())
        }
        Geometry::Line(l) => Blade::Lines(vec![*l]),
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => {
            return Err(Error::Unsupported {
                func,
                reason: "a line can only be split by a point or a line".into(),
            });
        }
        _ => {
            return Err(Error::Unsupported {
                func,
                reason: "unsupported blade".into(),
            });
        }
    })
}

/// Walk each part, breaking it wherever a cut point falls on it.
#[cfg(feature = "overlay")]
fn split_lines(parts: Vec<Vec<Coord<f64>>>, blade: &Blade) -> Vec<Vec<Coord<f64>>> {
    let mut out = Vec::new();
    for part in parts {
        if part.len() < 2 {
            continue;
        }
        let mut current = vec![part[0]];
        for w in part.windows(2) {
            let seg = Line::new(w[0], w[1]);
            // Every cut strictly inside this segment, ordered along it.
            let mut hits: Vec<Coord<f64>> = match blade {
                Blade::Points(ps) => ps.iter().copied().filter(|&p| on_segment(seg, p)).collect(),
                Blade::Lines(ls) => ls
                    .iter()
                    .filter_map(|&b| match line_intersection(seg, b) {
                        Some(LineIntersection::SinglePoint { intersection, .. }) => {
                            Some(intersection)
                        }
                        _ => None,
                    })
                    .collect(),
            };
            hits.retain(|&p| key(p) != key(seg.start) && key(p) != key(seg.end));
            hits.sort_by(|a, b| {
                let d = |c: &Coord<f64>| (c.x - seg.start.x).powi(2) + (c.y - seg.start.y).powi(2);
                d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
            });
            hits.dedup_by_key(|p| key(*p));
            for h in hits {
                current.push(h);
                out.push(std::mem::take(&mut current));
                current = vec![h];
            }
            current.push(w[1]);
            // A cut that lands exactly on this vertex breaks here too.
            let ends_at_cut = match blade {
                Blade::Points(ps) => ps.iter().any(|&p| key(p) == key(w[1])),
                Blade::Lines(ls) => ls.iter().any(|&b| {
                    matches!(
                        line_intersection(seg, b),
                        Some(LineIntersection::SinglePoint { intersection, .. }) if key(intersection) == key(w[1])
                    )
                }),
            };
            if ends_at_cut && key(w[1]) != key(*part.last().unwrap()) {
                out.push(std::mem::take(&mut current));
                current = vec![w[1]];
            }
        }
        if current.len() >= 2 {
            out.push(current);
        }
    }
    out
}

/// Does `p` lie on `seg` (endpoints included)?
#[cfg(feature = "overlay")]
fn on_segment(seg: Line<f64>, p: Coord<f64>) -> bool {
    use geo::algorithm::{Kernel, kernels::RobustKernel};
    if RobustKernel::orient2d(seg.start, seg.end, p) != geo::algorithm::Orientation::Collinear {
        return false;
    }
    let within = |a: f64, b: f64, v: f64| v >= a.min(b) && v <= a.max(b);
    within(seg.start.x, seg.end.x, p.x) && within(seg.start.y, seg.end.y, p.y)
}

/// Slice areal input with a lineal blade via `i_overlay`.
#[cfg(feature = "overlay")]
fn slice_areal(
    func: &'static str,
    input: &Geometry<f64>,
    blade: &Geometry<f64>,
) -> Result<MultiPolygon<f64>> {
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::float::slice::FloatSlice;

    let polygons: Vec<Polygon<f64>> = match input {
        Geometry::Polygon(p) => vec![p.clone()],
        Geometry::MultiPolygon(mp) => mp.0.clone(),
        Geometry::Rect(r) => vec![r.to_polygon()],
        Geometry::Triangle(t) => vec![t.to_polygon()],
        _ => unreachable!("caller checked the class"),
    };
    let cutter: Vec<Vec<[f64; 2]>> = match blade {
        Geometry::LineString(ls) => vec![path(&ls.0)],
        Geometry::MultiLineString(mls) => mls.0.iter().map(|l| path(&l.0)).collect(),
        Geometry::Line(l) => vec![vec![[l.start.x, l.start.y], [l.end.x, l.end.y]]],
        Geometry::Point(_) | Geometry::MultiPoint(_) => {
            return Err(Error::Unsupported {
                func,
                reason: "splitting a polygon by a point is unsupported (PostGIS refuses this too)"
                    .into(),
            });
        }
        _ => {
            return Err(Error::Unsupported {
                func,
                reason: "a polygon can only be split by a line".into(),
            });
        }
    };

    // i_overlay reads a contour's winding as its sign: shells must be
    // counter-clockwise and holes clockwise, or `NonZero` fills the hole and
    // the split silently gains area. WKT carries whatever winding the author
    // wrote, so orient before handing them over.
    use geo::algorithm::orient::{Direction, Orient};
    let shapes: Vec<Vec<Vec<[f64; 2]>>> = polygons
        .iter()
        .map(|p| {
            let p = p.orient(Direction::Default);
            std::iter::once(path(&p.exterior().0))
                .chain(p.interiors().iter().map(|r| path(&r.0)))
                .collect()
        })
        .collect();
    let sliced = shapes.slice_by(&cutter, FillRule::NonZero);
    Ok(MultiPolygon::new(
        sliced
            .into_iter()
            .filter(|shape| !shape.is_empty())
            .map(|shape| {
                let mut contours = shape.into_iter().map(ring);
                let exterior = contours.next().unwrap_or_else(|| LineString::new(vec![]));
                Polygon::new(exterior, contours.collect())
            })
            .collect(),
    ))
}

#[cfg(feature = "overlay")]
fn path(coords: &[Coord<f64>]) -> Vec<[f64; 2]> {
    // i_overlay's contours are implicitly closed; a repeated last point would
    // become a zero-length edge.
    let mut v: Vec<[f64; 2]> = coords.iter().map(|c| [c.x, c.y]).collect();
    if v.len() > 1 && v.first() == v.last() {
        v.pop();
    }
    v
}

#[cfg(feature = "overlay")]
fn ring(contour: Vec<[f64; 2]>) -> LineString<f64> {
    let mut coords: Vec<Coord<f64>> = contour
        .into_iter()
        .map(|p| Coord { x: p[0], y: p[1] })
        .collect();
    if let Some(&first) = coords.first() {
        coords.push(first);
    }
    LineString::new(coords)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    fn simple(wkt: &str) -> bool {
        st_is_simple(&g(wkt)).unwrap()
    }

    /// Every one of these was read off PostGIS 3.5 before it was written
    /// down; the interesting ones are the near-misses.
    #[test]
    fn simplicity_matches_postgis_on_the_cases_that_distinguish_the_rules() {
        // Plain and crossing lines.
        assert!(simple("LINESTRING(0 0,10 10)"));
        assert!(!simple("LINESTRING(0 0,10 10,0 10,10 0)"));
        // A closed ring is simple; the same ring with a tail on the closing
        // vertex is not — the vertex-multiplicity pass is what tells them
        // apart, since no segments actually cross in either.
        assert!(simple("LINESTRING(0 0,10 0,10 10,0 10,0 0)"));
        assert!(!simple("LINESTRING(0 0,10 0,10 10,0 10,0 0,5 5)"));
        assert!(simple("LINESTRING(0 0,10 0,10 10,0 0)"));
        // Retracing its own steps: collinear overlap, adjacent segments.
        assert!(!simple("LINESTRING(0 0,5 0,5 5,5 0,10 0)"));
        assert!(!simple("LINESTRING(0 0,10 0,5 0)"));
        assert!(!simple("LINESTRING(0 0,1 1,0 0)"));
        // An end landing part-way along an earlier segment, with no vertex
        // there for pass 1 to see.
        assert!(!simple("LINESTRING(0 0,10 0,5 5,5 0)"));
        // Repeated coordinates are not an anomaly.
        assert!(simple("LINESTRING(0 0,0 0,1 1)"));
        assert!(simple("LINESTRING EMPTY"));
    }

    #[test]
    fn simplicity_across_components_and_dimensions() {
        // Components may meet, but only end to end — including three at once.
        assert!(simple("MULTILINESTRING((0 0,10 10),(10 10,20 20))"));
        assert!(simple("MULTILINESTRING((0 0,1 1),(1 1,2 0),(1 1,0 2))"));
        assert!(!simple("MULTILINESTRING((0 0,10 10),(0 10,10 0))"));
        assert!(!simple("MULTILINESTRING((0 0,10 0),(5 0,15 0))"));
        assert!(!simple("MULTILINESTRING((0 0,5 5,10 10),(5 5,10 0))"));
        assert!(!simple("MULTILINESTRING((0 0,1 1),(1 1,0 0))"));
        // Points.
        assert!(simple("MULTIPOINT(0 0,1 1)"));
        assert!(!simple("MULTIPOINT(0 0,0 0)"));
        assert!(simple("POINT(0 0)"));
        // Areal input is judged ring by ring and nothing else: a bow-tie ring
        // fails, a hole touching its shell passes, and two MULTIPOLYGON
        // members sitting on top of each other are still simple.
        assert!(simple("POLYGON((0 0,10 0,10 10,0 10,0 0))"));
        assert!(!simple("POLYGON((0 0,10 10,10 0,0 10,0 0))"));
        assert!(simple(
            "POLYGON((0 0,10 0,10 10,0 10,0 0),(0 0,5 2,5 5,0 0))"
        ));
        assert!(!simple(
            "POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,8 2,2 8,8 8,2 2))"
        ));
        assert!(simple(
            "MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((0 0,1 0,1 1,0 0)))"
        ));
    }

    fn merged(wkt: &str) -> String {
        st_as_text(&st_line_merge(&g(wkt)).unwrap()).unwrap()
    }

    #[test]
    fn merging_joins_only_where_exactly_two_ends_meet() {
        assert_eq!(
            merged("MULTILINESTRING((0 0,1 1),(1 1,2 2))"),
            "LINESTRING(0 0,1 1,2 2)"
        );
        // Direction is not an obstacle by default.
        // Both of these are the same chain, and PostGIS spells both
        // (0 0,1 1,2 2) — but the direction of a merge that had to reverse a
        // part is arbitrary, so the assertion is on the chain, not on which
        // end it starts from.
        for wkt in [
            "MULTILINESTRING((1 1,0 0),(1 1,2 2))",
            "MULTILINESTRING((2 2,1 1),(0 0,1 1))",
        ] {
            assert!(
                matches!(
                    merged(wkt).as_str(),
                    "LINESTRING(0 0,1 1,2 2)" | "LINESTRING(2 2,1 1,0 0)"
                ),
                "{wkt} -> {}",
                merged(wkt)
            );
        }
        // A single line, and a chain that closes.
        assert_eq!(merged("LINESTRING(0 0,1 1)"), "LINESTRING(0 0,1 1)");
        // Three lines closing into a ring: one LINESTRING of four points,
        // closed. Which vertex it starts from is as arbitrary as the
        // direction — PostGIS starts at (0 0), kenro at (2 2).
        let ring = merged("MULTILINESTRING((0 0,1 1),(1 1,2 2),(2 2,0 0))");
        assert!(ring.starts_with("LINESTRING("), "{ring}");
        let pts: Vec<&str> = ring
            .trim_start_matches("LINESTRING(")
            .trim_end_matches(')')
            .split(',')
            .collect();
        assert_eq!(pts.len(), 4, "{ring}");
        assert_eq!(pts[0], pts[3], "{ring}");
        let mut sorted = pts[..3].to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, ["0 0", "1 1", "2 2"], "{ring}");
        // Nothing to join.
        assert_eq!(
            merged("MULTILINESTRING((0 0,1 1),(2 2,3 3))"),
            "MULTILINESTRING((0 0,1 1),(2 2,3 3))"
        );
        // A Y junction is degree 3: no arm is merged into another. Two lines
        // that merely cross are not merged either — that would need noding.
        let y = merged("MULTILINESTRING((0 0,1 1),(1 1,2 0),(1 1,0 2))");
        assert!(y.starts_with("MULTILINESTRING("), "{y}");
        assert_eq!(y.matches("),(").count(), 2, "{y}");
        assert_eq!(
            merged("MULTILINESTRING((0 0,2 2),(1 1,3 3))"),
            "MULTILINESTRING((0 0,2 2),(1 1,3 3))"
        );
        // Non-lineal input is refused rather than silently emptied.
        assert!(st_line_merge(&g("POLYGON((0 0,1 0,1 1,0 0))")).is_err());
    }

    #[test]
    fn directed_merging_honours_the_original_directions() {
        let d = |wkt: &str| st_as_text(&st_line_merge_directed(&g(wkt), true).unwrap()).unwrap();
        assert_eq!(
            d("MULTILINESTRING((0 0,1 1),(1 1,2 2))"),
            "LINESTRING(0 0,1 1,2 2)"
        );
        assert_eq!(
            d("MULTILINESTRING((0 0,1 1),(2 2,1 1))"),
            "MULTILINESTRING((0 0,1 1),(2 2,1 1))"
        );
    }

    #[cfg(feature = "overlay")]
    #[test]
    fn splitting_a_line_yields_the_pieces_in_order() {
        let split = |a: &str, b: &str| st_as_text(&st_split(&g(a), &g(b)).unwrap()).unwrap();
        assert_eq!(
            split("LINESTRING(0 0,10 0)", "POINT(5 0)"),
            "MULTILINESTRING((0 0,5 0),(5 0,10 0))"
        );
        assert_eq!(
            split("LINESTRING(0 0,10 0)", "MULTIPOINT(3 0,7 0)"),
            "MULTILINESTRING((0 0,3 0),(3 0,7 0),(7 0,10 0))"
        );
        assert_eq!(
            split("LINESTRING(0 0,10 10)", "LINESTRING(0 10,10 0)"),
            "MULTILINESTRING((0 0,5 5),(5 5,10 10))"
        );
        assert_eq!(
            split(
                "MULTILINESTRING((0 0,10 0),(0 5,10 5))",
                "LINESTRING(5 -1,5 6)"
            ),
            "MULTILINESTRING((0 0,5 0),(5 0,10 0),(0 5,5 5),(5 5,10 5))"
        );
        // A blade that misses returns the input, still as a multi.
        assert_eq!(
            split("LINESTRING(0 0,10 0)", "POINT(5 5)"),
            "MULTILINESTRING((0 0,10 0))"
        );
        // A cut exactly on an existing vertex breaks there once, not twice.
        assert_eq!(
            split("LINESTRING(0 0,5 0,10 0)", "POINT(5 0)"),
            "MULTILINESTRING((0 0,5 0),(5 0,10 0))"
        );
    }

    #[cfg(feature = "overlay")]
    #[test]
    fn splitting_a_polygon_preserves_area_and_holes() {
        use crate::functions::accessors::st_area;
        let square = g("POLYGON((0 0,10 0,10 10,0 10,0 0))");
        let cut = st_split(&square, &g("LINESTRING(5 -1,5 11)")).unwrap();
        assert_eq!(
            crate::functions::accessors::st_num_geometries(&cut).unwrap(),
            2
        );
        assert!((st_area(&cut).unwrap() - 100.0).abs() < 1e-9);
        // Two blades quarter it.
        let quartered = st_split(&square, &g("MULTILINESTRING((5 -1,5 11),(-1 5,11 5))")).unwrap();
        assert_eq!(
            crate::functions::accessors::st_num_geometries(&quartered).unwrap(),
            4
        );
        assert!((st_area(&quartered).unwrap() - 100.0).abs() < 1e-9);
        // The hole survives the cut: area is 100 - 4 either side of it.
        let holed = g("POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,4 2,4 4,2 4,2 2))");
        let cut = st_split(&holed, &g("LINESTRING(5 -1,5 11)")).unwrap();
        assert!((st_area(&cut).unwrap() - 96.0).abs() < 1e-9);
        // A blade that does not reach across leaves one piece.
        let uncut = st_split(&square, &g("LINESTRING(-1 -1,-1 5)")).unwrap();
        assert_eq!(
            crate::functions::accessors::st_num_geometries(&uncut).unwrap(),
            1
        );
        assert!((st_area(&uncut).unwrap() - 100.0).abs() < 1e-9);
        // PostGIS refuses a point blade on a polygon, and so does kenro.
        assert!(st_split(&square, &g("POINT(5 5)")).is_err());
    }
}
