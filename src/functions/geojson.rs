//! GeoJSON I/O.
//!
//! The emitter is kenro's own (~150 lines): geozero's writer has no
//! precision control and puts spaces after colons, while PostGIS emits
//! compact JSON with `maxdecimaldigits` (default 9) — and matching PostGIS
//! output byte-for-byte is the point. Parsing uses the `geojson` crate.

use geo_types::{Geometry, LineString, Polygon};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

/// `ST_AsGeoJSON(geom [, maxdecimaldigits])`. Default 9 digits (PostGIS).
/// A `crs` member is included iff the SRID is neither 0 nor 4326 (PostGIS
/// default options=8 "short CRS when not EPSG:4326").
pub fn st_as_geojson(bytes: &[u8], maxdecimaldigits: Option<i64>) -> Result<String> {
    decoded::st_as_geojson(&geom::decode_auto(bytes)?, maxdecimaldigits)
}

/// GeoJSON output for a geometry that is already decoded — see
/// [`crate::functions::predicates::decoded`] for why that exists.
pub mod decoded {
    use super::*;

    pub fn st_as_geojson(geom: &Geom, maxdecimaldigits: Option<i64>) -> Result<String> {
        if geom.has_zm {
            return Err(Error::Unsupported {
                func: "ST_AsGeoJSON",
                reason: "3D/M output is not supported in kenro 0.1; predicates and \
                         R-tree functions accept 3D input"
                    .into(),
            });
        }
        let digits = maxdecimaldigits.unwrap_or(9).clamp(0, 15) as i32;
        let mut out = String::new();
        write_geometry(&mut out, geom, digits, true);
        Ok(out)
    }
}

/// `ST_GeomFromGeoJSON(text)` → geometry with SRID 4326 (PostGIS ≥ 3.0,
/// per RFC 7946).
pub fn st_geom_from_geojson(s: &str) -> Result<Vec<u8>> {
    let gj: geojson::GeoJson = s
        .parse()
        .map_err(|e: geojson::Error| Error::InvalidGeoJson(e.to_string()))?;
    let geojson::GeoJson::Geometry(g) = gj else {
        return Err(Error::Unsupported {
            func: "ST_GeomFromGeoJSON",
            reason: "expects a Geometry object; Feature/FeatureCollection input is not \
                     accepted (PostGIS raises here too)"
                .into(),
        });
    };
    if value_has_third_dim(&g.value) {
        return Err(Error::Unsupported {
            func: "ST_GeomFromGeoJSON",
            reason: "3D positions are not supported in kenro 0.1 (PostGIS keeps Z; kenro \
                     refuses rather than silently dropping it)"
                .into(),
        });
    }
    let geometry =
        Geometry::<f64>::try_from(g.value).map_err(|e| Error::InvalidGeoJson(e.to_string()))?;
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid: 4326,
            has_zm: false,
        },
        "ST_GeomFromGeoJSON",
    )
}

fn value_has_third_dim(value: &geojson::Value) -> bool {
    use geojson::Value;
    fn pos(p: &[f64]) -> bool {
        p.len() > 2
    }
    match value {
        Value::Point(p) => pos(p),
        Value::MultiPoint(ps) | Value::LineString(ps) => ps.iter().any(|p| pos(p)),
        Value::MultiLineString(ls) | Value::Polygon(ls) => ls.iter().flatten().any(|p| pos(p)),
        Value::MultiPolygon(polys) => polys.iter().flatten().flatten().any(|p| pos(p)),
        Value::GeometryCollection(gs) => gs.iter().any(|g| value_has_third_dim(&g.value)),
    }
}

/// Round to `digits` decimals, then print the shortest representation
/// (Rust's f64 Display — same Ryu family as PostGIS ≥ 3.1).
fn fmt_num(out: &mut String, x: f64, digits: i32) {
    let factor = 10f64.powi(digits);
    let mut r = (x * factor).round() / factor;
    if r == 0.0 {
        r = 0.0; // normalize -0
    }
    out.push_str(&format!("{r}"));
}

fn write_pos(out: &mut String, x: f64, y: f64, digits: i32) {
    out.push('[');
    fmt_num(out, x, digits);
    out.push(',');
    fmt_num(out, y, digits);
    out.push(']');
}

fn write_seq(out: &mut String, coords: impl Iterator<Item = (f64, f64)>, digits: i32) {
    out.push('[');
    for (i, (x, y)) in coords.enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_pos(out, x, y, digits);
    }
    out.push(']');
}

fn write_ring_list<'a>(
    out: &mut String,
    rings: impl Iterator<Item = &'a LineString<f64>>,
    digits: i32,
) {
    out.push('[');
    for (i, ring) in rings.enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_seq(out, ring.coords().map(|c| (c.x, c.y)), digits);
    }
    out.push(']');
}

fn polygon_rings(p: &Polygon<f64>) -> impl Iterator<Item = &LineString<f64>> {
    std::iter::once(p.exterior()).chain(p.interiors().iter())
}

fn write_geometry(out: &mut String, geom: &Geom, digits: i32, top_level: bool) {
    let g = &geom.geometry;
    out.push_str("{\"type\":\"");
    out.push_str(geojson_type_name(g));
    out.push('"');
    if top_level && geom.srid != 0 && geom.srid != 4326 {
        out.push_str(&format!(
            ",\"crs\":{{\"type\":\"name\",\"properties\":{{\"name\":\"EPSG:{}\"}}}}",
            geom.srid
        ));
    }
    match g {
        Geometry::GeometryCollection(gc) => {
            out.push_str(",\"geometries\":[");
            for (i, member) in gc.0.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let member_geom = Geom {
                    geometry: member.clone(),
                    srid: geom.srid,
                    has_zm: false,
                };
                write_geometry(out, &member_geom, digits, false);
            }
            out.push(']');
        }
        _ => {
            out.push_str(",\"coordinates\":");
            write_coordinates(out, g, digits);
        }
    }
    out.push('}');
}

fn write_coordinates(out: &mut String, g: &Geometry<f64>, digits: i32) {
    if geom::is_empty(g) {
        out.push_str("[]");
        return;
    }
    match g {
        Geometry::Point(p) => write_pos(out, p.x(), p.y(), digits),
        Geometry::MultiPoint(mp) => write_seq(out, mp.iter().map(|p| (p.x(), p.y())), digits),
        Geometry::Line(l) => write_seq(
            out,
            [(l.start.x, l.start.y), (l.end.x, l.end.y)].into_iter(),
            digits,
        ),
        Geometry::LineString(ls) => write_seq(out, ls.coords().map(|c| (c.x, c.y)), digits),
        Geometry::MultiLineString(mls) => {
            write_ring_list(out, mls.0.iter(), digits);
        }
        Geometry::Polygon(p) => write_ring_list(out, polygon_rings(p), digits),
        Geometry::Rect(r) => write_ring_list(
            out,
            std::iter::once(&r.to_polygon().exterior().clone()),
            digits,
        ),
        Geometry::Triangle(t) => write_ring_list(
            out,
            std::iter::once(&t.to_polygon().exterior().clone()),
            digits,
        ),
        Geometry::MultiPolygon(mp) => {
            out.push('[');
            for (i, p) in mp.0.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_ring_list(out, polygon_rings(p), digits);
            }
            out.push(']');
        }
        Geometry::GeometryCollection(_) => unreachable!("handled in write_geometry"),
    }
}

fn geojson_type_name(g: &Geometry<f64>) -> &'static str {
    match g {
        Geometry::Point(_) => "Point",
        Geometry::Line(_) | Geometry::LineString(_) => "LineString",
        Geometry::Polygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) => "Polygon",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GeometryCollection",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text, st_srid};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    #[test]
    fn emits_compact_postgis_shaped_json() {
        assert_eq!(
            st_as_geojson(&g("POINT(1 2)"), None).unwrap(),
            r#"{"type":"Point","coordinates":[1,2]}"#
        );
        assert_eq!(
            st_as_geojson(&g("LINESTRING(0 0,1 1)"), None).unwrap(),
            r#"{"type":"LineString","coordinates":[[0,0],[1,1]]}"#
        );
        assert_eq!(
            st_as_geojson(
                &g("POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))"),
                None
            )
            .unwrap(),
            r#"{"type":"Polygon","coordinates":[[[0,0],[4,0],[4,4],[0,4],[0,0]],[[1,1],[2,1],[2,2],[1,2],[1,1]]]}"#
        );
        assert_eq!(
            st_as_geojson(&g("GEOMETRYCOLLECTION(POINT(1 2))"), None).unwrap(),
            r#"{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1,2]}]}"#
        );
    }

    #[test]
    fn digit_rounding() {
        let p = g("POINT(139.745433012345 35.65)");
        assert_eq!(
            st_as_geojson(&p, Some(3)).unwrap(),
            r#"{"type":"Point","coordinates":[139.745,35.65]}"#
        );
        assert_eq!(
            st_as_geojson(&p, Some(0)).unwrap(),
            r#"{"type":"Point","coordinates":[140,36]}"#
        );
    }

    #[test]
    fn crs_member_only_for_non_wgs84_srid() {
        let plain = st_geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        assert!(!st_as_geojson(&plain, None).unwrap().contains("crs"));
        let mercator = st_geom_from_text("POINT(1 2)", Some(3857)).unwrap();
        assert_eq!(
            st_as_geojson(&mercator, None).unwrap(),
            r#"{"type":"Point","crs":{"type":"name","properties":{"name":"EPSG:3857"}},"coordinates":[1,2]}"#
        );
    }

    #[test]
    fn empty_geometry_gets_empty_coordinates() {
        assert_eq!(
            st_as_geojson(&g("LINESTRING EMPTY"), None).unwrap(),
            r#"{"type":"LineString","coordinates":[]}"#
        );
    }

    #[test]
    fn parse_roundtrip_and_srid() {
        let blob = st_geom_from_geojson(r#"{"type":"Point","coordinates":[139.7,35.7]}"#).unwrap();
        assert_eq!(st_as_text(&blob).unwrap(), "POINT(139.7 35.7)");
        assert_eq!(st_srid(&blob).unwrap(), 4326);
    }

    #[test]
    fn parse_rejects_features_3d_and_garbage() {
        let feature =
            r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{}}"#;
        assert!(st_geom_from_geojson(feature).is_err());
        assert!(st_geom_from_geojson(r#"{"type":"Point","coordinates":[1,2,3]}"#).is_err());
        assert!(st_geom_from_geojson("not json").is_err());
    }
}
