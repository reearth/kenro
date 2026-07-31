//! Coordinate reference systems: a curated EPSG → proj4-string table and the
//! proj4rs-backed geometry transform.
//!
//! proj4rs carries no EPSG database of its own; kenro embeds the codes its
//! audience actually uses (global basics + the Japanese national systems)
//! instead of linking the megabytes-large full registry. The `crs-full`
//! cargo feature adds the full `crs-definitions` table as a fallback for
//! anything else (u16 codes only).
//!
//! Accuracy caveat (measured and documented in docs/accuracy.md): WGS84,
//! JGD2000 and JGD2011 are all GRS80/WGS84-class with zero or absent Helmert
//! shifts here, so datum transforms among them are identity at this level —
//! the real-world JGD2000↔JGD2011 displacement requires datum grids that
//! neither proj4rs nor a gridless PROJ applies. Survey-grade work needs
//! full PROJ.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use geo::MapCoordsInPlace;
use geo_types::Geometry;
use proj4rs::Proj;

use crate::error::{Error, Result};

/// (lat_0, lon_0) of the Japanese plane rectangular zones I–XIX, in zone
/// order. Shared by JGD2000 (EPSG 2443–2461) and JGD2011 (EPSG 6669–6687).
const JGD_PLANE_ZONES: [(&str, &str); 19] = [
    ("33", "129.5"),             // I
    ("33", "131"),               // II
    ("36", "132.1666666666667"), // III
    ("33", "133.5"),             // IV
    ("36", "134.3333333333333"), // V
    ("36", "136"),               // VI
    ("36", "137.1666666666667"), // VII
    ("36", "138.5"),             // VIII
    ("36", "139.8333333333333"), // IX
    ("40", "140.8333333333333"), // X
    ("44", "140.25"),            // XI
    ("44", "142.25"),            // XII
    ("44", "144.25"),            // XIII
    ("26", "142"),               // XIV
    ("26", "127.5"),             // XV
    ("26", "124"),               // XVI
    ("26", "131"),               // XVII
    ("20", "136"),               // XVIII
    ("26", "154"),               // XIX
];

fn jgd_plane(zone_index: i32, towgs84: bool) -> String {
    let (lat_0, lon_0) = JGD_PLANE_ZONES[zone_index as usize];
    let datum = if towgs84 {
        " +towgs84=0,0,0,0,0,0,0"
    } else {
        ""
    };
    format!(
        "+proj=tmerc +lat_0={lat_0} +lon_0={lon_0} +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80{datum} +units=m +no_defs"
    )
}

/// The curated proj4 definition for an EPSG code, if kenro knows it.
pub fn proj4_def(epsg: i32) -> Option<String> {
    match epsg {
        // WGS84 geographic.
        4326 => Some("+proj=longlat +datum=WGS84 +no_defs".into()),
        // Web Mercator.
        3857 => Some(
            "+proj=merc +a=6378137 +b=6378137 +lat_ts=0.0 +lon_0=0.0 +x_0=0.0 +y_0=0 +k=1.0 +units=m +nadgrids=@null +no_defs"
                .into(),
        ),
        // JGD2000 geographic.
        4612 => Some("+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".into()),
        // JGD2011 geographic (EPSG defines no Helmert to WGS84; pass-through).
        6668 => Some("+proj=longlat +ellps=GRS80 +no_defs".into()),
        // WGS84 UTM zones 51N–56N (Japan).
        32651..=32656 => Some(format!(
            "+proj=utm +zone={} +datum=WGS84 +units=m +no_defs",
            epsg - 32600
        )),
        // JGD2000 plane rectangular I–XIX.
        2443..=2461 => Some(jgd_plane(epsg - 2443, true)),
        // JGD2011 plane rectangular I–XIX.
        6669..=6687 => Some(jgd_plane(epsg - 6669, false)),
        _ => None,
    }
}

thread_local! {
    static PROJ_CACHE: RefCell<HashMap<i32, Rc<Proj>>> = RefCell::new(HashMap::new());
}

fn unknown_epsg(func: &'static str, epsg: i32) -> Error {
    Error::Unsupported {
        func,
        reason: format!(
            "EPSG:{epsg} is not in kenro's built-in CRS table (WGS84, WebMercator, \
             UTM 51N-56N, JGD2000/JGD2011 geographic and plane rectangular I-XIX); \
             see the README \"Supported CRS\" section"
        ),
    }
}

fn cached_proj(func: &'static str, epsg: i32) -> Result<Rc<Proj>> {
    PROJ_CACHE.with(|cache| {
        if let Some(p) = cache.borrow().get(&epsg) {
            return Ok(Rc::clone(p));
        }
        let proj = match proj4_def(epsg) {
            Some(def) => Proj::from_proj_string(&def).map_err(|e| Error::Unsupported {
                func,
                reason: format!("EPSG:{epsg}: {e}"),
            })?,
            None => from_full_registry(func, epsg)?,
        };
        let proj = Rc::new(proj);
        cache.borrow_mut().insert(epsg, Rc::clone(&proj));
        Ok(proj)
    })
}

#[cfg(feature = "crs-full")]
fn from_full_registry(func: &'static str, epsg: i32) -> Result<Proj> {
    let code = u16::try_from(epsg).map_err(|_| unknown_epsg(func, epsg))?;
    Proj::from_epsg_code(code).map_err(|_| unknown_epsg(func, epsg))
}

#[cfg(not(feature = "crs-full"))]
fn from_full_registry(func: &'static str, epsg: i32) -> Result<Proj> {
    Err(unknown_epsg(func, epsg))
}

/// Reproject a geometry in place. Geographic CRS coordinates are degrees on
/// the kenro side; proj4rs works in radians for lat/long CRS, so conversion
/// happens exactly here.
pub fn transform_geometry(
    func: &'static str,
    geometry: &mut Geometry<f64>,
    from_epsg: i32,
    to_epsg: i32,
) -> Result<()> {
    let src = cached_proj(func, from_epsg)?;
    let dst = cached_proj(func, to_epsg)?;
    if src.is_latlong() {
        geometry.map_coords_in_place(|c| (c.x.to_radians(), c.y.to_radians()).into());
    }
    proj4rs::transform::transform(&src, &dst, geometry).map_err(|e| Error::Unsupported {
        func,
        reason: format!("transform from EPSG:{from_epsg} to EPSG:{to_epsg} failed: {e}"),
    })?;
    if dst.is_latlong() {
        geometry.map_coords_in_place(|c| (c.x.to_degrees(), c.y.to_degrees()).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::{Geometry, Point};

    const TOKYO_TOWER: (f64, f64) = (139.745433, 35.658581);

    fn transform_point(x: f64, y: f64, from: i32, to: i32) -> (f64, f64) {
        let mut g: Geometry<f64> = Geometry::Point(Point::new(x, y));
        transform_geometry("test", &mut g, from, to).unwrap();
        let Geometry::Point(p) = g else {
            unreachable!()
        };
        (p.x(), p.y())
    }

    #[test]
    fn every_curated_code_parses() {
        let codes: Vec<i32> = [4326, 3857, 4612, 6668]
            .into_iter()
            .chain(32651..=32656)
            .chain(2443..=2461)
            .chain(6669..=6687)
            .collect();
        assert_eq!(codes.len(), 48);
        for code in codes {
            let def = proj4_def(code).unwrap_or_else(|| panic!("EPSG:{code} missing"));
            Proj::from_proj_string(&def).unwrap_or_else(|e| panic!("EPSG:{code}: {e}"));
        }
    }

    #[test]
    fn web_mercator_matches_the_analytic_formula() {
        // Spherical Mercator has a closed form: x = R·λ, y = R·ln(tan(π/4 + φ/2)).
        let (x, y) = transform_point(TOKYO_TOWER.0, TOKYO_TOWER.1, 4326, 3857);
        let r = 6378137.0;
        let expect_x = r * TOKYO_TOWER.0.to_radians();
        let expect_y = r
            * (std::f64::consts::FRAC_PI_4 + TOKYO_TOWER.1.to_radians() / 2.0)
                .tan()
                .ln();
        assert!((x - expect_x).abs() < 1e-6, "{x} vs {expect_x}");
        assert!((y - expect_y).abs() < 1e-6, "{y} vs {expect_y}");
    }

    #[test]
    fn plane_rectangular_zone_ix_plausibility_and_roundtrip() {
        // Tokyo Tower is SW of the zone IX origin (36N 139.8333E): both
        // easting and northing must be negative and km-scale.
        let (x, y) = transform_point(TOKYO_TOWER.0, TOKYO_TOWER.1, 4326, 6677);
        assert!((-9000.0..-7000.0).contains(&x), "easting {x}");
        assert!((-39000.0..-37000.0).contains(&y), "northing {y}");
        let (lon, lat) = transform_point(x, y, 6677, 4326);
        assert!((lon - TOKYO_TOWER.0).abs() < 1e-9, "{lon}");
        assert!((lat - TOKYO_TOWER.1).abs() < 1e-9, "{lat}");
    }

    #[test]
    fn jgd_datum_pairs_are_identity_at_this_level() {
        for pair in [(4326, 6668), (4612, 6668), (4326, 4612)] {
            let (x, y) = transform_point(TOKYO_TOWER.0, TOKYO_TOWER.1, pair.0, pair.1);
            assert!((x - TOKYO_TOWER.0).abs() < 1e-9, "{pair:?}: {x}");
            assert!((y - TOKYO_TOWER.1).abs() < 1e-9, "{pair:?}: {y}");
        }
    }

    #[test]
    fn utm_54n_plausibility() {
        // Tokyo is in UTM zone 54N; easting ~380km (west of the 141E central
        // meridian), northing ~3.94Mm.
        let (x, y) = transform_point(TOKYO_TOWER.0, TOKYO_TOWER.1, 4326, 32654);
        assert!((300_000.0..500_000.0).contains(&x), "easting {x}");
        assert!((3_800_000.0..4_050_000.0).contains(&y), "northing {y}");
    }

    // With crs-full, 27700 (OSGB) resolves from the full registry instead.
    #[cfg(not(feature = "crs-full"))]
    #[test]
    fn unknown_epsg_names_the_code_and_the_readme() {
        let mut g: Geometry<f64> = Geometry::Point(Point::new(0.0, 0.0));
        let err = transform_geometry("ST_Transform", &mut g, 4326, 27700).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("EPSG:27700"), "{msg}");
        assert!(msg.contains("Supported CRS"), "{msg}");
    }
}
