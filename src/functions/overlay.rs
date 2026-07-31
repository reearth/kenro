//! Overlay operations: ST_Intersection, ST_Difference, ST_SymDifference,
//! ST_Union — pure-Rust via geo's BooleanOps (i_overlay).
//!
//! Operand-class decision matrix (P = puntal, L = lineal, A = areal):
//! P×anything uses exact Relate-based point filtering; L×A uses
//! `BooleanOps::clip`; A×A uses the boolean ops (areal results ONLY — the
//! headline documented divergence: polygons that merely touch produce an
//! empty result where GEOS returns the shared lower-dimensional piece).
//! Combinations that would need line noding or mixed-dimension collections
//! (L×L, and most mixed cases) raise `Unsupported` — never a wrong-looking
//! answer. GeometryCollection operands are rejected like the predicates.

use geo::BooleanOps;
use geo::Relate;
use geo_types::{Geometry, LineString, MultiPolygon, Point, Polygon};

use crate::error::{Error, Result};
use crate::functions::classify::{
    Class, classify, ensure_finite, normalize_lines, normalize_points, normalize_polygons,
    points_of, to_multi_line, to_multi_polygon,
};
use crate::geom::{self, Geom};

fn unsupported(func: &'static str, a: Class, b: Class, why: &str) -> Error {
    let name = |c: Class| match c {
        Class::Puntal => "point",
        Class::Lineal => "line",
        Class::Areal => "polygon",
    };
    Error::Unsupported {
        func,
        reason: format!(
            "{} × {} operands are not supported ({why}); use PostGIS or DuckDB spatial for \
             this combination",
            name(a),
            name(b)
        ),
    }
}

fn decode_operands(func: &'static str, a: &[u8], b: &[u8]) -> Result<(Geom, Geom, Class, Class)> {
    let ga = geom::decode_auto(a)?;
    let gb = geom::decode_auto(b)?;
    if ga.srid > 0 && gb.srid > 0 && ga.srid != gb.srid {
        return Err(Error::MixedSrid {
            func,
            a: ga.srid,
            b: gb.srid,
        });
    }
    let ca = classify(func, &ga.geometry)?;
    let cb = classify(func, &gb.geometry)?;
    ensure_finite(func, &ga.geometry)?;
    ensure_finite(func, &gb.geometry)?;
    Ok((ga, gb, ca, cb))
}

fn encode(geometry: Geometry<f64>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid,
            has_zm: false,
        },
        func,
    )
}

/// Filter a puntal operand's points by an intersection test against the
/// other geometry — exact, no divergence.
fn filter_points(
    points: &Geometry<f64>,
    other: &Geometry<f64>,
    keep_intersecting: bool,
) -> Vec<Point<f64>> {
    points_of(points)
        .into_iter()
        .filter(|p| {
            let hits = if geom::is_empty(other) {
                false
            } else {
                Geometry::Point(*p).relate(other).is_intersects()
            };
            hits == keep_intersecting
        })
        .collect()
}

/// `ST_Intersection(a, b)`.
pub fn st_intersection(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Intersection";
    let (ga, gb, ca, cb) = decode_operands(FUNC, a, b)?;
    let result: Geometry<f64> = match (ca, cb) {
        (Class::Puntal, _) => normalize_points(filter_points(&ga.geometry, &gb.geometry, true)),
        (_, Class::Puntal) => normalize_points(filter_points(&gb.geometry, &ga.geometry, true)),
        (Class::Lineal, Class::Areal) => normalize_lines(
            to_multi_polygon(&gb.geometry).clip(&to_multi_line(&ga.geometry), false),
        ),
        (Class::Areal, Class::Lineal) => normalize_lines(
            to_multi_polygon(&ga.geometry).clip(&to_multi_line(&gb.geometry), false),
        ),
        (Class::Areal, Class::Areal) => normalize_polygons(
            to_multi_polygon(&ga.geometry).intersection(&to_multi_polygon(&gb.geometry)),
        ),
        (Class::Lineal, Class::Lineal) => {
            return Err(unsupported(
                FUNC,
                ca,
                cb,
                "line-line intersection needs noding",
            ));
        }
    };
    encode(result, ga.srid.max(gb.srid), FUNC)
}

/// `ST_Difference(a, b)` — "a minus b".
pub fn st_difference(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Difference";
    let (ga, gb, ca, cb) = decode_operands(FUNC, a, b)?;
    let result: Geometry<f64> = match (ca, cb) {
        (Class::Puntal, _) => normalize_points(filter_points(&ga.geometry, &gb.geometry, false)),
        // Removing lower-dimensional content leaves `a` unchanged
        // (PostGIS-consistent, golden-verified).
        (Class::Lineal, Class::Puntal) | (Class::Areal, Class::Puntal) => ga.geometry.clone(),
        (Class::Areal, Class::Lineal) => ga.geometry.clone(),
        (Class::Lineal, Class::Areal) => {
            normalize_lines(to_multi_polygon(&gb.geometry).clip(&to_multi_line(&ga.geometry), true))
        }
        (Class::Areal, Class::Areal) => normalize_polygons(
            to_multi_polygon(&ga.geometry).difference(&to_multi_polygon(&gb.geometry)),
        ),
        (Class::Lineal, Class::Lineal) => {
            return Err(unsupported(
                FUNC,
                ca,
                cb,
                "line-line difference needs noding",
            ));
        }
    };
    encode(result, ga.srid.max(gb.srid), FUNC)
}

/// `ST_SymDifference(a, b)` — areal × areal via xor; puntal × puntal via
/// exact set logic. Mixed dimensions (GeometryCollections in PostGIS) are
/// unsupported.
pub fn st_sym_difference(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_SymDifference";
    let (ga, gb, ca, cb) = decode_operands(FUNC, a, b)?;
    let result: Geometry<f64> = match (ca, cb) {
        (Class::Puntal, Class::Puntal) => {
            let mut points = filter_points(&ga.geometry, &gb.geometry, false);
            points.extend(filter_points(&gb.geometry, &ga.geometry, false));
            normalize_points(points)
        }
        (Class::Areal, Class::Areal) => {
            normalize_polygons(to_multi_polygon(&ga.geometry).xor(&to_multi_polygon(&gb.geometry)))
        }
        _ => {
            return Err(unsupported(
                FUNC,
                ca,
                cb,
                "mixed-dimension symmetric difference produces a GeometryCollection",
            ));
        }
    };
    encode(result, ga.srid.max(gb.srid), FUNC)
}

/// `ST_Union(a, b)` — scalar form. Areal × areal and puntal × puntal only;
/// line unions need noding and mixed dimensions produce collections
/// (both unsupported).
pub fn st_union(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Union";
    let (ga, gb, ca, cb) = decode_operands(FUNC, a, b)?;
    let result: Geometry<f64> = match (ca, cb) {
        (Class::Puntal, Class::Puntal) => {
            let mut points = points_of(&ga.geometry);
            for p in points_of(&gb.geometry) {
                if !points.contains(&p) {
                    points.push(p);
                }
            }
            normalize_points(points)
        }
        (Class::Areal, Class::Areal) => normalize_polygons(
            to_multi_polygon(&ga.geometry).union(&to_multi_polygon(&gb.geometry)),
        ),
        (Class::Lineal, Class::Lineal) => {
            return Err(unsupported(FUNC, ca, cb, "line unions need noding"));
        }
        _ => {
            return Err(unsupported(
                FUNC,
                ca,
                cb,
                "mixed-dimension unions produce a GeometryCollection",
            ));
        }
    };
    encode(result, ga.srid.max(gb.srid), FUNC)
}

/// `ST_MakeValid(geom)` — pure-Rust polygon repair with GEOS's *structure*
/// method semantics: the ring linework is resolved into an arrangement and
/// unioned (via i_overlay), so bowties split into multiple polygons, holes
/// outside their shell become polygons of their own, and zero-area parts
/// disappear (`keepcollapsed=false`). PostGIS's default *linework* method
/// can return lower-dimensional pieces (a collapsed sliver as a LINESTRING,
/// wrapped in a GeometryCollection) — kenro deliberately returns areal
/// results only, matching the overlay family. Points and lines are always
/// OGC-valid and pass through unchanged; already-valid polygons return
/// unchanged (per kenro's `ST_IsValid`, which shares georust validation's
/// documented split-interior gap).
pub fn st_make_valid(bytes: &[u8]) -> Result<Vec<u8>> {
    use geo::algorithm::Validation;
    const FUNC: &str = "ST_MakeValid";
    let g = geom::decode_auto(bytes)?;
    let class = classify(FUNC, &g.geometry)?;
    ensure_finite(FUNC, &g.geometry)?;
    if geom::is_empty(&g.geometry) || class != Class::Areal || g.geometry.is_valid() {
        return encode(g.geometry, g.srid, FUNC);
    }
    let repaired = repair_multi_polygon(to_multi_polygon(&g.geometry));
    encode(normalize_polygons(repaired), g.srid, FUNC)
}

/// The areal repair core (also used by the MVT pipeline in `full` builds):
/// drop rings the arrangement cannot use (fewer than 3 distinct vertices),
/// then resolve self-intersections by unioning with nothing.
pub(crate) fn repair_multi_polygon(mp: MultiPolygon<f64>) -> MultiPolygon<f64> {
    use geo::orient::{Direction, Orient};
    let cleaned = MultiPolygon(
        mp.0.into_iter()
            .filter_map(|p| {
                let keep = |ring: &LineString<f64>| {
                    let mut distinct = ring.0.clone();
                    distinct.dedup();
                    if distinct.first() == distinct.last() {
                        distinct.pop();
                    }
                    distinct.len() >= 3
                };
                if !keep(p.exterior()) {
                    return None;
                }
                let (exterior, interiors) = p.into_inner();
                Some(Polygon::new(
                    exterior,
                    interiors.into_iter().filter(keep).collect(),
                ))
            })
            .collect(),
    );
    cleaned
        .orient(Direction::Default)
        .union(&MultiPolygon(vec![]))
}

/// Accumulator for the 1-arg `ST_Union(geom)` aggregate (dissolve).
///
/// PostGIS aggregate semantics: NULL rows are skipped (the binding layers
/// enforce this — a documented exception to the scalar NULL-strictness),
/// zero rows yield SQL NULL. All-areal input is oriented and
/// unary-unioned; all-puntal input deduplicates; lineal or
/// collection members raise at the offending row.
#[derive(Default)]
pub struct UnionAggregate {
    srid: Option<i32>,
    polygons: Vec<Polygon<f64>>,
    points: Vec<Point<f64>>,
    seen_any: bool,
}

impl UnionAggregate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, bytes: &[u8]) -> Result<()> {
        const FUNC: &str = "ST_Union";
        let geom = geom::decode_auto(bytes)?;
        ensure_finite(FUNC, &geom.geometry)?;
        if let Some(existing) = self.srid {
            if existing > 0 && geom.srid > 0 && existing != geom.srid {
                return Err(Error::MixedSrid {
                    func: FUNC,
                    a: existing,
                    b: geom.srid,
                });
            }
        }
        if geom.srid > 0 {
            self.srid = Some(geom.srid);
        } else {
            self.srid.get_or_insert(geom.srid);
        }
        match classify(FUNC, &geom.geometry)? {
            Class::Puntal => {
                if !self.polygons.is_empty() {
                    return Err(unsupported(
                        FUNC,
                        Class::Puntal,
                        Class::Areal,
                        "mixed-dimension aggregate unions produce a GeometryCollection",
                    ));
                }
                for p in points_of(&geom.geometry) {
                    if !self.points.contains(&p) {
                        self.points.push(p);
                    }
                }
            }
            Class::Areal => {
                if !self.points.is_empty() {
                    return Err(unsupported(
                        FUNC,
                        Class::Areal,
                        Class::Puntal,
                        "mixed-dimension aggregate unions produce a GeometryCollection",
                    ));
                }
                self.polygons.extend(to_multi_polygon(&geom.geometry).0);
            }
            Class::Lineal => {
                return Err(unsupported(
                    FUNC,
                    Class::Lineal,
                    Class::Lineal,
                    "line unions need noding",
                ));
            }
        }
        self.seen_any = true;
        Ok(())
    }

    /// `None` = SQL NULL (zero rows aggregated).
    pub fn finish(self) -> Result<Option<Vec<u8>>> {
        use geo::orient::{Direction, Orient};
        const FUNC: &str = "ST_Union";
        if !self.seen_any {
            return Ok(None);
        }
        let srid = self.srid.unwrap_or(0);
        let geometry = if !self.polygons.is_empty() {
            // unary_union requires consistent winding across inputs; kenro
            // decodes arbitrary user rings, so orient first.
            let oriented: Vec<Polygon<f64>> = self
                .polygons
                .iter()
                .map(|p| p.orient(Direction::Default))
                .collect();
            normalize_polygons(geo::unary_union(oriented.iter()))
        } else {
            normalize_points(self.points)
        };
        Some(encode(geometry, srid, FUNC)).transpose()
    }
}

/// Options accepted by `ST_Buffer(geom, distance, options)`, PostGIS text
/// syntax: `quad_segs=8 endcap=round|flat|butt|square join=round|mitre|bevel
/// mitre_limit=5`. `side=` is not supported.
struct BufferOptions {
    quad_segs: u32,
    endcap: EndCap,
    join: JoinStyle,
    mitre_limit: f64,
}

enum EndCap {
    Round,
    Flat,
    Square,
}

enum JoinStyle {
    Round,
    Mitre,
    Bevel,
}

impl Default for BufferOptions {
    fn default() -> Self {
        // PostGIS defaults: quad_segs=8, round caps/joins, mitre_limit=5.
        BufferOptions {
            quad_segs: 8,
            endcap: EndCap::Round,
            join: JoinStyle::Round,
            mitre_limit: 5.0,
        }
    }
}

fn parse_buffer_options(func: &'static str, text: &str) -> Result<BufferOptions> {
    let mut options = BufferOptions::default();
    for token in text.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            return Err(Error::Unsupported {
                func,
                reason: format!("malformed buffer option {token:?} (expected key=value)"),
            });
        };
        match key.to_ascii_lowercase().as_str() {
            "quad_segs" => {
                options.quad_segs =
                    value
                        .parse::<u32>()
                        .ok()
                        .filter(|q| *q > 0)
                        .ok_or_else(|| Error::Unsupported {
                            func,
                            reason: format!("quad_segs must be a positive integer, got {value:?}"),
                        })?;
            }
            "endcap" => {
                options.endcap = match value.to_ascii_lowercase().as_str() {
                    "round" => EndCap::Round,
                    "flat" | "butt" => EndCap::Flat,
                    "square" => EndCap::Square,
                    other => {
                        return Err(Error::Unsupported {
                            func,
                            reason: format!("unknown endcap style {other:?}"),
                        });
                    }
                };
            }
            "join" => {
                options.join = match value.to_ascii_lowercase().as_str() {
                    "round" => JoinStyle::Round,
                    "mitre" | "miter" => JoinStyle::Mitre,
                    "bevel" => JoinStyle::Bevel,
                    other => {
                        return Err(Error::Unsupported {
                            func,
                            reason: format!("unknown join style {other:?}"),
                        });
                    }
                };
            }
            "mitre_limit" | "miter_limit" => {
                options.mitre_limit =
                    value
                        .parse::<f64>()
                        .ok()
                        .filter(|m| *m > 0.0)
                        .ok_or_else(|| Error::Unsupported {
                            func,
                            reason: format!("mitre_limit must be positive, got {value:?}"),
                        })?;
            }
            "side" => {
                return Err(Error::Unsupported {
                    func,
                    reason: "side= buffers are not supported".into(),
                });
            }
            other => {
                return Err(Error::Unsupported {
                    func,
                    reason: format!("unknown buffer option {other:?}"),
                });
            }
        }
    }
    Ok(options)
}

/// `ST_Buffer(geom, distance [, options])` — pure-Rust buffering via geo.
/// Negative distances erode areal geometries (and empty everything else,
/// as in PostGIS). Arc tessellation differs from GEOS; golden vectors
/// bound the area difference.
pub fn st_buffer(bytes: &[u8], distance: f64, options_text: Option<&str>) -> Result<Vec<u8>> {
    use geo::algorithm::buffer::{Buffer, BufferStyle, LineCap, LineJoin};
    const FUNC: &str = "ST_Buffer";
    if !distance.is_finite() {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "buffer distance must be finite".into(),
        });
    }
    let options = match options_text {
        Some(text) => parse_buffer_options(FUNC, text)?,
        None => BufferOptions::default(),
    };
    let geom = geom::decode_auto(bytes)?;
    ensure_finite(FUNC, &geom.geometry)?;
    // quad_segs → arc step angle: θ = π / (2·quad_segs), exactly PostGIS's
    // quarter-circle subdivision.
    let angle = std::f64::consts::PI / (2.0 * f64::from(options.quad_segs));
    let style = BufferStyle::new(distance)
        .line_cap(match options.endcap {
            EndCap::Round => LineCap::Round(angle),
            EndCap::Flat => LineCap::Butt,
            EndCap::Square => LineCap::Square,
        })
        .line_join(match options.join {
            JoinStyle::Round => LineJoin::Round(angle),
            JoinStyle::Mitre => LineJoin::Miter(options.mitre_limit),
            JoinStyle::Bevel => LineJoin::Bevel,
        });
    let buffered = geom.geometry.buffer_with_style(style);
    encode(normalize_polygons(buffered), geom.srid, FUNC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }

    #[test]
    fn make_valid_repairs_a_bowtie_into_two_triangles() {
        use crate::functions::accessors::{st_area, st_is_valid};
        let out = st_make_valid(&g("POLYGON((0 0,2 2,2 0,0 2,0 0))")).unwrap();
        assert!(st_is_valid(&out).unwrap());
        assert!((st_area(&out).unwrap() - 2.0).abs() < 1e-12);
        assert!(st_as_text(&out).unwrap().starts_with("MULTIPOLYGON"));
    }

    #[test]
    fn make_valid_returns_valid_and_nonareal_input_unchanged() {
        for wkt in [
            "POLYGON((0 0,3 0,3 3,0 3,0 0))",
            "POINT(1 2)",
            "LINESTRING(0 0,2 2,2 0,0 2)", // self-crossing lines are OGC-valid
        ] {
            let out = st_make_valid(&g(wkt)).unwrap();
            assert_eq!(st_as_text(&out).unwrap(), wkt);
        }
    }

    #[test]
    fn make_valid_moves_an_outside_hole_into_its_own_polygon() {
        use crate::functions::accessors::st_area;
        let out =
            st_make_valid(&g("POLYGON((0 0,4 0,4 4,0 4,0 0),(5 5,6 5,6 6,5 6,5 5))")).unwrap();
        assert!((st_area(&out).unwrap() - 17.0).abs() < 1e-12);
    }

    fn text(blob: &[u8]) -> String {
        st_as_text(blob).unwrap()
    }

    const SQUARE: &str = "POLYGON((0 0,10 0,10 10,0 10,0 0))";

    #[test]
    fn areal_boolean_ops() {
        let other = g("POLYGON((5 5,15 5,15 15,5 15,5 5))");
        let inter = text(&st_intersection(&g(SQUARE), &other).unwrap());
        assert!(inter.starts_with("POLYGON"), "{inter}");
        let union = text(&st_union(&g(SQUARE), &other).unwrap());
        assert!(union.starts_with("POLYGON"), "{union}");
        let diff = text(&st_difference(&g(SQUARE), &other).unwrap());
        assert!(diff.starts_with("POLYGON"), "{diff}");
        let xor = text(&st_sym_difference(&g(SQUARE), &other).unwrap());
        assert!(
            xor.starts_with("MULTIPOLYGON") || xor.starts_with("POLYGON"),
            "{xor}"
        );
    }

    #[test]
    fn touching_polygons_yield_empty_not_a_line() {
        // The headline documented divergence: GEOS returns the shared edge
        // as a LINESTRING; i_overlay's areal-only result is empty.
        let adjacent = g("POLYGON((10 0,20 0,20 10,10 10,10 0))");
        assert_eq!(
            text(&st_intersection(&g(SQUARE), &adjacent).unwrap()),
            "POLYGON EMPTY"
        );
    }

    #[test]
    fn point_filtering_is_exact() {
        let pts = g("MULTIPOINT(5 5,20 20,10 5)");
        assert_eq!(
            text(&st_intersection(&pts, &g(SQUARE)).unwrap()),
            "MULTIPOINT((5 5),(10 5))" // boundary point intersects
        );
        assert_eq!(
            text(&st_difference(&pts, &g(SQUARE)).unwrap()),
            "POINT(20 20)"
        );
        assert_eq!(
            text(&st_union(&g("POINT(1 1)"), &g("MULTIPOINT(1 1,2 2)")).unwrap()),
            "MULTIPOINT((1 1),(2 2))"
        );
        assert_eq!(
            text(&st_sym_difference(&g("MULTIPOINT(1 1,2 2)"), &g("MULTIPOINT(2 2,3 3)")).unwrap()),
            "MULTIPOINT((1 1),(3 3))"
        );
    }

    #[test]
    fn line_clip_against_polygons() {
        let crossing = g("LINESTRING(-5 5,15 5)");
        let inter = text(&st_intersection(&crossing, &g(SQUARE)).unwrap());
        assert!(inter.contains("0 5") && inter.contains("10 5"), "{inter}");
        let outside = text(&st_difference(&crossing, &g(SQUARE)).unwrap());
        assert!(outside.starts_with("MULTILINESTRING"), "{outside}");
    }

    #[test]
    fn unsupported_combinations_are_loud() {
        let line_a = g("LINESTRING(0 0,10 10)");
        let line_b = g("LINESTRING(0 10,10 0)");
        assert!(st_intersection(&line_a, &line_b).is_err());
        assert!(st_union(&line_a, &line_b).is_err());
        assert!(st_union(&line_a, &g(SQUARE)).is_err());
        assert!(st_sym_difference(&line_a, &g(SQUARE)).is_err());
        let gc = g("GEOMETRYCOLLECTION(POINT(1 1))");
        assert!(st_intersection(&gc, &g(SQUARE)).is_err());
    }

    #[test]
    fn union_aggregate_dissolves() {
        use geo::Area;
        // Overlapping squares dissolve into one polygon of area 175.
        let mut agg = UnionAggregate::new();
        agg.step(&g(SQUARE)).unwrap();
        agg.step(&g("POLYGON((5 5,15 5,15 15,5 15,5 5))")).unwrap();
        let blob = agg.finish().unwrap().unwrap();
        let decoded = crate::geom::decode_auto(&blob).unwrap();
        assert!((decoded.geometry.unsigned_area() - 175.0).abs() < 1e-6);

        // Zero rows → SQL NULL.
        assert_eq!(UnionAggregate::new().finish().unwrap(), None);

        // Points dedup; mixed dimensions and lines are loud.
        let mut agg = UnionAggregate::new();
        agg.step(&g("POINT(1 1)")).unwrap();
        agg.step(&g("MULTIPOINT(1 1,2 2)")).unwrap();
        let blob = agg.finish().unwrap().unwrap();
        assert_eq!(text(&blob), "MULTIPOINT((1 1),(2 2))");

        let mut agg = UnionAggregate::new();
        agg.step(&g(SQUARE)).unwrap();
        assert!(agg.step(&g("POINT(1 1)")).is_err());
        let mut agg = UnionAggregate::new();
        assert!(agg.step(&g("LINESTRING(0 0,1 1)")).is_err());
    }

    #[test]
    fn buffer_basics() {
        use geo::Area;
        // Round point buffer of r=1 approximates π.
        let buffered = st_buffer(&g("POINT(0 0)"), 1.0, None).unwrap();
        let decoded = crate::geom::decode_auto(&buffered).unwrap();
        let area = decoded.geometry.unsigned_area();
        assert!((area - std::f64::consts::PI).abs() < 0.05, "{area}");
        // Erosion of a polygon shrinks it.
        let eroded = st_buffer(&g(SQUARE), -1.0, None).unwrap();
        let decoded = crate::geom::decode_auto(&eroded).unwrap();
        assert!((decoded.geometry.unsigned_area() - 64.0).abs() < 0.5);
        // Full erosion and negative non-areal buffers empty out.
        assert_eq!(
            text(&st_buffer(&g(SQUARE), -100.0, None).unwrap()),
            "POLYGON EMPTY"
        );
        assert_eq!(
            text(&st_buffer(&g("POINT(0 0)"), -1.0, None).unwrap()),
            "POLYGON EMPTY"
        );
        // Options parse; side= and junk are loud.
        assert!(st_buffer(&g("POINT(0 0)"), 1.0, Some("quad_segs=2 endcap=square")).is_ok());
        assert!(st_buffer(&g("POINT(0 0)"), 1.0, Some("side=left")).is_err());
        assert!(st_buffer(&g("POINT(0 0)"), 1.0, Some("nonsense")).is_err());
        assert!(st_buffer(&g("POINT(0 0)"), 1.0, Some("quad_segs=0")).is_err());
    }

    #[test]
    fn empty_results_carry_postgis_typed_empties() {
        let far = g("POLYGON((100 100,110 100,110 110,100 110,100 100))");
        assert_eq!(
            text(&st_intersection(&g(SQUARE), &far).unwrap()),
            "POLYGON EMPTY"
        );
        assert_eq!(
            text(&st_intersection(&g("POINT(50 50)"), &g(SQUARE)).unwrap()),
            "POINT EMPTY"
        );
        let outside_line = g("LINESTRING(50 50,60 60)");
        assert_eq!(
            text(&st_intersection(&outside_line, &g(SQUARE)).unwrap()),
            "LINESTRING EMPTY"
        );
    }
}
