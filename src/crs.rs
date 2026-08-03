//! Coordinate reference systems: a built-in EPSG → proj4-string table and
//! the proj4rs-backed geometry transform.
//!
//! proj4rs carries no EPSG database of its own. kenro stays neutral about
//! regions: the built-in table contains only globally-defined,
//! algorithmically-derivable systems — WGS84 geographic, Web Mercator, and
//! every WGS84 UTM zone (north and south). National and regional systems
//! are all served the same way: the `crs-full` cargo feature adds the full
//! `crs-definitions` registry as a fallback (u16 codes only).
//!
//! Accuracy caveat (measured and documented in docs/accuracy.md): this is
//! gridless projection math. Datum-grid transformations (national datum
//! modernizations, earthquake displacement models, …) are applied by
//! neither kenro nor a gridless PROJ — survey-grade work needs full PROJ
//! with the official grids.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use geo::MapCoordsInPlace;
use geo_types::Geometry;
use proj4rs::Proj;

use crate::error::{Error, Result};

/// The built-in proj4 definition for an EPSG code, if kenro knows it.
pub fn proj4_def(epsg: i32) -> Option<String> {
    match epsg {
        // WGS84 geographic.
        4326 => Some("+proj=longlat +datum=WGS84 +no_defs".into()),
        // Web Mercator.
        3857 => Some(
            "+proj=merc +a=6378137 +b=6378137 +lat_ts=0.0 +lon_0=0.0 +x_0=0.0 +y_0=0 +k=1.0 +units=m +nadgrids=@null +no_defs"
                .into(),
        ),
        // WGS84 UTM, northern hemisphere (zones 1N–60N).
        32601..=32660 => Some(format!(
            "+proj=utm +zone={} +datum=WGS84 +units=m +no_defs",
            epsg - 32600
        )),
        // WGS84 UTM, southern hemisphere (zones 1S–60S).
        32701..=32760 => Some(format!(
            "+proj=utm +zone={} +south +datum=WGS84 +units=m +no_defs",
            epsg - 32700
        )),
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
            "EPSG:{epsg} is not in kenro's built-in CRS table (WGS84 4326, WebMercator \
             3857, WGS84 UTM 32601-32660 north / 32701-32760 south); enable the \
             `crs-full` cargo feature for the full EPSG registry — see the README \
             \"Supported CRS\" section"
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

/// A reprojection, ready to be driven coordinate by coordinate.
///
/// [`transform_geometry`] hands proj4rs a whole `geo_types` value, which means
/// it cannot see a Z (there is nowhere in `geo_types` for one) and cannot touch
/// a surface collection at all. This is the same transform expressed so the
/// caller supplies the coordinates instead — the encoding-level path in
/// `functions::transform` walks the WKB and feeds them through.
///
/// proj4rs is coordinate-oriented underneath: its `Transform` trait hands each
/// coordinate to a closure as `(x, y, z)`. So a Z is not merely *preserved*
/// here, it participates — a datum shift routed through geocentric coordinates
/// reads the height, exactly as PROJ does. (For same-datum pairs it comes back
/// untouched; measured on PostGIS 3.5, `4326 → 32654` returns z = 100 for
/// z = 100 in, and moving the height does not move x or y.)
pub struct Reprojection {
    src: Rc<Proj>,
    dst: Rc<Proj>,
    func: &'static str,
    from: i32,
    to: i32,
}

impl Reprojection {
    pub fn new(func: &'static str, from_epsg: i32, to_epsg: i32) -> Result<Self> {
        Ok(Self {
            src: cached_proj(func, from_epsg)?,
            dst: cached_proj(func, to_epsg)?,
            func,
            from: from_epsg,
            to: to_epsg,
        })
    }

    /// Run the transform, letting `drive` present every coordinate.
    ///
    /// `drive` receives a closure that takes and returns `(x, y, z)` in kenro's
    /// units — degrees for a geographic CRS, with the radian conversion proj4rs
    /// wants applied on both sides here so no caller has to remember it.
    pub fn run<D>(&self, drive: D) -> Result<()>
    where
        D: FnOnce(&mut dyn FnMut(f64, f64, f64) -> Option<(f64, f64, f64)>),
    {
        let (src_ll, dst_ll) = (self.src.is_latlong(), self.dst.is_latlong());
        let mut failed = false;
        let mut step = |x: f64, y: f64, z: f64| -> Option<(f64, f64, f64)> {
            let (x, y) = if src_ll {
                (x.to_radians(), y.to_radians())
            } else {
                (x, y)
            };
            let mut point = OneCoord(x, y, z);
            if proj4rs::transform::transform(&self.src, &self.dst, &mut point).is_err() {
                failed = true;
                return None;
            }
            let (x, y) = if dst_ll {
                (point.0.to_degrees(), point.1.to_degrees())
            } else {
                (point.0, point.1)
            };
            Some((x, y, point.2))
        };
        drive(&mut step);
        if failed {
            return Err(Error::Unsupported {
                func: self.func,
                reason: format!(
                    "transform from EPSG:{} to EPSG:{} failed",
                    self.from, self.to
                ),
            });
        }
        Ok(())
    }
}

/// One coordinate, so proj4rs can be asked to transform exactly one.
struct OneCoord(f64, f64, f64);

impl proj4rs::transform::Transform for OneCoord {
    fn transform_coordinates<F>(&mut self, f: &mut F) -> proj4rs::errors::Result<()>
    where
        F: proj4rs::transform::TransformClosure,
    {
        f(self.0, self.1, self.2).map(|(x, y, z)| {
            self.0 = x;
            self.1 = y;
            self.2 = z;
        })
    }
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
    const SYDNEY_OPERA: (f64, f64) = (151.215, -33.8568);

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
        let codes: Vec<i32> = [4326, 3857]
            .into_iter()
            .chain(32601..=32660)
            .chain(32701..=32760)
            .collect();
        assert_eq!(codes.len(), 122);
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
    fn utm_north_plausibility_and_roundtrip() {
        // Tokyo is in UTM zone 54N; easting ~380 km (west of the 141°E
        // central meridian), northing ~3.94 Mm.
        let (x, y) = transform_point(TOKYO_TOWER.0, TOKYO_TOWER.1, 4326, 32654);
        assert!((300_000.0..500_000.0).contains(&x), "easting {x}");
        assert!((3_800_000.0..4_050_000.0).contains(&y), "northing {y}");
        let (lon, lat) = transform_point(x, y, 32654, 4326);
        assert!((lon - TOKYO_TOWER.0).abs() < 1e-9, "{lon}");
        assert!((lat - TOKYO_TOWER.1).abs() < 1e-9, "{lat}");
    }

    #[test]
    fn utm_south_has_the_false_northing() {
        // Sydney is in UTM zone 56S; the 10 Mm false northing keeps
        // southern-hemisphere northings positive (~6.25 Mm here).
        let (x, y) = transform_point(SYDNEY_OPERA.0, SYDNEY_OPERA.1, 4326, 32756);
        assert!((300_000.0..400_000.0).contains(&x), "easting {x}");
        assert!((6_200_000.0..6_300_000.0).contains(&y), "northing {y}");
        let (lon, lat) = transform_point(x, y, 32756, 4326);
        assert!((lon - SYDNEY_OPERA.0).abs() < 1e-9, "{lon}");
        assert!((lat - SYDNEY_OPERA.1).abs() < 1e-9, "{lat}");
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
        assert!(msg.contains("crs-full"), "{msg}");
    }
}
