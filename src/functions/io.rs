//! Geometry constructors and output functions.

use crate::error::Result;
use crate::geom::{self, Geom};
use crate::gpb::{self, GpbHeader};

/// `ST_GeomFromText(wkt [, srid])` → canonical GPB.
pub fn st_geom_from_text(wkt: &str, srid: Option<i32>) -> Result<Vec<u8>> {
    let geom = geom::decode_wkt(wkt, srid.unwrap_or(0))?;
    geom::encode_canonical_gpb(&geom, "ST_GeomFromText")
}

/// `ST_GeomFromWKB(wkb [, srid])` → canonical GPB. Accepts ISO WKB and EWKB
/// (and, leniently, GPB); an explicit srid argument overrides an embedded one
/// (PostGIS behavior).
pub fn st_geom_from_wkb(bytes: &[u8], srid: Option<i32>) -> Result<Vec<u8>> {
    let mut geom = if gpb::is_gpb(bytes) {
        geom::decode_gpb(bytes)?.1
    } else {
        geom::decode_wkb(bytes, srid)?
    };
    if let Some(srid) = srid {
        geom.srid = srid;
    }
    geom::encode_canonical_gpb(&geom, "ST_GeomFromWKB")
}

/// `ST_GeomFromGPB(gpb)` → validated, normalized GPB (little-endian header,
/// envelope stripped). The WKB payload is passed through byte-for-byte after
/// being validated, so 3D payloads survive losslessly.
pub fn st_geom_from_gpb(bytes: &[u8]) -> Result<Vec<u8>> {
    let (header, _geom) = geom::decode_gpb(bytes)?;
    Ok(gpb::write_gpb(
        &bytes[header.wkb_offset..],
        header.srid,
        None,
        header.empty,
    ))
}

/// `ST_AsText(geom)` → WKT.
pub fn st_as_text(bytes: &[u8]) -> Result<String> {
    decoded::st_as_text(&geom::decode_auto(bytes)?)
}

/// `ST_AsBinary(geom)` → ISO WKB, little-endian, SRID dropped (as in PostGIS).
pub fn st_as_binary(bytes: &[u8]) -> Result<Vec<u8>> {
    decoded::st_as_binary(&geom::decode_auto(bytes)?)
}

/// `ST_AsGPB(geom)` → storage-grade GPB (XY envelope for non-point
/// geometries), preserving srid.
pub fn st_as_gpb(bytes: &[u8]) -> Result<Vec<u8>> {
    decoded::st_as_gpb(&geom::decode_auto(bytes)?)
}

/// Serialization for a geometry that is already decoded — see
/// [`crate::functions::predicates::decoded`] for why that exists. These are
/// one-liners over `geom::encode_*`; they exist so the function name in an
/// error message is the SQL one either way.
pub mod decoded {
    use super::*;

    pub fn st_as_text(geom: &Geom) -> Result<String> {
        geom::encode_wkt(geom, "ST_AsText")
    }

    pub fn st_as_binary(geom: &Geom) -> Result<Vec<u8>> {
        geom::encode_wkb(geom, "ST_AsBinary")
    }

    pub fn st_as_gpb(geom: &Geom) -> Result<Vec<u8>> {
        geom::encode_storage_gpb(geom, "ST_AsGPB")
    }
}

/// `ST_SetSRID(geom, srid)` — relabel only, coordinates unchanged. Operates
/// at the byte level so 3D payloads survive losslessly.
pub fn st_set_srid(bytes: &[u8], srid: i32) -> Result<Vec<u8>> {
    if gpb::is_gpb(bytes) {
        let header = GpbHeader::parse(bytes)?;
        Ok(gpb::write_gpb(
            &bytes[header.wkb_offset..],
            srid,
            None,
            header.empty,
        ))
    } else {
        let geom = geom::decode_wkb(bytes, None)?; // validates the WKB
        Ok(gpb::write_gpb(
            bytes,
            srid,
            None,
            geom::is_empty(&geom.geometry),
        ))
    }
}

/// `ST_SRID(geom)` — 0 means unknown, as in PostGIS/GeoPackage.
pub fn st_srid(bytes: &[u8]) -> Result<i32> {
    Ok(geom::decode_auto(bytes)?.srid)
}

/// `ST_MakePoint(x, y)` — POINT with unknown SRID. (The 3/4-arg z/m forms
/// are not shipped; kenro is 2D.)
pub fn st_make_point(x: f64, y: f64) -> Result<Vec<u8>> {
    st_point(x, y, None)
}

/// `ST_Point(x, y [, srid])` — like ST_MakePoint; the srid arity is
/// PostGIS ≥ 3.2.
pub fn st_point(x: f64, y: f64, srid: Option<i32>) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &crate::geom::Geom {
            geometry: geo_types::Geometry::Point(geo_types::Point::new(x, y)),
            srid: srid.unwrap_or(0),
            has_zm: false,
        },
        "ST_Point",
    )
}

/// `ST_MakeEnvelope(xmin, ymin, xmax, ymax [, srid])` — rectangular
/// POLYGON in PostGIS vertex order; degenerate envelopes stay polygons
/// (unlike ST_Envelope's output collapsing).
pub fn st_make_envelope(
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
    srid: Option<i32>,
) -> Result<Vec<u8>> {
    use geo_types::{Geometry, LineString, Polygon, coord};
    let ring = LineString::new(vec![
        coord! { x: xmin, y: ymin },
        coord! { x: xmin, y: ymax },
        coord! { x: xmax, y: ymax },
        coord! { x: xmax, y: ymin },
        coord! { x: xmin, y: ymin },
    ]);
    geom::encode_canonical_gpb(
        &crate::geom::Geom {
            geometry: Geometry::Polygon(Polygon::new(ring, vec![])),
            srid: srid.unwrap_or(0),
            has_zm: false,
        },
        "ST_MakeEnvelope",
    )
}

/// Re-exported for binding layers that want header-only access.
pub use crate::gpb::is_gpb;

#[allow(unused)]
pub fn parse_gpb_header(bytes: &[u8]) -> Result<GpbHeader> {
    GpbHeader::parse(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_return_canonical_gpb() {
        let blob = st_geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        let h = GpbHeader::parse(&blob).unwrap();
        assert_eq!(h.srid, 4326);
        assert_eq!(h.envelope, None);
        assert!(!h.empty);
        assert_eq!(st_as_text(&blob).unwrap(), "POINT(1 2)");
    }

    #[test]
    fn from_wkb_srid_override() {
        let wkb = st_as_binary(&st_geom_from_text("POINT(1 2)", Some(4326)).unwrap()).unwrap();
        let h = GpbHeader::parse(&st_geom_from_wkb(&wkb, None).unwrap()).unwrap();
        assert_eq!(h.srid, 0); // ISO WKB carries no srid
        let h = GpbHeader::parse(&st_geom_from_wkb(&wkb, Some(3857)).unwrap()).unwrap();
        assert_eq!(h.srid, 3857);
    }

    #[test]
    fn from_gpb_normalizes_and_strips_envelope() {
        let stored =
            st_as_gpb(&st_geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap()).unwrap();
        assert!(GpbHeader::parse(&stored).unwrap().envelope.is_some());
        let normalized = st_geom_from_gpb(&stored).unwrap();
        assert_eq!(GpbHeader::parse(&normalized).unwrap().envelope, None);
        assert_eq!(
            st_as_text(&normalized).unwrap(),
            "POLYGON((0 0,4 0,4 4,0 4,0 0))"
        );
    }

    #[test]
    fn as_gpb_envelope_policy() {
        // Points: no envelope.
        let p = st_as_gpb(&st_geom_from_text("POINT(1 2)", None).unwrap()).unwrap();
        assert_eq!(GpbHeader::parse(&p).unwrap().envelope, None);
        // Non-point: XY envelope present and correct.
        let l = st_as_gpb(&st_geom_from_text("LINESTRING(0 0,10 5)", None).unwrap()).unwrap();
        let env = GpbHeader::parse(&l).unwrap().envelope.unwrap();
        assert_eq!(
            (env.min_x, env.max_x, env.min_y, env.max_y),
            (0.0, 10.0, 0.0, 5.0)
        );
        // Empty: empty flag, no envelope.
        let e = st_as_gpb(&st_geom_from_text("LINESTRING EMPTY", None).unwrap()).unwrap();
        let h = GpbHeader::parse(&e).unwrap();
        assert!(h.empty);
        assert_eq!(h.envelope, None);
    }

    #[test]
    fn empty_wkt_roundtrip() {
        for wkt in [
            "LINESTRING EMPTY",
            "POLYGON EMPTY",
            "MULTIPOINT EMPTY",
            "MULTILINESTRING EMPTY",
            "MULTIPOLYGON EMPTY",
            "GEOMETRYCOLLECTION EMPTY",
        ] {
            let blob = st_geom_from_text(wkt, None).unwrap();
            assert!(GpbHeader::parse(&blob).unwrap().empty, "{wkt}");
            assert_eq!(st_as_text(&blob).unwrap(), wkt);
        }
    }

    #[test]
    fn garbage_input_errors() {
        assert!(st_as_text(&[0xFF, 0x00, 0x01]).is_err());
        assert!(st_geom_from_gpb(&[0x01, 0x01, 0x00]).is_err()); // WKB is not GPB
        assert!(st_geom_from_text("POINT(a b)", None).is_err());
    }
}
