//! The internal geometry hub: everything decodes into [`Geom`], all
//! algorithms run on it, all encoders consume it.

use geo_types::Geometry;
use geozero::wkb::{Ewkb, Wkb};
use geozero::{CoordDimensions, ToGeo, ToWkb, ToWkt};

use crate::error::{Error, Result};
use crate::gpb::{self, Envelope, GpbHeader};

#[derive(Debug, Clone, PartialEq)]
pub struct Geom {
    pub geometry: Geometry<f64>,
    /// EPSG code; 0 or negative means unknown/undefined (GeoPackage convention).
    pub srid: i32,
    /// True when the source encoding carried Z and/or M ordinates (which
    /// `geo_types` cannot represent — they are dropped on decode). Encoders
    /// refuse to serialize such a geometry rather than silently emit 2D.
    pub has_zm: bool,
}

/// Decode a geometry blob, auto-detecting GPB (magic "GP") vs WKB
/// (byte-order byte 0x00/0x01). The two prefixes are disjoint.
pub fn decode_auto(bytes: &[u8]) -> Result<Geom> {
    if gpb::is_gpb(bytes) {
        let (_, geom) = decode_gpb(bytes)?;
        Ok(geom)
    } else {
        decode_wkb(bytes, None)
    }
}

/// Decode ISO WKB or EWKB. An EWKB-embedded SRID populates `srid` unless
/// `srid_override` is given (PostGIS behavior for `ST_GeomFromWKB(wkb, srid)`).
pub fn decode_wkb(bytes: &[u8], srid_override: Option<i32>) -> Result<Geom> {
    let has_zm = wkb_has_zm(bytes)?;
    let geometry = wkb_to_geo(bytes)?;
    let srid = srid_override.or_else(|| ewkb_srid(bytes)).unwrap_or(0);
    Ok(Geom {
        geometry,
        srid,
        has_zm,
    })
}

/// Decode WKB bytes, dispatching on the type-code flag bits: EWKB dimension
/// or SRID flags select the EWKB reader, plain/ISO type codes (including the
/// ISO +1000/+2000/+3000 Z/M forms) select the ISO reader. The readers are
/// not interchangeable for 3D input.
fn wkb_to_geo(bytes: &[u8]) -> Result<Geometry<f64>> {
    if bytes.len() < 5 {
        return Err(Error::InvalidWkb("shorter than a 5-byte WKB header".into()));
    }
    let ty_bytes: [u8; 4] = bytes[1..5].try_into().expect("length checked");
    let ty = match bytes[0] {
        0 => u32::from_be_bytes(ty_bytes),
        1 => u32::from_le_bytes(ty_bytes),
        b => {
            return Err(Error::InvalidWkb(format!(
                "invalid byte-order marker {b:#04x}"
            )));
        }
    };
    // Hostile-input guard: element counts inside WKB must be backed by
    // actual bytes BEFORE geozero decodes (its readers pre-allocate from
    // the counts — a random 4-byte count can demand gigabytes and abort
    // the process, which is not an Err).
    validate_wkb(bytes)?;

    // POINT EMPTY arrives as a point with NaN coordinates (the GeoPackage
    // spec's own convention); geozero's geo-types writer refuses it, so
    // build it directly.
    if let Some(p) = parse_nan_point(bytes, ty) {
        return Ok(p);
    }
    let result = if ty & 0xE000_0000 != 0 {
        Ewkb(bytes).to_geo()
    } else {
        Wkb(bytes).to_geo()
    };
    result.map_err(|e| Error::InvalidWkb(e.to_string()))
}

/// Structural WKB validation: walks the geometry tree checking that every
/// element count is backed by enough remaining bytes, so downstream
/// decoding can never allocate more than the input's size. Handles ISO and
/// EWKB type codes (Z/M dims change the coordinate size; an EWKB SRID adds
/// 4 bytes).
fn validate_wkb(bytes: &[u8]) -> Result<()> {
    fn fail(msg: &str) -> Error {
        Error::InvalidWkb(msg.into())
    }
    fn walk(b: &[u8], pos: &mut usize, depth: u8) -> Result<()> {
        if depth > 32 {
            return Err(fail("geometry nesting too deep"));
        }
        let need = |pos: usize, n: usize| {
            if pos.checked_add(n).is_none_or(|end| end > b.len()) {
                Err(fail(
                    "truncated WKB (element count exceeds available bytes)",
                ))
            } else {
                Ok(())
            }
        };
        need(*pos, 5)?;
        let le = match b[*pos] {
            0 => false,
            1 => true,
            _ => return Err(fail("invalid byte-order marker")),
        };
        let read_u32 = |b: &[u8], at: usize| {
            let raw: [u8; 4] = b[at..at + 4].try_into().expect("bounds checked");
            if le {
                u32::from_le_bytes(raw)
            } else {
                u32::from_be_bytes(raw)
            }
        };
        let ty = read_u32(b, *pos + 1);
        *pos += 5;
        if ty & 0x2000_0000 != 0 {
            need(*pos, 4)?;
            *pos += 4; // EWKB SRID
        }
        let iso_dim_code = (ty & 0x0000_FFFF) / 1000;
        let mut dims = 2 + usize::from(ty & 0x8000_0000 != 0) + usize::from(ty & 0x4000_0000 != 0);
        dims = dims.max(match iso_dim_code {
            1 | 2 => 3,
            3 => 4,
            _ => 2,
        });
        let coord_size = 8 * dims;
        let base = (ty & 0x0000_FFFF) % 1000;
        match base {
            1 => {
                need(*pos, coord_size)?;
                *pos += coord_size;
            }
            2 => {
                need(*pos, 4)?;
                let n = read_u32(b, *pos) as usize;
                *pos += 4;
                let total = n
                    .checked_mul(coord_size)
                    .ok_or_else(|| fail("count overflow"))?;
                need(*pos, total)?;
                *pos += total;
            }
            3 => {
                need(*pos, 4)?;
                let rings = read_u32(b, *pos) as usize;
                *pos += 4;
                for _ in 0..rings {
                    need(*pos, 4)?;
                    let n = read_u32(b, *pos) as usize;
                    *pos += 4;
                    let total = n
                        .checked_mul(coord_size)
                        .ok_or_else(|| fail("count overflow"))?;
                    need(*pos, total)?;
                    *pos += total;
                }
            }
            4..=7 => {
                need(*pos, 4)?;
                let n = read_u32(b, *pos) as usize;
                *pos += 4;
                for _ in 0..n {
                    walk(b, pos, depth + 1)?;
                }
            }
            _ => return Err(fail("unknown WKB geometry type")),
        }
        Ok(())
    }
    let mut pos = 0;
    walk(bytes, &mut pos, 0)
}

fn parse_nan_point(bytes: &[u8], ty: u32) -> Option<Geometry<f64>> {
    if (ty & 0x0FFF_FFFF) % 1000 != 1 {
        return None; // not a point
    }
    let le = bytes[0] == 1;
    let offset = if ty & 0x2000_0000 != 0 { 9 } else { 5 }; // skip EWKB SRID
    let x_raw: [u8; 8] = bytes.get(offset..offset + 8)?.try_into().ok()?;
    let y_raw: [u8; 8] = bytes.get(offset + 8..offset + 16)?.try_into().ok()?;
    let (x, y) = if le {
        (f64::from_le_bytes(x_raw), f64::from_le_bytes(y_raw))
    } else {
        (f64::from_be_bytes(x_raw), f64::from_be_bytes(y_raw))
    };
    if x.is_nan() && y.is_nan() {
        Some(Geometry::Point(geo_types::Point::new(f64::NAN, f64::NAN)))
    } else {
        None
    }
}

/// Decode a full GPB blob, returning both the parsed header and the geometry.
pub fn decode_gpb(bytes: &[u8]) -> Result<(GpbHeader, Geom)> {
    let header = GpbHeader::parse(bytes)?;
    let payload = &bytes[header.wkb_offset..];
    let has_zm = wkb_has_zm(payload)?;
    let geometry = wkb_to_geo(payload)?;
    let geom = Geom {
        geometry,
        srid: header.srid,
        has_zm,
    };
    Ok((header, geom))
}

/// Decode WKT. Z/M input is rejected: constructors re-encode, and emitting 2D
/// for 3D input would be a silently-different result.
pub fn decode_wkt(wkt: &str, srid: i32) -> Result<Geom> {
    if wkt_has_zm(wkt) {
        return Err(Error::Unsupported {
            func: "ST_GeomFromText",
            reason: "3D/M geometries are not supported in kenro 0.1".into(),
        });
    }
    let geometry = geozero::wkt::Wkt(wkt).to_geo().map_err(|e| {
        let msg = e.to_string();
        if wkt.to_ascii_uppercase().contains("POINT") && wkt.to_ascii_uppercase().contains("EMPTY")
        {
            Error::InvalidWkt(format!(
                "{msg} (POINT EMPTY cannot be represented; kenro accepts empty \
                 line/polygon/multi geometries but not empty points from WKT)"
            ))
        } else {
            Error::InvalidWkt(msg)
        }
    })?;
    Ok(Geom {
        geometry,
        srid,
        has_zm: false,
    })
}

/// Encode as ISO WKB, little-endian (PostGIS `ST_AsBinary` default).
pub fn encode_wkb(geom: &Geom, func: &'static str) -> Result<Vec<u8>> {
    reject_zm(geom, func)?;
    // POINT EMPTY: geozero's writer refuses it; hand-write the NaN-coordinate
    // form (the GeoPackage spec's own representation).
    if let Geometry::Point(p) = &geom.geometry
        && p.x().is_nan()
        && p.y().is_nan()
    {
        let mut wkb = vec![0x01, 0x01, 0x00, 0x00, 0x00];
        wkb.extend_from_slice(&f64::NAN.to_le_bytes());
        wkb.extend_from_slice(&f64::NAN.to_le_bytes());
        return Ok(wkb);
    }
    geom.geometry
        .to_wkb(CoordDimensions::xy())
        .map_err(Error::Geozero)
}

/// Encode as WKT (PostGIS `ST_AsText` formatting is verified by golden tests).
pub fn encode_wkt(geom: &Geom, func: &'static str) -> Result<String> {
    reject_zm(geom, func)?;
    if is_empty(&geom.geometry) {
        return Ok(format!("{} EMPTY", wkt_type_name(&geom.geometry)));
    }
    let wkt = geom.geometry.to_wkt().map_err(Error::Geozero)?;
    // PostGIS (3.x) writes each member point parenthesized:
    // MULTIPOINT((1 2),(3 4)) — geozero writes the flat legacy form.
    if matches!(geom.geometry, Geometry::MultiPoint(_)) && !wkt.contains("((") {
        if let Some(inner) = wkt
            .strip_prefix("MULTIPOINT(")
            .and_then(|s| s.strip_suffix(")"))
        {
            let parts: Vec<String> = inner.split(',').map(|p| format!("({p})")).collect();
            return Ok(format!("MULTIPOINT({})", parts.join(",")));
        }
    }
    Ok(wkt)
}

/// Encode as the canonical GPB value format: little-endian header, no
/// envelope. Constructors return this.
pub fn encode_canonical_gpb(geom: &Geom, func: &'static str) -> Result<Vec<u8>> {
    let wkb = encode_wkb(geom, func)?;
    Ok(gpb::write_gpb(
        &wkb,
        geom.srid,
        None,
        is_empty(&geom.geometry),
    ))
}

/// Encode as storage-grade GPB: XY envelope for non-point, non-empty
/// geometries (points get envelope indicator 0, matching GDAL/QGIS practice).
pub fn encode_storage_gpb(geom: &Geom, func: &'static str) -> Result<Vec<u8>> {
    let wkb = encode_wkb(geom, func)?;
    let env = if matches!(geom.geometry, Geometry::Point(_)) {
        None
    } else {
        envelope(&geom.geometry)
    };
    Ok(gpb::write_gpb(
        &wkb,
        geom.srid,
        env,
        is_empty(&geom.geometry),
    ))
}

/// Empty test covering the `POINT EMPTY` NaN-coordinate convention, which
/// `geo::HasDimensions` cannot see (a `geo_types` point is never "empty").
pub fn is_empty(g: &Geometry<f64>) -> bool {
    use geo::HasDimensions;
    if let Geometry::Point(p) = g {
        return p.x().is_nan() && p.y().is_nan();
    }
    g.is_empty()
}

/// Bounding envelope; `None` for empty geometries.
pub fn envelope(g: &Geometry<f64>) -> Option<Envelope> {
    use geo::BoundingRect;
    if is_empty(g) {
        return None;
    }
    g.bounding_rect().map(|r| Envelope {
        min_x: r.min().x,
        max_x: r.max().x,
        min_y: r.min().y,
        max_y: r.max().y,
    })
}

pub fn wkt_type_name(g: &Geometry<f64>) -> &'static str {
    match g {
        Geometry::Point(_) => "POINT",
        Geometry::Line(_) | Geometry::LineString(_) => "LINESTRING",
        Geometry::Polygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => "POLYGON",
        Geometry::MultiPoint(_) => "MULTIPOINT",
        Geometry::MultiLineString(_) => "MULTILINESTRING",
        Geometry::MultiPolygon(_) => "MULTIPOLYGON",
        Geometry::GeometryCollection(_) => "GEOMETRYCOLLECTION",
    }
}

fn reject_zm(geom: &Geom, func: &'static str) -> Result<()> {
    if geom.has_zm {
        return Err(Error::Unsupported {
            func,
            reason: "3D/M output is not supported in kenro 0.1; predicates and \
                     R-tree functions accept 3D input"
                .into(),
        });
    }
    Ok(())
}

/// Inspect the WKB type code for Z/M ordinates: ISO (base + 1000/2000/3000)
/// and EWKB (flag bits 0x80000000 / 0x40000000). Only the top-level type is
/// inspected; WKB requires uniform dimensionality in practice.
fn wkb_has_zm(bytes: &[u8]) -> Result<bool> {
    if bytes.len() < 5 {
        return Err(Error::InvalidWkb("shorter than a 5-byte WKB header".into()));
    }
    let ty_bytes: [u8; 4] = bytes[1..5].try_into().expect("length checked");
    let ty = match bytes[0] {
        0 => u32::from_be_bytes(ty_bytes),
        1 => u32::from_le_bytes(ty_bytes),
        b => {
            return Err(Error::InvalidWkb(format!(
                "invalid byte-order marker {b:#04x}"
            )));
        }
    };
    if ty & 0xC000_0000 != 0 {
        return Ok(true); // EWKB Z/M flags
    }
    let iso_dim = (ty & 0x0000_FFFF) / 1000;
    Ok((1..=3).contains(&iso_dim))
}

/// Extract the SRID from an EWKB header, if the SRID flag is set.
fn ewkb_srid(bytes: &[u8]) -> Option<i32> {
    if bytes.len() < 9 {
        return None;
    }
    let le = match bytes[0] {
        0 => false,
        1 => true,
        _ => return None,
    };
    let ty_bytes: [u8; 4] = bytes[1..5].try_into().ok()?;
    let ty = if le {
        u32::from_le_bytes(ty_bytes)
    } else {
        u32::from_be_bytes(ty_bytes)
    };
    if ty & 0x2000_0000 == 0 {
        return None;
    }
    let srid_bytes: [u8; 4] = bytes[5..9].try_into().ok()?;
    Some(if le {
        i32::from_le_bytes(srid_bytes)
    } else {
        i32::from_be_bytes(srid_bytes)
    })
}

/// Heuristic Z/M detection for WKT: the dimension keyword (`Z`/`M`/`ZM`)
/// before the first paren, or more than two ordinates in the first
/// coordinate.
fn wkt_has_zm(wkt: &str) -> bool {
    let upper = wkt.trim().to_ascii_uppercase();
    let head = upper.split('(').next().unwrap_or("");
    for token in head.split_whitespace().skip(1) {
        if matches!(token, "Z" | "M" | "ZM") {
            return true;
        }
    }
    // Count numbers in the first coordinate: text between the innermost '('
    // run and the first ',' or ')'.
    let after_parens = upper.trim_start_matches(|c: char| c != '(');
    let inner = after_parens.trim_start_matches('(');
    let first_coord = inner.split([',', ')']).next().unwrap_or("");
    first_coord.split_whitespace().count() > 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::{Geometry, Point, line_string, polygon};

    fn point_geom(x: f64, y: f64) -> Geom {
        Geom {
            geometry: Geometry::Point(Point::new(x, y)),
            srid: 4326,
            has_zm: false,
        }
    }

    #[test]
    fn wkt_roundtrip_all_types() {
        let cases = [
            "POINT(1 2)",
            "LINESTRING(0 0,1 1,2 0)",
            "POLYGON((0 0,10 0,10 10,0 10,0 0))",
            "POLYGON((0 0,10 0,10 10,0 10,0 0),(2 2,4 2,4 4,2 4,2 2))",
            "MULTIPOINT(1 2,3 4)",
            "MULTILINESTRING((0 0,1 1),(2 2,3 3))",
            "MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))",
            "GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))",
        ];
        for wkt in cases {
            let g = decode_wkt(wkt, 0).unwrap();
            let wkb = encode_wkb(&g, "test").unwrap();
            let g2 = decode_wkb(&wkb, None).unwrap();
            assert_eq!(g.geometry, g2.geometry, "{wkt}");
        }
    }

    #[test]
    fn auto_detect_dispatches_gpb_and_wkb() {
        let g = decode_wkt("POINT(1 2)", 4326).unwrap();
        let gpb_blob = encode_canonical_gpb(&g, "test").unwrap();
        let wkb_blob = encode_wkb(&g, "test").unwrap();
        assert_eq!(decode_auto(&gpb_blob).unwrap().srid, 4326);
        assert_eq!(decode_auto(&wkb_blob).unwrap().srid, 0);
        assert_eq!(
            decode_auto(&gpb_blob).unwrap().geometry,
            decode_auto(&wkb_blob).unwrap().geometry
        );
    }

    #[test]
    fn ewkb_srid_is_read_and_override_wins() {
        // EWKB POINT(1 2) with SRID 4326, little-endian.
        let mut ewkb = vec![0x01];
        ewkb.extend_from_slice(&(1u32 | 0x2000_0000).to_le_bytes());
        ewkb.extend_from_slice(&4326i32.to_le_bytes());
        ewkb.extend_from_slice(&1.0f64.to_le_bytes());
        ewkb.extend_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(decode_wkb(&ewkb, None).unwrap().srid, 4326);
        assert_eq!(decode_wkb(&ewkb, Some(6668)).unwrap().srid, 6668);
    }

    #[test]
    fn z_wkb_is_flagged() {
        // ISO WKB POINT Z (1 2 3): type 1001.
        let mut wkb = vec![0x01];
        wkb.extend_from_slice(&1001u32.to_le_bytes());
        for v in [1.0f64, 2.0, 3.0] {
            wkb.extend_from_slice(&v.to_le_bytes());
        }
        let g = decode_wkb(&wkb, None).unwrap();
        assert!(g.has_zm);
        assert!(encode_wkb(&g, "ST_AsBinary").is_err());
        assert!(encode_wkt(&g, "ST_AsText").is_err());
    }

    #[test]
    fn zm_wkt_is_rejected() {
        for wkt in [
            "POINT Z (1 2 3)",
            "POINT ZM (1 2 3 4)",
            "POINT M (1 2 3)",
            "POINT(1 2 3)",
            "LINESTRING(0 0 0,1 1 1)",
        ] {
            assert!(decode_wkt(wkt, 0).is_err(), "{wkt}");
        }
        assert!(decode_wkt("POINT(1 2)", 0).is_ok());
    }

    #[test]
    fn hostile_wkb_counts_error_instead_of_allocating() {
        // MultiPoint claiming ~2^31 members in a 9-byte buffer: must be an
        // Err, never an allocation attempt (this aborted CI once).
        let mut wkb = vec![0x01, 0x04, 0x00, 0x00, 0x00];
        wkb.extend_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
        assert!(decode_wkb(&wkb, None).is_err());
        // LineString with a huge vertex count.
        let mut wkb = vec![0x01, 0x02, 0x00, 0x00, 0x00];
        wkb.extend_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        assert!(decode_wkb(&wkb, None).is_err());
        // Polygon with a huge ring count.
        let mut wkb = vec![0x01, 0x03, 0x00, 0x00, 0x00];
        wkb.extend_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        assert!(decode_wkb(&wkb, None).is_err());
    }

    #[test]
    fn point_empty_wkt_errors_with_hint() {
        let err = decode_wkt("POINT EMPTY", 0).unwrap_err();
        assert!(err.to_string().contains("empty points"), "{err}");
    }

    #[test]
    fn empty_geometries() {
        let empty_ls = Geom {
            geometry: Geometry::LineString(line_string![]),
            srid: 0,
            has_zm: false,
        };
        assert!(is_empty(&empty_ls.geometry));
        assert!(is_empty(&Geometry::Point(Point::new(f64::NAN, f64::NAN))));
        assert!(!is_empty(&point_geom(1.0, 2.0).geometry));
        assert_eq!(encode_wkt(&empty_ls, "test").unwrap(), "LINESTRING EMPTY");
        assert_eq!(envelope(&empty_ls.geometry), None);
    }

    #[test]
    fn envelope_of_polygon() {
        let g: Geometry<f64> = Geometry::Polygon(polygon![
            (x: 0.0, y: 0.0), (x: 10.0, y: 0.0), (x: 10.0, y: 5.0), (x: 0.0, y: 0.0)
        ]);
        let e = envelope(&g).unwrap();
        assert_eq!((e.min_x, e.max_x, e.min_y, e.max_y), (0.0, 10.0, 0.0, 5.0));
    }
}
