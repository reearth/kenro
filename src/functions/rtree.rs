//! The function set required by the GeoPackage R-tree spatial index triggers
//! (spec Annex F.3): `ST_IsEmpty`, `ST_MinX`, `ST_MaxX`, `ST_MinY`,
//! `ST_MaxY`. Contract: NULL on NULL input (handled in the binding layer);
//! min/max return NULL for empty geometries; `ST_IsEmpty` returns 1/0.
//!
//! Order of attack per call: header envelope present → answer from the
//! header without parsing WKB (the fast path the triggers hit on well-formed
//! gpkg data); empty flag set → NULL / 1; otherwise decode the WKB payload
//! and compute the bounding rect.

use crate::error::Result;
use crate::geom;
use crate::gpb::{self, Envelope, GpbHeader};

pub fn st_min_x(blob: &[u8]) -> Result<Option<f64>> {
    Ok(envelope_of(blob)?.map(|e| e.min_x))
}

pub fn st_max_x(blob: &[u8]) -> Result<Option<f64>> {
    Ok(envelope_of(blob)?.map(|e| e.max_x))
}

pub fn st_min_y(blob: &[u8]) -> Result<Option<f64>> {
    Ok(envelope_of(blob)?.map(|e| e.min_y))
}

pub fn st_max_y(blob: &[u8]) -> Result<Option<f64>> {
    Ok(envelope_of(blob)?.map(|e| e.max_y))
}

pub fn st_is_empty(blob: &[u8]) -> Result<bool> {
    if gpb::is_gpb(blob) {
        let header = GpbHeader::parse(blob)?;
        if header.empty {
            return Ok(true);
        }
        // A present envelope implies a non-empty geometry.
        if header.envelope.is_some() {
            return Ok(false);
        }
    }
    let g = geom::decode_auto(blob)?;
    Ok(geom::is_empty(&g.geometry))
}

fn envelope_of(blob: &[u8]) -> Result<Option<Envelope>> {
    if gpb::is_gpb(blob) {
        let header = GpbHeader::parse(blob)?;
        if header.empty {
            return Ok(None);
        }
        if let Some(env) = header.envelope {
            return Ok(Some(env));
        }
    }
    let g = geom::decode_auto(blob)?;
    Ok(geom::envelope(&g.geometry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_gpb, st_geom_from_text};

    #[test]
    fn header_envelope_fast_path_and_fallback_agree() {
        let canonical = st_geom_from_text("POLYGON((1 2,5 2,5 8,1 8,1 2))", None).unwrap();
        let stored = st_as_gpb(&canonical).unwrap();
        assert!(GpbHeader::parse(&canonical).unwrap().envelope.is_none());
        assert!(GpbHeader::parse(&stored).unwrap().envelope.is_some());
        for blob in [&canonical, &stored] {
            assert_eq!(st_min_x(blob).unwrap(), Some(1.0));
            assert_eq!(st_max_x(blob).unwrap(), Some(5.0));
            assert_eq!(st_min_y(blob).unwrap(), Some(2.0));
            assert_eq!(st_max_y(blob).unwrap(), Some(8.0));
            assert!(!st_is_empty(blob).unwrap());
        }
    }

    #[test]
    fn empty_geometry_contract() {
        let e = st_geom_from_text("MULTIPOLYGON EMPTY", None).unwrap();
        assert!(st_is_empty(&e).unwrap());
        assert_eq!(st_min_x(&e).unwrap(), None);
        assert_eq!(st_max_y(&e).unwrap(), None);
    }

    #[test]
    fn plain_wkb_accepted_leniently() {
        let wkb = crate::functions::io::st_as_binary(
            &st_geom_from_text("LINESTRING(3 4,7 9)", None).unwrap(),
        )
        .unwrap();
        assert_eq!(st_min_x(&wkb).unwrap(), Some(3.0));
        assert_eq!(st_max_y(&wkb).unwrap(), Some(9.0));
        assert!(!st_is_empty(&wkb).unwrap());
    }

    #[test]
    fn invalid_blob_is_an_error_not_null() {
        assert!(st_min_x(&[0xFF, 0xFE]).is_err());
        assert!(st_is_empty(&[0x47, 0x50, 0x00]).is_err()); // truncated GPB
    }
}
