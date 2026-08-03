//! Coordinate rewriting at the encoding level.
//!
//! `geo_types` has no room for Z, so everything that decodes through
//! [`crate::geom::decode_auto`] drops it and every encoder then refuses to
//! write the result (see `functions::threed`). That is the right default for
//! anything that *computes* — kenro's areas and predicates really are planar
//! — but it is wrong for the few functions that only move coordinates
//! around. PostGIS leaves Z alone under a 2D matrix and transforms it under a
//! 3D one, and either way the geometry's structure comes back unchanged.
//!
//! This module is that second path. It walks the WKB and rewrites each
//! coordinate where it lies, which buys three properties worth stating:
//!
//! - **dimensionality never changes.** A 2D geometry stays 2D (a 3D matrix's
//!   Z row is evaluated against `z = 0` and discarded, as PostGIS does); a 3D
//!   geometry keeps its Z. Nothing here can *add* an ordinate — that would
//!   change the type code and the byte length. Raising dimensionality is what
//!   `ST_Force3D` and `ST_MakePoint(x, y, z)` would need, and it is
//!   deliberately not possible here: a visitor that assigns a Z where the
//!   encoding has no slot for one is ignored.
//! - **M is never touched.** ISO dimension code 2 is XYM — three ordinates,
//!   none of them Z — so keying off the ordinate *count* would transform a
//!   measure as if it were a height. Measured on PostGIS 3.5:
//!   `ST_Affine(POINT M (1 2 99), 1,0,0, 0,1,0, 0,0,1, 10,20,30)` is
//!   `POINTM(11 22 99)`; the `zoff` does not reach the 99.
//! - **surface collections work.** POLYHEDRALSURFACE, TIN and TRIANGLE are
//!   nested WKB like any multi-geometry, so *moving a building* needs no
//!   geometry model at all. Measured: PostGIS transforms them too.
//!
//! What this is **not** is a second geometry model. It never holds a
//! geometry, only one coordinate at a time, and it cannot answer a single
//! question about shape. That is the line `tmp/3d-geometry-design.md` draws:
//! a caller that needs to know anything beyond "here is a coordinate, here is
//! its replacement" does not belong here, and wants the decoded 3D value that
//! design note defers.

use crate::error::{Error, Result};
use crate::gpb::{self, GpbHeader};

/// One coordinate, as the encoding holds it.
///
/// `z` is `None` when the geometry has no Z slot. A visitor may overwrite an
/// existing Z; assigning one where there is no slot is **ignored**, because
/// the encoding has nowhere to put it. That is what keeps this path from ever
/// raising dimensionality.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coord3 {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
}

/// Rewrite every coordinate of an encoded geometry, leaving its structure,
/// dimensionality and byte order exactly as they were.
///
/// Returns a canonical GeoPackage blob, like every other kenro function.
/// `ST_SetSRID` sets the precedent for how: the WKB payload is carried across
/// verbatim and re-wrapped, which is what lets a 3D or surface payload
/// survive a round trip that the 2D encoders would refuse.
pub fn map_coords(bytes: &[u8], f: &mut dyn FnMut(&mut Coord3)) -> Result<Vec<u8>> {
    if gpb::is_gpb(bytes) {
        let header = GpbHeader::parse(bytes)?;
        let mut wkb = bytes[header.wkb_offset..].to_vec();
        rewrite(&mut wkb, f)?;
        // Envelope dropped rather than recomputed: `write_gpb` emits the
        // canonical (envelope-free) form, which is what constructors already
        // return. `ST_AsGPB` is the way to a storage-grade blob.
        Ok(gpb::write_gpb(&wkb, header.srid, None, header.empty))
    } else {
        let mut wkb = bytes.to_vec();
        let scan = rewrite(&mut wkb, f)?;
        Ok(gpb::write_gpb(&wkb, scan.srid, None, scan.is_empty()))
    }
}

/// What the walk learned on the way through, for the fields a GPB header
/// needs that the payload alone does not spell out.
struct Scan {
    /// An EWKB-embedded SRID, or 0.
    srid: i32,
    coords: usize,
    /// A top-level point with NaN ordinates: the GeoPackage spec's own
    /// `POINT EMPTY`.
    nan_point: bool,
}

impl Scan {
    fn is_empty(&self) -> bool {
        self.coords == 0 || self.nan_point
    }
}

fn rewrite(buf: &mut [u8], f: &mut dyn FnMut(&mut Coord3)) -> Result<Scan> {
    let mut scan = Scan {
        srid: 0,
        coords: 0,
        nan_point: false,
    };
    let mut pos = 0usize;
    walk(buf, &mut pos, 0, f, &mut scan)?;
    Ok(scan)
}

fn walk(
    buf: &mut [u8],
    pos: &mut usize,
    depth: u8,
    f: &mut dyn FnMut(&mut Coord3),
    scan: &mut Scan,
) -> Result<()> {
    if depth > 32 {
        return Err(bad("geometry nesting too deep"));
    }
    need(buf.len(), *pos, 5)?;
    let le = match buf[*pos] {
        0 => false,
        1 => true,
        b => return Err(bad(&format!("invalid byte-order marker {b:#04x}"))),
    };
    let ty = rd_u32(buf, *pos + 1, le);
    *pos += 5;
    if ty & 0x2000_0000 != 0 {
        need(buf.len(), *pos, 4)?;
        if depth == 0 {
            scan.srid = rd_u32(buf, *pos, le) as i32;
        }
        *pos += 4; // EWKB SRID
    }
    // Dimensionality, from either convention. Z and M are tracked apart on
    // purpose: the ordinate count alone cannot tell XYZ from XYM.
    let (has_z, has_m) = match (ty & 0x0000_FFFF) / 1000 {
        1 => (true, false),
        2 => (false, true),
        3 => (true, true),
        _ => (ty & 0x8000_0000 != 0, ty & 0x4000_0000 != 0),
    };
    let dims = 2 + usize::from(has_z) + usize::from(has_m);
    let top_level = depth == 0;
    match (ty & 0x0000_FFFF) % 1000 {
        1 => run(buf, pos, 1, dims, has_z, le, f, scan, top_level)?,
        2 => {
            let n = count(buf, pos, le)?;
            run(buf, pos, n, dims, has_z, le, f, scan, false)?;
        }
        // Polygon and Triangle: rings, each a point run.
        3 | 17 => {
            let rings = count(buf, pos, le)?;
            for _ in 0..rings {
                let n = count(buf, pos, le)?;
                run(buf, pos, n, dims, has_z, le, f, scan, false)?;
            }
        }
        // Multi/collection, plus PolyhedralSurface (15) and TIN (16): a
        // count, then whole nested geometries carrying their own headers.
        4..=7 | 15 | 16 => {
            let n = count(buf, pos, le)?;
            for _ in 0..n {
                walk(buf, pos, depth + 1, f, scan)?;
            }
        }
        _ => return Err(bad("unknown WKB geometry type")),
    }
    Ok(())
}

/// One run of `n` coordinates: read, hand to the visitor, write back.
#[allow(clippy::too_many_arguments)]
fn run(
    buf: &mut [u8],
    pos: &mut usize,
    n: usize,
    dims: usize,
    has_z: bool,
    le: bool,
    f: &mut dyn FnMut(&mut Coord3),
    scan: &mut Scan,
    top_level: bool,
) -> Result<()> {
    let stride = 8 * dims;
    // Checked before the loop, so a hostile count is an error rather than a
    // long walk off the end (the guard `geom::validate_wkb` exists for).
    let total = n.checked_mul(stride).ok_or_else(|| bad("count overflow"))?;
    need(buf.len(), *pos, total)?;
    for _ in 0..n {
        let at = *pos;
        let mut c = Coord3 {
            x: rd_f64(buf, at, le),
            y: rd_f64(buf, at + 8, le),
            z: if has_z {
                Some(rd_f64(buf, at + 16, le))
            } else {
                None
            },
        };
        if top_level && c.x.is_nan() && c.y.is_nan() {
            scan.nan_point = true;
        }
        f(&mut c);
        wr_f64(buf, at, le, c.x);
        wr_f64(buf, at + 8, le, c.y);
        // Gated on the *encoding's* Z, not the visitor's: a visitor that
        // assigns `Some` to a coordinate with no Z slot is asking for a byte
        // that does not exist, and gets ignored rather than corrupting the
        // next ordinate along.
        if has_z && let Some(z) = c.z {
            wr_f64(buf, at + 16, le, z);
        }
        scan.coords += 1;
        *pos = at + stride;
    }
    Ok(())
}

fn count(buf: &[u8], pos: &mut usize, le: bool) -> Result<usize> {
    need(buf.len(), *pos, 4)?;
    let n = rd_u32(buf, *pos, le) as usize;
    *pos += 4;
    Ok(n)
}

fn need(len: usize, pos: usize, want: usize) -> Result<()> {
    if pos.checked_add(want).is_none_or(|end| end > len) {
        return Err(bad("truncated WKB (element count exceeds available bytes)"));
    }
    Ok(())
}

fn bad(msg: &str) -> Error {
    Error::InvalidWkb(msg.into())
}

fn rd_u32(buf: &[u8], at: usize, le: bool) -> u32 {
    let raw: [u8; 4] = buf[at..at + 4].try_into().expect("bounds checked");
    if le {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    }
}

fn rd_f64(buf: &[u8], at: usize, le: bool) -> f64 {
    let raw: [u8; 8] = buf[at..at + 8].try_into().expect("bounds checked");
    if le {
        f64::from_le_bytes(raw)
    } else {
        f64::from_be_bytes(raw)
    }
}

fn wr_f64(buf: &mut [u8], at: usize, le: bool, v: f64) {
    let raw = if le { v.to_le_bytes() } else { v.to_be_bytes() };
    buf[at..at + 8].copy_from_slice(&raw);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ISO WKB `POINT M (1 2 99)`: type 2001, three ordinates, no Z.
    fn point_m() -> Vec<u8> {
        let mut v = vec![0x01];
        v.extend_from_slice(&2001u32.to_le_bytes());
        for value in [1.0f64, 2.0, 99.0] {
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    }

    /// ISO WKB `POINT ZM (1 2 3 99)`: type 3001.
    fn point_zm() -> Vec<u8> {
        let mut v = vec![0x01];
        v.extend_from_slice(&3001u32.to_le_bytes());
        for value in [1.0f64, 2.0, 3.0, 99.0] {
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    }

    /// Big-endian ISO WKB `POINT Z (1 2 3)`, to prove byte order survives.
    fn point_z_be() -> Vec<u8> {
        let mut v = vec![0x00];
        v.extend_from_slice(&1001u32.to_be_bytes());
        for value in [1.0f64, 2.0, 3.0] {
            v.extend_from_slice(&value.to_be_bytes());
        }
        v
    }

    /// Shift every coordinate by (10, 20, 30) — the Z only if there is one.
    fn shift(c: &mut Coord3) {
        c.x += 10.0;
        c.y += 20.0;
        if let Some(z) = c.z.as_mut() {
            *z += 30.0;
        }
    }

    fn payload(blob: &[u8]) -> Vec<u8> {
        let h = GpbHeader::parse(blob).unwrap();
        blob[h.wkb_offset..].to_vec()
    }

    #[test]
    fn m_is_never_mistaken_for_z() {
        // The whole reason has_z is tracked apart from the ordinate count.
        let out = payload(&map_coords(&point_m(), &mut shift).unwrap());
        assert_eq!(out[..5], point_m()[..5], "type code must be unchanged");
        let read = |at: usize| f64::from_le_bytes(out[at..at + 8].try_into().unwrap());
        assert_eq!((read(5), read(13), read(21)), (11.0, 22.0, 99.0));
    }

    #[test]
    fn zm_transforms_the_z_and_leaves_the_m() {
        let out = payload(&map_coords(&point_zm(), &mut shift).unwrap());
        let read = |at: usize| f64::from_le_bytes(out[at..at + 8].try_into().unwrap());
        assert_eq!(
            (read(5), read(13), read(21), read(29)),
            (11.0, 22.0, 33.0, 99.0)
        );
    }

    #[test]
    fn big_endian_input_stays_big_endian() {
        let out = payload(&map_coords(&point_z_be(), &mut shift).unwrap());
        assert_eq!(out[0], 0x00, "byte-order marker must survive");
        let read = |at: usize| f64::from_be_bytes(out[at..at + 8].try_into().unwrap());
        assert_eq!((read(5), read(13), read(21)), (11.0, 22.0, 33.0));
    }

    #[test]
    fn a_2d_geometry_cannot_grow_a_z() {
        let flat = crate::functions::io::st_geom_from_text("POINT(1 2)", None).unwrap();
        // A visitor that tries to add Z is ignored: `z` is None and stays None.
        let out = map_coords(&flat, &mut |c| {
            c.x += 1.0;
            c.z = Some(999.0);
        })
        .unwrap();
        assert_eq!(
            crate::functions::io::st_as_text(&out).unwrap(),
            "POINT(2 2)"
        );
        assert!(!crate::functions::threed::st_has_z(&out).unwrap());
    }

    #[test]
    fn surface_collections_go_through_untouched_in_structure() {
        let cube = crate::functions::surface::fixtures::cube(6);
        let moved = map_coords(&cube, &mut shift).unwrap();
        // Still a closed six-patch shell, now somewhere else.
        assert_eq!(
            crate::functions::surface::st_num_patches(&moved).unwrap(),
            Some(6)
        );
        assert_eq!(
            crate::functions::surface::is_closed(&moved).unwrap(),
            Some(true)
        );
        assert_eq!(
            crate::functions::rtree::st_min_x(&moved).unwrap(),
            Some(10.0)
        );
        assert_eq!(
            crate::functions::threed::st_zmin(&moved).unwrap(),
            Some(30.0)
        );
    }

    #[test]
    fn nested_geometries_are_all_visited() {
        let g = crate::functions::io::st_geom_from_text(
            "GEOMETRYCOLLECTION(POINT(1 2),MULTIPOLYGON(((0 0,1 0,1 1,0 0))))",
            None,
        )
        .unwrap();
        let out = map_coords(&g, &mut shift).unwrap();
        assert_eq!(
            crate::functions::io::st_as_text(&out).unwrap(),
            "GEOMETRYCOLLECTION(POINT(11 22),MULTIPOLYGON(((10 20,11 20,11 21,10 20))))"
        );
    }

    #[test]
    fn the_empty_flag_survives_both_containers() {
        let empty = crate::functions::io::st_geom_from_text("LINESTRING EMPTY", None).unwrap();
        let out = map_coords(&empty, &mut shift).unwrap();
        assert!(GpbHeader::parse(&out).unwrap().empty);
        // POINT EMPTY arrives as NaN ordinates; raw-WKB input has no header
        // flag to copy, so the walk has to notice.
        let mut nan_wkb = vec![0x01, 0x01, 0x00, 0x00, 0x00];
        nan_wkb.extend_from_slice(&f64::NAN.to_le_bytes());
        nan_wkb.extend_from_slice(&f64::NAN.to_le_bytes());
        assert!(
            GpbHeader::parse(&map_coords(&nan_wkb, &mut shift).unwrap())
                .unwrap()
                .empty
        );
    }

    #[test]
    fn an_ewkb_srid_is_carried_over() {
        let mut ewkb = vec![0x01];
        ewkb.extend_from_slice(&(1u32 | 0x2000_0000).to_le_bytes());
        ewkb.extend_from_slice(&4326i32.to_le_bytes());
        ewkb.extend_from_slice(&1.0f64.to_le_bytes());
        ewkb.extend_from_slice(&2.0f64.to_le_bytes());
        let out = map_coords(&ewkb, &mut shift).unwrap();
        assert_eq!(GpbHeader::parse(&out).unwrap().srid, 4326);
    }

    #[test]
    fn hostile_and_truncated_input_errors_instead_of_walking_off() {
        // A LineString claiming 2^32-16 vertices in a 9-byte buffer.
        let mut wkb = vec![0x01, 0x02, 0x00, 0x00, 0x00];
        wkb.extend_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        assert!(map_coords(&wkb, &mut shift).is_err());
        // Every truncation of a real geometry.
        let full = crate::functions::surface::fixtures::cube(6);
        for cut in 1..full.len() {
            let _ = map_coords(&full[..cut], &mut shift);
        }
        // An unknown type code is named, not guessed at.
        let mut junk = vec![0x01];
        junk.extend_from_slice(&99u32.to_le_bytes());
        let err = map_coords(&junk, &mut shift).unwrap_err().to_string();
        assert!(err.contains("unknown WKB geometry type"), "{err}");
    }
}
