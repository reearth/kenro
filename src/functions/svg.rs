//! `ST_AsSVG` — SVG path data (and point attributes).
//!
//! The output is a *fragment*, not a document: a `d=` path body for lines and
//! polygons, or the coordinate attributes for points. The caller wraps it.
//!
//! Two details are easy to get wrong and were measured rather than guessed:
//!
//! - **Y is negated.** SVG's Y axis grows downward, so `POINT(1 2)` prints
//!   `cy="-2"`. Every ordinate, in every geometry type.
//! - **`rel` changes the point spelling, not just the path commands.** With
//!   `rel = 1` a point is `x="1" y="-2"` and a path uses lowercase relative
//!   commands (`l`, `z`); with `rel = 0` it is `cx="1" cy="-2"` and `L`/`Z`.
//!   The attribute swap is not documented anywhere kenro could find — it came
//!   from a live PostGIS session.
//!
//! Relative deltas are computed on the full-precision coordinates and rounded
//! afterwards, which is why `0.123456 → 1.111111` at 2 digits prints `0.99`
//! rather than the `0.99` you would also get by subtracting the rounded
//! values here, but not in general. Also measured.

use geo_types::{Coord, Geometry, Polygon};

use crate::error::{Error, Result};
use crate::functions::num;
use crate::geom;

const FUNC: &str = "ST_AsSVG";

/// `ST_AsSVG(geom [, rel [, maxdecimaldigits]])`.
///
/// ⚠️ **Divergences from PostGIS.** 3D input is an error rather than being
/// flattened silently, as with every other kenro encoder (`ST_Force2D` is
/// the opt-in). Everything else matches byte for byte, including the empty
/// string for an empty geometry and the `,` separator between the points of
/// a MULTIPOINT where the other multis use a space.
pub fn st_as_svg(bytes: &[u8], rel: Option<i64>, digits: Option<i64>) -> Result<String> {
    let g = geom::decode_auto(bytes)?;
    if g.has_zm {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "3D/M output is not supported in kenro; use ST_Force2D".into(),
        });
    }
    // PostGIS treats any non-zero rel as relative.
    let relative = rel.unwrap_or(0) != 0;
    let digits = digits.unwrap_or(15).clamp(0, 15) as usize;
    let mut out = String::new();
    write_geometry(&mut out, &g.geometry, relative, digits)?;
    Ok(out)
}

fn write_geometry(out: &mut String, g: &Geometry<f64>, rel: bool, d: usize) -> Result<()> {
    match g {
        Geometry::Point(p) => write_point(out, p.0, rel, d),
        Geometry::MultiPoint(mp) => {
            for (i, p) in mp.0.iter().enumerate() {
                if i > 0 {
                    // The one place SVG output uses a comma between parts.
                    out.push(',');
                }
                write_point(out, p.0, rel, d);
            }
        }
        Geometry::Line(l) => write_path(out, &[l.start, l.end], false, rel, d),
        Geometry::LineString(l) => write_path(out, &l.0, false, rel, d),
        Geometry::MultiLineString(mls) => {
            for (i, l) in mls.0.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_path(out, &l.0, false, rel, d);
            }
        }
        Geometry::Polygon(p) => write_polygon(out, p, rel, d),
        Geometry::Rect(r) => write_polygon(out, &r.to_polygon(), rel, d),
        Geometry::Triangle(t) => write_polygon(out, &t.to_polygon(), rel, d),
        Geometry::MultiPolygon(mp) => {
            for (i, p) in mp.0.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_polygon(out, p, rel, d);
            }
        }
        Geometry::GeometryCollection(_) => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "GeometryCollection is not supported".into(),
            });
        }
    }
    Ok(())
}

/// `cx="…" cy="…"` absolute, `x="…" y="…"` relative. Yes, really.
fn write_point(out: &mut String, c: Coord<f64>, rel: bool, d: usize) {
    if !c.x.is_finite() || !c.y.is_finite() {
        return;
    }
    let (nx, ny) = if rel { ("x", "y") } else { ("cx", "cy") };
    out.push_str(&format!(
        "{nx}=\"{}\" {ny}=\"{}\"",
        num(c.x, d),
        num(-c.y, d)
    ));
}

fn write_polygon(out: &mut String, p: &Polygon<f64>, rel: bool, d: usize) {
    if p.exterior().0.is_empty() {
        return;
    }
    write_path(out, &p.exterior().0, true, rel, d);
    for ring in p.interiors() {
        out.push(' ');
        write_path(out, &ring.0, true, rel, d);
    }
}

/// `M` then the vertices; a ring drops its closing duplicate and ends with
/// `Z`/`z` instead.
fn write_path(out: &mut String, coords: &[Coord<f64>], ring: bool, rel: bool, d: usize) {
    let coords = if ring && coords.len() > 1 && coords.first() == coords.last() {
        &coords[..coords.len() - 1]
    } else {
        coords
    };
    let Some(&first) = coords.first() else {
        return;
    };
    out.push_str(&format!("M {} {}", num(first.x, d), num(-first.y, d)));
    if coords.len() > 1 {
        out.push_str(if rel { " l" } else { " L" });
        let mut prev = first;
        for &c in &coords[1..] {
            let (x, y) = if rel {
                // Deltas from the unrounded previous vertex, then rounded.
                (c.x - prev.x, -(c.y - prev.y))
            } else {
                (c.x, -c.y)
            };
            out.push_str(&format!(" {} {}", num(x, d), num(y, d)));
            prev = c;
        }
    }
    if ring {
        out.push_str(if rel { " z" } else { " Z" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::st_geom_from_text;

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    fn svg(wkt: &str) -> String {
        st_as_svg(&g(wkt), None, None).unwrap()
    }

    /// Byte-for-byte against PostGIS 3.5.
    #[test]
    fn absolute_output_is_byte_identical_to_postgis() {
        assert_eq!(svg("POINT(1 2)"), r#"cx="1" cy="-2""#);
        assert_eq!(
            svg("POINT(1.23456789 2.3456789)"),
            r#"cx="1.23456789" cy="-2.3456789""#
        );
        // Negating -2.5 gives 2.5, not -2.5 — the sign really does flip.
        assert_eq!(svg("POINT(-1.5 -2.5)"), r#"cx="-1.5" cy="2.5""#);
        assert_eq!(svg("LINESTRING(0 0,1 1,2 0)"), "M 0 0 L 1 -1 2 0");
        // A ring drops its closing vertex in favour of Z.
        assert_eq!(
            svg("POLYGON((0 0,4 0,4 4,0 4,0 0))"),
            "M 0 0 L 4 0 4 -4 0 -4 Z"
        );
        assert_eq!(
            svg("POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))"),
            "M 0 0 L 4 0 4 -4 0 -4 Z M 1 -1 L 2 -1 2 -2 1 -2 Z"
        );
        // MULTIPOINT joins with a comma; the other multis with a space.
        assert_eq!(
            svg("MULTIPOINT(0 0,1 1)"),
            r#"cx="0" cy="0",cx="1" cy="-1""#
        );
        // Negating a zero Y must not print "-0"; nor must a small negative
        // that rounds to zero. PostGIS prints "0" for both.
        assert_eq!(
            st_as_svg(&g("POINT(-0.001 0.001)"), Some(0), Some(2)).unwrap(),
            r#"cx="0" cy="0""#
        );
        assert_eq!(
            st_as_svg(&g("LINESTRING(0 0,0 0.001)"), Some(1), Some(2)).unwrap(),
            "M 0 0 l 0 0"
        );
        assert_eq!(
            svg("MULTILINESTRING((0 0,1 1),(2 2,3 3))"),
            "M 0 0 L 1 -1 M 2 -2 L 3 -3"
        );
        assert_eq!(
            svg("MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))"),
            "M 0 0 L 1 0 1 -1 Z M 5 -5 L 6 -5 6 -6 Z"
        );
        assert_eq!(svg("LINESTRING EMPTY"), "");
        assert_eq!(svg("POLYGON EMPTY"), "");
    }

    #[test]
    fn relative_output_switches_the_commands_and_the_point_attributes() {
        let rel = |wkt: &str| st_as_svg(&g(wkt), Some(1), None).unwrap();
        // The attribute names change with rel — the detail worth a test.
        assert_eq!(rel("POINT(1 2)"), r#"x="1" y="-2""#);
        assert_eq!(rel("MULTIPOINT(0 0,1 1)"), r#"x="0" y="0",x="1" y="-1""#);
        assert_eq!(rel("LINESTRING(0 0,1 1,2 0)"), "M 0 0 l 1 -1 1 1");
        assert_eq!(
            rel("POLYGON((0 0,4 0,4 4,0 4,0 0))"),
            "M 0 0 l 4 0 0 -4 -4 0 z"
        );
        assert_eq!(
            rel("POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))"),
            "M 0 0 l 4 0 0 -4 -4 0 z M 1 -1 l 1 0 0 -1 -1 0 z"
        );
        assert_eq!(
            rel("MULTILINESTRING((0 0,1 1),(2 2,3 3))"),
            "M 0 0 l 1 -1 M 2 -2 l 1 -1"
        );
    }

    #[test]
    fn precision_matches_postgis_including_the_relative_rounding() {
        assert_eq!(
            st_as_svg(&g("POINT(1.23456789 2.3456789)"), Some(0), Some(3)).unwrap(),
            r#"cx="1.235" cy="-2.346""#
        );
        assert_eq!(
            st_as_svg(
                &g("LINESTRING(0.123456 0.7654321,1.111111 1.999999)"),
                Some(0),
                Some(2)
            )
            .unwrap(),
            "M 0.12 -0.77 L 1.11 -2"
        );
        // The relative delta is taken on the full-precision coordinates and
        // rounded after: 1.111111 - 0.123456 = 0.987655 → 0.99, not
        // 1.11 - 0.12 = 0.99 by luck. The Y delta is where they part:
        // -(1.999999 - 0.7654321) = -1.2345669 → -1.23.
        assert_eq!(
            st_as_svg(
                &g("LINESTRING(0.123456 0.7654321,1.111111 1.999999)"),
                Some(1),
                Some(2)
            )
            .unwrap(),
            "M 0.12 -0.77 l 0.99 -1.23"
        );
        // Half-to-even, as PostGIS's printf does.
        assert_eq!(
            st_as_svg(&g("POINT(1.5 2.5)"), Some(0), Some(0)).unwrap(),
            r#"cx="2" cy="-2""#
        );
        // Never scientific notation, at either end of the range.
        assert_eq!(
            st_as_svg(&g("POINT(0.0000001 0.0000001)"), Some(0), Some(15)).unwrap(),
            r#"cx="0.0000001" cy="-0.0000001""#
        );
        assert_eq!(
            st_as_svg(&g("POINT(1000000 2000000)"), None, None).unwrap(),
            r#"cx="1000000" cy="-2000000""#
        );
    }
}
