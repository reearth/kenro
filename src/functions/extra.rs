//! The remainder of the PostGIS surface kenro can reach without a new
//! dependency: a second predicate pair, the general affine form, vertex and
//! bbox accessors, angles, and geohash.
//!
//! Everything here was cheap *because* of what the earlier phases built —
//! `ST_DFullyWithin` is `ST_MaxDistance` with a comparison, `ST_RelateMatch`
//! matches the DE-9IM string `ST_Relate` already returns, `ST_Affine`
//! generalizes the rotate/scale/translate already in `affine.rs`.
//!
//! Conventions verified against a live PostGIS 3.5, including two that are
//! easy to invert: `ST_Angle` measures **clockwise** in [0, 2π), and
//! `ST_TransScale` translates *before* scaling.

use geo_types::{Coord, Geometry, LineString, MultiPoint, Point};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

fn out(geometry: Geometry<f64>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid,
            has_zm: false,
        },
        func,
    )
}

fn pair(func: &'static str, a: &[u8], b: &[u8]) -> Result<(Geom, Geom)> {
    let (ga, gb) = (geom::decode_auto(a)?, geom::decode_auto(b)?);
    if ga.srid > 0 && gb.srid > 0 && ga.srid != gb.srid {
        return Err(Error::MixedSrid {
            func,
            a: ga.srid,
            b: gb.srid,
        });
    }
    Ok((ga, gb))
}

/// `ST_ContainsProperly(a, b)` — b lies in a's interior and touches neither
/// its boundary nor its exterior. A polygon does not properly contain its own
/// corner.
///
/// This is the DE-9IM pattern `T**FF*FF*`, so it goes through the matrix
/// `ST_Relate` already produces and [`st_relate_match`] already reads —
/// one code path rather than a second containment implementation.
pub fn st_contains_properly(a: &[u8], b: &[u8]) -> Result<bool> {
    let matrix = crate::functions::predicates::st_relate(a, b)?;
    st_relate_match(&matrix, "T**FF*FF*")
}

/// `ST_DFullyWithin(a, b, d)` — **every** part of each is within `d` of the
/// other, i.e. the maximum distance is at most `d`.
pub fn st_d_fully_within(a: &[u8], b: &[u8], d: f64) -> Result<bool> {
    if d < 0.0 {
        return Err(Error::Unsupported {
            func: "ST_DFullyWithin",
            reason: "tolerance cannot be less than zero".into(),
        });
    }
    Ok(crate::functions::linear::st_max_distance(a, b)?.is_some_and(|max| max <= d))
}

/// `ST_RelateMatch(matrix, pattern)` — does a DE-9IM matrix satisfy a
/// pattern? `T` = any non-empty dimension, `F` = empty, `*` = anything,
/// `0`/`1`/`2` = that exact dimension.
pub fn st_relate_match(matrix: &str, pattern: &str) -> Result<bool> {
    const FUNC: &str = "ST_RelateMatch";
    let (m, p) = (matrix.as_bytes(), pattern.as_bytes());
    if m.len() != 9 || p.len() != 9 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "both arguments must be 9-character DE-9IM strings".into(),
        });
    }
    for (cell, want) in m.iter().zip(p) {
        let cell = cell.to_ascii_uppercase();
        let ok = match want.to_ascii_uppercase() {
            b'*' => true,
            b'T' => cell != b'F',
            b'F' => cell == b'F',
            d @ (b'0' | b'1' | b'2') => cell == d,
            other => {
                return Err(Error::Unsupported {
                    func: FUNC,
                    reason: format!("unknown pattern character {:?}", other as char),
                });
            }
        };
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `ST_Affine(geom, a, b, d, e, xoff, yoff)` — the 2D affine form:
/// `x' = a·x + b·y + xoff`, `y' = d·x + e·y + yoff`.
///
/// Z and M ride through untouched. That is PostGIS's behaviour (measured on
/// 3.5: `ST_Affine(POINT Z (1 2 3), 2,0,0,2, 10,20)` is `POINT(12 24 3)`) and
/// it is why this goes through [`crate::coords`] rather than the 2D geometry
/// model, which would have refused the input. Surface collections transform
/// too, for the same reason.
pub fn st_affine(
    bytes: &[u8],
    a: f64,
    b: f64,
    d: f64,
    e: f64,
    xoff: f64,
    yoff: f64,
) -> Result<Vec<u8>> {
    crate::coords::map_coords(bytes, &mut |p| {
        let (x, y) = (p.x, p.y);
        p.x = a * x + b * y + xoff;
        p.y = d * x + e * y + yoff;
    })
}

/// `ST_Affine(geom, a,b,c, d,e,f, g,h,i, xoff,yoff,zoff)` — the 3D form, the
/// row-major upper 3×4 of a 4×4 matrix:
///
/// ```text
/// x' = a·x + b·y + c·z + xoff
/// y' = d·x + e·y + f·z + yoff
/// z' = g·x + h·y + i·z + zoff
/// ```
///
/// **A 2D geometry stays 2D**: `z` is taken as 0 for the `x'`/`y'` rows and
/// `z'` is discarded, so the third row can only be observed on input that
/// already carries a Z. Measured on PostGIS 3.5 —
/// `ST_Affine(POINT(1 2), 1,2,3, 4,5,6, 7,8,9, 10,20,30)` is `POINT(15 34)`,
/// and the same matrix on `POINT Z (1 2 3)` is `POINT(24 52 80)`.
///
/// This is the form CityGML's implicit geometry needs: a relative geometry
/// placed into the world by a 4×4 transformation matrix.
#[allow(clippy::too_many_arguments)]
pub fn st_affine_3d(
    bytes: &[u8],
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
    g: f64,
    h: f64,
    i: f64,
    xoff: f64,
    yoff: f64,
    zoff: f64,
) -> Result<Vec<u8>> {
    crate::coords::map_coords(bytes, &mut |p| {
        let (x, y, z) = (p.x, p.y, p.z.unwrap_or(0.0));
        p.x = a * x + b * y + c * z + xoff;
        p.y = d * x + e * y + f * z + yoff;
        if let Some(pz) = p.z.as_mut() {
            *pz = g * x + h * y + i * z + zoff;
        }
    })
}

/// `ST_TransScale(geom, dx, dy, xfactor, yfactor)` — translate **then**
/// scale: `x' = (x + dx)·xfactor`. (PostGIS's order, verified live.)
pub fn st_trans_scale(
    bytes: &[u8],
    dx: f64,
    dy: f64,
    x_factor: f64,
    y_factor: f64,
) -> Result<Vec<u8>> {
    map_geometry(bytes, "ST_TransScale", |c| Coord {
        x: (c.x + dx) * x_factor,
        y: (c.y + dy) * y_factor,
    })
}

/// `ST_ReducePrecision(geom, gridsize)` — round every ordinate onto a grid.
///
/// ⚠️ PostGIS also repairs the result (its precision reducer can collapse
/// slivers); kenro only rounds, which is `ST_SnapToGrid`'s behavior. Follow
/// it with `ST_MakeValid` if you need the repair.
pub fn st_reduce_precision(bytes: &[u8], gridsize: f64) -> Result<Vec<u8>> {
    if gridsize <= 0.0 {
        return Err(Error::Unsupported {
            func: "ST_ReducePrecision",
            reason: "grid size must be positive".into(),
        });
    }
    map_geometry(bytes, "ST_ReducePrecision", |c| Coord {
        x: (c.x / gridsize).round() * gridsize,
        y: (c.y / gridsize).round() * gridsize,
    })
}

fn map_geometry(
    bytes: &[u8],
    func: &'static str,
    mut f: impl FnMut(Coord<f64>) -> Coord<f64>,
) -> Result<Vec<u8>> {
    let mut g = geom::decode_auto(bytes)?;
    crate::functions::edit::map_coords_pub(&mut g.geometry, &mut f);
    out(g.geometry, g.srid, func)
}

/// `ST_Angle(p1, p2, p3, p4)` — the angle between vectors p1→p2 and p3→p4,
/// **clockwise**, in [0, 2π). The three-point form uses p2→p1 and p2→p3.
pub fn st_angle_4(p1: &[u8], p2: &[u8], p3: &[u8], p4: &[u8]) -> Result<Option<f64>> {
    let (a, b) = (point_of(p1, "ST_Angle")?, point_of(p2, "ST_Angle")?);
    let (c, d) = (point_of(p3, "ST_Angle")?, point_of(p4, "ST_Angle")?);
    Ok(angle_between(a, b, c, d))
}

/// The three-point form: the angle at `p2`, from p2→p1 to p2→p3.
pub fn st_angle_3(p1: &[u8], p2: &[u8], p3: &[u8]) -> Result<Option<f64>> {
    let (a, b) = (point_of(p1, "ST_Angle")?, point_of(p2, "ST_Angle")?);
    let c = point_of(p3, "ST_Angle")?;
    Ok(angle_between(b, a, b, c))
}

fn angle_between(a: Coord<f64>, b: Coord<f64>, c: Coord<f64>, d: Coord<f64>) -> Option<f64> {
    let (v1, v2) = ((b.x - a.x, b.y - a.y), (d.x - c.x, d.y - c.y));
    if (v1.0 == 0.0 && v1.1 == 0.0) || (v2.0 == 0.0 && v2.1 == 0.0) {
        return None;
    }
    // Clockwise from v1 to v2: negate the usual counter-clockwise difference.
    let theta = v1.1.atan2(v1.0) - v2.1.atan2(v2.0);
    let tau = std::f64::consts::TAU;
    Some(theta.rem_euclid(tau))
}

fn point_of(bytes: &[u8], func: &'static str) -> Result<Coord<f64>> {
    match geom::decode_auto(bytes)?.geometry {
        Geometry::Point(p) => Ok(p.0),
        _ => Err(Error::Unsupported {
            func,
            reason: "arguments must be POINTs".into(),
        }),
    }
}

/// `ST_LineInterpolatePoints(line, fraction)` — a point at every multiple of
/// `fraction` along the line, the far end included.
pub fn st_line_interpolate_points(bytes: &[u8], fraction: f64) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_LineInterpolatePoints";
    if !(0.0..=1.0).contains(&fraction) || fraction <= 0.0 {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "fraction must satisfy 0 < fraction <= 1".into(),
        });
    }
    let g = geom::decode_auto(bytes)?;
    let Geometry::LineString(line) = &g.geometry else {
        return Ok(None);
    };
    let mut points = Vec::new();
    let mut t = fraction;
    while t <= 1.0 + 1e-12 {
        if let Some(p) = interpolate(line, t.min(1.0)) {
            points.push(Point::from(p));
        }
        t += fraction;
    }
    out(Geometry::MultiPoint(MultiPoint::new(points)), g.srid, FUNC).map(Some)
}

fn interpolate(line: &LineString<f64>, t: f64) -> Option<Coord<f64>> {
    let total: f64 = line.lines().map(|l| hypot(l.start, l.end)).sum();
    if total == 0.0 {
        return line.0.first().copied();
    }
    let target = total * t;
    let mut walked = 0.0;
    for seg in line.lines() {
        let len = hypot(seg.start, seg.end);
        if walked + len >= target {
            let f = if len == 0.0 {
                0.0
            } else {
                (target - walked) / len
            };
            return Some(Coord {
                x: seg.start.x + (seg.end.x - seg.start.x) * f,
                y: seg.start.y + (seg.end.y - seg.start.y) * f,
            });
        }
        walked += len;
    }
    line.0.last().copied()
}

fn hypot(a: Coord<f64>, b: Coord<f64>) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

/// `ST_Points(geom)` — every vertex as a MULTIPOINT, duplicates and all
/// (a closed ring contributes its closing vertex twice, as in PostGIS).
pub fn st_points(bytes: &[u8]) -> Result<Vec<u8>> {
    use geo::algorithm::CoordsIter;
    let g = geom::decode_auto(bytes)?;
    let points: Vec<Point<f64>> = g.geometry.coords_iter().map(Point::from).collect();
    out(
        Geometry::MultiPoint(MultiPoint::new(points)),
        g.srid,
        "ST_Points",
    )
}

/// `ST_BoundingDiagonal(geom)` — the LINESTRING from the bounding box's
/// lower-left to its upper-right.
pub fn st_bounding_diagonal(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_BoundingDiagonal";
    let g = geom::decode_auto(bytes)?;
    let Some(env) = geom::envelope(&g.geometry) else {
        return Ok(None);
    };
    out(
        Geometry::LineString(LineString::new(vec![
            Coord {
                x: env.min_x,
                y: env.min_y,
            },
            Coord {
                x: env.max_x,
                y: env.max_y,
            },
        ])),
        g.srid,
        FUNC,
    )
    .map(Some)
}

/// `ST_OrderingEquals(a, b)` — the same geometry *and* the same vertex
/// order, unlike `ST_Equals`, which is topological.
pub fn st_ordering_equals(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb) = pair("ST_OrderingEquals", a, b)?;
    Ok(ga.geometry == gb.geometry)
}

/// `ST_GeoHash(geom [, maxchars])` — the geohash of the geometry's centre,
/// 20 characters by default (PostGIS's precision for a point).
///
/// A non-point geometry is hashed to the precision its bounding box
/// justifies: PostGIS returns the shared prefix of the box's corners, and so
/// does kenro.
pub fn st_geohash(bytes: &[u8], maxchars: Option<i64>) -> Result<Option<String>> {
    const FUNC: &str = "ST_GeoHash";
    let g = geom::decode_auto(bytes)?;
    if let Some(n) = maxchars
        && n < 1
    {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "maxchars must be positive".into(),
        });
    }
    let Some(env) = geom::envelope(&g.geometry) else {
        return Ok(None);
    };
    if !(-180.0..=180.0).contains(&env.min_x)
        || !(-180.0..=180.0).contains(&env.max_x)
        || !(-90.0..=90.0).contains(&env.min_y)
        || !(-90.0..=90.0).contains(&env.max_y)
    {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "geometry must be in lon/lat degrees to be geohashed".into(),
        });
    }
    let cap = maxchars.unwrap_or(20) as usize;
    let full = encode_geohash(
        (env.min_x + env.max_x) / 2.0,
        (env.min_y + env.max_y) / 2.0,
        20,
    );
    // For an extended geometry, only the prefix its corners agree on is real.
    let stable = if env.min_x == env.max_x && env.min_y == env.max_y {
        full.len()
    } else {
        let lo = encode_geohash(env.min_x, env.min_y, 20);
        let hi = encode_geohash(env.max_x, env.max_y, 20);
        lo.bytes()
            .zip(hi.bytes())
            .take_while(|(a, b)| a == b)
            .count()
    };
    Ok(Some(full[..stable.min(cap)].to_string()))
}

const BASE32: &[u8] = b"0123456789bcdefghjkmnpqrstuvwxyz";

fn encode_geohash(lon: f64, lat: f64, chars: usize) -> String {
    let (mut lon_range, mut lat_range) = ((-180.0f64, 180.0f64), (-90.0f64, 90.0f64));
    let mut out = String::with_capacity(chars);
    let (mut bit, mut value, mut even) = (0, 0usize, true);
    while out.len() < chars {
        if even {
            let mid = (lon_range.0 + lon_range.1) / 2.0;
            if lon >= mid {
                value = (value << 1) | 1;
                lon_range.0 = mid;
            } else {
                value <<= 1;
                lon_range.1 = mid;
            }
        } else {
            let mid = (lat_range.0 + lat_range.1) / 2.0;
            if lat >= mid {
                value = (value << 1) | 1;
                lat_range.0 = mid;
            } else {
                value <<= 1;
                lat_range.1 = mid;
            }
        }
        even = !even;
        bit += 1;
        if bit == 5 {
            out.push(BASE32[value] as char);
            bit = 0;
            value = 0;
        }
    }
    out
}

/// `ST_Extent(geom)` aggregate state — the bounding box of every row.
///
/// ⚠️ PostGIS returns its `box2d` type; SQLite has none, so kenro returns a
/// POLYGON (what `ST_Envelope` would give). NULL rows are skipped, and an
/// all-NULL group yields NULL.
#[derive(Debug, Default)]
pub struct ExtentAggregate {
    srid: Option<i32>,
    bounds: Option<(f64, f64, f64, f64)>,
}

impl ExtentAggregate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, bytes: &[u8]) -> Result<()> {
        let g = geom::decode_auto(bytes)?;
        if self.srid.is_none() && g.srid > 0 {
            self.srid = Some(g.srid);
        }
        if let Some(env) = geom::envelope(&g.geometry) {
            self.bounds = Some(match self.bounds {
                None => (env.min_x, env.min_y, env.max_x, env.max_y),
                Some((minx, miny, maxx, maxy)) => (
                    minx.min(env.min_x),
                    miny.min(env.min_y),
                    maxx.max(env.max_x),
                    maxy.max(env.max_y),
                ),
            });
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Option<Vec<u8>>> {
        let Some((minx, miny, maxx, maxy)) = self.bounds else {
            return Ok(None);
        };
        let ring = LineString::new(vec![
            Coord { x: minx, y: miny },
            Coord { x: minx, y: maxy },
            Coord { x: maxx, y: maxy },
            Coord { x: maxx, y: miny },
            Coord { x: minx, y: miny },
        ]);
        out(
            Geometry::Polygon(geo_types::Polygon::new(ring, vec![])),
            self.srid.unwrap_or(0),
            "ST_Extent",
        )
        .map(Some)
    }
}

/// `ST_3DExtent(geom)` aggregate state — the 3D bounding box of every row.
///
/// ⚠️ PostGIS returns its `box3d` type. SQLite has no such type, and kenro
/// cannot write a 3D geometry to stand in for one, so this returns **the text
/// PostGIS renders a box3d as**: `BOX3D(minx miny minz,maxx maxy maxz)`. That
/// keeps the shape recognisable even though the type cannot be.
///
/// Two consequences worth knowing before reaching for it:
///
/// - The digits are kenro's (Rust's shortest round-trip), not PostGIS's.
///   PostGIS renders a box3d through the server's `extra_float_digits`, so its
///   own output is not a fixed string either — which is why kenro's golden
///   tests compare the six numbers rather than the rendering.
/// - Nothing consumes it yet. PostGIS's `ST_XMin`/`ST_ZMin` family accepts a
///   box3d; kenro's takes a geometry blob only. For the six numbers, use
///   `min(ST_MinX(g))` … `max(ST_ZMax(g))` with SQLite's own aggregates —
///   which is what a query needing them should do anyway.
///
/// A 2D row contributes Z = 0 rather than nothing, following `ST_ZMin` /
/// `ST_ZMax` (measured: PostGIS answers `BOX3D(0 0 0,5 5 0)` for
/// `LINESTRING(0 0,5 5)`, and `BOX3D(0 0 0,1 1 5)` for a 2D and a 3D row
/// together). Empty geometries contribute nothing; an all-empty or zero-row
/// group is NULL.
#[derive(Debug, Default)]
pub struct Extent3DAggregate {
    /// minx, miny, minz, maxx, maxy, maxz.
    bounds: Option<[f64; 6]>,
}

impl Extent3DAggregate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, bytes: &[u8]) -> Result<()> {
        use crate::functions::{rtree, threed};
        // Read through the accessors rather than a fresh walk: they already
        // handle surface collections and the 2D-Z-is-zero rule, and the cost
        // matches the `min(ST_MinX(g))` a caller would write by hand.
        let (Some(minx), Some(miny), Some(maxx), Some(maxy)) = (
            rtree::st_min_x(bytes)?,
            rtree::st_min_y(bytes)?,
            rtree::st_max_x(bytes)?,
            rtree::st_max_y(bytes)?,
        ) else {
            return Ok(()); // empty geometry: nothing to contribute
        };
        let minz = threed::st_zmin(bytes)?.unwrap_or(0.0);
        let maxz = threed::st_zmax(bytes)?.unwrap_or(0.0);
        self.bounds = Some(match self.bounds {
            None => [minx, miny, minz, maxx, maxy, maxz],
            Some(b) => [
                b[0].min(minx),
                b[1].min(miny),
                b[2].min(minz),
                b[3].max(maxx),
                b[4].max(maxy),
                b[5].max(maxz),
            ],
        });
        Ok(())
    }

    pub fn finish(self) -> Result<Option<String>> {
        Ok(self.bounds.map(|b| {
            format!(
                "BOX3D({} {} {},{} {} {})",
                b[0], b[1], b[2], b[3], b[4], b[5]
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }
    fn wkt(b: &[u8]) -> String {
        st_as_text(b).unwrap()
    }

    #[test]
    fn contains_properly_excludes_the_boundary() {
        let poly = g("POLYGON((0 0,3 0,3 3,0 3,0 0))");
        // PostGIS 3.5: interior point → true, corner → false.
        assert!(st_contains_properly(&poly, &g("POINT(1 1)")).unwrap());
        assert!(!st_contains_properly(&poly, &g("POINT(0 0)")).unwrap());
    }

    #[test]
    fn d_fully_within_uses_the_maximum_distance() {
        let (p, l) = (g("POINT(0 0)"), g("LINESTRING(2 -1,2 1)"));
        // PostGIS 3.5: true at 3, false at 2 (max distance is 2.236…).
        assert!(st_d_fully_within(&p, &l, 3.0).unwrap());
        assert!(!st_d_fully_within(&p, &l, 2.0).unwrap());
        assert!(st_d_fully_within(&p, &l, -1.0).is_err());
    }

    #[test]
    fn relate_match_reads_the_de9im_pattern_language() {
        // PostGIS 3.5: ST_RelateMatch('101202FFF','TTTTTTFFF') → true
        assert!(st_relate_match("101202FFF", "TTTTTTFFF").unwrap());
        assert!(st_relate_match("101202FFF", "*********").unwrap());
        assert!(!st_relate_match("101202FFF", "FFFFFFFFF").unwrap());
        assert!(st_relate_match("101202FFF", "1********").unwrap());
        assert!(!st_relate_match("101202FFF", "2********").unwrap());
        assert!(st_relate_match("FFF", "TTT").is_err());
        assert!(st_relate_match("101202FFF", "XXXXXXXXX").is_err());
    }

    #[test]
    fn affine_and_trans_scale_match_postgis_argument_order() {
        // PostGIS 3.5: ST_Affine(LINESTRING(1 2,3 4),2,0,0,2,10,20)
        assert_eq!(
            wkt(&st_affine(&g("LINESTRING(1 2,3 4)"), 2.0, 0.0, 0.0, 2.0, 10.0, 20.0).unwrap()),
            "LINESTRING(12 24,16 28)"
        );
        // PostGIS 3.5: ST_TransScale(POINT(1 2),1,2,3,4) → POINT(6 16):
        // translate first, then scale.
        assert_eq!(
            wkt(&st_trans_scale(&g("POINT(1 2)"), 1.0, 2.0, 3.0, 4.0).unwrap()),
            "POINT(6 16)"
        );
    }

    #[test]
    fn angle_is_measured_clockwise() {
        // PostGIS 3.5: ST_Angle((0 0),(1 0),(0 0),(0 1)) → 270°, not 90°.
        let a = st_angle_4(
            &g("POINT(0 0)"),
            &g("POINT(1 0)"),
            &g("POINT(0 0)"),
            &g("POINT(0 1)"),
        )
        .unwrap()
        .unwrap();
        assert!((a.to_degrees() - 270.0).abs() < 1e-9, "{}", a.to_degrees());
        // Three-point form at the vertex: same answer for this configuration.
        let b = st_angle_3(&g("POINT(1 0)"), &g("POINT(0 0)"), &g("POINT(0 1)"))
            .unwrap()
            .unwrap();
        assert!((b.to_degrees() - 270.0).abs() < 1e-9, "{}", b.to_degrees());
        // A zero-length vector has no angle.
        assert!(
            st_angle_3(&g("POINT(0 0)"), &g("POINT(0 0)"), &g("POINT(0 1)"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn vertex_and_bbox_accessors() {
        // PostGIS 3.5: MULTIPOINT((2.5 0),(5 0),(7.5 0),(10 0))
        assert_eq!(
            wkt(
                &st_line_interpolate_points(&g("LINESTRING(0 0,10 0)"), 0.25)
                    .unwrap()
                    .unwrap()
            ),
            "MULTIPOINT((2.5 0),(5 0),(7.5 0),(10 0))"
        );
        // The closing vertex appears twice, as in PostGIS.
        assert_eq!(
            wkt(&st_points(&g("POLYGON((0 0,1 0,1 1,0 0))")).unwrap()),
            "MULTIPOINT((0 0),(1 0),(1 1),(0 0))"
        );
        assert_eq!(
            wkt(&st_bounding_diagonal(&g("LINESTRING(1 2,5 9)"))
                .unwrap()
                .unwrap()),
            "LINESTRING(1 2,5 9)"
        );
        assert!(st_ordering_equals(&g("LINESTRING(0 0,1 1)"), &g("LINESTRING(0 0,1 1)")).unwrap());
        // Reversed: topologically equal, but not ordering-equal.
        assert!(!st_ordering_equals(&g("LINESTRING(0 0,1 1)"), &g("LINESTRING(1 1,0 0)")).unwrap());
    }

    #[test]
    fn geohash_matches_postgis() {
        let tokyo = st_geom_from_text("POINT(139.7 35.68)", Some(4326)).unwrap();
        // PostGIS 3.5: 'xn76fzq7jfn42q30gmb9' (20 chars), and 'xn76f' at 5.
        assert_eq!(
            st_geohash(&tokyo, None).unwrap().as_deref(),
            Some("xn76fzq7jfn42q30gmb9")
        );
        assert_eq!(
            st_geohash(&tokyo, Some(5)).unwrap().as_deref(),
            Some("xn76f")
        );
        // An extended geometry only keeps the prefix its corners agree on.
        let line = st_geom_from_text("LINESTRING(139.7 35.68,139.8 35.7)", Some(4326)).unwrap();
        assert_eq!(st_geohash(&line, None).unwrap().as_deref(), Some("xn7"));
        // Outside lon/lat, a geohash is meaningless.
        let projected = st_geom_from_text("POINT(15551574 4257201)", Some(3857)).unwrap();
        assert!(st_geohash(&projected, None).is_err());
    }

    #[test]
    fn reduce_precision_rounds_onto_the_grid() {
        // PostGIS 3.5: ST_ReducePrecision(POINT(1.234 5.678), 0.1) → POINT(1.2 5.7)
        let p = st_reduce_precision(&g("POINT(1.234 5.678)"), 0.1).unwrap();
        let x = crate::functions::accessors::st_x(&p).unwrap().unwrap();
        assert!((x - 1.2).abs() < 1e-9, "{x}");
        assert!(st_reduce_precision(&g("POINT(1 2)"), 0.0).is_err());
    }

    #[test]
    fn extent_folds_every_row_and_skips_an_empty_group() {
        let mut agg = ExtentAggregate::new();
        agg.step(&g("POINT(1 2)")).unwrap();
        agg.step(&g("POINT(5 0)")).unwrap();
        // PostGIS 3.5: BOX(1 0,5 2) — kenro returns the same box as a polygon.
        assert_eq!(
            wkt(&agg.finish().unwrap().unwrap()),
            "POLYGON((1 0,1 2,5 2,5 0,1 0))"
        );
        assert!(ExtentAggregate::new().finish().unwrap().is_none());
    }

    /// ISO WKB `POINT Z (x y z)` — the route 3D input actually takes into
    /// kenro (written by GDAL/QGIS, not by a constructor).
    fn point_z(x: f64, y: f64, z: f64) -> Vec<u8> {
        let mut v = vec![0x01];
        v.extend_from_slice(&1001u32.to_le_bytes());
        for value in [x, y, z] {
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    }

    #[test]
    fn affine_2d_form_carries_z_through_like_postgis() {
        // Measured on PostGIS 3.5:
        //   ST_Affine(POINT Z (1 2 3), 2,0,0,2, 10,20) → POINT(12 24 3)
        // kenro used to refuse this outright; the Z now rides along.
        let out = st_affine(&point_z(1.0, 2.0, 3.0), 2.0, 0.0, 0.0, 2.0, 10.0, 20.0).unwrap();
        use crate::functions::{rtree, threed};
        assert_eq!(rtree::st_min_x(&out).unwrap(), Some(12.0));
        assert_eq!(rtree::st_min_y(&out).unwrap(), Some(24.0));
        assert_eq!(threed::st_z(&out).unwrap(), Some(3.0));
        assert_eq!(threed::st_coord_dim(&out).unwrap(), 3);
    }

    #[test]
    fn affine_3d_form_matches_the_measured_matrix() {
        use crate::functions::{rtree, threed};
        // Measured on PostGIS 3.5, matrix (1,2,3 / 4,5,6 / 7,8,9) + (10,20,30):
        //   POINT Z (1 2 3) → POINT(24 52 80)
        let out = st_affine_3d(
            &point_z(1.0, 2.0, 3.0),
            1.0,
            2.0,
            3.0,
            4.0,
            5.0,
            6.0,
            7.0,
            8.0,
            9.0,
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        assert_eq!(rtree::st_min_x(&out).unwrap(), Some(24.0));
        assert_eq!(rtree::st_min_y(&out).unwrap(), Some(52.0));
        assert_eq!(threed::st_z(&out).unwrap(), Some(80.0));

        // …and the same matrix on 2D input → POINT(15 34): z is taken as 0
        // for the x/y rows, the z row is discarded, the result stays 2D.
        let flat = st_affine_3d(
            &g("POINT(1 2)"),
            1.0,
            2.0,
            3.0,
            4.0,
            5.0,
            6.0,
            7.0,
            8.0,
            9.0,
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        assert_eq!(wkt(&flat), "POINT(15 34)");
        assert!(!threed::st_has_z(&flat).unwrap());
    }

    #[test]
    fn affine_3d_moves_a_building() {
        // The CityGML case: place a relative geometry into the world. No 2D
        // function can touch a POLYHEDRALSURFACE, but this one can.
        let cube = crate::functions::surface::fixtures::cube(6);
        let placed = st_affine_3d(
            &cube, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1000.0, 2000.0, 50.0,
        )
        .unwrap();
        use crate::functions::{rtree, surface, threed};
        assert_eq!(surface::st_num_patches(&placed).unwrap(), Some(6));
        assert_eq!(surface::is_closed(&placed).unwrap(), Some(true));
        assert_eq!(rtree::st_min_x(&placed).unwrap(), Some(1000.0));
        assert_eq!(rtree::st_max_y(&placed).unwrap(), Some(2001.0));
        assert_eq!(threed::st_zmin(&placed).unwrap(), Some(50.0));
        assert_eq!(threed::st_zmax(&placed).unwrap(), Some(51.0));
    }

    #[test]
    fn extent_3d_reports_the_box_postgis_reports() {
        let mut agg = Extent3DAggregate::new();
        agg.step(&point_z(1.0, 2.0, 3.0)).unwrap();
        agg.step(&point_z(7.0, 8.0, 9.0)).unwrap();
        // Measured on PostGIS 3.5: BOX3D(1 2 3,7 8 9).
        assert_eq!(agg.finish().unwrap().unwrap(), "BOX3D(1 2 3,7 8 9)");

        // A 2D row contributes Z = 0, not nothing — PostGIS answers
        // BOX3D(0 0 0,5 5 0) for LINESTRING(0 0,5 5).
        let mut agg = Extent3DAggregate::new();
        agg.step(&g("LINESTRING(0 0,5 5)")).unwrap();
        assert_eq!(agg.finish().unwrap().unwrap(), "BOX3D(0 0 0,5 5 0)");

        // Mixed 2D and 3D rows: PostGIS gives BOX3D(0 0 0,1 1 5).
        let mut agg = Extent3DAggregate::new();
        agg.step(&g("POINT(0 0)")).unwrap();
        agg.step(&point_z(1.0, 1.0, 5.0)).unwrap();
        assert_eq!(agg.finish().unwrap().unwrap(), "BOX3D(0 0 0,1 1 5)");

        // Surfaces are covered, since the accessors already were.
        let mut agg = Extent3DAggregate::new();
        agg.step(&crate::functions::surface::fixtures::cube(6))
            .unwrap();
        assert_eq!(agg.finish().unwrap().unwrap(), "BOX3D(0 0 0,1 1 1)");

        // Zero rows, and an all-empty group, are NULL (PostGIS: NULL).
        assert!(Extent3DAggregate::new().finish().unwrap().is_none());
        let mut agg = Extent3DAggregate::new();
        agg.step(&g("LINESTRING EMPTY")).unwrap();
        assert!(agg.finish().unwrap().is_none());
    }
}
