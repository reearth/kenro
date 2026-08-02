//! PostGIS name compatibility: alternative spellings, typed constructors and
//! the EWKT/EWKB pair.
//!
//! Nothing here is a new algorithm — the point is that SQL written against
//! PostGIS keeps working. Most of the surface is manifest-level aliasing
//! (`ST_XMin` → the same code as `ST_MinX`); this module holds the few cases
//! that need real, if small, code.

use geo_types::Geometry;

use crate::error::{Error, Result};
use crate::geom::{self, Geom};
use crate::gpb;

/// `ST_Force2D(geom)` — drop Z/M.
///
/// kenro decodes 3D input (predicates and R-tree functions accept it) but
/// refuses to *encode* it rather than silently writing 2D. This is the
/// explicit opt-in to that flattening, and the only way to get a 3D
/// GeoPackage column through the rest of kenro.
pub fn st_force_2d(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut geom = geom::decode_auto(bytes)?;
    geom.has_zm = false; // the ordinates were already dropped on decode
    geom::encode_canonical_gpb(&geom, "ST_Force2D")
}

/// `ST_AsEWKT(geom)` — WKT with PostGIS's `SRID=n;` prefix, omitted when the
/// SRID is unknown (0), matching PostGIS exactly.
pub fn st_as_ewkt(bytes: &[u8]) -> Result<String> {
    let geom = geom::decode_auto(bytes)?;
    let wkt = geom::encode_wkt(&geom, "ST_AsEWKT")?;
    Ok(if geom.srid > 0 {
        format!("SRID={};{}", geom.srid, wkt)
    } else {
        wkt
    })
}

/// `ST_GeomFromEWKT(text)` — WKT with an optional `SRID=n;` prefix.
pub fn st_geom_from_ewkt(text: &str) -> Result<Vec<u8>> {
    let (srid, wkt) = split_ewkt(text)?;
    let geom = geom::decode_wkt(wkt, srid)?;
    geom::encode_canonical_gpb(&geom, "ST_GeomFromEWKT")
}

fn split_ewkt(text: &str) -> Result<(i32, &str)> {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed
        .strip_prefix("SRID=")
        .or_else(|| trimmed.strip_prefix("srid="))
    else {
        return Ok((0, text));
    };
    let Some((digits, wkt)) = rest.split_once(';') else {
        return Err(Error::InvalidWkt(
            "EWKT SRID prefix is missing its ';'".into(),
        ));
    };
    let srid = digits
        .trim()
        .parse::<i32>()
        .map_err(|_| Error::InvalidWkt(format!("EWKT SRID prefix {digits:?} is not an integer")))?;
    Ok((srid, wkt))
}

/// `ST_AsEWKB(geom)` — ISO WKB with PostGIS's high-bit SRID flag and the SRID
/// spliced in after the type word. A zero/unknown SRID emits plain WKB, as
/// PostGIS does.
pub fn st_as_ewkb(bytes: &[u8]) -> Result<Vec<u8>> {
    let geom = geom::decode_auto(bytes)?;
    let wkb = geom::encode_wkb(&geom, "ST_AsEWKB")?;
    if geom.srid <= 0 {
        return Ok(wkb);
    }
    if wkb.len() < 5 || wkb[0] != 0x01 {
        // encode_wkb is little-endian by construction; guard the assumption
        // rather than mangle bytes if that ever changes.
        return Err(Error::Unsupported {
            func: "ST_AsEWKB",
            reason: "expected little-endian WKB from the encoder".into(),
        });
    }
    let mut out = Vec::with_capacity(wkb.len() + 4);
    out.push(wkb[0]);
    let mut type_word = u32::from_le_bytes([wkb[1], wkb[2], wkb[3], wkb[4]]);
    type_word |= 0x2000_0000; // EWKB "has SRID"
    out.extend_from_slice(&type_word.to_le_bytes());
    out.extend_from_slice(&geom.srid.to_le_bytes());
    out.extend_from_slice(&wkb[5..]);
    Ok(out)
}

/// `ST_AsHexEWKB(geom)` — `ST_AsEWKB` in upper-case hex, as PostGIS renders it.
pub fn st_as_hex_ewkb(bytes: &[u8]) -> Result<String> {
    let ewkb = st_as_ewkb(bytes)?;
    let mut out = String::with_capacity(ewkb.len() * 2);
    for byte in ewkb {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02X}");
    }
    Ok(out)
}

/// The geometry types PostGIS's typed constructors accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Point,
    LineString,
    Polygon,
    MultiPoint,
    MultiLineString,
    MultiPolygon,
}

impl Expect {
    fn matches(self, g: &Geometry<f64>) -> bool {
        matches!(
            (self, g),
            (Expect::Point, Geometry::Point(_))
                | (Expect::LineString, Geometry::LineString(_))
                | (Expect::Polygon, Geometry::Polygon(_))
                | (Expect::MultiPoint, Geometry::MultiPoint(_))
                | (Expect::MultiLineString, Geometry::MultiLineString(_))
                | (Expect::MultiPolygon, Geometry::MultiPolygon(_))
        )
    }
}

/// `ST_PointFromText` & family — parse, then **return NULL when the geometry
/// is not of the expected type**. That NULL (rather than an error) is
/// PostGIS's documented behavior, verified against 3.5.
pub fn from_text_typed(wkt: &str, srid: Option<i32>, expect: Expect) -> Result<Option<Vec<u8>>> {
    let geom = geom::decode_wkt(wkt, srid.unwrap_or(0))?;
    typed(geom, expect, "ST_GeomFromText")
}

/// `ST_PointFromWKB` & family — the same contract over WKB/EWKB/GPB input.
pub fn from_wkb_typed(bytes: &[u8], srid: Option<i32>, expect: Expect) -> Result<Option<Vec<u8>>> {
    let mut geom = if gpb::is_gpb(bytes) {
        geom::decode_gpb(bytes)?.1
    } else {
        geom::decode_wkb(bytes, srid)?
    };
    if let Some(srid) = srid {
        geom.srid = srid;
    }
    typed(geom, expect, "ST_GeomFromWKB")
}

fn typed(geom: Geom, expect: Expect, func: &'static str) -> Result<Option<Vec<u8>>> {
    if !expect.matches(&geom.geometry) {
        return Ok(None);
    }
    geom::encode_canonical_gpb(&geom, func).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    #[test]
    fn ewkt_roundtrip_carries_srid_and_omits_zero() {
        let with = st_geom_from_ewkt("SRID=4326;POINT(1 2)").unwrap();
        assert_eq!(st_as_ewkt(&with).unwrap(), "SRID=4326;POINT(1 2)");
        let without = st_geom_from_ewkt("POINT(1 2)").unwrap();
        assert_eq!(st_as_ewkt(&without).unwrap(), "POINT(1 2)");
    }

    #[test]
    fn ewkt_rejects_a_malformed_prefix() {
        assert!(st_geom_from_ewkt("SRID=abc;POINT(1 2)").is_err());
        assert!(st_geom_from_ewkt("SRID=4326 POINT(1 2)").is_err());
    }

    #[test]
    fn hex_ewkb_matches_postgis_byte_for_byte() {
        // PostGIS 3.5: SELECT ST_AsHexEWKB(ST_GeomFromText('POINT(1 2)',4326))
        let g = st_geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        assert_eq!(
            st_as_hex_ewkb(&g).unwrap(),
            "0101000020E6100000000000000000F03F0000000000000040"
        );
        // Unknown SRID drops the flag, leaving plain WKB.
        let plain = st_geom_from_text("POINT(1 2)", None).unwrap();
        assert_eq!(
            st_as_hex_ewkb(&plain).unwrap(),
            "0101000000000000000000F03F0000000000000040"
        );
    }

    #[test]
    fn typed_constructors_return_null_on_a_type_mismatch() {
        assert!(
            from_text_typed("POINT(1 2)", None, Expect::Point)
                .unwrap()
                .is_some()
        );
        assert!(
            from_text_typed("LINESTRING(0 0,1 1)", None, Expect::Point)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn typed_constructors_keep_the_srid_argument() {
        let blob = from_text_typed("POINT(1 2)", Some(3857), Expect::Point)
            .unwrap()
            .unwrap();
        assert_eq!(crate::functions::io::st_srid(&blob).unwrap(), 3857);
    }

    #[test]
    fn force_2d_lets_a_3d_payload_through_the_encoders() {
        // ISO WKB POINT Z (1 2 3): every encoder refuses it until Force2D.
        let mut wkb = vec![0x01];
        wkb.extend_from_slice(&1001u32.to_le_bytes());
        for v in [1.0f64, 2.0, 3.0] {
            wkb.extend_from_slice(&v.to_le_bytes());
        }
        assert!(st_as_text(&wkb).is_err());
        let flat = st_force_2d(&wkb).unwrap();
        assert_eq!(st_as_text(&flat).unwrap(), "POINT(1 2)");
    }
}
