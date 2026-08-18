//! The two SFCGAL measurements that are theorems rather than conventions:
//! the area of a surface in 3D, and the volume a closed shell encloses.
//!
//! Everything else in SFCGAL's column — `ST_3DIntersection`, `ST_3DUnion`,
//! `ST_Extrude`, `ST_StraightSkeleton` — needs CGAL's exact predicates and a
//! topology model, and stays out of scope. These two need neither: one is a
//! sum of cross products, the other is the divergence theorem.
//!
//! **Why one of them is not called `ST_Volume`.** Measured against
//! `postgis_sfcgal` 1.3.8 in the reference image, on a closed box of
//! 3.3 × 1.7 × 3.6 (`tests/golden/threed_sfcgal.jsonl`):
//!
//! | input | `ST_3DArea` | `ST_Volume` |
//! |---|---|---|
//! | the box as POLYHEDRALSURFACE | 47.22 | **0** |
//! | the box as TIN | 47.22 | **0** |
//! | `ST_MakeSolid` of either | **0** | 20.196 |
//!
//! A surface encloses nothing, so SFCGAL's `ST_Volume` answers 0 for one; the
//! volume appears only once the same coordinates are a SOLID. kenro has no
//! SOLID type ([docs/scope.md]), so a kenro `ST_Volume` returning 20.196 for a
//! POLYHEDRALSURFACE would be a silently different result under a shared name.
//! Hence [`kenro_volume`], which wears its own name and reproduces the
//! `ST_MakeSolid` column — sign included: SFCGAL's solid volume is **signed**
//! by shell orientation, and reversing every face turns 20.196 into −20.196
//! (measured, both on the box and on an irregular tetrahedron).
//!
//! `ST_3DArea` has no such problem and wears the PostGIS name.
//!
//! Two places kenro is deliberately *less* strict than SFCGAL, both recorded as
//! divergence vectors. SFCGAL validates a surface before measuring it and
//! refuses on one flipped ring or a non-planar patch, `ST_3DArea` included;
//! kenro measures what it was given, because a face's area does not depend on
//! which way its ring runs. There is no input where both produce a number and
//! the numbers differ.

use std::collections::HashMap;

use crate::coords;
use crate::error::{Error, Result};
use crate::functions::surface;

/// Twice the vector area of a ring: `Σ vᵢ × vᵢ₊₁` (Newell's method).
///
/// Exact for any planar ring, convex or not — measured: SFCGAL answers 3 for an
/// L-shaped hexagon, which a fan of triangle magnitudes would overcount. For a
/// non-planar ring it is the area of the best-fit planar projection; SFCGAL
/// refuses those outright ("points don't lie in the same plane").
fn double_area_vector(ring: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0f64; 3];
    for w in ring.windows(2) {
        let (a, b) = (w[0], w[1]);
        n[0] += a[1] * b[2] - a[2] * b[1];
        n[1] += a[2] * b[0] - a[0] * b[2];
        n[2] += a[0] * b[1] - a[1] * b[0];
    }
    n
}

fn norm(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

// ---- SQL functions ----

/// `ST_3DArea(geom)` — the area of a geometry's faces measured in 3D.
///
/// Non-areal parts contribute 0, as they do in SFCGAL: a POINT, a LINESTRING
/// and an empty polygon all answer 0 rather than NULL.
///
/// Holes subtract. Rings arrive from the encoding with their ordinal, so a
/// polygon's vector areas are summed *before* the magnitude is taken — an
/// interior ring is wound against its shell, so the subtraction is the sum. A
/// 2D polygon is simply one whose Z is everywhere absent: PostGIS answers the
/// planar area for it (8.84 for a 3.4 × 2.6 rectangle, the same as `ST_Area`),
/// and so does this.
pub fn st_3d_area(bytes: &[u8]) -> Result<f64> {
    let mut total = 0.0f64;
    // The vector area of the polygon currently being read, shell and holes.
    let mut acc = [0.0f64; 3];
    let mut open = false;
    let mut ring: Vec<[f64; 3]> = Vec::new();

    let close_ring = |ring: &mut Vec<[f64; 3]>, acc: &mut [f64; 3]| {
        let v = double_area_vector(ring);
        for k in 0..3 {
            acc[k] += v[k];
        }
        ring.clear();
    };

    coords::for_each_ring_run(bytes, &mut |c, first, base, ordinal| {
        let areal = base == coords::base::POLYGON || base == coords::base::TRIANGLE;
        if first {
            if open {
                close_ring(&mut ring, &mut acc);
            }
            // A shell — ordinal 0 — starts a new polygon, so the previous
            // one's magnitude is final. Anything non-areal ends one too.
            if open && (!areal || ordinal == 0) {
                total += norm(acc) / 2.0;
                acc = [0.0; 3];
            }
            open = areal;
        }
        if areal {
            ring.push([c.x, c.y, c.z.unwrap_or(0.0)]);
        }
    })?;
    if open {
        close_ring(&mut ring, &mut acc);
        total += norm(acc) / 2.0;
    }
    Ok(total)
}

/// `kenro_volume(geom)` — the volume enclosed by a closed polyhedral shell.
///
/// NULL for anything that is not a surface collection (POLYHEDRALSURFACE, TIN
/// or TRIANGLE), the same shape `ST_IsClosed` uses to say "not my kind of
/// value". 0 for an empty one.
///
/// **Gated, loudly.** The divergence theorem gives `Σ (1/6) a·(b × c)` over the
/// triangulated faces, and that number means something only when the faces
/// bound a region and all agree which side is out. Both conditions are one
/// test: in a closed, consistently oriented shell every **directed** edge
/// occurs exactly once, and its reverse exactly once. A missing partner is an
/// open shell; a directed edge seen twice is a flipped face. Either way the
/// answer is an `Unsupported` error naming the gap, never a number.
///
/// That is stricter than [`surface::is_closed`], which counts edges
/// undirected — a shell with one face reversed is `ST_IsClosed = true` and is
/// refused here. SFCGAL refuses it too, before measuring anything at all.
///
/// **The sign is kept.** SFCGAL's `ST_Volume(ST_MakeSolid(…))` is signed by
/// shell orientation — outward-facing gives +20.196 for the reference box,
/// inward-facing −20.196 — and reproducing it means a caller can tell the two
/// apart. Wrap in `abs()` for a magnitude.
pub fn kenro_volume(bytes: &[u8]) -> Result<Option<f64>> {
    const FUNC: &str = "kenro_volume";
    let Some(s) = surface::surfaces(bytes)? else {
        return Ok(None);
    };
    if s.is_empty() {
        return Ok(Some(0.0));
    }

    let mut rings: Vec<Vec<[f64; 3]>> = Vec::with_capacity(s.len());
    for i in 0..s.len() {
        if let Some(r) = s.patch_3d(i)? {
            rings.push(r);
        }
    }

    // Directed edges, keyed on bit patterns so -0.0 and 0.0 agree and NaN
    // never matches — the same key `surface::is_closed` uses.
    let key = |c: [f64; 3]| {
        [
            (c[0] + 0.0).to_bits(),
            (c[1] + 0.0).to_bits(),
            (c[2] + 0.0).to_bits(),
        ]
    };
    let mut edges: HashMap<([u64; 3], [u64; 3]), usize> = HashMap::new();
    for ring in &rings {
        for w in ring.windows(2) {
            let (a, b) = (key(w[0]), key(w[1]));
            if a == b {
                continue; // a degenerate edge bounds nothing
            }
            *edges.entry((a, b)).or_insert(0) += 1;
        }
    }
    if edges.is_empty() {
        return Ok(Some(0.0));
    }
    for ((a, b), n) in &edges {
        if *n > 1 {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "the patches are not consistently oriented — an edge is traversed the \
                         same way by two of them, so there is no inside. Reverse the offending \
                         ring; SFCGAL refuses this input too"
                    .into(),
            });
        }
        if !edges.contains_key(&(*b, *a)) {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "not a closed shell — an edge belongs to only one patch, so the faces \
                         enclose nothing (ST_IsClosed says false). A volume here would be a \
                         convention, not a measurement"
                    .into(),
            });
        }
    }

    let mut six_v = 0.0f64;
    for ring in &rings {
        // Fan from the ring's first vertex, as everywhere else in the 3D
        // family. The sum is triangulation-independent for a closed shell.
        for i in 1..ring.len().saturating_sub(1) {
            let (a, b, c) = (ring[0], ring[i], ring[i + 1]);
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            six_v += a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2];
        }
    }
    Ok(Some(six_v / 6.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::surface::fixtures;

    /// WKB for a POLYHEDRALSURFACE Z from explicit rings.
    fn phs(patches: &[&[[f64; 3]]]) -> Vec<u8> {
        let mut v = vec![0x01u8];
        v.extend_from_slice(&1015u32.to_le_bytes());
        v.extend_from_slice(&(patches.len() as u32).to_le_bytes());
        for p in patches {
            v.push(0x01);
            v.extend_from_slice(&1003u32.to_le_bytes());
            v.extend_from_slice(&1u32.to_le_bytes());
            v.extend_from_slice(&(p.len() as u32).to_le_bytes());
            for c in *p {
                for o in c {
                    v.extend_from_slice(&o.to_le_bytes());
                }
            }
        }
        v
    }

    /// POLYGON Z with an optional hole.
    fn poly(rings: &[&[[f64; 3]]]) -> Vec<u8> {
        let mut v = vec![0x01u8];
        v.extend_from_slice(&1003u32.to_le_bytes());
        v.extend_from_slice(&(rings.len() as u32).to_le_bytes());
        for r in rings {
            v.extend_from_slice(&(r.len() as u32).to_le_bytes());
            for c in *r {
                for o in c {
                    v.extend_from_slice(&o.to_le_bytes());
                }
            }
        }
        v
    }

    /// The reference box, 3.3 × 1.7 × 3.6 at an irregular origin: area 47.22,
    /// volume 20.196, both by hand.
    fn box_faces() -> Vec<Vec<[f64; 3]>> {
        let (x0, x1, y0, y1, z0, z1) = (0.4, 3.7, 1.2, 2.9, 0.5, 4.1);
        let close = |mut r: Vec<[f64; 3]>| {
            r.push(r[0]);
            r
        };
        vec![
            close(vec![[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]]),
            close(vec![[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]),
            close(vec![[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]]),
            close(vec![[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]]),
            close(vec![[x1, y1, z0], [x0, y1, z0], [x0, y1, z1], [x1, y1, z1]]),
            close(vec![[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]]),
        ]
    }

    fn box_blob(faces: &[Vec<[f64; 3]>]) -> Vec<u8> {
        let refs: Vec<&[[f64; 3]]> = faces.iter().map(|f| f.as_slice()).collect();
        phs(&refs)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn box_area_and_volume_are_the_hand_derived_numbers() {
        let blob = box_blob(&box_faces());
        assert!(close(st_3d_area(&blob).unwrap(), 47.22));
        assert!(close(kenro_volume(&blob).unwrap().unwrap(), 20.196));
    }

    #[test]
    fn reversing_every_face_negates_the_volume_and_leaves_the_area() {
        let faces: Vec<Vec<[f64; 3]>> = box_faces()
            .into_iter()
            .map(|f| f.into_iter().rev().collect())
            .collect();
        let blob = box_blob(&faces);
        assert!(close(st_3d_area(&blob).unwrap(), 47.22));
        assert!(close(kenro_volume(&blob).unwrap().unwrap(), -20.196));
    }

    /// An irregular tetrahedron: no axis-aligned face, no round coordinate.
    /// SFCGAL measured 32.914111804468945 and −8.9155 for this winding.
    #[test]
    fn irregular_tetrahedron() {
        let (a, b, c, d) = (
            [0.3, 0.7, 1.1],
            [4.2, 0.9, 1.3],
            [1.4, 3.6, 2.7],
            [2.1, 1.5, 6.4],
        );
        let f = |x: [f64; 3], y: [f64; 3], z: [f64; 3]| vec![x, y, z, x];
        let faces = vec![f(a, b, c), f(a, c, d), f(a, d, b), f(b, d, c)];
        let blob = box_blob(&faces);
        assert!(close(st_3d_area(&blob).unwrap(), 32.914111804468945));
        // 1/6 |det| of the three edge vectors from `a`, by hand: 8.9155.
        assert!(close(kenro_volume(&blob).unwrap().unwrap(), -8.9155));
    }

    #[test]
    fn an_open_shell_is_refused_by_name() {
        let mut faces = box_faces();
        faces.remove(0); // the floor
        let err = kenro_volume(&box_blob(&faces)).unwrap_err().to_string();
        assert!(err.contains("not a closed shell"), "{err}");
        // The area of what is left is still measurable: 47.22 − 3.3 × 1.7.
        assert!(close(st_3d_area(&box_blob(&faces)).unwrap(), 41.61));
    }

    #[test]
    fn one_flipped_face_is_refused_even_though_st_isclosed_says_true() {
        let mut faces = box_faces();
        let last = faces.len() - 1;
        faces[last].reverse();
        let blob = box_blob(&faces);
        // Undirected edge counting cannot see the flip.
        assert_eq!(surface::is_closed(&blob).unwrap(), Some(true));
        let err = kenro_volume(&blob).unwrap_err().to_string();
        assert!(err.contains("not consistently oriented"), "{err}");
        // ST_3DArea does not care which way a ring runs.
        assert!(close(st_3d_area(&blob).unwrap(), 47.22));
    }

    #[test]
    fn holes_subtract_and_non_convex_rings_are_exact() {
        let ring = |v: &[[f64; 3]]| v.to_vec();
        let shell = ring(&[
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [0.0, 4.0, 0.0],
            [0.0, 0.0, 0.0],
        ]);
        let hole = ring(&[
            [1.0, 1.0, 0.0],
            [1.0, 2.0, 0.0],
            [2.0, 2.0, 0.0],
            [2.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ]);
        let blob = poly(&[&shell, &hole]);
        assert!(close(st_3d_area(&blob).unwrap(), 15.0)); // 16 − 1, as SFCGAL

        // An L-shaped hexagon: 3, which a fan of triangle magnitudes misses.
        let l = ring(&[
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 0.0],
        ]);
        assert!(close(st_3d_area(&poly(&[&l])).unwrap(), 3.0));
    }

    #[test]
    fn a_tilted_face_is_larger_than_its_plan() {
        // 3 across and 4 up over 2.6 deep: a 5 × 2.6 slope.
        let r = [
            [0.0, 0.0, 0.0],
            [3.0, 0.0, 4.0],
            [3.0, 2.6, 4.0],
            [0.0, 2.6, 0.0],
            [0.0, 0.0, 0.0],
        ];
        assert!(close(st_3d_area(&poly(&[&r])).unwrap(), 13.0));
    }

    #[test]
    fn non_areal_and_empty_inputs_are_zero_not_null() {
        let mut pt = vec![0x01u8];
        pt.extend_from_slice(&1001u32.to_le_bytes());
        for o in [1.3f64, 2.7, 3.9] {
            pt.extend_from_slice(&o.to_le_bytes());
        }
        assert_eq!(st_3d_area(&pt).unwrap(), 0.0);
        // A non-surface value has no shell, so no volume to speak of.
        assert_eq!(kenro_volume(&pt).unwrap(), None);

        let mut empty = vec![0x01u8];
        empty.extend_from_slice(&1003u32.to_le_bytes());
        empty.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(st_3d_area(&empty).unwrap(), 0.0);
    }

    #[test]
    fn the_unit_cube_fixture_agrees_with_both() {
        let cube = fixtures::cube(6);
        assert!(close(st_3d_area(&cube).unwrap(), 6.0));
        let v = kenro_volume(&cube).unwrap().unwrap();
        assert!(close(v.abs(), 1.0), "{v}");
    }
}
