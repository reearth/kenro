//! `ST_AsKML` — KML 2.2 geometry.
//!
//! The surprise here, and the reason this is not "GML with different tags":
//! **KML is defined in WGS84 and PostGIS reprojects to it**. `ST_AsKML` on a
//! geometry in EPSG:3857 does not label the output 3857, it transforms the
//! coordinates; on a geometry with SRID 0 it refuses outright, with the same
//! "unknown (0) SRID" wording `ST_Transform` uses. Measured, not assumed —
//! `ST_AsGML` right next door does the opposite, labelling `srsName` and
//! leaving the numbers alone.
//!
//! kenro keeps that behaviour, which is why this function needs the
//! `transform` feature. The reprojection is kenro's gridless one, so a KML
//! export inherits [the accuracy note](../../docs/accuracy.md) — sub-metre
//! for the curated EPSG table, not survey-grade.
//!
//! No XML library is involved. The only text that is not a number is the
//! caller's namespace prefix, and that is validated rather than escaped: a
//! prefix is an XML name or it is rejected, because escaping it would
//! produce a document with a literally-named `&lt;script` element rather
//! than an error.

use geo_types::{Geometry, LineString, Polygon};

use crate::error::{Error, Result};
use crate::functions::num;
use crate::geom;

const FUNC: &str = "ST_AsKML";

/// `ST_AsKML(geom [, maxdecimaldigits [, nprefix]])`.
///
/// ⚠️ **Divergences from PostGIS.** 3D input is an error rather than a third
/// ordinate in `<coordinates>`: kenro's geometry model has no Z, and every
/// other encoder refuses rather than silently writing 2D (`ST_Force2D` is
/// the opt-in). PostGIS emits `x,y,z` here. Everything else — the tag set,
/// `outerBoundaryIs`/`innerBoundaryIs`, the closing vertex kept on every
/// ring, `MultiGeometry` for all three multi types, the empty string for an
/// empty geometry, and the refusal of a GeometryCollection — matches.
pub fn st_as_kml(bytes: &[u8], digits: Option<i64>, prefix: Option<&str>) -> Result<String> {
    let mut g = geom::decode_auto(bytes)?;
    if g.has_zm {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "3D/M output is not supported in kenro; use ST_Force2D".into(),
        });
    }
    let prefix = match prefix {
        None | Some("") => String::new(),
        Some(p) => {
            if !is_xml_name(p) {
                return Err(Error::Unsupported {
                    func: FUNC,
                    reason: format!(
                        "namespace prefix {p:?} is not an XML name; it is written into the \
                         output verbatim, so it must be one"
                    ),
                });
            }
            format!("{p}:")
        }
    };
    let digits = digits.unwrap_or(15).clamp(0, 15) as usize;

    // KML is WGS84 by definition and PostGIS transforms rather than labels.
    // SRID 0 is refused for the same reason ST_Transform refuses it: there
    // is no way to know what the numbers mean.
    if g.srid != 4326 {
        crate::functions::transform::decoded::st_transform_in_place(&mut g, 4326).map_err(|e| {
            match e {
                Error::Unsupported { reason, .. } => Error::Unsupported { func: FUNC, reason },
                other => other,
            }
        })?;
    }

    let mut out = String::new();
    write_geometry(&mut out, &g.geometry, digits, &prefix)?;
    Ok(out)
}

fn write_geometry(out: &mut String, g: &Geometry<f64>, d: usize, p: &str) -> Result<()> {
    match g {
        Geometry::Point(pt) => {
            if !pt.0.x.is_finite() || !pt.0.y.is_finite() {
                return Ok(()); // an empty POINT decodes to nothing printable
            }
            out.push_str(&format!(
                "<{p}Point><{p}coordinates>{},{}</{p}coordinates></{p}Point>",
                num(pt.0.x, d),
                num(pt.0.y, d)
            ));
        }
        Geometry::Line(l) => write_line(out, &LineString::new(vec![l.start, l.end]), d, p),
        Geometry::LineString(l) => write_line(out, l, d, p),
        Geometry::Polygon(poly) => write_polygon(out, poly, d, p),
        Geometry::Rect(r) => write_polygon(out, &r.to_polygon(), d, p),
        Geometry::Triangle(t) => write_polygon(out, &t.to_polygon(), d, p),
        Geometry::MultiPoint(mp) => {
            if mp.0.is_empty() {
                return Ok(());
            }
            out.push_str(&format!("<{p}MultiGeometry>"));
            for pt in &mp.0 {
                write_geometry(out, &Geometry::Point(*pt), d, p)?;
            }
            out.push_str(&format!("</{p}MultiGeometry>"));
        }
        Geometry::MultiLineString(mls) => {
            if mls.0.is_empty() {
                return Ok(());
            }
            out.push_str(&format!("<{p}MultiGeometry>"));
            for l in &mls.0 {
                write_line(out, l, d, p);
            }
            out.push_str(&format!("</{p}MultiGeometry>"));
        }
        Geometry::MultiPolygon(mp) => {
            if mp.0.is_empty() {
                return Ok(());
            }
            out.push_str(&format!("<{p}MultiGeometry>"));
            for poly in &mp.0 {
                write_polygon(out, poly, d, p);
            }
            out.push_str(&format!("</{p}MultiGeometry>"));
        }
        Geometry::GeometryCollection(_) => {
            // PostGIS: "lwgeom_to_kml2: 'GeometryCollection' geometry type
            // not supported". KML's MultiGeometry could hold one, but the
            // reference implementation declines and so does kenro.
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "GeometryCollection is not supported (PostGIS refuses this too)".into(),
            });
        }
    }
    Ok(())
}

fn write_line(out: &mut String, l: &LineString<f64>, d: usize, p: &str) {
    if l.0.is_empty() {
        return;
    }
    out.push_str(&format!("<{p}LineString><{p}coordinates>"));
    write_coords(out, l, d);
    out.push_str(&format!("</{p}coordinates></{p}LineString>"));
}

fn write_polygon(out: &mut String, poly: &Polygon<f64>, d: usize, p: &str) {
    if poly.exterior().0.is_empty() {
        return;
    }
    out.push_str(&format!(
        "<{p}Polygon><{p}outerBoundaryIs><{p}LinearRing><{p}coordinates>"
    ));
    write_coords(out, poly.exterior(), d);
    out.push_str(&format!(
        "</{p}coordinates></{p}LinearRing></{p}outerBoundaryIs>"
    ));
    for ring in poly.interiors() {
        out.push_str(&format!(
            "<{p}innerBoundaryIs><{p}LinearRing><{p}coordinates>"
        ));
        write_coords(out, ring, d);
        out.push_str(&format!(
            "</{p}coordinates></{p}LinearRing></{p}innerBoundaryIs>"
        ));
    }
    out.push_str(&format!("</{p}Polygon>"));
}

/// `x,y` pairs separated by spaces — and the ring's closing vertex is kept,
/// unlike SVG's `Z`, because KML has no close command.
fn write_coords(out: &mut String, l: &LineString<f64>, d: usize) {
    for (i, c) in l.0.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{},{}", num(c.x, d), num(c.y, d)));
    }
}

/// XML 1.0 `Name`, restricted to ASCII: enough to accept every prefix anyone
/// writes (`kml`, `gx`) and reject anything that would break the document.
fn is_xml_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_geom_from_text, st_set_srid};

    fn g(wkt: &str) -> Vec<u8> {
        st_set_srid(&st_geom_from_text(wkt, None).unwrap(), 4326).unwrap()
    }

    fn kml(wkt: &str) -> String {
        st_as_kml(&g(wkt), None, None).unwrap()
    }

    /// Byte-for-byte against PostGIS 3.5, read off a live session.
    #[test]
    fn output_is_byte_identical_to_postgis() {
        assert_eq!(
            kml("POINT(1 2)"),
            "<Point><coordinates>1,2</coordinates></Point>"
        );
        assert_eq!(
            kml("POINT(1.23456789 2.3456789)"),
            "<Point><coordinates>1.23456789,2.3456789</coordinates></Point>"
        );
        assert_eq!(
            kml("LINESTRING(0 0,1 1,2 0)"),
            "<LineString><coordinates>0,0 1,1 2,0</coordinates></LineString>"
        );
        // The closing vertex stays: KML has no close command.
        assert_eq!(
            kml("POLYGON((0 0,4 0,4 4,0 4,0 0))"),
            "<Polygon><outerBoundaryIs><LinearRing><coordinates>0,0 4,0 4,4 0,4 0,0\
             </coordinates></LinearRing></outerBoundaryIs></Polygon>"
        );
        assert_eq!(
            kml("POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))"),
            "<Polygon><outerBoundaryIs><LinearRing><coordinates>0,0 4,0 4,4 0,4 0,0\
             </coordinates></LinearRing></outerBoundaryIs><innerBoundaryIs><LinearRing>\
             <coordinates>1,1 2,1 2,2 1,2 1,1</coordinates></LinearRing></innerBoundaryIs>\
             </Polygon>"
        );
        // All three multi types become MultiGeometry.
        assert_eq!(
            kml("MULTIPOINT(0 0,1 1)"),
            "<MultiGeometry><Point><coordinates>0,0</coordinates></Point>\
             <Point><coordinates>1,1</coordinates></Point></MultiGeometry>"
        );
        assert_eq!(
            kml("MULTILINESTRING((0 0,1 1),(2 2,3 3))"),
            "<MultiGeometry><LineString><coordinates>0,0 1,1</coordinates></LineString>\
             <LineString><coordinates>2,2 3,3</coordinates></LineString></MultiGeometry>"
        );
        assert_eq!(
            kml("MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))"),
            "<MultiGeometry><Polygon><outerBoundaryIs><LinearRing><coordinates>0,0 1,0 1,1 0,0\
             </coordinates></LinearRing></outerBoundaryIs></Polygon><Polygon><outerBoundaryIs>\
             <LinearRing><coordinates>5,5 6,5 6,6 5,5</coordinates></LinearRing>\
             </outerBoundaryIs></Polygon></MultiGeometry>"
        );
        // Empty geometries print nothing at all, not an empty element.
        assert_eq!(kml("LINESTRING EMPTY"), "");
        assert_eq!(kml("POLYGON EMPTY"), "");
        // A GeometryCollection is refused, as in PostGIS.
        assert!(st_as_kml(&g("GEOMETRYCOLLECTION(POINT(0 0))"), None, None).is_err());
    }

    #[test]
    fn digits_and_prefix_match_postgis() {
        assert_eq!(
            st_as_kml(&g("POINT(1.23456789 2.3456789)"), Some(3), None).unwrap(),
            "<Point><coordinates>1.235,2.346</coordinates></Point>"
        );
        assert_eq!(
            st_as_kml(&g("POINT(1 2)"), Some(15), Some("kml")).unwrap(),
            "<kml:Point><kml:coordinates>1,2</kml:coordinates></kml:Point>"
        );
        // Every tag takes the prefix, including the boundary wrappers.
        assert_eq!(
            st_as_kml(&g("POLYGON((0 0,1 0,1 1,0 0))"), Some(15), Some("kml")).unwrap(),
            "<kml:Polygon><kml:outerBoundaryIs><kml:LinearRing><kml:coordinates>0,0 1,0 1,1 0,0\
             </kml:coordinates></kml:LinearRing></kml:outerBoundaryIs></kml:Polygon>"
        );
        // PostGIS rounds half to even here (1.5 → 2, 2.5 → 2), which is
        // also what Rust's formatter does — checked, not assumed.
        assert_eq!(
            st_as_kml(&g("POINT(1.5 2.5)"), Some(0), None).unwrap(),
            "<Point><coordinates>2,2</coordinates></Point>"
        );
        // A prefix that would break the document is an error, not escaped.
        assert!(st_as_kml(&g("POINT(1 2)"), None, Some("<script")).is_err());
        assert!(st_as_kml(&g("POINT(1 2)"), None, Some("1abc")).is_err());
        assert_eq!(
            st_as_kml(&g("POINT(1 2)"), None, Some("")).unwrap(),
            "<Point><coordinates>1,2</coordinates></Point>"
        );
    }

    #[test]
    fn the_output_is_always_wgs84() {
        // PostGIS transforms rather than labels: POINT(1 2) in 3857 is a
        // fraction of a degree from the origin, not "1,2".
        let webmerc = st_set_srid(&st_geom_from_text("POINT(1 2)", None).unwrap(), 3857).unwrap();
        let out = st_as_kml(&webmerc, None, None).unwrap();
        assert!(out.starts_with("<Point><coordinates>0.0000089"), "{out}");
        // SRID 0 is refused with PostGIS's wording rather than being
        // assumed to already be lon/lat.
        let unknown = st_geom_from_text("POINT(1 2)", None).unwrap();
        let err = st_as_kml(&unknown, None, None).unwrap_err().to_string();
        assert!(err.contains("ST_AsKML"), "{err}");
        assert!(err.contains("SRID"), "{err}");
    }
}
