//! The function set required by the GeoPackage R-tree spatial index triggers
//! (spec Annex F.3): `ST_IsEmpty`, `ST_MinX`, `ST_MaxX`, `ST_MinY`,
//! `ST_MaxY`. Contract: NULL on NULL input (handled in the binding layer);
//! min/max return NULL for empty geometries; `ST_IsEmpty` returns 1/0.
//!
//! Order of attack per call: box text → answer from the six numbers (see
//! `functions::box3d`); header envelope present → answer from the header
//! without parsing WKB (the fast path the triggers hit on well-formed gpkg
//! data); empty flag set → NULL / 1; otherwise decode the WKB payload and
//! compute the bounding rect.
//!
//! The four min/max functions are `Kind::BlobOrText`, matching the only
//! overload PostGIS gives `ST_XMin` and friends — `box3d`, which a geometry
//! reaches through an implicit cast SQLite has no equivalent of. A geometry
//! encoding never begins with a printable character that is not the `GP`
//! magic, so the R-tree triggers' path through here is byte-for-byte the one
//! it always was.

use crate::error::Result;
use crate::functions::box3d;
use crate::geom;
use crate::gpb::{self, Envelope, GpbHeader};

pub fn st_min_x(blob: &[u8]) -> Result<Option<f64>> {
    if box3d::looks_like_text(blob) {
        return box3d::min_ordinate(blob, 0, "ST_MinX");
    }
    if let Some((minx, ..)) = crate::functions::surface::envelope(blob)? {
        return Ok(Some(minx));
    }
    Ok(envelope_of(blob)?.map(|e| e.min_x))
}

pub fn st_max_x(blob: &[u8]) -> Result<Option<f64>> {
    if box3d::looks_like_text(blob) {
        return box3d::max_ordinate(blob, 0, "ST_MaxX");
    }
    if let Some((_, _, maxx, _)) = crate::functions::surface::envelope(blob)? {
        return Ok(Some(maxx));
    }
    Ok(envelope_of(blob)?.map(|e| e.max_x))
}

pub fn st_min_y(blob: &[u8]) -> Result<Option<f64>> {
    if box3d::looks_like_text(blob) {
        return box3d::min_ordinate(blob, 1, "ST_MinY");
    }
    if let Some((_, miny, ..)) = crate::functions::surface::envelope(blob)? {
        return Ok(Some(miny));
    }
    Ok(envelope_of(blob)?.map(|e| e.min_y))
}

pub fn st_max_y(blob: &[u8]) -> Result<Option<f64>> {
    if box3d::looks_like_text(blob) {
        return box3d::max_ordinate(blob, 1, "ST_MaxY");
    }
    if let Some((.., maxy)) = crate::functions::surface::envelope(blob)? {
        return Ok(Some(maxy));
    }
    Ok(envelope_of(blob)?.map(|e| e.max_y))
}

pub fn st_is_empty(blob: &[u8]) -> Result<bool> {
    // The R-tree triggers call this on every row, so it must answer for a
    // surface column rather than raising.
    if let Some(n) = crate::functions::surface::st_num_patches(blob)? {
        return Ok(n == 0);
    }
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

/// `GPKG_IsAssignable(expected_type_name, actual_type_name)` — the function
/// the GeoPackage geometry-type-trigger extension requires. Both the
/// GeoPackage spellings (`POINT`, upper-case) and the PostGIS spellings
/// kenro's `ST_GeometryType` produces (`ST_Point`) are accepted, so the
/// spec's trigger DDL works verbatim against kenro's outputs. Assignability
/// follows the GeoPackage core geometry hierarchy; unknown names are only
/// assignable to themselves.
pub fn gpkg_is_assignable(expected: &str, actual: &str) -> Result<bool> {
    fn normalize(name: &str) -> String {
        let upper = name.trim().to_ascii_uppercase();
        upper.strip_prefix("ST_").unwrap_or(&upper).to_string()
    }
    fn parent(t: &str) -> Option<&'static str> {
        match t {
            "POINT" | "CURVE" | "SURFACE" | "GEOMETRYCOLLECTION" => Some("GEOMETRY"),
            "LINESTRING" | "CIRCULARSTRING" | "COMPOUNDCURVE" => Some("CURVE"),
            "POLYGON" | "CURVEPOLYGON" => Some("SURFACE"),
            "MULTIPOINT" | "MULTICURVE" | "MULTISURFACE" => Some("GEOMETRYCOLLECTION"),
            "MULTILINESTRING" => Some("MULTICURVE"),
            "MULTIPOLYGON" => Some("MULTISURFACE"),
            _ => None,
        }
    }
    let expected = normalize(expected);
    let mut current = Some(normalize(actual));
    while let Some(t) = current {
        if t == expected {
            return Ok(true);
        }
        current = parent(&t).map(str::to_string);
    }
    Ok(false)
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
    fn is_assignable_hierarchy_and_both_spellings() {
        let yes = |e: &str, a: &str| gpkg_is_assignable(e, a).unwrap();
        // Exact and hierarchy.
        assert!(yes("POINT", "POINT"));
        assert!(yes("GEOMETRY", "POINT"));
        assert!(yes("GEOMETRY", "MULTIPOLYGON"));
        assert!(yes("CURVE", "LINESTRING"));
        assert!(yes("SURFACE", "POLYGON"));
        assert!(yes("GEOMETRYCOLLECTION", "MULTIPOINT"));
        assert!(yes("GEOMETRYCOLLECTION", "MULTIPOLYGON")); // via MULTISURFACE
        assert!(!yes("POINT", "GEOMETRY")); // supertype is not assignable down
        assert!(!yes("POLYGON", "POINT"));
        // PostGIS-style spellings from kenro's ST_GeometryType.
        assert!(yes("POLYGON", "ST_Polygon"));
        assert!(yes("ST_Geometry", "st_multilinestring"));
        // Unknown names: only exact matches.
        assert!(yes("WIDGET", "widget"));
        assert!(!yes("WIDGET", "POINT"));
    }

    #[test]
    fn invalid_blob_is_an_error_not_null() {
        assert!(st_min_x(&[0xFF, 0xFE]).is_err());
        assert!(st_is_empty(&[0x47, 0x50, 0x00]).is_err()); // truncated GPB
    }
}
