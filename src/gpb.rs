//! GeoPackage geometry blob (GPB) header codec.
//!
//! Operates on raw bytes without decoding the WKB payload, so that the R-tree
//! helper functions can answer from the header envelope alone, and so that
//! empty geometries (including `POINT EMPTY`, which `geo_types` cannot
//! represent) are handled before any geometry decoding.
//!
//! Header layout per the GeoPackage spec (StandardGeoPackageBinary):
//! magic "GP", 1-byte version (0), 1-byte flags, i32 srs_id in the header
//! byte order, optional envelope of doubles ordered per axis as
//! (minx, maxx, miny, maxy[, minz, maxz][, minm, maxm]), then WKB.

use crate::error::{Error, Result};

const MAGIC: [u8; 2] = [0x47, 0x50]; // "GP"

const FLAG_BYTE_ORDER_LE: u8 = 0b0000_0001;
const FLAG_EMPTY: u8 = 0b0001_0000;
const FLAG_EXTENDED: u8 = 0b0010_0000;
const FLAG_RESERVED: u8 = 0b1100_0000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Envelope {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GpbHeader {
    pub srid: i32,
    pub envelope: Option<Envelope>,
    pub empty: bool,
    /// Byte offset where the WKB payload starts.
    pub wkb_offset: usize,
}

impl GpbHeader {
    pub fn parse(bytes: &[u8]) -> Result<GpbHeader> {
        if bytes.len() < 8 {
            return Err(Error::InvalidGpb("blob shorter than the 8-byte header"));
        }
        if bytes[0..2] != MAGIC {
            return Err(Error::InvalidGpb("missing \"GP\" magic"));
        }
        if bytes[2] != 0 {
            return Err(Error::InvalidGpb("unsupported version (expected 0)"));
        }
        let flags = bytes[3];
        if flags & FLAG_EXTENDED != 0 {
            return Err(Error::InvalidGpb(
                "ExtendedGeoPackageBinary is not supported",
            ));
        }
        if flags & FLAG_RESERVED != 0 {
            return Err(Error::InvalidGpb("reserved flag bits are set"));
        }
        let little_endian = flags & FLAG_BYTE_ORDER_LE != 0;
        let envelope_indicator = (flags >> 1) & 0b111;
        let envelope_doubles = match envelope_indicator {
            0 => 0,
            1 => 4,     // XY
            2 | 3 => 6, // XYZ / XYM
            4 => 8,     // XYZM
            _ => return Err(Error::InvalidGpb("invalid envelope indicator (5-7)")),
        };
        let wkb_offset = 8 + envelope_doubles * 8;
        if bytes.len() < wkb_offset {
            return Err(Error::InvalidGpb("blob truncated inside the envelope"));
        }

        let srid_bytes: [u8; 4] = bytes[4..8].try_into().expect("length checked");
        let srid = if little_endian {
            i32::from_le_bytes(srid_bytes)
        } else {
            i32::from_be_bytes(srid_bytes)
        };

        let envelope = if envelope_doubles >= 4 {
            let mut d = [0.0f64; 4];
            for (i, v) in d.iter_mut().enumerate() {
                let start = 8 + i * 8;
                let raw: [u8; 8] = bytes[start..start + 8].try_into().expect("length checked");
                *v = if little_endian {
                    f64::from_le_bytes(raw)
                } else {
                    f64::from_be_bytes(raw)
                };
            }
            // Envelope doubles are ordered per axis: minx, maxx, miny, maxy.
            Some(Envelope {
                min_x: d[0],
                max_x: d[1],
                min_y: d[2],
                max_y: d[3],
            })
        } else {
            None
        };

        Ok(GpbHeader {
            srid,
            envelope,
            empty: flags & FLAG_EMPTY != 0,
            wkb_offset,
        })
    }
}

/// Assemble a StandardGeoPackageBinary blob (little-endian header) from an
/// ISO WKB payload.
pub fn write_gpb(wkb: &[u8], srid: i32, envelope: Option<Envelope>, empty: bool) -> Vec<u8> {
    let mut flags = FLAG_BYTE_ORDER_LE;
    if envelope.is_some() {
        flags |= 1 << 1; // envelope indicator 1: XY
    }
    if empty {
        flags |= FLAG_EMPTY;
    }
    let mut out = Vec::with_capacity(8 + if envelope.is_some() { 32 } else { 0 } + wkb.len());
    out.extend_from_slice(&MAGIC);
    out.push(0); // version
    out.push(flags);
    out.extend_from_slice(&srid.to_le_bytes());
    if let Some(e) = envelope {
        for v in [e.min_x, e.max_x, e.min_y, e.max_y] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out.extend_from_slice(wkb);
    out
}

/// True if the blob starts with the GPB magic (used for input auto-detection).
pub fn is_gpb(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0..2] == MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ISO WKB for POINT(1 2), little-endian.
    fn point_wkb() -> Vec<u8> {
        let mut wkb = vec![0x01, 0x01, 0x00, 0x00, 0x00];
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb
    }

    #[test]
    fn parse_header_without_envelope_byte_literal() {
        // Hand-built: magic, version 0, flags 0x01 (LE, no envelope), srid 4326 LE.
        let mut blob = vec![0x47, 0x50, 0x00, 0x01, 0xE6, 0x10, 0x00, 0x00];
        blob.extend_from_slice(&point_wkb());
        let h = GpbHeader::parse(&blob).unwrap();
        assert_eq!(h.srid, 4326);
        assert_eq!(h.envelope, None);
        assert!(!h.empty);
        assert_eq!(h.wkb_offset, 8);
    }

    #[test]
    fn parse_envelope_value_order_is_minx_maxx_miny_maxy() {
        // flags 0x03 = LE + XY envelope. Envelope values 1,2,3,4 in stored order.
        let mut blob = vec![0x47, 0x50, 0x00, 0x03, 0xE6, 0x10, 0x00, 0x00];
        for v in [1.0f64, 2.0, 3.0, 4.0] {
            blob.extend_from_slice(&v.to_le_bytes());
        }
        blob.extend_from_slice(&point_wkb());
        let h = GpbHeader::parse(&blob).unwrap();
        // Stored order is per-axis min/max, NOT (minx, miny, maxx, maxy).
        assert_eq!(
            h.envelope,
            Some(Envelope {
                min_x: 1.0,
                max_x: 2.0,
                min_y: 3.0,
                max_y: 4.0
            })
        );
        assert_eq!(h.wkb_offset, 40);
    }

    #[test]
    fn parse_big_endian_header() {
        // flags 0x02 = BE + XY envelope.
        let mut blob = vec![0x47, 0x50, 0x00, 0x02];
        blob.extend_from_slice(&4326i32.to_be_bytes());
        for v in [1.0f64, 2.0, 3.0, 4.0] {
            blob.extend_from_slice(&v.to_be_bytes());
        }
        blob.extend_from_slice(&point_wkb());
        let h = GpbHeader::parse(&blob).unwrap();
        assert_eq!(h.srid, 4326);
        assert_eq!(h.envelope.unwrap().max_y, 4.0);
    }

    #[test]
    fn parse_all_envelope_indicators() {
        for (indicator, doubles) in [(0usize, 0usize), (1, 4), (2, 6), (3, 6), (4, 8)] {
            let flags = 0x01 | (indicator as u8) << 1;
            let mut blob = vec![0x47, 0x50, 0x00, flags, 0x00, 0x00, 0x00, 0x00];
            for i in 0..doubles {
                blob.extend_from_slice(&(i as f64).to_le_bytes());
            }
            blob.extend_from_slice(&point_wkb());
            let h = GpbHeader::parse(&blob).unwrap();
            assert_eq!(h.wkb_offset, 8 + doubles * 8, "indicator {indicator}");
            assert_eq!(h.envelope.is_some(), doubles >= 4);
        }
    }

    #[test]
    fn parse_empty_flag() {
        let blob = vec![0x47, 0x50, 0x00, 0x11, 0x00, 0x00, 0x00, 0x00];
        let h = GpbHeader::parse(&blob).unwrap();
        assert!(h.empty);
    }

    #[test]
    fn rejects_bad_magic_version_and_flags() {
        let ok = vec![0x47, 0x50, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
        let cases: Vec<(usize, u8)> = vec![
            (0, 0x00), // bad magic
            (2, 0x01), // bad version
            (3, 0x21), // extended bit
            (3, 0x41), // reserved bit
            (3, 0x0B), // envelope indicator 5
        ];
        for (pos, byte) in cases {
            let mut blob = ok.clone();
            blob[pos] = byte;
            assert!(GpbHeader::parse(&blob).is_err(), "byte {byte:#x} at {pos}");
        }
    }

    #[test]
    fn truncation_at_every_offset_errors_never_panics() {
        let mut blob = vec![0x47, 0x50, 0x00, 0x03, 0xE6, 0x10, 0x00, 0x00];
        for v in [1.0f64, 2.0, 3.0, 4.0] {
            blob.extend_from_slice(&v.to_le_bytes());
        }
        let full = GpbHeader::parse(&blob);
        assert!(full.is_ok());
        for len in 0..blob.len() {
            assert!(GpbHeader::parse(&blob[..len]).is_err(), "prefix {len}");
        }
    }

    #[test]
    fn write_parse_roundtrip() {
        let env = Envelope {
            min_x: -1.5,
            max_x: 2.5,
            min_y: -3.5,
            max_y: 4.5,
        };
        for (envelope, empty) in [(None, false), (Some(env), false), (None, true)] {
            let blob = write_gpb(&point_wkb(), 3857, envelope, empty);
            let h = GpbHeader::parse(&blob).unwrap();
            assert_eq!(h.srid, 3857);
            assert_eq!(h.envelope, envelope);
            assert_eq!(h.empty, empty);
            assert_eq!(&blob[h.wkb_offset..], &point_wkb()[..]);
        }
    }

    #[test]
    fn is_gpb_detects_magic() {
        assert!(is_gpb(&[0x47, 0x50, 0x00, 0x01]));
        assert!(!is_gpb(&[0x01, 0x01]));
        assert!(!is_gpb(&[0x47]));
    }
}
