//! CRS transform. (SRID relabeling — ST_SetSRID / ST_SRID — lives in
//! `io.rs`: it is byte-level and independent of the `transform` feature.)

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

/// `ST_Transform(geom, to_srid)` — PostGIS-exact two-argument form: the
/// source CRS is the geometry's embedded SRID. (PostGIS has no
/// `(geom, from, to)` integer overload, so neither does kenro.)
///
/// Runs on the **encoding**, not on a decoded `geo_types` value, which is what
/// lets a 3D column and a surface collection through — the two things a
/// CityGML workflow reprojects most. Measured on PostGIS 3.5:
/// `ST_Transform(POINT Z (139.7 35.7 100), 32654)` keeps the 100, and a
/// `POLYHEDRALSURFACE Z` comes back reprojected rather than rejected.
///
/// The Z is not merely carried: proj4rs takes `(x, y, z)` per coordinate, so a
/// datum shift routed through geocentric coordinates reads the height, as PROJ
/// does. Same-datum pairs leave it exactly as it was.
pub fn st_transform(bytes: &[u8], to_srid: i32) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Transform";
    // Read the SRID from the header: `decode_auto` would refuse a surface
    // collection before we ever got to the CRS.
    let from_srid = geom::srid_of(bytes)?;
    if from_srid <= 0 {
        return Err(unknown_srid(from_srid));
    }
    if from_srid == to_srid {
        // Nothing moves, but the blob is still normalized to canonical GPB —
        // the same shape every other kenro function returns.
        return crate::coords::map_coords(bytes, &mut |_| {});
    }
    let reprojection = crate::crs::Reprojection::new(FUNC, from_srid, to_srid)?;
    let mut out = None;
    let mut walk_err = None;
    reprojection.run(|step| {
        // One proj4rs call drives the whole walk: it hands `step` each
        // coordinate, and `step` is what the rewriter applies.
        match crate::coords::map_coords_relabelled(bytes, to_srid, &mut |p| {
            if let Some((x, y, z)) = step(p.x, p.y, p.z.unwrap_or(0.0)) {
                p.x = x;
                p.y = y;
                if let Some(pz) = p.z.as_mut() {
                    *pz = z;
                }
            }
        }) {
            Ok(v) => out = Some(v),
            Err(e) => walk_err = Some(e),
        }
    })?;
    if let Some(e) = walk_err {
        return Err(e);
    }
    out.ok_or_else(|| Error::Unsupported {
        func: FUNC,
        reason: "the reprojection produced nothing".into(),
    })
}

fn unknown_srid(srid: i32) -> Error {
    // Mirrors PostGIS's "Input geometry has unknown (0) SRID".
    Error::Unsupported {
        func: "ST_Transform",
        reason: format!(
            "Input geometry has unknown ({}) SRID; set one with ST_SetSRID, \
             ST_GeomFromText(wkt, srid), or read it from a GeoPackage blob",
            srid.max(0)
        ),
    }
}

/// Reprojection for a geometry that is already decoded — see
/// [`crate::functions::predicates::decoded`] for why that exists.
pub mod decoded {
    use super::*;

    /// Reproject in place. Takes `&mut` rather than returning a new `Geom`
    /// so the blob path above pays no clone; a caller holding a shared
    /// handle clones first.
    pub fn st_transform_in_place(geom: &mut Geom, to_srid: i32) -> Result<()> {
        if geom.srid <= 0 {
            // Mirrors PostGIS's "Input geometry has unknown (0) SRID".
            return Err(Error::Unsupported {
                func: "ST_Transform",
                reason: format!(
                    "Input geometry has unknown ({}) SRID; set one with ST_SetSRID, \
                     ST_GeomFromText(wkt, srid), or read it from a GeoPackage blob",
                    geom.srid.max(0)
                ),
            });
        }
        if geom.srid != to_srid {
            crate::crs::transform_geometry("ST_Transform", &mut geom.geometry, geom.srid, to_srid)?;
            geom.srid = to_srid;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text, st_set_srid, st_srid};
    use crate::gpb::GpbHeader;

    #[test]
    fn transform_tokyo_to_utm54_and_back() {
        let src = st_geom_from_text("POINT(139.745433 35.658581)", Some(4326)).unwrap();
        let projected = st_transform(&src, 32654).unwrap();
        assert_eq!(st_srid(&projected).unwrap(), 32654);
        let back = st_transform(&projected, 4326).unwrap();
        let wkt = st_as_text(&back).unwrap();
        assert!(wkt.starts_with("POINT(139.745"), "{wkt}");
    }

    #[test]
    fn same_srid_is_a_noop() {
        let src = st_geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        assert_eq!(st_transform(&src, 4326).unwrap(), src);
    }

    #[test]
    fn unknown_srid_input_errors_like_postgis() {
        let src = st_geom_from_text("POINT(1 2)", None).unwrap();
        let err = st_transform(&src, 4326).unwrap_err().to_string();
        assert!(err.contains("unknown (0) SRID"), "{err}");
        assert!(err.contains("ST_SetSRID"), "{err}");
    }

    #[test]
    fn set_srid_relabels_without_touching_coordinates() {
        let src = st_geom_from_text("POINT(139.7 35.7)", None).unwrap();
        let labeled = st_set_srid(&src, 4326).unwrap();
        assert_eq!(st_srid(&labeled).unwrap(), 4326);
        assert_eq!(st_as_text(&labeled).unwrap(), "POINT(139.7 35.7)");
        // And the combination unlocks ST_Transform from bare WKT input.
        assert!(st_transform(&labeled, 32654).is_ok());
    }

    #[test]
    fn set_srid_preserves_3d_payload_losslessly() {
        // ISO WKB POINT Z (1 2 3).
        let mut wkb = vec![0x01];
        wkb.extend_from_slice(&1001u32.to_le_bytes());
        for v in [1.0f64, 2.0, 3.0] {
            wkb.extend_from_slice(&v.to_le_bytes());
        }
        let blob = st_set_srid(&wkb, 4326).unwrap();
        let header = GpbHeader::parse(&blob).unwrap();
        assert_eq!(header.srid, 4326);
        assert_eq!(&blob[header.wkb_offset..], &wkb[..]);
    }

    #[test]
    fn transform_polygon_transforms_every_vertex() {
        let src = st_geom_from_text(
            "POLYGON((139.7 35.6,139.8 35.6,139.8 35.7,139.7 35.7,139.7 35.6))",
            Some(4326),
        )
        .unwrap();
        let out = st_transform(&src, 3857).unwrap();
        let wkt = st_as_text(&out).unwrap();
        // All eastings around 15.55–15.56 Mm; no vertex left in degrees.
        assert!(wkt.contains("155"), "{wkt}");
        assert!(!wkt.contains("139."), "{wkt}");
    }
}
