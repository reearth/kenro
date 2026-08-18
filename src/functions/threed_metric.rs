//! 3D distance, and the predicates built on it.
//!
//! The nine functions here are the ones core PostGIS has *without* SFCGAL —
//! `ST_3DDistance`, `ST_3DDWithin`, `ST_3DDFullyWithin`, `ST_3DMaxDistance`,
//! `ST_3DClosestPoint`, `ST_3DShortestLine`, `ST_3DLongestLine`,
//! `ST_3DIntersects` and `ST_3DLineInterpolatePoint`. Everything in the
//! solid-modelling family (`ST_Volume`, `ST_3DIntersection`, `ST_Extrude`, …)
//! belongs to SFCGAL and is not here; the reasoning is
//! `tmp/3d-predicates.md`.
//!
//! **No 3D geometry model is involved.** The measurement that makes that
//! possible: a POLYHEDRALSURFACE is treated as a *set of faces*, never as a
//! volume. On PostGIS 3.5, a point at the dead centre of a closed unit cube
//! gives `ST_3DIntersects = false`. So there is no point-in-solid test, no
//! shell orientation and no topology — only primitives against primitives:
//!
//! | pair | primitive |
//! |---|---|
//! | point / point | one subtraction |
//! | point / segment | project, clamp `t` to `[0, 1]` |
//! | segment / segment | the closed form for skew segments |
//! | point / face | project onto the plane, inside-test, else fall back to edges |
//!
//! Faces are **filled**: a segment through the middle of a polygon intersects it
//! (measured), so `ST_3DDistance(POINT Z (5 5 10), POLYGON Z ((0 0 0,…)))` is 10
//! rather than the 11.18 the boundary alone would give.
//!
//! Two semantics are copied deliberately, because both are wrong by default:
//!
//! - **A missing Z means "any value", not zero.** PostGIS says so in a notice,
//!   and `ST_3DDistance(POINT Z (0 0 10), POINT(0 0))` is **0** — the
//!   Z-less operand behaves as a vertical line. Two 2D operands fall back to
//!   plain 2D distance (5 for `POINT(0 0)`/`POINT(3 4)`). Both cases are served
//!   by delegating to the 2D functions, which is exactly equivalent and needs no
//!   special machinery.
//! - **`ST_3DMaxDistance` is vertex-to-vertex.** Measured: a unit-square face
//!   against its own corner gives √200, and `ST_3DLongestLine` returns
//!   `LINESTRING(10 10 0,0 0 0)` — two vertices, no interior point.

use crate::coords;
use crate::error::{Error, Result};
use crate::geom;

/// A geometry as the primitives see it. Rings become faces, linestrings become
/// segments, points stay points — the distinction the walker reports for free.
#[derive(Default)]
struct Parts {
    points: Vec<[f64; 3]>,
    segments: Vec<([f64; 3], [f64; 3])>,
    /// Each face is a ring, fan-triangulated on use.
    faces: Vec<Vec<[f64; 3]>>,
}

impl Parts {
    /// Every vertex, for the max-distance family.
    fn vertices(&self) -> impl Iterator<Item = [f64; 3]> + '_ {
        self.points
            .iter()
            .copied()
            .chain(self.segments.iter().flat_map(|(a, b)| [*a, *b]))
            .chain(self.faces.iter().flatten().copied())
    }

    /// Triangles of every face, fanned from the ring's first vertex.
    ///
    /// A non-planar ring has no single plane, and PostGIS triangulates rather
    /// than flattening to a best fit — measured: a ring whose corners sit at
    /// z = 0, 0, 0, 10 answers 90.27735042633894 against a point at z = 100,
    /// where a planar ring answers exactly 100. ⚠️ Which triangulation is not
    /// documented, but on that case every candidate diagonal gives the same
    /// minimum, so a fan is as defensible as any.
    fn triangles(&self) -> impl Iterator<Item = ([f64; 3], [f64; 3], [f64; 3])> + '_ {
        self.faces.iter().flat_map(|ring| {
            let n = ring.len();
            (1..n.saturating_sub(1)).map(move |i| (ring[0], ring[i], ring[i + 1]))
        })
    }
}

/// Read a geometry's 3D primitives straight out of its encoding.
///
/// Deliberately not "decode, then look the heights up in a `ZIndex`": a vertical
/// wall has two vertices at one `(x, y)`, which that index correctly calls
/// ambiguous. 3D metrics need the coordinates in order, and the walk provides
/// them.
fn parts(bytes: &[u8], func: &'static str) -> Result<Parts> {
    let mut parts = Parts::default();
    let mut current: Vec<[f64; 3]> = Vec::new();
    let mut current_base = 0u32;
    let flush = |base: u32, run: &mut Vec<[f64; 3]>, parts: &mut Parts| {
        if run.is_empty() {
            return;
        }
        match base {
            coords::base::POINT => parts.points.extend(run.iter().copied()),
            coords::base::LINESTRING => {
                for pair in run.windows(2) {
                    parts.segments.push((pair[0], pair[1]));
                }
            }
            coords::base::POLYGON | coords::base::TRIANGLE => {
                parts.faces.push(std::mem::take(run));
            }
            // Anything else reaching here would be a walker change, not input.
            _ => {}
        }
        run.clear();
    };
    coords::for_each_coord_typed(bytes, &mut |c, first, base| {
        if first {
            flush(current_base, &mut current, &mut parts);
            current_base = base;
        }
        current.push([c.x, c.y, c.z.unwrap_or(0.0)]);
    })?;
    flush(current_base, &mut current, &mut parts);
    // An empty geometry yields empty parts rather than an error: PostGIS answers
    // NULL for ST_3DDistance against one and `false` for ST_3DDWithin (measured),
    // which is what `closest`/`farthest` returning `None` already produces.
    let _ = func;
    Ok(parts)
}

// ---- primitives ----

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + t * (b[0] - a[0]),
        a[1] + t * (b[1] - a[1]),
        a[2] + t * (b[2] - a[2]),
    ]
}
fn norm(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}

/// A distance and the two points that realise it. Every function here is a
/// search for the smallest — or largest — of these.
#[derive(Debug, Clone, Copy)]
struct Witness {
    d: f64,
    a: [f64; 3],
    b: [f64; 3],
}

impl Witness {
    fn between(a: [f64; 3], b: [f64; 3]) -> Self {
        Self {
            d: norm(sub(a, b)),
            a,
            b,
        }
    }
    fn min(self, other: Self) -> Self {
        if other.d < self.d { other } else { self }
    }
    fn flipped(self) -> Self {
        Self {
            d: self.d,
            a: self.b,
            b: self.a,
        }
    }
}

/// Point to segment: project, clamp to the segment, measure.
fn pt_seg(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> Witness {
    let ab = sub(b, a);
    let len_sq = dot(ab, ab);
    let t = if len_sq == 0.0 {
        0.0
    } else {
        (dot(sub(p, a), ab) / len_sq).clamp(0.0, 1.0)
    };
    Witness::between(p, lerp(a, b, t))
}

/// Segment to segment, including skew and parallel.
///
/// The closed form: solve for the parameters that minimise the squared distance
/// between the infinite lines, clamp both to `[0, 1]`, and re-solve the other
/// against the clamped one so a clamped corner is still optimal.
fn seg_seg(p1: [f64; 3], q1: [f64; 3], p2: [f64; 3], q2: [f64; 3]) -> Witness {
    let (d1, d2, r) = (sub(q1, p1), sub(q2, p2), sub(p1, p2));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    // A degenerate segment is a point; fall back rather than divide by zero.
    if a <= f64::EPSILON && e <= f64::EPSILON {
        return Witness::between(p1, p2);
    }
    if a <= f64::EPSILON {
        return pt_seg(p1, p2, q2).flipped().flipped();
    }
    if e <= f64::EPSILON {
        return pt_seg(p2, p1, q1).flipped();
    }
    let c = dot(d1, r);
    let b = dot(d1, d2);
    let denom = a * e - b * b;
    let mut s = if denom != 0.0 {
        ((b * f - c * e) / denom).clamp(0.0, 1.0)
    } else {
        0.0 // parallel: any s does, so take the start and let t decide
    };
    let mut t = (b * s + f) / e;
    if t < 0.0 {
        t = 0.0;
        s = (-c / a).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / a).clamp(0.0, 1.0);
    }
    Witness::between(lerp(p1, q1, s), lerp(p2, q2, t))
}

/// Point to a filled triangle: the perpendicular foot when it lands inside,
/// otherwise the nearest point on an edge.
fn pt_tri(p: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Witness {
    let n = cross(sub(b, a), sub(c, a));
    let n_sq = dot(n, n);
    if n_sq > 0.0 {
        // Project onto the plane, then test with barycentric coordinates
        // computed as signed sub-triangle areas against the same normal.
        let foot = {
            let dist = dot(sub(p, a), n) / n_sq;
            [p[0] - dist * n[0], p[1] - dist * n[1], p[2] - dist * n[2]]
        };
        let inside = [(a, b), (b, c), (c, a)]
            .iter()
            .all(|(u, v)| dot(cross(sub(*v, *u), sub(foot, *u)), n) >= 0.0);
        if inside {
            return Witness::between(p, foot);
        }
    }
    pt_seg(p, a, b).min(pt_seg(p, b, c)).min(pt_seg(p, c, a))
}

/// Triangle to triangle: every edge pair, plus each triangle's vertices against
/// the other's face. The second half is what catches one face nested inside
/// another, where no edges cross.
fn tri_tri(t1: ([f64; 3], [f64; 3], [f64; 3]), t2: ([f64; 3], [f64; 3], [f64; 3])) -> Witness {
    let e1 = [(t1.0, t1.1), (t1.1, t1.2), (t1.2, t1.0)];
    let e2 = [(t2.0, t2.1), (t2.1, t2.2), (t2.2, t2.0)];
    let mut best = Witness {
        d: f64::INFINITY,
        a: t1.0,
        b: t2.0,
    };
    for (a1, b1) in e1 {
        for (a2, b2) in e2 {
            best = best.min(seg_seg(a1, b1, a2, b2));
            if best.d == 0.0 {
                return best;
            }
        }
    }
    for v in [t1.0, t1.1, t1.2] {
        best = best.min(pt_tri(v, t2.0, t2.1, t2.2));
    }
    for v in [t2.0, t2.1, t2.2] {
        best = best.min(pt_tri(v, t1.0, t1.1, t1.2).flipped());
    }
    best
}

/// An axis-aligned box around one primitive — the whole of the prefilter's
/// state. Without it the sweep below is a full cross product of both operands'
/// points, segments and triangles, which two meshes turn quadratic.
#[derive(Clone, Copy)]
struct Aabb {
    lo: [f64; 3],
    hi: [f64; 3],
}

/// The bounds are computed in floating point, so they are loosened by a few
/// ULPs before they are believed: a lower bound a hair too high — or an upper
/// bound a hair too low — would prune the pair that owns the answer.
const SLACK: f64 = 8.0 * f64::EPSILON;

impl Aabb {
    fn point(p: [f64; 3]) -> Self {
        Self { lo: p, hi: p }
    }

    fn of(points: impl IntoIterator<Item = [f64; 3]>) -> Option<Self> {
        let mut it = points.into_iter();
        let mut b = Self::point(it.next()?);
        for p in it {
            b.add(p);
        }
        Some(b)
    }

    fn add(&mut self, p: [f64; 3]) {
        for (i, c) in p.iter().enumerate() {
            self.lo[i] = self.lo[i].min(*c);
            self.hi[i] = self.hi[i].max(*c);
        }
    }

    fn union(self, other: Self) -> Self {
        let mut b = self;
        b.add(other.lo);
        b.add(other.hi);
        b
    }

    /// The squared distance no pair of points, one drawn from each box, can go
    /// below. Zero for boxes that touch or overlap.
    fn min_dist_sq(&self, other: &Self) -> f64 {
        let mut sum = 0.0;
        for i in 0..3 {
            // `max` on a NaN operand returns the other, so a NaN coordinate
            // yields a zero gap — no pruning, which is the safe direction.
            let gap = (self.lo[i] - other.hi[i])
                .max(other.lo[i] - self.hi[i])
                .max(0.0);
            sum += gap * gap;
        }
        sum
    }

    /// The squared distance no such pair can exceed.
    fn max_dist_sq(&self, other: &Self) -> f64 {
        let mut sum = 0.0;
        for i in 0..3 {
            let span = (self.hi[i] - other.lo[i])
                .abs()
                .max((other.hi[i] - self.lo[i]).abs());
            sum += span * span;
        }
        sum
    }
}

/// A `Parts` with its per-primitive boxes, and its triangles materialised —
/// [`Parts::triangles`] is walked once per primitive of the other operand
/// otherwise.
struct Boxed<'a> {
    parts: &'a Parts,
    segments: Vec<Aabb>,
    triangles: Vec<([f64; 3], [f64; 3], [f64; 3])>,
    triangle_boxes: Vec<Aabb>,
    /// Every primitive's box, or `None` when there are no primitives at all.
    all: Option<Aabb>,
}

impl<'a> Boxed<'a> {
    fn new(parts: &'a Parts) -> Self {
        let segments: Vec<Aabb> = parts
            .segments
            .iter()
            .map(|(a, b)| Aabb::point(*a).union(Aabb::point(*b)))
            .collect();
        let triangles: Vec<_> = parts.triangles().collect();
        let triangle_boxes: Vec<Aabb> = triangles
            .iter()
            .map(|t| {
                Aabb::point(t.0)
                    .union(Aabb::point(t.1))
                    .union(Aabb::point(t.2))
            })
            .collect();
        let all = parts
            .points
            .iter()
            .map(|p| Aabb::point(*p))
            .chain(segments.iter().copied())
            .chain(triangle_boxes.iter().copied())
            .reduce(Aabb::union);
        Self {
            parts,
            segments,
            triangles,
            triangle_boxes,
            all,
        }
    }
}

/// Can a pair whose boxes are `lb_sq` apart at the very closest still beat
/// `best`? Ties are prunable because [`Witness::min`] keeps the incumbent on
/// one — see the ordering note on [`closest`].
fn no_closer(best: Option<Witness>, lb_sq: f64) -> bool {
    best.is_some_and(|w| lb_sq * (1.0 - SLACK) >= w.d * w.d)
}

/// The closest pair of points between two geometries.
///
/// `stop_at_zero` lets the predicates leave early: `ST_3DIntersects` only needs
/// to know whether the minimum reaches zero.
///
/// An AABB prefilter skips a pair whose boxes are already at least `best` apart.
/// **The sweep order is exactly the unfiltered one**, deliberately: `min` keeps
/// the first witness of an equal distance, and `ST_3DClosestPoint` /
/// `ST_3DShortestLine` return that witness as geometry, so reordering the pairs
/// — or seeding `best` out of order to make the pruning bite sooner — would
/// change the answer on a tie rather than only its cost. Pruning alone cannot:
/// a skipped pair is one that could only have tied or lost.
fn closest(a: &Parts, b: &Parts, stop_at_zero: bool) -> Option<Witness> {
    closest_inner(a, b, stop_at_zero, true)
}

fn closest_inner(a: &Parts, b: &Parts, stop_at_zero: bool, prefilter: bool) -> Option<Witness> {
    let (a, b) = (Boxed::new(a), Boxed::new(b));
    let (Some(_), Some(b_all)) = (a.all, b.all) else {
        return None; // nothing to pair off; the sweep would find nothing either
    };
    let mut best: Option<Witness> = None;
    // Skip this pair? Never when the prefilter is off, which is how the tests
    // compare the two sweeps.
    macro_rules! skip {
        ($outer:expr, $inner:expr) => {
            prefilter && no_closer(best, $outer.min_dist_sq($inner))
        };
    }
    macro_rules! offer {
        ($w:expr) => {{
            let w = $w;
            best = Some(match best {
                None => w,
                Some(seen) => seen.min(w),
            });
            if stop_at_zero && best.is_some_and(|x| x.d == 0.0) {
                return best;
            }
        }};
    }
    for &p in &a.parts.points {
        let pb = Aabb::point(p);
        if skip!(pb, &b_all) {
            continue;
        }
        for &q in &b.parts.points {
            if skip!(pb, &Aabb::point(q)) {
                continue;
            }
            offer!(Witness::between(p, q));
        }
        for (&(s, e), sb) in b.parts.segments.iter().zip(&b.segments) {
            if skip!(pb, sb) {
                continue;
            }
            offer!(pt_seg(p, s, e));
        }
        for (t, tb) in b.triangles.iter().zip(&b.triangle_boxes) {
            if skip!(pb, tb) {
                continue;
            }
            offer!(pt_tri(p, t.0, t.1, t.2));
        }
    }
    for (&(s, e), ab) in a.parts.segments.iter().zip(&a.segments) {
        if skip!(ab, &b_all) {
            continue;
        }
        for &q in &b.parts.points {
            if skip!(ab, &Aabb::point(q)) {
                continue;
            }
            offer!(pt_seg(q, s, e).flipped());
        }
        for (&(s2, e2), sb) in b.parts.segments.iter().zip(&b.segments) {
            if skip!(ab, sb) {
                continue;
            }
            offer!(seg_seg(s, e, s2, e2));
        }
        for (t, tb) in b.triangles.iter().zip(&b.triangle_boxes) {
            if skip!(ab, tb) {
                continue;
            }
            // A segment against a filled triangle: its ends against the face,
            // and the face's edges against the segment.
            offer!(pt_tri(s, t.0, t.1, t.2));
            offer!(pt_tri(e, t.0, t.1, t.2));
            for (u, v) in [(t.0, t.1), (t.1, t.2), (t.2, t.0)] {
                offer!(seg_seg(s, e, u, v));
            }
        }
    }
    for (&t1, ab) in a.triangles.iter().zip(&a.triangle_boxes) {
        if skip!(ab, &b_all) {
            continue;
        }
        for &q in &b.parts.points {
            if skip!(ab, &Aabb::point(q)) {
                continue;
            }
            offer!(pt_tri(q, t1.0, t1.1, t1.2).flipped());
        }
        for (&(s2, e2), sb) in b.parts.segments.iter().zip(&b.segments) {
            if skip!(ab, sb) {
                continue;
            }
            offer!(pt_tri(s2, t1.0, t1.1, t1.2).flipped());
            offer!(pt_tri(e2, t1.0, t1.1, t1.2).flipped());
            for (u, v) in [(t1.0, t1.1), (t1.1, t1.2), (t1.2, t1.0)] {
                offer!(seg_seg(u, v, s2, e2));
            }
        }
        for (&t2, tb) in b.triangles.iter().zip(&b.triangle_boxes) {
            if skip!(ab, tb) {
                continue;
            }
            offer!(tri_tri(t1, t2));
        }
    }
    best
}

/// Both operands' 3D primitives, or `None` when either lacks a Z.
///
/// A Z-less operand means "any height" in PostGIS, which is the same answer the
/// 2D functions already give — so the callers delegate rather than model it.
fn both(a: &[u8], b: &[u8], func: &'static str) -> Result<Option<(Parts, Parts)>> {
    if !geom::has_z_encoded(a)? || !geom::has_z_encoded(b)? {
        return Ok(None);
    }
    Ok(Some((parts(a, func)?, parts(b, func)?)))
}

// ---- SQL functions ----

/// `ST_3DDistance(a, b)` — the shortest distance in 3D, NULL for an empty
/// operand (measured).
pub fn st_3d_distance(a: &[u8], b: &[u8]) -> Result<Option<f64>> {
    const FUNC: &str = "ST_3DDistance";
    let Some((pa, pb)) = both(a, b, FUNC)? else {
        return crate::functions::predicates::st_distance(a, b);
    };
    Ok(closest(&pa, &pb, false).map(|w| w.d))
}

/// `ST_3DDWithin(a, b, d)` — within `d` in 3D. An empty operand is `false`
/// (measured), not NULL.
pub fn st_3d_dwithin(a: &[u8], b: &[u8], d: f64) -> Result<bool> {
    const FUNC: &str = "ST_3DDWithin";
    if d < 0.0 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "tolerance cannot be less than zero".into(),
        });
    }
    let Some((pa, pb)) = both(a, b, FUNC)? else {
        return crate::functions::predicates::st_dwithin(a, b, d);
    };
    Ok(closest(&pa, &pb, false).is_some_and(|w| w.d <= d))
}

/// `ST_3DIntersects(a, b)` — do they touch in 3D?
///
/// A POLYHEDRALSURFACE is a set of faces, so a point inside a closed cube does
/// **not** intersect it (measured on PostGIS 3.5). Faces themselves are filled.
pub fn st_3d_intersects(a: &[u8], b: &[u8]) -> Result<bool> {
    const FUNC: &str = "ST_3DIntersects";
    let Some((pa, pb)) = both(a, b, FUNC)? else {
        return crate::functions::predicates::st_intersects(a, b);
    };
    Ok(closest(&pa, &pb, true).is_some_and(|w| w.d == 0.0))
}

/// `ST_3DMaxDistance(a, b)` — the greatest distance between them, which PostGIS
/// takes **vertex to vertex** (measured).
pub fn st_3d_max_distance(a: &[u8], b: &[u8]) -> Result<Option<f64>> {
    const FUNC: &str = "ST_3DMaxDistance";
    let Some((pa, pb)) = both(a, b, FUNC)? else {
        return crate::functions::linear::st_max_distance(a, b);
    };
    Ok(farthest(&pa, &pb).map(|w| w.d))
}

/// The farthest pair of *vertices*, which is what PostGIS measures.
///
/// The prefilter inverts here: a vertex is skipped when even the far corner of
/// the other operand's box is no farther than the best seen. Ties are again
/// prunable because the incumbent wins them, and again nothing is reordered —
/// `ST_3DLongestLine` returns the winning pair as geometry. The box is the
/// whole of the other operand rather than one per primitive: this loop is
/// vertex-to-vertex, so there are no primitives to box, and a single box is
/// what keeps the guard to one test per vertex.
fn farthest(a: &Parts, b: &Parts) -> Option<Witness> {
    farthest_inner(a, b, true)
}

fn farthest_inner(a: &Parts, b: &Parts, prefilter: bool) -> Option<Witness> {
    let b_all = Aabb::of(b.vertices())?;
    let mut best: Option<Witness> = None;
    for p in a.vertices() {
        if prefilter
            && best.is_some_and(|w| Aabb::point(p).max_dist_sq(&b_all) * (1.0 + SLACK) <= w.d * w.d)
        {
            continue;
        }
        for q in b.vertices() {
            let w = Witness::between(p, q);
            best = Some(match best {
                None => w,
                Some(seen) if w.d > seen.d => w,
                Some(seen) => seen,
            });
        }
    }
    best
}

/// `ST_3DDFullyWithin(a, b, d)` — every part within `d`, i.e. the maximum
/// distance is at most `d`.
pub fn st_3d_dfully_within(a: &[u8], b: &[u8], d: f64) -> Result<bool> {
    if d < 0.0 {
        return Err(Error::Unsupported {
            func: "ST_3DDFullyWithin",
            reason: "tolerance cannot be less than zero".into(),
        });
    }
    Ok(st_3d_max_distance(a, b)?.is_some_and(|max| max <= d))
}

/// `ST_3DClosestPoint(a, b)` — the point on `a` closest to `b`, in 3D.
pub fn st_3d_closest_point(a: &[u8], b: &[u8]) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_3DClosestPoint";
    let Some((pa, pb)) = both(a, b, FUNC)? else {
        return crate::functions::measures::st_closest_point(a, b);
    };
    let Some(w) = closest(&pa, &pb, false) else {
        return Ok(None);
    };
    point_z(w.a, geom::srid_of(a)?, FUNC).map(Some)
}

/// `ST_3DShortestLine(a, b)` — the segment realising [`st_3d_distance`].
pub fn st_3d_shortest_line(a: &[u8], b: &[u8]) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_3DShortestLine";
    let Some((pa, pb)) = both(a, b, FUNC)? else {
        return crate::functions::linear::st_shortest_line(a, b);
    };
    let Some(w) = closest(&pa, &pb, false) else {
        return Ok(None);
    };
    line_z(w.a, w.b, geom::srid_of(a)?, FUNC).map(Some)
}

/// `ST_3DLongestLine(a, b)` — the vertex pair realising [`st_3d_max_distance`].
pub fn st_3d_longest_line(a: &[u8], b: &[u8]) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_3DLongestLine";
    let Some((pa, pb)) = both(a, b, FUNC)? else {
        return crate::functions::linear::st_longest_line(a, b);
    };
    let Some(w) = farthest(&pa, &pb) else {
        return Ok(None);
    };
    line_z(w.a, w.b, geom::srid_of(a)?, FUNC).map(Some)
}

/// `ST_3DLineInterpolatePoint(line, fraction)` — a point at `fraction` of the
/// line's **3D** length.
///
/// The 2D `ST_LineInterpolatePoint` takes the fraction by 2D length; measured on
/// `LINESTRING Z (0 0 0,10 0 10,20 0 30)` at 0.5, the two disagree —
/// `POINT(11.837722339831622 0 13.675444679663242)` here against
/// `POINT(10 0 10)` there.
pub fn st_3d_line_interpolate_point(bytes: &[u8], fraction: f64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_3DLineInterpolatePoint";
    if !(0.0..=1.0).contains(&fraction) {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "fraction must be between 0 and 1".into(),
        });
    }
    let p = parts(bytes, FUNC)?;
    if !p.points.is_empty() || !p.faces.is_empty() || p.segments.is_empty() {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "the first argument must be a LINESTRING".into(),
        });
    }
    let total: f64 = p.segments.iter().map(|(a, b)| norm(sub(*b, *a))).sum();
    if total == 0.0 {
        return point_z(p.segments[0].0, geom::srid_of(bytes)?, FUNC);
    }
    let target = fraction * total;
    let mut walked = 0.0;
    for (a, b) in &p.segments {
        let len = norm(sub(*b, *a));
        if walked + len >= target || (a, b) == p.segments.last().map(|(a, b)| (a, b)).unwrap() {
            let t = if len == 0.0 {
                0.0
            } else {
                ((target - walked) / len).clamp(0.0, 1.0)
            };
            return point_z(lerp(*a, *b, t), geom::srid_of(bytes)?, FUNC);
        }
        walked += len;
    }
    unreachable!("the loop always returns on its last iteration")
}

fn point_z(p: [f64; 3], srid: i32, func: &'static str) -> Result<Vec<u8>> {
    let index = coords::ZIndex::at(p[0], p[1], p[2]);
    let g = geo_types::Geometry::Point(geo_types::Point::new(p[0], p[1]));
    let wkb = coords::write_wkb_z(&g, &index, func)?;
    Ok(crate::gpb::write_gpb(&wkb, srid, None, false))
}

fn line_z(a: [f64; 3], b: [f64; 3], srid: i32, func: &'static str) -> Result<Vec<u8>> {
    // Hand-built rather than routed through `write_wkb_z`: the two ends can
    // share an (x, y) — a vertical shortest line does — and a coordinate-keyed
    // index would call that ambiguous, which it is not here.
    let mut wkb = vec![0x01u8];
    wkb.extend_from_slice(&1002u32.to_le_bytes());
    wkb.extend_from_slice(&2u32.to_le_bytes());
    for p in [a, b] {
        for o in p {
            wkb.extend_from_slice(&o.to_le_bytes());
        }
    }
    let _ = func;
    Ok(crate::gpb::write_gpb(&wkb, srid, None, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(ty: u32, counts: &[usize], coords: &[[f64; 3]]) -> Vec<u8> {
        let mut v = vec![0x01u8];
        v.extend_from_slice(&(1000 + ty).to_le_bytes());
        for c in counts {
            v.extend_from_slice(&(*c as u32).to_le_bytes());
        }
        for c in coords {
            for o in c {
                v.extend_from_slice(&o.to_le_bytes());
            }
        }
        v
    }
    fn pt(x: f64, y: f64, z: f64) -> Vec<u8> {
        blob(1, &[], &[[x, y, z]])
    }
    fn line(cs: &[[f64; 3]]) -> Vec<u8> {
        blob(2, &[cs.len()], cs)
    }
    fn poly(cs: &[[f64; 3]]) -> Vec<u8> {
        blob(3, &[1, cs.len()], cs)
    }
    fn square() -> Vec<u8> {
        poly(&[
            [0., 0., 0.],
            [10., 0., 0.],
            [10., 10., 0.],
            [0., 10., 0.],
            [0., 0., 0.],
        ])
    }
    fn near(got: Option<f64>, want: f64, what: &str) {
        let g = got.unwrap_or(f64::NAN);
        assert!((g - want).abs() < 1e-9, "{what}: got {g}, want {want}");
    }

    /// Every value here was measured on PostGIS 3.5.
    #[test]
    fn distances_match_the_reference() {
        near(
            st_3d_distance(&pt(0., 0., 0.), &pt(1., 1., 1.)).unwrap(),
            3f64.sqrt(),
            "pt/pt",
        );
        near(
            st_3d_distance(&pt(0., 0., 10.), &line(&[[0., 0., 0.], [10., 0., 0.]])).unwrap(),
            10.0,
            "pt/line",
        );
        // Skew segments: 4 apart in z where they cross in plan.
        near(
            st_3d_distance(
                &line(&[[0., 0., 0.], [10., 0., 0.]]),
                &line(&[[5., -5., 4.], [5., 5., 4.]]),
            )
            .unwrap(),
            4.0,
            "line/line skew",
        );
        // A filled face: the point is above its interior, so 10 — not the
        // 11.18 the boundary alone would give.
        near(
            st_3d_distance(&pt(5., 5., 10.), &square()).unwrap(),
            10.0,
            "pt/face interior",
        );
        near(
            st_3d_distance(
                &poly(&[[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 0., 0.]]),
                &poly(&[[0., 0., 5.], [1., 0., 5.], [1., 1., 5.], [0., 0., 5.]]),
            )
            .unwrap(),
            5.0,
            "face/face parallel",
        );
    }

    /// The measurement that keeps a solid model out: a POLYHEDRALSURFACE is a
    /// set of faces, so its interior is not inside anything.
    #[test]
    fn a_closed_shell_has_no_interior() {
        // The unit cube from `functions::surface`'s fixtures.
        let cube = crate::functions::surface::fixtures::cube(6);
        assert!(
            !st_3d_intersects(&cube, &pt(0.5, 0.5, 0.5)).unwrap(),
            "a point at the centre of a closed cube must not intersect it"
        );
        // A point on a face does.
        assert!(st_3d_intersects(&cube, &pt(0.5, 0.5, 0.0)).unwrap());
        // And distance to the shell is the distance to the nearest face.
        near(
            st_3d_distance(&cube, &pt(0.5, 0.5, 3.0)).unwrap(),
            2.0,
            "cube/pt above",
        );
    }

    /// 3D beats 2D exactly where it should: crossing in plan, apart in height.
    #[test]
    fn the_third_dimension_actually_separates() {
        let (a, b) = (
            line(&[[0., 0., 0.], [10., 0., 0.]]),
            line(&[[5., -5., 4.], [5., 5., 4.]]),
        );
        assert!(!st_3d_intersects(&a, &b).unwrap());
        // The 2D predicate says they cross, which is why the 3D one exists.
        assert!(crate::functions::predicates::st_intersects(&a, &b).unwrap());
        assert!(st_3d_dwithin(&a, &b, 4.0).unwrap());
        assert!(!st_3d_dwithin(&a, &b, 3.9).unwrap());
    }

    #[test]
    fn max_distance_is_vertex_to_vertex() {
        // Measured: a unit square face against its own corner gives sqrt(200).
        near(
            st_3d_max_distance(&square(), &pt(0., 0., 0.)).unwrap(),
            200f64.sqrt(),
            "maxdist",
        );
        assert!(st_3d_dfully_within(&square(), &pt(0., 0., 0.), 14.15).unwrap());
        assert!(!st_3d_dfully_within(&square(), &pt(0., 0., 0.), 14.14).unwrap());
    }

    #[test]
    fn the_witnesses_are_the_measured_ones() {
        use crate::functions::{rtree, threed};
        // ST_3DShortestLine(...) -> LINESTRING(5 0 0,5 0 4)
        let l = st_3d_shortest_line(
            &line(&[[0., 0., 0.], [10., 0., 0.]]),
            &line(&[[5., -5., 4.], [5., 5., 4.]]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(threed::st_zmin(&l).unwrap(), Some(0.0));
        assert_eq!(threed::st_zmax(&l).unwrap(), Some(4.0));
        assert_eq!(rtree::st_min_x(&l).unwrap(), Some(5.0));
        // ST_3DClosestPoint(line, POINT Z (5 0 9)) -> POINT(5 0 0)
        let p = st_3d_closest_point(&line(&[[0., 0., 0.], [10., 0., 0.]]), &pt(5., 0., 9.))
            .unwrap()
            .unwrap();
        assert_eq!(threed::st_z(&p).unwrap(), Some(0.0));
        assert_eq!(rtree::st_min_x(&p).unwrap(), Some(5.0));
    }

    /// 3D fractions, which is the whole difference from the 2D sibling.
    #[test]
    fn line_interpolate_takes_the_fraction_by_3d_length() {
        use crate::functions::{rtree, threed};
        let l = line(&[[0., 0., 0.], [10., 0., 10.], [20., 0., 30.]]);
        let p = st_3d_line_interpolate_point(&l, 0.5).unwrap();
        // Measured: POINT(11.837722339831622 0 13.675444679663242)
        near(
            rtree::st_min_x(&p).unwrap(),
            11.837_722_339_831_622,
            "3d lip x",
        );
        near(
            threed::st_z(&p).unwrap(),
            13.675_444_679_663_242,
            "3d lip z",
        );
    }

    /// A Z-less operand means "any height", which is the 2D answer.
    #[test]
    fn a_missing_z_delegates_to_the_2d_functions() {
        let flat = crate::functions::io::st_geom_from_text("POINT(0 0)", None).unwrap();
        near(
            st_3d_distance(&pt(0., 0., 10.), &flat).unwrap(),
            0.0,
            "z vs 2d",
        );
        let two = crate::functions::io::st_geom_from_text("POINT(3 4)", None).unwrap();
        near(st_3d_distance(&flat, &two).unwrap(), 5.0, "both 2d");
    }

    // ---- the AABB prefilter ----
    //
    // Every case here is stated the same way: the prefiltered sweep must return
    // the *same witness*, not merely the same distance — `ST_3DClosestPoint`
    // and `ST_3DShortestLine` hand the witness back as geometry.

    fn parts_of(b: &[u8]) -> Parts {
        parts(b, "test").unwrap()
    }

    /// Both sweeps, asserted identical down to the two witness points.
    fn agrees(a: &Parts, b: &Parts, what: &str) -> Option<Witness> {
        let naive = closest_inner(a, b, false, false);
        let filtered = closest_inner(a, b, false, true);
        match (naive, filtered) {
            (None, None) => None,
            (Some(n), Some(f)) => {
                assert_eq!(
                    (n.d, n.a, n.b),
                    (f.d, f.a, f.b),
                    "{what}: the prefilter changed the winning pair"
                );
                Some(f)
            }
            (n, f) => panic!("{what}: naive {n:?} against filtered {f:?}"),
        }
    }

    /// One box wholly inside another: no gap on any axis, so the bound is zero
    /// and the prefilter must prune nothing.
    #[test]
    fn nested_boxes_prune_nothing_and_agree() {
        let outer = crate::functions::surface::fixtures::cube(6);
        let inner = poly(&[
            [0.4, 0.4, 0.5],
            [0.6, 0.4, 0.5],
            [0.6, 0.6, 0.5],
            [0.4, 0.6, 0.5],
            [0.4, 0.4, 0.5],
        ]);
        let (po, pi) = (parts_of(&outer), parts_of(&inner));
        let w = agrees(&po, &pi, "nested").unwrap();
        // The small face floats at z = 0.5 inside the unit cube, so the nearest
        // shell face is 0.4 away in x.
        assert!((w.d - 0.4).abs() < 1e-9, "nested distance {}", w.d);
    }

    /// Touching boxes: the bound is exactly zero everywhere, which is the case
    /// a `>` prune would get right and a wrongly-signed one would not.
    #[test]
    fn touching_boxes_still_find_the_zero() {
        let a = square();
        let b = poly(&[
            [10., 0., 0.],
            [20., 0., 0.],
            [20., 10., 0.],
            [10., 10., 0.],
            [10., 0., 0.],
        ]);
        let (pa, pb) = (parts_of(&a), parts_of(&b));
        let w = agrees(&pa, &pb, "touching").unwrap();
        assert_eq!(w.d, 0.0);
        assert!(st_3d_intersects(&a, &b).unwrap(), "stop_at_zero path");
    }

    /// Two candidates at exactly the same distance. The first in sweep order
    /// wins, and the prefilter — which prunes ties — must not move it.
    #[test]
    fn a_tie_keeps_the_same_witness() {
        let p = pt(0., 0., 0.);
        // Segment one lies at y = 5, segment two at x = 5; both are 5 away.
        let l = line(&[[-5., 5., 0.], [5., 5., 0.], [5., -5., 0.]]);
        let (pa, pb) = (parts_of(&p), parts_of(&l));
        let w = agrees(&pa, &pb, "tie").unwrap();
        assert_eq!(w.d, 5.0);
        assert_eq!(w.b, [0., 5., 0.], "the first segment must keep the tie");
        // And through the public geometry-returning face of it.
        let cp = st_3d_closest_point(&l, &p).unwrap().unwrap();
        assert_eq!(crate::functions::rtree::st_min_x(&cp).unwrap(), Some(0.0));
        let sl = st_3d_shortest_line(&p, &l).unwrap().unwrap();
        assert_eq!(crate::functions::rtree::st_max_y(&sl).unwrap(), Some(5.0));
    }

    /// The max path, where the bound inverts.
    #[test]
    fn the_max_path_agrees_too() {
        let a = crate::functions::surface::fixtures::cube(6);
        let b = line(&[[7., 7., 7.], [9., 1., 3.], [2., 8., 6.]]);
        let (pa, pb) = (parts_of(&a), parts_of(&b));
        let (naive, filtered) = (
            farthest_inner(&pa, &pb, false),
            farthest_inner(&pa, &pb, true),
        );
        let (n, f) = (naive.unwrap(), filtered.unwrap());
        assert_eq!((n.d, n.a, n.b), (f.d, f.a, f.b), "max witness moved");
        // A tie on the max path: a square against a point equidistant from two
        // of its corners.
        let (sq, p) = (parts_of(&square()), parts_of(&pt(5., 5., 0.)));
        let (n, f) = (
            farthest_inner(&sq, &p, false).unwrap(),
            farthest_inner(&sq, &p, true).unwrap(),
        );
        assert_eq!((n.d, n.a, n.b), (f.d, f.a, f.b), "max tie moved");
    }

    /// A grid of quads, `n` by `n`, offset by `(dx, dy, dz)`.
    fn grid(n: usize, dx: f64, dy: f64, dz: f64) -> Parts {
        let mut p = Parts::default();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f64 + dx, j as f64 + dy);
                p.faces.push(vec![
                    [x, y, dz],
                    [x + 1., y, dz],
                    [x + 1., y + 1., dz],
                    [x, y + 1., dz],
                    [x, y, dz],
                ]);
            }
        }
        p
    }

    /// The reason the prefilter exists: two meshes whose cross product is the
    /// whole cost. Timed rather than asserted tightly — the assertion is
    /// generous on purpose, and the agreement is the part that must hold.
    #[test]
    fn two_meshes_are_much_cheaper_with_the_prefilter() {
        let (a, b) = (grid(14, 0., 0., 0.), grid(14, 40., 40., 10.));
        let t0 = std::time::Instant::now();
        let naive = closest_inner(&a, &b, false, false).unwrap();
        let naive_ms = t0.elapsed();
        let t1 = std::time::Instant::now();
        let filtered = closest_inner(&a, &b, false, true).unwrap();
        let filtered_ms = t1.elapsed();
        assert_eq!(
            (naive.d, naive.a, naive.b),
            (filtered.d, filtered.a, filtered.b),
            "the mesh answer moved"
        );
        println!("closest: naive {naive_ms:?}, prefiltered {filtered_ms:?}");
        assert!(
            filtered_ms <= naive_ms,
            "the prefilter should not cost more: naive {naive_ms:?}, filtered {filtered_ms:?}"
        );
    }

    #[test]
    fn a_vertical_wall_is_not_ambiguous_here() {
        // Two vertices at one (x, y): the ZIndex would call this ambiguous, and
        // the encoding reader must not.
        let wall = poly(&[
            [0., 0., 0.],
            [0., 0., 10.],
            [10., 0., 10.],
            [10., 0., 0.],
            [0., 0., 0.],
        ]);
        near(
            st_3d_distance(&wall, &pt(0., 5., 5.)).unwrap(),
            5.0,
            "wall/pt",
        );
    }
}
