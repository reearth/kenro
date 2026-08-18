//! The `BOX3D(…)` / `BOX(…)` text `ST_3DExtent` and PostGIS's `ST_Extent`
//! render, parsed back into six numbers so the box accessors can consume it.
//!
//! ## Why the box accessors take text at all
//!
//! PostGIS's `ST_XMin`/`ST_YMin`/`ST_ZMin`/`ST_XMax`/`ST_YMax`/`ST_ZMax` have
//! exactly **one** overload each and its argument type is `box3d` — measured
//! against PostGIS 3.5.2:
//!
//! ```text
//! st_xmin|box3d|double precision      (…and the other five, identically)
//! ```
//!
//! A geometry reaches them through the implicit `geometry → box3d` cast, and
//! a bare SQL string literal reaches them through `box3d_in`. SQLite has
//! neither a `box3d` type nor a cast to route a value through, so kenro's
//! accessors take the union directly: a geometry BLOB, or the box text.
//!
//! ## What PostGIS's parser accepts (measured, PostGIS 3.5.2)
//!
//! | input | PostGIS | kenro |
//! |---|---|---|
//! | `BOX3D(1 2 3,4 5 6)` | 3D box | same |
//! | `BOX3D(1 2,4 5)` | 2D box, z = 0 | same |
//! | `BOX3D(4 5 6,1 2 3)` | normalized per axis (xmin = 1) | same |
//! | `BOX3D(1e2 2 3,…)`, `+1`, `.5` | accepted | same |
//! | `BOX(1 2,4 5)` | **rejected** — "BOX3D parser - doesn't start with BOX3D(" | **accepted**, z = 0 |
//! | `box3d(…)` (lower case) | rejected | accepted |
//! | `   BOX3D(…)` (leading space) | rejected | accepted |
//! | `BOX3D (…)` (space before paren) | rejected | accepted |
//! | `BOX3D(1 2 3,4 5 6)junk` | **accepted**, junk ignored | rejected |
//! | `BOX3D(1 2 3,4 5 6` (no close paren) | **accepted** | rejected |
//! | `BOX3D(1 2,4 5 6)` (2D then 3D corner) | **accepted**, z = 0 | rejected |
//! | `BOX3D(1 2 3,4 5)` (3D then 2D corner) | rejected | rejected |
//! | `BOX3D(1 2 3)`, `BOX3D()`, `BOX3D EMPTY` | rejected | rejected |
//!
//! The two families of deviation are deliberate and go in opposite
//! directions, for the same reason: PostGIS's `box3d_in` is a pair of
//! `sscanf` calls, and `sscanf` neither anchors the tail nor case-folds.
//!
//! - **kenro is more lenient about the spelling.** Case, surrounding
//!   whitespace, a space before the paren, and the `BOX(…)` spelling all
//!   parse. PostGIS can afford to be strict because a `box3d` there is
//!   usually a *value* flowing out of a cast, never a string a human typed;
//!   in SQLite there is no cast, so the string a human typed — very possibly
//!   pasted from PostGIS's own `ST_Extent(…)::text`, which renders
//!   `BOX(0 0,5 5)` — is the only way in.
//! - **kenro is stricter about the contents.** Trailing junk, a missing
//!   close paren and a 2D-then-3D corner pair are `sscanf` accidents, not
//!   contracts. Accepting them would mean silently reading `BOX3D(1 2 3,4 5
//!   6` as a box while the caller's string was truncated.

use crate::error::{Error, Result};

/// A parsed box: `[minx, miny, minz]`, `[maxx, maxy, maxz]`, normalized so
/// each min is ≤ its max (PostGIS's `box3d_in` normalizes too — measured:
/// `ST_XMin('BOX3D(4 5 6,1 2 3)')` is 1, not 4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box3d {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// Does this argument want the text path rather than the geometry path?
///
/// The two arrive at the pure layer as the same `&[u8]` — every binding
/// marshals `Kind::BlobOrText` as bytes, TEXT as its UTF-8 — so the
/// discriminator has to be the content. It is unambiguous, because
/// `geom::decode_auto` reads exactly two encodings: a GeoPackage blob, whose
/// first two bytes are the `GP` magic, and WKB, whose first byte is the
/// byte-order marker 0x00 or 0x01. Neither is printable ASCII (`GP` is, but
/// the magic check catches it first), so "starts with a printable character"
/// means text and nothing else.
///
/// The R-tree trigger functions therefore keep their exact BLOB behaviour:
/// nothing they were ever handed can land on the text path.
///
/// The test is deliberately wider than `BOX`. Every string that is not a box
/// should get [`bad`]'s message — which names the box spellings *and*
/// `ST_GeomFromText` — rather than falling through to a WKB decoder that
/// would report "invalid byte-order marker 0x4c" at someone who passed WKT.
pub fn looks_like_text(bytes: &[u8]) -> bool {
    if crate::gpb::is_gpb(bytes) {
        return false;
    }
    bytes
        .iter()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(u8::is_ascii_graphic)
}

/// Parse `BOX3D(…)` / `BOX(…)` text, given as its UTF-8 bytes.
pub fn parse(bytes: &[u8], func: &'static str) -> Result<Box3d> {
    let text = std::str::from_utf8(bytes).map_err(|_| bad(func, "not valid UTF-8"))?;
    parse_str(text, func)
}

pub fn parse_str(text: &str, func: &'static str) -> Result<Box3d> {
    let s = text.trim();
    // `BOX3D` before `BOX`: the longer spelling is a prefix-superset.
    let rest = strip_prefix_ci(s, "BOX3D")
        .or_else(|| strip_prefix_ci(s, "BOX"))
        .ok_or_else(|| bad(func, "expected a box literal starting with BOX3D( or BOX("))?;
    let rest = rest.trim_start();
    let inner = rest
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .ok_or_else(|| bad(func, "expected parentheses around the two corners"))?;

    let (lo, hi) = inner
        .split_once(',')
        .ok_or_else(|| bad(func, "expected two comma-separated corners"))?;
    let lo = ordinates(lo, func)?;
    let hi = ordinates(hi, func)?;
    if lo.len() != hi.len() {
        return Err(bad(
            func,
            "both corners must have the same number of ordinates",
        ));
    }
    // A 2D box has z = 0 on both corners, which is what PostGIS reports:
    // `ST_ZMin('BOX3D(1 2,4 5)')` and `ST_ZMax(…)` are both 0 (measured),
    // and `'BOX3D(1 2,4 5)'::box3d::text` renders `BOX3D(1 2 0,4 5 0)`.
    let z = |c: &[f64]| c.get(2).copied().unwrap_or(0.0);
    let (lo3, hi3) = ([lo[0], lo[1], z(&lo)], [hi[0], hi[1], z(&hi)]);
    Ok(Box3d {
        min: [lo3[0].min(hi3[0]), lo3[1].min(hi3[1]), lo3[2].min(hi3[2])],
        max: [lo3[0].max(hi3[0]), lo3[1].max(hi3[1]), lo3[2].max(hi3[2])],
    })
}

/// The `n`th ordinate of the minimum corner (`n` = 0/1/2 → x/y/z), for
/// `ST_MinX` / `ST_MinY` / `ST_ZMin`.
pub fn min_ordinate(bytes: &[u8], n: usize, func: &'static str) -> Result<Option<f64>> {
    Ok(Some(parse(bytes, func)?.min[n]))
}

/// The `n`th ordinate of the maximum corner, for `ST_MaxX` / `ST_MaxY` /
/// `ST_ZMax`.
pub fn max_ordinate(bytes: &[u8], n: usize, func: &'static str) -> Result<Option<f64>> {
    Ok(Some(parse(bytes, func)?.max[n]))
}

fn ordinates(corner: &str, func: &'static str) -> Result<Vec<f64>> {
    let parts: Vec<&str> = corner.split_ascii_whitespace().collect();
    if parts.len() != 2 && parts.len() != 3 {
        return Err(bad(
            func,
            "each corner needs 2 or 3 space-separated ordinates",
        ));
    }
    parts
        .iter()
        .map(|p| {
            p.parse::<f64>()
                .map_err(|_| bad(func, &format!("`{p}` is not a number")))
        })
        .collect()
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let (head, tail) = s.split_at_checked(prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then_some(tail)
}

/// The rejection message the six accessors give for unparseable TEXT.
///
/// It replaces the "did you mean ST_GeomFromText?" hint `blob_or_null` gives
/// everywhere else, so it has to carry the same amount of help: what the
/// argument may be, and the other way in.
fn bad(func: &'static str, why: &str) -> Error {
    Error::Unsupported {
        func,
        reason: format!(
            "got TEXT that is not a box literal ({why}); \
             expected a geometry BLOB, `BOX3D(minx miny minz,maxx maxy maxz)` \
             or `BOX(minx miny,maxx maxy)` \
             (for a geometry given as text, wrap it in ST_GeomFromText)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Box3d {
        parse_str(s, "ST_MinX").unwrap()
    }

    #[test]
    fn parses_the_three_d_form() {
        assert_eq!(
            p("BOX3D(1 2 3,4 5 6)"),
            Box3d {
                min: [1.0, 2.0, 3.0],
                max: [4.0, 5.0, 6.0]
            }
        );
    }

    #[test]
    fn two_d_forms_get_zero_z() {
        // Measured: PostGIS answers ST_ZMin/ST_ZMax = 0 for `BOX3D(1 2,4 5)`.
        let b = Box3d {
            min: [1.0, 2.0, 0.0],
            max: [4.0, 5.0, 0.0],
        };
        assert_eq!(p("BOX3D(1 2,4 5)"), b);
        assert_eq!(p("BOX(1 2,4 5)"), b);
    }

    #[test]
    fn corners_are_normalized_per_axis() {
        // Measured: ST_XMin('BOX3D(4 5 6,1 2 3)') = 1, ST_XMax = 4.
        assert_eq!(
            p("BOX3D(4 5 6,1 2 3)"),
            Box3d {
                min: [1.0, 2.0, 3.0],
                max: [4.0, 5.0, 6.0]
            }
        );
    }

    #[test]
    fn number_spellings_postgis_accepts() {
        assert_eq!(p("BOX3D(1e2 2 3,4 5 6)").max[0], 100.0);
        assert_eq!(p("BOX3D(+1 2 3,4 5 6)").min[0], 1.0);
        assert_eq!(p("BOX3D(.5 2 3,4 5 6)").min[0], 0.5);
        assert_eq!(p("BOX3D(-1.5 -2 -3,4 5 6)").min[0], -1.5);
    }

    #[test]
    fn whitespace_and_case_are_kenro_leniencies() {
        let b = p("BOX3D(1 2 3,4 5 6)");
        assert_eq!(p("  box3d( 1 2 3 , 4 5 6 )  "), b);
        assert_eq!(p("BOX3D (1 2 3,4 5 6)"), b);
        assert_eq!(p("Box3D(1 2 3,4 5 6)"), b);
    }

    #[test]
    fn sscanf_accidents_are_rejected() {
        for s in [
            "BOX3D(1 2 3,4 5 6)junk", // PostGIS accepts, ignoring the junk
            "BOX3D(1 2 3,4 5 6",      // PostGIS accepts, no close paren needed
            "BOX3D(1 2,4 5 6)",       // PostGIS accepts, dropping the 6
        ] {
            assert!(parse_str(s, "ST_MinX").is_err(), "{s} should be rejected");
        }
    }

    #[test]
    fn malformed_boxes_postgis_rejects_too() {
        for s in [
            "BOX3D(1 2 3,4 5)",
            "BOX3D(1 2 3 9,4 5 6 9)",
            "BOX3D(1 2 3)",
            "BOX3D()",
            "BOX3D EMPTY",
            "BOX3D(a b c,d e f)",
            "POINT(1 2)",
            "",
        ] {
            assert!(parse_str(s, "ST_MinX").is_err(), "{s} should be rejected");
        }
    }

    #[test]
    fn the_error_names_both_ways_in() {
        let e = parse_str("BOX3D EMPTY", "ST_MinX").unwrap_err().to_string();
        assert!(e.contains("BOX3D(minx miny minz"), "{e}");
        assert!(e.contains("ST_GeomFromText"), "{e}");
    }

    #[test]
    fn geometry_encodings_never_look_like_box_text() {
        // GeoPackage blob magic, and both WKB byte-order bytes.
        assert!(!looks_like_text(b"GP\x00\x01"));
        assert!(!looks_like_text(&[0x00, 0x00, 0x00, 0x00, 0x01]));
        assert!(!looks_like_text(&[0x01, 0x01, 0x00, 0x00, 0x00]));
        assert!(!looks_like_text(b""));
        assert!(looks_like_text(b"BOX3D(1 2 3,4 5 6)"));
        assert!(looks_like_text(b"  box(1 2,3 4)"));
    }
}
