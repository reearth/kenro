//! Mapbox Vector Tile functions: `ST_AsMVTGeom` (world → tile-space
//! transform with optional clipping) and the `ST_AsMVT` aggregate (features
//! → encoded tile layer).
//!
//! `ST_AsMVT`'s signature deliberately diverges from PostGIS: SQLite has no
//! record type, so instead of `ST_AsMVT(row, name, extent, geom_column)` the
//! aggregate takes `(geom [, name [, extent [, props_json]]])` where
//! `props_json` is a JSON object (build it with `json_object(...)`). A
//! PostGIS-style call fails loudly at the type level — never silently.

use geo::BooleanOps;
use geo_types::{Coord, Geometry, LineString, MultiPolygon, Point, Polygon, Rect};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};
use crate::mvt;

use super::overlay::{
    Class, classify, ensure_finite, normalize_lines, normalize_points, normalize_polygons,
    points_of, to_multi_line, to_multi_polygon,
};

// ---- degenerate cleanup on tile-space (integer) coordinates ----

/// Collapse consecutive duplicate coordinates after rounding.
fn dedup_rounded(coords: &[Coord<f64>]) -> Vec<Coord<f64>> {
    let mut out: Vec<Coord<f64>> = Vec::with_capacity(coords.len());
    for c in coords {
        let r = Coord {
            x: c.x.round(),
            y: c.y.round(),
        };
        if out.last() != Some(&r) {
            out.push(r);
        }
    }
    out
}

fn clean_line(ls: &LineString<f64>) -> Option<LineString<f64>> {
    let coords = dedup_rounded(&ls.0);
    (coords.len() >= 2).then(|| LineString::new(coords))
}

fn clean_ring(ring: &LineString<f64>) -> Option<LineString<f64>> {
    let mut coords = dedup_rounded(&ring.0);
    // Re-close after dedup (rounding can merge the closing point away).
    if coords.first() != coords.last() {
        if let Some(&first) = coords.first() {
            coords.push(first);
        }
    }
    if coords.len() < 4 {
        return None;
    }
    let ls = LineString::new(coords);
    use geo::Area;
    let area = Polygon::new(ls.clone(), vec![]).unsigned_area();
    (area > 0.0).then_some(ls)
}

fn clean_polygon(p: &Polygon<f64>) -> Option<Polygon<f64>> {
    let exterior = clean_ring(p.exterior())?;
    let interiors = p.interiors().iter().filter_map(clean_ring).collect();
    Some(Polygon::new(exterior, interiors))
}

/// Round a tile-space geometry to the integer grid and drop pieces the
/// rounding degenerated (zero-length lines, zero-area rings). `None` means
/// nothing survived — SQL NULL.
fn clean_tile_geometry(g: &Geometry<f64>) -> Option<Geometry<f64>> {
    let cleaned = match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => {
            let points: Vec<Point<f64>> = points_of(g)
                .into_iter()
                .map(|p| Point::new(p.x().round(), p.y().round()))
                .collect();
            if points.is_empty() {
                return None;
            }
            normalize_points(points)
        }
        Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_) => {
            let lines: Vec<LineString<f64>> =
                to_multi_line(g).0.iter().filter_map(clean_line).collect();
            if lines.is_empty() {
                return None;
            }
            normalize_lines(geo_types::MultiLineString(lines))
        }
        _ => {
            let polys: Vec<Polygon<f64>> = to_multi_polygon(g)
                .0
                .iter()
                .filter_map(clean_polygon)
                .collect();
            if polys.is_empty() {
                return None;
            }
            normalize_polygons(MultiPolygon(polys))
        }
    };
    Some(cleaned)
}

// ---- ST_AsMVTGeom ----

/// `ST_AsMVTGeom(geom, bounds [, extent [, buffer [, clip]]])` — transform a
/// world-space geometry into integer tile coordinates (Y down). `bounds` is
/// any geometry whose envelope defines the tile. Returns `None` (SQL NULL)
/// when nothing remains inside the tile, like PostGIS.
pub fn st_as_mvt_geom(
    bytes: &[u8],
    bounds: &[u8],
    extent: Option<i32>,
    buffer: Option<i32>,
    clip: Option<i32>,
) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_AsMVTGeom";
    let extent = extent.unwrap_or(4096);
    if extent <= 0 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!("extent must be positive, got {extent}"),
        });
    }
    let buffer = buffer.unwrap_or(256);
    if buffer < 0 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: format!("buffer must be non-negative, got {buffer}"),
        });
    }
    let clip = clip.map(|v| v != 0).unwrap_or(true);

    let gb = geom::decode_auto(bounds)?;
    let env = geom::envelope(&gb.geometry).ok_or_else(|| Error::Unsupported {
        func: FUNC,
        reason: "bounds must be a non-empty geometry".into(),
    })?;
    let (width, height) = (env.max_x - env.min_x, env.max_y - env.min_y);
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "bounds envelope is degenerate (zero width or height)".into(),
        });
    }

    let g = geom::decode_auto(bytes)?;
    if geom::is_empty(&g.geometry) {
        return Ok(None);
    }
    let class = classify(FUNC, &g.geometry)?;
    ensure_finite(FUNC, &g.geometry)?;

    let mut geometry = g.geometry;
    if clip {
        let bx = buffer as f64 / extent as f64 * width;
        let by = buffer as f64 / extent as f64 * height;
        let rect = Rect::new(
            Coord {
                x: env.min_x - bx,
                y: env.min_y - by,
            },
            Coord {
                x: env.max_x + bx,
                y: env.max_y + by,
            },
        );
        geometry = match class {
            Class::Puntal => {
                let inside: Vec<Point<f64>> = points_of(&geometry)
                    .into_iter()
                    .filter(|p| {
                        p.x() >= rect.min().x
                            && p.x() <= rect.max().x
                            && p.y() >= rect.min().y
                            && p.y() <= rect.max().y
                    })
                    .collect();
                if inside.is_empty() {
                    return Ok(None);
                }
                normalize_points(inside)
            }
            Class::Lineal => {
                let clip_poly = MultiPolygon(vec![rect.to_polygon()]);
                let clipped = clip_poly.clip(&to_multi_line(&geometry), false);
                let result = normalize_lines(clipped);
                if geom::is_empty(&result) {
                    return Ok(None);
                }
                result
            }
            Class::Areal => {
                let clip_poly = MultiPolygon(vec![rect.to_polygon()]);
                let clipped = clip_poly.intersection(&to_multi_polygon(&geometry));
                let result = normalize_polygons(clipped);
                if geom::is_empty(&result) {
                    return Ok(None);
                }
                result
            }
        };
    }

    use geo::MapCoords;
    let (fx, fy) = (f64::from(extent) / width, f64::from(extent) / height);
    let tile = geometry.map_coords(|c| Coord {
        x: ((c.x - env.min_x) * fx).round(),
        y: ((env.max_y - c.y) * fy).round(),
    });

    match clean_tile_geometry(&tile) {
        None => Ok(None),
        Some(geometry) => geom::encode_canonical_gpb(
            &Geom {
                geometry,
                srid: 0,
                has_zm: false,
            },
            FUNC,
        )
        .map(Some),
    }
}

// ---- ST_AsMVT aggregate ----

fn parse_props(json: &str) -> Result<Vec<(String, mvt::Value)>> {
    const FUNC: &str = "ST_AsMVT";
    let parsed: serde_json::Value = serde_json::from_str(json).map_err(|e| Error::Unsupported {
        func: FUNC,
        reason: format!("properties must be valid JSON (use json_object(...)): {e}"),
    })?;
    let object = parsed.as_object().ok_or_else(|| Error::Unsupported {
        func: FUNC,
        reason: "properties must be a JSON object (use json_object(...))".into(),
    })?;
    let mut props = Vec::with_capacity(object.len());
    for (key, value) in object {
        let value = match value {
            serde_json::Value::Null => continue, // NULL property → omitted, like PostGIS
            serde_json::Value::Bool(b) => mvt::Value::Bool(*b),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(i) => mvt::Value::Int(i),
                None => mvt::Value::Double(n.as_f64().unwrap_or(f64::NAN)),
            },
            serde_json::Value::String(s) => mvt::Value::Str(s.clone()),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                return Err(Error::Unsupported {
                    func: FUNC,
                    reason: format!(
                        "property {key:?} has a nested JSON value; MVT properties \
                                     are scalar (string, number, boolean)"
                    ),
                });
            }
        };
        props.push((key.clone(), value));
    }
    Ok(props)
}

/// Accumulator behind the `ST_AsMVT(geom [, name [, extent [, props_json]]])`
/// aggregate. Layer name and extent must be constant within one aggregation
/// group; geometries must already be in tile space (`ST_AsMVTGeom` output).
pub struct MvtAggregate {
    layer_name: Option<String>,
    extent: Option<i32>,
    features: Vec<mvt::Feature>,
}

impl MvtAggregate {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        MvtAggregate {
            layer_name: None,
            extent: None,
            features: Vec::new(),
        }
    }

    pub fn step(
        &mut self,
        geom: &[u8],
        name: Option<&str>,
        extent: Option<i32>,
        props: Option<&str>,
    ) -> Result<()> {
        const FUNC: &str = "ST_AsMVT";
        let name = name.unwrap_or("default");
        let extent = extent.unwrap_or(4096);
        if extent <= 0 {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: format!("extent must be positive, got {extent}"),
            });
        }
        match &self.layer_name {
            None => {
                self.layer_name = Some(name.to_string());
                self.extent = Some(extent);
            }
            Some(existing) => {
                if existing != name || self.extent != Some(extent) {
                    return Err(Error::Unsupported {
                        func: FUNC,
                        reason: "layer name and extent must be constant within one \
                                 aggregation group"
                            .into(),
                    });
                }
            }
        }
        let g = geom::decode_auto(geom)?;
        if geom::is_empty(&g.geometry) {
            return Ok(()); // empty geometries contribute no feature
        }
        classify(FUNC, &g.geometry)?; // reject GeometryCollection loudly
        ensure_finite(FUNC, &g.geometry)?;
        // Snap to the integer grid and drop rounding degenerates, so hostile
        // (non-ST_AsMVTGeom) input cannot produce malformed command streams.
        let Some(geometry) = clean_tile_geometry(&g.geometry) else {
            return Ok(());
        };
        let props = match props {
            None => Vec::new(),
            Some(json) => parse_props(json)?,
        };
        self.features.push(mvt::Feature { geometry, props });
        Ok(())
    }

    /// `None` = SQL NULL (zero input rows). Rows whose geometries all
    /// degenerated still produce a valid empty layer.
    pub fn finish(self) -> Result<Option<Vec<u8>>> {
        let Some(name) = self.layer_name else {
            return Ok(None);
        };
        let extent = self.extent.unwrap_or(4096) as u32;
        mvt::encode_tile("ST_AsMVT", &name, extent, &self.features).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::decode_wkt;

    fn gpb(wkt: &str) -> Vec<u8> {
        crate::geom::encode_canonical_gpb(&decode_wkt(wkt, 0).unwrap(), "test").unwrap()
    }

    fn as_wkt(bytes: &[u8]) -> String {
        crate::geom::encode_wkt(&crate::geom::decode_auto(bytes).unwrap(), "test").unwrap()
    }

    #[test]
    fn point_transforms_into_tile_space() {
        // Tile bounds (0,0)-(100,100), extent 10: point (50, 90) → (5, 1)
        // after the Y flip.
        let out = st_as_mvt_geom(
            &gpb("POINT(50 90)"),
            &gpb("POLYGON((0 0,100 0,100 100,0 100,0 0))"),
            Some(10),
            Some(0),
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(as_wkt(&out), "POINT(5 1)");
    }

    #[test]
    fn outside_geometry_becomes_null() {
        let out = st_as_mvt_geom(
            &gpb("POINT(500 500)"),
            &gpb("POLYGON((0 0,100 0,100 100,0 100,0 0))"),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn clip_false_keeps_outside_geometry() {
        let out = st_as_mvt_geom(
            &gpb("POINT(200 0)"),
            &gpb("POLYGON((0 0,100 0,100 100,0 100,0 0))"),
            Some(10),
            Some(0),
            Some(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(as_wkt(&out), "POINT(20 10)");
    }

    #[test]
    fn polygon_is_clipped_to_buffered_bounds() {
        let out = st_as_mvt_geom(
            &gpb("POLYGON((50 50,150 50,150 60,50 60,50 50))"),
            &gpb("POLYGON((0 0,100 0,100 100,0 100,0 0))"),
            Some(100),
            Some(0),
            None,
        )
        .unwrap()
        .unwrap();
        let decoded = crate::geom::decode_auto(&out).unwrap();
        use geo::Area;
        // Clipped to x ∈ [50,100] world = 50×10 world units = 50×10 tile
        // units at extent 100 over a 100-wide tile.
        assert_eq!(decoded.geometry.unsigned_area(), 500.0);
    }

    #[test]
    fn degenerate_after_rounding_is_null() {
        // A sliver far thinner than one tile pixel collapses to zero area.
        let out = st_as_mvt_geom(
            &gpb("POLYGON((10 10,20 10,20 10.001,10 10.001,10 10))"),
            &gpb("POLYGON((0 0,100 0,100 100,0 100,0 0))"),
            Some(100),
            Some(0),
            None,
        )
        .unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn aggregate_builds_a_decodable_layer() {
        let mut acc = MvtAggregate::new();
        acc.step(
            &gpb("POINT(10 10)"),
            Some("parks"),
            Some(4096),
            Some(r#"{"name":"yoyogi","area":54.3,"open":true,"count":7,"skip":null}"#),
        )
        .unwrap();
        acc.step(
            &gpb("LINESTRING(0 0,10 10)"),
            Some("parks"),
            Some(4096),
            None,
        )
        .unwrap();
        let tile = acc.finish().unwrap().unwrap();
        // Tile → layers field (3, len-delim).
        assert_eq!(tile[0], 0x1a);
        // The layer must contain the name and the deduped keys.
        let hay = String::from_utf8_lossy(&tile);
        assert!(hay.contains("parks"));
        assert!(hay.contains("yoyogi"));
        assert!(hay.contains("count"));
        assert!(!hay.contains("skip"), "null props must be omitted");
    }

    #[test]
    fn aggregate_rejects_changing_layer_name() {
        let mut acc = MvtAggregate::new();
        acc.step(&gpb("POINT(1 1)"), Some("a"), None, None).unwrap();
        let err = acc.step(&gpb("POINT(2 2)"), Some("b"), None, None);
        assert!(err.is_err());
    }

    #[test]
    fn aggregate_zero_rows_is_null() {
        assert!(MvtAggregate::new().finish().unwrap().is_none());
    }

    #[test]
    fn aggregate_rejects_nested_props() {
        let mut acc = MvtAggregate::new();
        let err = acc.step(&gpb("POINT(1 1)"), None, None, Some(r#"{"a":[1,2]}"#));
        assert!(err.is_err());
    }
}
