//! H3 cell functions for mesh aggregation (`GROUP BY h3_latlng_to_cell(...)`).
//!
//! Names follow the h3-pg PostgreSQL extension (`h3_latlng_to_cell` — also
//! DuckDB's name), for AI-prior compatibility. Cells are SQL INTEGERs: valid
//! H3 indexes have bit 63 (and bits 56–58) zero, so `u64 → i64` is lossless
//! and always non-negative.

use geo_types::Geometry;
use h3o::{CellIndex, LatLng, Resolution};

use crate::error::{Error, Result};
use crate::geom;

/// `h3_latlng_to_cell(geom, resolution)` → cell as INTEGER. POINT only, in
/// lon/lat coordinates (SRID 4326/4612/6668 or unknown).
pub fn h3_latlng_to_cell(bytes: &[u8], resolution: i64) -> Result<i64> {
    const FUNC: &str = "h3_latlng_to_cell";
    let geom = geom::decode_auto(bytes)?;
    let Geometry::Point(p) = geom.geometry else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!(
                "expects a POINT geometry, got {}",
                geom::wkt_type_name(&geom.geometry)
            ),
        });
    };
    if p.x().is_nan() || p.y().is_nan() {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "geometry is empty".into(),
        });
    }
    if !matches!(geom.srid, 0 | 4326 | 4612 | 6668) && geom.srid > 0 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!(
                "expects lon/lat coordinates (SRID 4326/4612/6668 or unknown), got SRID {}; \
                 run ST_Transform(geom, 4326) first",
                geom.srid
            ),
        });
    }
    // H3 wants (lat, lng); geometry x is longitude, y is latitude. This swap
    // lives exactly here and nowhere else.
    let latlng = LatLng::new(p.y(), p.x()).map_err(|e| Error::Unsupported {
        func: FUNC,
        reason: e.to_string(),
    })?;
    Ok(u64::from(latlng.to_cell(parse_resolution(FUNC, resolution)?)) as i64)
}

/// `h3_cell_to_parent(cell, resolution)` → coarser cell as INTEGER.
pub fn h3_cell_to_parent(cell: i64, resolution: i64) -> Result<i64> {
    const FUNC: &str = "h3_cell_to_parent";
    let cell = parse_cell(FUNC, cell)?;
    let res = parse_resolution(FUNC, resolution)?;
    cell.parent(res)
        .map(|c| u64::from(c) as i64)
        .ok_or_else(|| Error::Unsupported {
            func: FUNC,
            reason: format!(
                "resolution {resolution} is finer than the cell's resolution {}",
                cell.resolution()
            ),
        })
}

/// `h3_cell_to_string(cell)` → lowercase hex TEXT.
pub fn h3_cell_to_string(cell: i64) -> Result<String> {
    Ok(parse_cell("h3_cell_to_string", cell)?.to_string())
}

/// `h3_string_to_cell(text)` → cell as INTEGER.
pub fn h3_string_to_cell(s: &str) -> Result<i64> {
    let cell: CellIndex = s.parse().map_err(|e| Error::Unsupported {
        func: "h3_string_to_cell",
        reason: format!("{s:?}: {e}"),
    })?;
    Ok(u64::from(cell) as i64)
}

fn parse_resolution(func: &'static str, res: i64) -> Result<Resolution> {
    u8::try_from(res)
        .ok()
        .and_then(|r| Resolution::try_from(r).ok())
        .ok_or_else(|| Error::Unsupported {
            func,
            reason: format!("resolution must be 0-15, got {res}"),
        })
}

fn parse_cell(func: &'static str, cell: i64) -> Result<CellIndex> {
    u64::try_from(cell)
        .ok()
        .and_then(|c| CellIndex::try_from(c).ok())
        .ok_or_else(|| Error::Unsupported {
            func,
            reason: format!("{cell} is not a valid H3 cell index"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::st_geom_from_text;

    fn tokyo() -> Vec<u8> {
        st_geom_from_text("POINT(139.745433 35.658581)", Some(4326)).unwrap()
    }

    #[test]
    fn latlng_argument_order_is_not_swapped() {
        // The classic bug: passing (lng, lat). Tokyo is at lat 35.66, lng
        // 139.75 — swapped, the "latitude" 139.75 is out of range and errors,
        // and even a valid swap would land in the wrong hemisphere. Assert
        // the cell's center is near Tokyo.
        let cell = h3_latlng_to_cell(&tokyo(), 9).unwrap();
        let center = LatLng::from(CellIndex::try_from(cell as u64).unwrap());
        assert!((center.lat() - 35.658581).abs() < 0.01, "{}", center.lat());
        assert!((center.lng() - 139.745433).abs() < 0.01, "{}", center.lng());
    }

    #[test]
    fn cells_are_positive_i64_at_every_resolution() {
        for res in 0..=15 {
            let cell = h3_latlng_to_cell(&tokyo(), res).unwrap();
            assert!(cell > 0, "res {res}: {cell}");
        }
    }

    #[test]
    fn parent_chain_and_string_roundtrip() {
        let cell = h3_latlng_to_cell(&tokyo(), 9).unwrap();
        let parent = h3_cell_to_parent(cell, 7).unwrap();
        assert_ne!(cell, parent);
        assert_eq!(h3_cell_to_parent(cell, 9).unwrap(), cell);
        assert!(h3_cell_to_parent(cell, 10).is_err()); // finer than the cell

        let s = h3_cell_to_string(cell).unwrap();
        assert_eq!(h3_string_to_cell(&s).unwrap(), cell);
        assert!(h3_string_to_cell("not-a-cell").is_err());
    }

    #[test]
    fn guards() {
        let line = st_geom_from_text("LINESTRING(0 0,1 1)", Some(4326)).unwrap();
        assert!(h3_latlng_to_cell(&line, 9).is_err());
        let projected = st_geom_from_text("POINT(-7813 -37912)", Some(6677)).unwrap();
        let err = h3_latlng_to_cell(&projected, 9).unwrap_err().to_string();
        assert!(err.contains("ST_Transform"), "{err}");
        assert!(h3_latlng_to_cell(&tokyo(), 16).is_err());
        assert!(h3_latlng_to_cell(&tokyo(), -1).is_err());
        assert!(h3_cell_to_parent(-5, 3).is_err());
    }
}
