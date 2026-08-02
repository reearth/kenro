//! GML 2 and 3 I/O.
//!
//! The reason SpatiaLite's GML support drags in libxml2 is schema validation
//! and its `XB_*` XmlBLOB machinery, not parsing. kenro validates nothing, so
//! reading needs only a pull parser — `quick-xml`, measured at +13 KB of wasm
//! — and writing needs no XML library at all, the same way the GeoJSON and
//! WKT emitters are hand-rolled here.
//!
//! Output is byte-compatible with PostGIS for the shapes kenro supports
//! (golden-tested), including its GML 3 habit of writing a `Curve` with
//! segments where a `LineString` would do.

use geo_types::{
    Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use quick_xml::events::Event;

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

const FUNC_OUT: &str = "ST_AsGML";
const FUNC_IN: &str = "ST_GeomFromGML";

/// `ST_AsGML([version, ] geom [, maxdecimaldigits])` — GML 2 or 3.
///
/// PostGIS's default version is 2 and its default precision 15 digits; both
/// are matched. The `options`, `nprefix` and `id` arguments are not
/// implemented — kenro always writes the `gml:` prefix and no id.
pub fn st_as_gml(bytes: &[u8], version: i64, digits: Option<i64>) -> Result<String> {
    if version != 2 && version != 3 {
        return Err(Error::Unsupported {
            func: FUNC_OUT,
            reason: format!("GML version must be 2 or 3, got {version}"),
        });
    }
    let g = geom::decode_auto(bytes)?;
    if g.has_zm {
        return Err(Error::Unsupported {
            func: FUNC_OUT,
            reason: "3D/M output is not supported in kenro 0.1; use ST_Force2D".into(),
        });
    }
    let digits = digits.unwrap_or(15).clamp(0, 15) as usize;
    let srs = if g.srid > 0 {
        format!(" srsName=\"EPSG:{}\"", g.srid)
    } else {
        String::new()
    };
    let mut out = String::new();
    write_geometry(&mut out, &g.geometry, version, digits, &srs)?;
    Ok(out)
}

fn write_geometry(
    out: &mut String,
    g: &Geometry<f64>,
    version: i64,
    digits: usize,
    srs: &str,
) -> Result<()> {
    match g {
        Geometry::Point(p) => {
            out.push_str(&format!("<gml:Point{srs}>"));
            write_coords(out, std::slice::from_ref(&p.0), version, digits, false);
            out.push_str("</gml:Point>");
        }
        Geometry::LineString(l) => write_line(out, l, version, digits, srs),
        Geometry::Polygon(p) => write_polygon(out, p, version, digits, srs),
        Geometry::MultiPoint(mp) => {
            out.push_str(&format!("<gml:MultiPoint{srs}>"));
            for p in mp {
                out.push_str("<gml:pointMember><gml:Point>");
                write_coords(out, std::slice::from_ref(&p.0), version, digits, false);
                out.push_str("</gml:Point></gml:pointMember>");
            }
            out.push_str("</gml:MultiPoint>");
        }
        Geometry::MultiLineString(mls) => {
            // GML 2 spells this MultiLineString; GML 3, MultiCurve.
            let (tag, member) = if version == 2 {
                ("MultiLineString", "lineStringMember")
            } else {
                ("MultiCurve", "curveMember")
            };
            out.push_str(&format!("<gml:{tag}{srs}>"));
            for l in mls {
                out.push_str(&format!("<gml:{member}>"));
                write_line(out, l, version, digits, "");
                out.push_str(&format!("</gml:{member}>"));
            }
            out.push_str(&format!("</gml:{tag}>"));
        }
        Geometry::MultiPolygon(mp) => {
            let (tag, member) = if version == 2 {
                ("MultiPolygon", "polygonMember")
            } else {
                ("MultiSurface", "surfaceMember")
            };
            out.push_str(&format!("<gml:{tag}{srs}>"));
            for p in mp {
                out.push_str(&format!("<gml:{member}>"));
                write_polygon(out, p, version, digits, "");
                out.push_str(&format!("</gml:{member}>"));
            }
            out.push_str(&format!("</gml:{tag}>"));
        }
        Geometry::GeometryCollection(_) => {
            return Err(Error::Unsupported {
                func: FUNC_OUT,
                reason: "GeometryCollection operands are not supported".into(),
            });
        }
        Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::Line(_) => {
            return Err(Error::Unsupported {
                func: FUNC_OUT,
                reason: "unsupported geometry type".into(),
            });
        }
    }
    Ok(())
}

fn write_line(out: &mut String, l: &LineString<f64>, version: i64, digits: usize, srs: &str) {
    if version == 2 {
        out.push_str(&format!("<gml:LineString{srs}>"));
        write_coords(out, &l.0, version, digits, false);
        out.push_str("</gml:LineString>");
    } else {
        // PostGIS writes a Curve with one LineStringSegment, verified live.
        out.push_str(&format!(
            "<gml:Curve{srs}><gml:segments><gml:LineStringSegment>"
        ));
        write_coords(out, &l.0, version, digits, true);
        out.push_str("</gml:LineStringSegment></gml:segments></gml:Curve>");
    }
}

fn write_polygon(out: &mut String, p: &Polygon<f64>, version: i64, digits: usize, srs: &str) {
    out.push_str(&format!("<gml:Polygon{srs}>"));
    let (outer, inner) = if version == 2 {
        ("outerBoundaryIs", "innerBoundaryIs")
    } else {
        ("exterior", "interior")
    };
    out.push_str(&format!("<gml:{outer}><gml:LinearRing>"));
    write_coords(out, &p.exterior().0, version, digits, true);
    out.push_str(&format!("</gml:LinearRing></gml:{outer}>"));
    for ring in p.interiors() {
        out.push_str(&format!("<gml:{inner}><gml:LinearRing>"));
        write_coords(out, &ring.0, version, digits, true);
        out.push_str(&format!("</gml:LinearRing></gml:{inner}>"));
    }
    out.push_str("</gml:Polygon>");
}

/// GML 2 writes `x,y x,y` inside `coordinates`; GML 3 writes `x y x y`
/// inside `pos` (a single coordinate) or `posList`.
fn write_coords(out: &mut String, coords: &[Coord<f64>], version: i64, digits: usize, list: bool) {
    if version == 2 {
        out.push_str("<gml:coordinates>");
        for (i, c) in coords.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&format!("{},{}", num(c.x, digits), num(c.y, digits)));
        }
        out.push_str("</gml:coordinates>");
        return;
    }
    let tag = if list { "posList" } else { "pos" };
    out.push_str(&format!("<gml:{tag} srsDimension=\"2\">"));
    for (i, c) in coords.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{} {}", num(c.x, digits), num(c.y, digits)));
    }
    out.push_str(&format!("</gml:{tag}>"));
}

/// Trailing zeros trimmed, as PostGIS prints them.
fn num(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// `ST_GeomFromGML(text [, srid])` — GML 2 or 3.
///
/// The parser is structural, not schema-driven: it walks elements by local
/// name, so any namespace prefix works and unknown elements are ignored.
/// That is what lets a CityGML fragment be read without carrying the schema.
pub fn st_geom_from_gml(text: &str, srid_override: Option<i32>) -> Result<Vec<u8>> {
    let parsed = parse(text)?;
    let srid = srid_override.unwrap_or(parsed.srid);
    geom::encode_canonical_gpb(
        &Geom {
            geometry: parsed.geometry,
            srid,
            has_zm: false,
        },
        FUNC_IN,
    )
}

struct Parsed {
    geometry: Geometry<f64>,
    srid: i32,
}

/// One frame per open element that can hold geometry.
#[derive(Debug)]
enum Frame {
    Point(Vec<Coord<f64>>),
    Ring(Vec<Coord<f64>>),
    Line(Vec<Coord<f64>>),
    Polygon {
        exterior: Option<LineString<f64>>,
        interiors: Vec<LineString<f64>>,
        in_interior: bool,
    },
    Multi {
        kind: MultiKind,
        parts: Vec<Geometry<f64>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MultiKind {
    Point,
    Line,
    Polygon,
}

fn parse(text: &str) -> Result<Parsed> {
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Frame> = Vec::new();
    let mut finished: Vec<Geometry<f64>> = Vec::new();
    let mut srid = 0;
    let mut coord_text = String::new();
    let mut collecting = false;
    let mut coord_style_is_gml2 = false;
    // CityGML writes srsDimension="3"; without it, GML defaults to 2. Reading
    // the attribute beats guessing from how many numbers arrived — "0 0 10
    // 1 1 30" is six either way.
    let mut coord_dim = 2usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                if srid == 0 {
                    if let Some(found) = srs_name(&e)? {
                        srid = found;
                    }
                }
                match name.as_str() {
                    "Point" => stack.push(Frame::Point(Vec::new())),
                    "LinearRing" => stack.push(Frame::Ring(Vec::new())),
                    "LineString" | "LineStringSegment" => stack.push(Frame::Line(Vec::new())),
                    "Polygon" => stack.push(Frame::Polygon {
                        exterior: None,
                        interiors: Vec::new(),
                        in_interior: false,
                    }),
                    "interior" | "innerBoundaryIs" => {
                        if let Some(Frame::Polygon { in_interior, .. }) = stack.last_mut() {
                            *in_interior = true;
                        }
                    }
                    "MultiPoint" => stack.push(Frame::Multi {
                        kind: MultiKind::Point,
                        parts: Vec::new(),
                    }),
                    "MultiLineString" | "MultiCurve" => stack.push(Frame::Multi {
                        kind: MultiKind::Line,
                        parts: Vec::new(),
                    }),
                    // CityGML's surface wrappers collapse to a MultiPolygon of
                    // their patches — kenro reads the structure and drops Z,
                    // as it does everywhere. A gml:Solid's shell is the same
                    // shape once its exterior is unwrapped.
                    "MultiPolygon"
                    | "MultiSurface"
                    | "CompositeSurface"
                    | "Surface"
                    | "TriangulatedSurface"
                    | "Solid" => stack.push(Frame::Multi {
                        kind: MultiKind::Polygon,
                        parts: Vec::new(),
                    }),
                    "Triangle" => stack.push(Frame::Polygon {
                        exterior: None,
                        interiors: Vec::new(),
                        in_interior: false,
                    }),
                    "pos" | "posList" | "coordinates" => {
                        collecting = true;
                        coord_style_is_gml2 = name == "coordinates";
                        coord_dim = srs_dimension(&e)?.unwrap_or(2);
                        coord_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if collecting => {
                coord_text.push_str(&String::from_utf8_lossy(t.as_ref()));
                coord_text.push(' ');
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "pos" | "posList" | "coordinates" => {
                        collecting = false;
                        let coords = parse_coords(&coord_text, coord_style_is_gml2, coord_dim)?;
                        match stack.last_mut() {
                            Some(Frame::Point(c) | Frame::Ring(c) | Frame::Line(c)) => {
                                c.extend(coords)
                            }
                            _ => {
                                return Err(Error::InvalidWkt(
                                    "GML coordinates outside a geometry element".into(),
                                ));
                            }
                        }
                    }
                    "Point"
                    | "LineString"
                    | "LineStringSegment"
                    | "LinearRing"
                    | "Polygon"
                    | "Triangle"
                    | "MultiPoint"
                    | "MultiLineString"
                    | "MultiCurve"
                    | "MultiPolygon"
                    | "MultiSurface"
                    | "CompositeSurface"
                    | "Surface"
                    | "TriangulatedSurface"
                    | "Solid" => {
                        let Some(frame) = stack.pop() else {
                            return Err(Error::InvalidWkt("unbalanced GML elements".into()));
                        };
                        close_frame(frame, &mut stack, &mut finished)?;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::InvalidWkt(format!("malformed GML: {e}")));
            }
            _ => {}
        }
    }

    let geometry = finished.pop().ok_or_else(|| {
        Error::InvalidWkt("no GML geometry element found (Point, LineString, Polygon, …)".into())
    })?;
    Ok(Parsed { geometry, srid })
}

/// Attach a finished element to its parent, or to the result list.
fn close_frame(frame: Frame, stack: &mut [Frame], finished: &mut Vec<Geometry<f64>>) -> Result<()> {
    let produced: Option<Geometry<f64>> = match frame {
        Frame::Point(c) => {
            let first = c
                .first()
                .copied()
                .ok_or_else(|| Error::InvalidWkt("GML Point has no coordinate".into()))?;
            Some(Geometry::Point(Point::from(first)))
        }
        Frame::Line(c) => Some(Geometry::LineString(LineString::new(c))),
        Frame::Ring(c) => {
            // A ring belongs to the polygon below it, not to the result.
            match stack.last_mut() {
                Some(Frame::Polygon {
                    exterior,
                    interiors,
                    in_interior,
                }) => {
                    let ring = LineString::new(c);
                    if *in_interior {
                        interiors.push(ring);
                        *in_interior = false;
                    } else {
                        *exterior = Some(ring);
                    }
                    None
                }
                _ => Some(Geometry::LineString(LineString::new(c))),
            }
        }
        Frame::Polygon {
            exterior,
            interiors,
            ..
        } => {
            let shell = exterior
                .ok_or_else(|| Error::InvalidWkt("GML Polygon has no exterior ring".into()))?;
            Some(Geometry::Polygon(Polygon::new(shell, interiors)))
        }
        Frame::Multi { kind, parts } => Some(assemble_multi(kind, parts)?),
    };
    let Some(g) = produced else { return Ok(()) };
    match stack.last_mut() {
        Some(Frame::Multi { parts, .. }) => parts.push(g),
        _ => finished.push(g),
    }
    Ok(())
}

fn assemble_multi(kind: MultiKind, parts: Vec<Geometry<f64>>) -> Result<Geometry<f64>> {
    // CityGML nests its wrappers — a gml:Solid holds a gml:CompositeSurface
    // holds the polygons — so a child may already be a multi. Flatten rather
    // than drop it, which is what an earlier version silently did.
    Ok(match kind {
        MultiKind::Point => Geometry::MultiPoint(MultiPoint::new(
            parts
                .into_iter()
                .flat_map(|g| match g {
                    Geometry::Point(p) => vec![p],
                    Geometry::MultiPoint(mp) => mp.0,
                    _ => vec![],
                })
                .collect(),
        )),
        MultiKind::Line => Geometry::MultiLineString(MultiLineString::new(
            parts
                .into_iter()
                .flat_map(|g| match g {
                    Geometry::LineString(l) => vec![l],
                    Geometry::MultiLineString(mls) => mls.0,
                    _ => vec![],
                })
                .collect(),
        )),
        MultiKind::Polygon => Geometry::MultiPolygon(MultiPolygon::new(
            parts
                .into_iter()
                .flat_map(|g| match g {
                    Geometry::Polygon(p) => vec![p],
                    Geometry::MultiPolygon(mp) => mp.0,
                    _ => vec![],
                })
                .collect(),
        )),
    })
}

fn local_name(raw: &[u8]) -> String {
    let name = raw.rsplit(|b| *b == b':').next().unwrap_or(raw);
    String::from_utf8_lossy(name).into_owned()
}

/// `srsName="EPSG:4326"`, or the URN forms CityGML tends to use.
fn srs_name(e: &quick_xml::events::BytesStart<'_>) -> Result<Option<i32>> {
    for attr in e.attributes() {
        let attr = attr.map_err(|err| Error::InvalidWkt(format!("malformed GML: {err}")))?;
        if local_name(attr.key.as_ref()) != "srsName" {
            continue;
        }
        let value = String::from_utf8_lossy(&attr.value).into_owned();
        let code = value.rsplit([':', '/']).find(|part| !part.is_empty());
        if let Some(code) = code.and_then(|c| c.trim().parse::<i32>().ok()) {
            return Ok(Some(code));
        }
    }
    Ok(None)
}

/// `srsDimension="3"` on a pos/posList element.
fn srs_dimension(e: &quick_xml::events::BytesStart<'_>) -> Result<Option<usize>> {
    for attr in e.attributes() {
        let attr = attr.map_err(|err| Error::InvalidWkt(format!("malformed GML: {err}")))?;
        if local_name(attr.key.as_ref()) != "srsDimension" {
            continue;
        }
        let raw = String::from_utf8_lossy(&attr.value);
        let dim = raw.trim().parse::<usize>().map_err(|_| {
            Error::InvalidWkt(format!("GML srsDimension {raw:?} is not an integer"))
        })?;
        if !(2..=4).contains(&dim) {
            return Err(Error::InvalidWkt(format!(
                "GML srsDimension must be 2, 3 or 4, got {dim}"
            )));
        }
        return Ok(Some(dim));
    }
    Ok(None)
}

fn parse_coords(text: &str, gml2: bool, dim: usize) -> Result<Vec<Coord<f64>>> {
    let mut out = Vec::new();
    if gml2 {
        // "x,y x,y" — a tuple per whitespace-separated token.
        for token in text.split_whitespace() {
            let mut parts = token.split(',');
            let (Some(x), Some(y)) = (parts.next(), parts.next()) else {
                return Err(Error::InvalidWkt(format!(
                    "GML coordinate tuple {token:?} is not x,y"
                )));
            };
            out.push(Coord {
                x: number(x)?,
                y: number(y)?,
            });
        }
        return Ok(out);
    }
    // "x y x y" in srsDimension-sized tuples. Ordinates past the second are
    // dropped, the same way kenro drops Z everywhere else.
    let values: Vec<f64> = text
        .split_whitespace()
        .map(number)
        .collect::<Result<Vec<_>>>()?;
    if values.len() % dim != 0 {
        return Err(Error::InvalidWkt(format!(
            "GML posList has {} numbers, not a multiple of srsDimension {dim}",
            values.len()
        )));
    }
    for chunk in values.chunks(dim) {
        out.push(Coord {
            x: chunk[0],
            y: chunk[1],
        });
    }
    Ok(out)
}

fn number(s: &str) -> Result<f64> {
    s.trim()
        .parse::<f64>()
        .map_err(|_| Error::InvalidWkt(format!("GML coordinate {s:?} is not a number")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text, st_srid};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, Some(4326)).unwrap()
    }

    #[test]
    fn gml2_output_is_byte_identical_to_postgis() {
        // PostGIS 3.5, one by one.
        assert_eq!(
            st_as_gml(&g("POINT(1 2)"), 2, None).unwrap(),
            r#"<gml:Point srsName="EPSG:4326"><gml:coordinates>1,2</gml:coordinates></gml:Point>"#
        );
        assert_eq!(
            st_as_gml(&g("LINESTRING(0 0,1 1)"), 2, None).unwrap(),
            r#"<gml:LineString srsName="EPSG:4326"><gml:coordinates>0,0 1,1</gml:coordinates></gml:LineString>"#
        );
    }

    #[test]
    fn gml3_output_is_byte_identical_to_postgis() {
        assert_eq!(
            st_as_gml(&g("POINT(1 2)"), 3, None).unwrap(),
            r#"<gml:Point srsName="EPSG:4326"><gml:pos srsDimension="2">1 2</gml:pos></gml:Point>"#
        );
        // PostGIS writes a Curve with segments, not a LineString.
        assert_eq!(
            st_as_gml(&g("LINESTRING(0 0,1 1)"), 3, None).unwrap(),
            r#"<gml:Curve srsName="EPSG:4326"><gml:segments><gml:LineStringSegment><gml:posList srsDimension="2">0 0 1 1</gml:posList></gml:LineStringSegment></gml:segments></gml:Curve>"#
        );
        assert_eq!(
            st_as_gml(&g("POLYGON((0 0,1 0,1 1,0 0))"), 3, None).unwrap(),
            r#"<gml:Polygon srsName="EPSG:4326"><gml:exterior><gml:LinearRing><gml:posList srsDimension="2">0 0 1 0 1 1 0 0</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon>"#
        );
        assert_eq!(
            st_as_gml(&g("MULTIPOINT((1 2),(3 4))"), 3, None).unwrap(),
            r#"<gml:MultiPoint srsName="EPSG:4326"><gml:pointMember><gml:Point><gml:pos srsDimension="2">1 2</gml:pos></gml:Point></gml:pointMember><gml:pointMember><gml:Point><gml:pos srsDimension="2">3 4</gml:pos></gml:Point></gml:pointMember></gml:MultiPoint>"#
        );
    }

    #[test]
    fn srs_and_precision_follow_postgis() {
        // No SRID → no srsName attribute at all.
        assert_eq!(
            st_as_gml(&st_geom_from_text("POINT(1 2)", None).unwrap(), 3, None).unwrap(),
            r#"<gml:Point><gml:pos srsDimension="2">1 2</gml:pos></gml:Point>"#
        );
        // PostGIS: ST_AsGML(3, POINT(1.123456789 2), 3) → 1.123
        assert_eq!(
            st_as_gml(&g("POINT(1.123456789 2)"), 3, Some(3)).unwrap(),
            r#"<gml:Point srsName="EPSG:4326"><gml:pos srsDimension="2">1.123 2</gml:pos></gml:Point>"#
        );
        assert!(st_as_gml(&g("POINT(1 2)"), 4, None).is_err());
    }

    #[test]
    fn reading_accepts_both_versions_and_any_prefix() {
        let cases = [
            (
                r#"<gml:Point srsName="EPSG:4326"><gml:pos>1 2</gml:pos></gml:Point>"#,
                "POINT(1 2)",
                4326,
            ),
            (
                "<gml:Point><gml:coordinates>1,2</gml:coordinates></gml:Point>",
                "POINT(1 2)",
                0,
            ),
            (
                "<gml:Polygon><gml:exterior><gml:LinearRing><gml:posList>0 0 1 0 1 1 0 0</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon>",
                "POLYGON((0 0,1 0,1 1,0 0))",
                0,
            ),
            (
                "<gml:Curve><gml:segments><gml:LineStringSegment><gml:posList>0 0 1 1</gml:posList></gml:LineStringSegment></gml:segments></gml:Curve>",
                "LINESTRING(0 0,1 1)",
                0,
            ),
            // No prefix at all, and a URN srsName as CityGML writes it.
            (
                r#"<Point srsName="urn:ogc:def:crs:EPSG::6697"><pos>1 2</pos></Point>"#,
                "POINT(1 2)",
                6697,
            ),
        ];
        for (xml, wkt, srid) in cases {
            let blob = st_geom_from_gml(xml, None).unwrap();
            assert_eq!(st_as_text(&blob).unwrap(), wkt, "{xml}");
            assert_eq!(st_srid(&blob).unwrap(), srid, "{xml}");
        }
    }

    #[test]
    fn reading_handles_holes_multis_and_3d_poslists() {
        let holed = st_geom_from_gml(
            "<gml:Polygon><gml:exterior><gml:LinearRing><gml:posList>0 0 4 0 4 4 0 4 0 0</gml:posList></gml:LinearRing></gml:exterior>\
             <gml:interior><gml:LinearRing><gml:posList>1 1 2 1 2 2 1 2 1 1</gml:posList></gml:LinearRing></gml:interior></gml:Polygon>",
            None,
        )
        .unwrap();
        assert_eq!(
            st_as_text(&holed).unwrap(),
            "POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))"
        );
        let multi = st_geom_from_gml(
            "<gml:MultiSurface><gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing>\
             <gml:posList>0 0 1 0 1 1 0 0</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface>",
            None,
        )
        .unwrap();
        assert_eq!(
            st_as_text(&multi).unwrap(),
            "MULTIPOLYGON(((0 0,1 0,1 1,0 0)))"
        );
        // A 3D posList — CityGML's normal case. Z is dropped, as everywhere.
        let three_d = st_geom_from_gml(
            "<gml:LineString><gml:posList srsDimension=\"3\">0 0 10 1 1 30</gml:posList></gml:LineString>",
            None,
        )
        .unwrap();
        assert_eq!(st_as_text(&three_d).unwrap(), "LINESTRING(0 0,1 1)");
    }

    #[test]
    fn round_trips_through_both_versions() {
        for wkt in [
            "POINT(1 2)",
            "LINESTRING(0 0,1 1,2 0)",
            "POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))",
            "MULTIPOINT((1 2),(3 4))",
            "MULTILINESTRING((0 0,1 1),(2 2,3 3))",
            "MULTIPOLYGON(((0 0,1 0,1 1,0 0)))",
        ] {
            for version in [2, 3] {
                let xml = st_as_gml(&g(wkt), version, None).unwrap();
                let back = st_geom_from_gml(&xml, None).unwrap();
                assert_eq!(st_as_text(&back).unwrap(), wkt, "GML{version}: {xml}");
                assert_eq!(st_srid(&back).unwrap(), 4326, "GML{version} lost the SRID");
            }
        }
    }

    #[test]
    fn citygml_surface_wrappers_read_as_multipolygons() {
        // A gml:Solid's shell, as CityGML LOD2 writes it: nested wrappers
        // around a handful of 3D polygons.
        let solid = st_geom_from_gml(
            "<gml:Solid><gml:exterior><gml:CompositeSurface>\
             <gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing>\
             <gml:posList srsDimension=\"3\">0 0 0 1 0 0 1 1 0 0 0 0</gml:posList>\
             </gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>\
             <gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing>\
             <gml:posList srsDimension=\"3\">0 0 1 1 0 1 1 1 1 0 0 1</gml:posList>\
             </gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>\
             </gml:CompositeSurface></gml:exterior></gml:Solid>",
            None,
        )
        .unwrap();
        assert_eq!(
            st_as_text(&solid).unwrap(),
            "MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((0 0,1 0,1 1,0 0)))"
        );
        // A TriangulatedSurface of gml:Triangle patches reads the same way.
        let tin = st_geom_from_gml(
            "<gml:TriangulatedSurface><gml:trianglePatches><gml:Triangle><gml:exterior>\
             <gml:LinearRing><gml:posList srsDimension=\"3\">0 0 0 1 0 0 1 1 0 0 0 0</gml:posList>\
             </gml:LinearRing></gml:exterior></gml:Triangle></gml:trianglePatches></gml:TriangulatedSurface>",
            None,
        )
        .unwrap();
        assert_eq!(
            st_as_text(&tin).unwrap(),
            "MULTIPOLYGON(((0 0,1 0,1 1,0 0)))"
        );
    }

    #[test]
    fn malformed_input_fails_loudly() {
        assert!(st_geom_from_gml("<gml:Point><gml:pos>1</gml:pos></gml:Point>", None).is_err());
        assert!(st_geom_from_gml("<gml:Point><gml:pos>a b</gml:pos></gml:Point>", None).is_err());
        assert!(st_geom_from_gml("<html><body/></html>", None).is_err());
        assert!(st_geom_from_gml("<gml:Point>", None).is_err());
    }
}
