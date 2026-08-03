//! 3D pass-through: reporting and reading Z/M without computing in 3D.
//!
//! kenro's geometry type is `geo_types`, which has no room for Z, so decoding
//! drops the third ordinate and every encoder refuses a geometry that had one
//! (`ST_Force2D` is the explicit opt-in to flattening). That is the right
//! default — silently writing 2D where 3D went in would be data loss — but it
//! left kenro unable to say anything at all about 3D input.
//!
//! This module closes that gap without changing the geometry model:
//!
//! - **storage round-trips.** `ST_GeomFromGPB` and `ST_SetSRID` copy the WKB
//!   payload byte-for-byte, so a 3D column survives being read and written.
//!   (`ST_GeomFromWKB` does *not*: it re-encodes, and the encoders refuse 3D.
//!   A 3D column gets there the way it does in practice — written by GDAL or
//!   QGIS — and kenro reads it in place.)
//! - **dimensionality is reported honestly.** `ST_NDims`/`ST_CoordDim` answer
//!   3 or 4 for 3D/M input rather than a flat 2, and `ST_HasZ`/`ST_HasM` say
//!   which.
//! - **the ordinates are readable.** `ST_Z`, `ST_ZMin`, `ST_ZMax` walk the
//!   encoded coordinates directly, so a CityGML-style workflow can filter on
//!   height while every predicate stays planar.
//!
//! What this module is *not* is the whole 3D story — it is the reporting half.
//! `functions::surface` reads POLYHEDRALSURFACE, `coords` moves a Z through the
//! transforms, `geom::encode_derived` carries one across a derived geometry, and
//! `functions::threed_metric` has `ST_3DDistance` and the rest of the family
//! core PostGIS ships without SFCGAL. None of them needs a decoded 3D value,
//! which is still the thing kenro does not have.

use geozero::{GeomProcessor, GeozeroGeometry};

use crate::error::{Error, Result};
use crate::geom;
use crate::gpb;

/// Collects the Z and M ordinates as geozero walks the encoded geometry.
///
/// Reading them here rather than from `Geom` is the whole point: `Geom` has
/// already dropped them.
#[derive(Default)]
struct Ordinates {
    z: Vec<f64>,
    m: Vec<f64>,
    dims: geozero::CoordDimensions,
}

impl GeomProcessor for Ordinates {
    fn dimensions(&self) -> geozero::CoordDimensions {
        // Ask for everything the source has; geozero hands us what exists.
        geozero::CoordDimensions {
            z: true,
            m: true,
            t: false,
            tm: false,
        }
    }

    fn coordinate(
        &mut self,
        _x: f64,
        _y: f64,
        z: Option<f64>,
        m: Option<f64>,
        _t: Option<f64>,
        _tm: Option<u64>,
        _idx: usize,
    ) -> geozero::error::Result<()> {
        if let Some(z) = z {
            self.z.push(z);
            self.dims.z = true;
        }
        if let Some(m) = m {
            self.m.push(m);
            self.dims.m = true;
        }
        Ok(())
    }
}

/// Walk the WKB payload of any blob kenro accepts, collecting Z/M.
fn ordinates(bytes: &[u8], func: &'static str) -> Result<Ordinates> {
    let wkb: &[u8] = if gpb::is_gpb(bytes) {
        let header = gpb::GpbHeader::parse(bytes)?;
        &bytes[header.wkb_offset..]
    } else {
        bytes
    };
    let mut sink = Ordinates::default();
    geozero::wkb::Wkb(wkb.to_vec())
        .process_geom(&mut sink)
        .map_err(|e| Error::Unsupported {
            func,
            reason: format!("could not read the encoded coordinates: {e}"),
        })?;
    Ok(sink)
}

/// `ST_HasZ(geom)` / `ST_HasM(geom)` — does the *stored* geometry carry the
/// ordinate? Answered from the encoding, not from kenro's decoded value.
pub fn st_has_z(bytes: &[u8]) -> Result<bool> {
    if crate::functions::surface::z_extent(bytes)?.is_some() {
        return Ok(true);
    }
    Ok(!ordinates(bytes, "ST_HasZ")?.z.is_empty())
}

pub fn st_has_m(bytes: &[u8]) -> Result<bool> {
    if crate::geom::surface_kind(bytes).is_some() {
        return Ok(false); // kenro's surface reader carries Z only
    }
    Ok(!ordinates(bytes, "ST_HasM")?.m.is_empty())
}

/// `ST_NDims(geom)` / `ST_CoordDim(geom)` — 2, 3 or 4, honestly.
///
/// This replaces the earlier constant 2: kenro computes in 2D, but the value
/// it was handed may not be, and reporting otherwise was a small lie.
pub fn st_coord_dim(bytes: &[u8]) -> Result<i64> {
    if let Some(extent) = crate::functions::surface::z_extent(bytes)? {
        let _ = extent;
        return Ok(3);
    }
    let o = ordinates(bytes, "ST_CoordDim")?;
    Ok(2 + i64::from(!o.z.is_empty()) + i64::from(!o.m.is_empty()))
}

/// `ST_Z(point)` — the Z of a POINT, or NULL when it has none.
pub fn st_z(bytes: &[u8]) -> Result<Option<f64>> {
    single_ordinate(bytes, "ST_Z", |o| &o.z)
}

/// `ST_M(point)` — the M of a POINT, or NULL when it has none.
pub fn st_m(bytes: &[u8]) -> Result<Option<f64>> {
    single_ordinate(bytes, "ST_M", |o| &o.m)
}

fn single_ordinate(
    bytes: &[u8],
    func: &'static str,
    pick: impl Fn(&Ordinates) -> &Vec<f64>,
) -> Result<Option<f64>> {
    let g = geom::decode_auto(bytes)?;
    if !matches!(g.geometry, geo_types::Geometry::Point(_)) {
        return Err(Error::Unsupported {
            func,
            reason: "argument must be a POINT".into(),
        });
    }
    let o = ordinates(bytes, func)?;
    Ok(pick(&o).first().copied())
}

/// `ST_ZMin(geom)` / `ST_ZMax(geom)` — the Z extent.
///
/// A 2D geometry answers **0**, not NULL: PostGIS derives these from a
/// bounding box whose Z slot is zero when there is no Z, and a query like
/// `WHERE ST_ZMax(g) > 100` should behave the same on both. (`ST_Z` differs —
/// it is per-vertex, and NULL when the vertex has no Z. Verified live.)
/// An empty geometry, having no coordinates at all, is NULL.
pub fn st_zmin(bytes: &[u8]) -> Result<Option<f64>> {
    z_extent(bytes, "ST_ZMin", f64::min)
}

pub fn st_zmax(bytes: &[u8]) -> Result<Option<f64>> {
    z_extent(bytes, "ST_ZMax", f64::max)
}

fn z_extent(bytes: &[u8], func: &'static str, pick: fn(f64, f64) -> f64) -> Result<Option<f64>> {
    if let Some((lo, hi)) = crate::functions::surface::z_extent(bytes)? {
        return Ok(Some(pick(lo, hi)));
    }
    let g = geom::decode_auto(bytes)?;
    if geom::is_empty(&g.geometry) {
        return Ok(None);
    }
    let o = ordinates(bytes, func)?;
    Ok(Some(
        o.z.iter()
            .copied()
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| pick(a, v)))
            })
            .unwrap_or(0.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text, st_set_srid, st_srid};

    /// ISO WKB POINT Z (1 2 3).
    fn point_z() -> Vec<u8> {
        let mut v = vec![0x01];
        v.extend_from_slice(&1001u32.to_le_bytes());
        for value in [1.0f64, 2.0, 3.0] {
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    }

    /// ISO WKB LINESTRING Z (0 0 10, 1 1 30).
    fn line_z() -> Vec<u8> {
        let mut v = vec![0x01];
        v.extend_from_slice(&1002u32.to_le_bytes());
        v.extend_from_slice(&2u32.to_le_bytes());
        for value in [0.0f64, 0.0, 10.0, 1.0, 1.0, 30.0] {
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    }

    #[test]
    fn dimensionality_is_reported_from_the_encoding() {
        assert!(st_has_z(&point_z()).unwrap());
        assert!(!st_has_m(&point_z()).unwrap());
        assert_eq!(st_coord_dim(&point_z()).unwrap(), 3);
        let flat = st_geom_from_text("POINT(1 2)", None).unwrap();
        assert!(!st_has_z(&flat).unwrap());
        assert_eq!(st_coord_dim(&flat).unwrap(), 2);
    }

    #[test]
    fn z_ordinates_are_readable_even_though_the_geometry_is_2d() {
        assert_eq!(st_z(&point_z()).unwrap(), Some(3.0));
        assert_eq!(
            st_z(&st_geom_from_text("POINT(1 2)", None).unwrap()).unwrap(),
            None
        );
        assert_eq!(st_zmin(&line_z()).unwrap(), Some(10.0));
        assert_eq!(st_zmax(&line_z()).unwrap(), Some(30.0));
        // Non-point input has no single Z.
        assert!(st_z(&line_z()).is_err());
        // A 2D geometry answers 0, as PostGIS does (its bbox has a zero Z
        // slot) — unlike ST_Z, which is NULL for a vertex with no Z.
        let flat = st_geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        assert_eq!(st_zmax(&flat).unwrap(), Some(0.0));
        assert_eq!(
            st_zmax(&st_geom_from_text("LINESTRING EMPTY", None).unwrap()).unwrap(),
            None
        );
    }

    #[test]
    fn a_3d_payload_survives_storage_unchanged() {
        // The pass-through guarantee: relabelling the SRID must not flatten
        // the geometry, and the Z must still be there afterwards.
        let labelled = st_set_srid(&point_z(), 6697).unwrap();
        assert_eq!(st_srid(&labelled).unwrap(), 6697);
        assert_eq!(st_z(&labelled).unwrap(), Some(3.0));
        assert_eq!(st_coord_dim(&labelled).unwrap(), 3);
        // …and the encoders still refuse to write it as 2D by accident.
        assert!(st_as_text(&labelled).is_err());
        // Only ST_Force2D flattens, and then the Z really is gone.
        let flat = crate::functions::compat::st_force_2d(&labelled).unwrap();
        assert_eq!(st_as_text(&flat).unwrap(), "POINT(1 2)");
        assert_eq!(st_z(&flat).unwrap(), None);
    }

    #[test]
    fn planar_work_still_works_on_3d_input() {
        // The point of pass-through: predicates and the R-tree keep working.
        let p = point_z();
        assert_eq!(crate::functions::rtree::st_min_x(&p).unwrap(), Some(1.0));
        let window = st_geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap();
        assert!(crate::functions::predicates::st_intersects(&p, &window).unwrap());
    }
}
