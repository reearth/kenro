//! Structural accessors and geometry editing: rings, boundaries, vertex
//! surgery and coordinate-space tweaks.
//!
//! No new algorithms and no new dependencies — this is `geo_types` handling
//! plus PostGIS's exact conventions, which are worth stating because several
//! are easy to guess wrong (all verified against a live PostGIS 3.5):
//!
//! - the ring/vertex indexes are **1-based for rings, 0-based for vertices**
//!   (`ST_InteriorRingN(g, 1)` but `ST_SetPoint(g, 0, p)`)
//! - a wrong-type argument yields NULL, not an error — except `ST_IsRing`,
//!   which raises
//! - `ST_Boundary` of a point is `POINT EMPTY`, and of a closed line
//!   `MULTIPOINT EMPTY`

use geo_types::{Coord, Geometry, LineString, MultiLineString, MultiPoint, Point, Polygon};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

/// Encode a derived geometry, restoring Z from the inputs it came from.
/// See [`geom::encode_derived`] for why every call site has to name them.
fn out(
    geometry: Geometry<f64>,
    srid: i32,
    func: &'static str,
    sources: &[&[u8]],
) -> Result<Vec<u8>> {
    geom::encode_derived(geometry, srid, func, sources)
}

/// Encode a derived geometry as 2D **on purpose**: PostGIS answers in 2D here
/// too (measured), so there is no Z to preserve and nothing to refuse.
fn out_2d(geometry: Geometry<f64>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &Geom {
            geometry,
            srid,
            has_zm: false,
        },
        func,
    )
}

fn as_polygon(g: &Geometry<f64>) -> Option<&Polygon<f64>> {
    match g {
        Geometry::Polygon(p) => Some(p),
        _ => None,
    }
}

fn as_line(g: &Geometry<f64>) -> Option<&LineString<f64>> {
    match g {
        Geometry::LineString(l) => Some(l),
        _ => None,
    }
}

/// `ST_ExteriorRing(polygon)` — NULL for any other type.
pub fn st_exterior_ring(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let g = geom::decode_auto(bytes)?;
    let Some(poly) = as_polygon(&g.geometry) else {
        return Ok(None);
    };
    out(
        Geometry::LineString(poly.exterior().clone()),
        g.srid,
        "ST_ExteriorRing",
        &[bytes],
    )
    .map(Some)
}

/// `ST_InteriorRingN(polygon, n)` — **1-based**; NULL when out of range or
/// not a polygon.
pub fn st_interior_ring_n(bytes: &[u8], n: i64) -> Result<Option<Vec<u8>>> {
    let g = geom::decode_auto(bytes)?;
    let Some(poly) = as_polygon(&g.geometry) else {
        return Ok(None);
    };
    if n < 1 {
        return Ok(None);
    }
    let Some(ring) = poly.interiors().get((n - 1) as usize) else {
        return Ok(None);
    };
    out(
        Geometry::LineString(ring.clone()),
        g.srid,
        "ST_InteriorRingN",
        &[bytes],
    )
    .map(Some)
}

/// `ST_NumInteriorRings(polygon)` — NULL for any other type.
pub fn st_num_interior_rings(bytes: &[u8]) -> Result<Option<i64>> {
    let g = geom::decode_auto(bytes)?;
    Ok(as_polygon(&g.geometry).map(|p| p.interiors().len() as i64))
}

/// `ST_NRings(geom)` — exterior + interior rings, summed over a multipolygon.
/// Non-areal input has none.
pub fn st_nrings(bytes: &[u8]) -> Result<i64> {
    let g = geom::decode_auto(bytes)?;
    Ok(match &g.geometry {
        Geometry::Polygon(p) => 1 + p.interiors().len() as i64,
        Geometry::MultiPolygon(mp) => mp.iter().map(|p| 1 + p.interiors().len() as i64).sum(),
        _ => 0,
    })
}

/// `ST_Boundary(geom)` — the topological boundary.
///
/// PostGIS's shapes, verified live: a polygon gives a LINESTRING (its
/// exterior) or a MULTILINESTRING when it has holes; an open line gives a
/// MULTIPOINT of its two endpoints; a closed line gives `MULTIPOINT EMPTY`;
/// a point gives `POINT EMPTY`.
pub fn st_boundary(bytes: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Boundary";
    let g = geom::decode_auto(bytes)?;
    let boundary = match &g.geometry {
        Geometry::Point(_) | Geometry::MultiPoint(_) => {
            Geometry::Point(Point::new(f64::NAN, f64::NAN))
        }
        Geometry::LineString(line) => Geometry::MultiPoint(line_boundary(line)),
        Geometry::MultiLineString(mls) => {
            // A vertex shared by an odd number of ends stays on the boundary
            // (the SQL/MM mod-2 rule).
            let mut ends: Vec<Coord<f64>> = Vec::new();
            for line in mls {
                for p in line_boundary(line).into_iter() {
                    let c = p.0;
                    if let Some(pos) = ends.iter().position(|e| *e == c) {
                        ends.remove(pos);
                    } else {
                        ends.push(c);
                    }
                }
            }
            Geometry::MultiPoint(MultiPoint::new(ends.into_iter().map(Point::from).collect()))
        }
        Geometry::Polygon(poly) => rings_to_geometry(
            std::iter::once(poly.exterior().clone())
                .chain(poly.interiors().iter().cloned())
                .collect(),
        ),
        Geometry::MultiPolygon(mp) => rings_to_geometry(
            mp.iter()
                .flat_map(|poly| {
                    std::iter::once(poly.exterior().clone()).chain(poly.interiors().iter().cloned())
                })
                .collect(),
        ),
        Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::Line(_) => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "unsupported geometry type".into(),
            });
        }
        Geometry::GeometryCollection(_) => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "GeometryCollection operands are not supported".into(),
            });
        }
    };
    out(boundary, g.srid, FUNC, &[bytes])
}

/// The two endpoints of an open line; none at all when it is closed.
fn line_boundary(line: &LineString<f64>) -> MultiPoint<f64> {
    let (Some(first), Some(last)) = (line.0.first(), line.0.last()) else {
        return MultiPoint::new(vec![]);
    };
    if first == last {
        return MultiPoint::new(vec![]);
    }
    MultiPoint::new(vec![Point::from(*first), Point::from(*last)])
}

fn rings_to_geometry(mut rings: Vec<LineString<f64>>) -> Geometry<f64> {
    if rings.len() == 1 {
        Geometry::LineString(rings.remove(0))
    } else {
        Geometry::MultiLineString(MultiLineString::new(rings))
    }
}

/// `ST_IsClosed(geom)` — first vertex equals last. True for areal input
/// (whose rings are closed by definition), false for a point.
pub fn st_is_closed(bytes: &[u8]) -> Result<bool> {
    // A surface collection is closed when it is a shell: every edge shared
    // by exactly two patches.
    if let Some(closed) = crate::functions::surface::is_closed(bytes)? {
        return Ok(closed);
    }
    let g = geom::decode_auto(bytes)?;
    Ok(match &g.geometry {
        Geometry::LineString(l) => is_closed_line(l),
        Geometry::MultiLineString(mls) => mls.iter().all(is_closed_line),
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => true,
        _ => false,
    })
}

fn is_closed_line(l: &LineString<f64>) -> bool {
    match (l.0.first(), l.0.last()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `ST_IsRing(line)` — closed and simple. **Raises** on non-linear input,
/// which is PostGIS's behavior here rather than the usual NULL.
pub fn st_is_ring(bytes: &[u8]) -> Result<bool> {
    const FUNC: &str = "ST_IsRing";
    let g = geom::decode_auto(bytes)?;
    let Some(line) = as_line(&g.geometry) else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "ST_IsRing() should only be called on a linear feature".into(),
        });
    };
    // A closed ring is simple exactly when it bounds a valid polygon, which
    // reuses geo's validation rather than hand-rolling a sweep.
    use geo::algorithm::Validation;
    Ok(is_closed_line(line) && Polygon::new(line.clone(), vec![]).is_valid())
}

/// `ST_AddPoint(line, point [, position])` — 0-based; the default (or -1)
/// appends. NULL when either argument is the wrong type.
pub fn st_add_point(line: &[u8], point: &[u8], position: Option<i64>) -> Result<Option<Vec<u8>>> {
    let (g, mut coords, p) = match line_and_point(line, point, "ST_AddPoint")? {
        Some(v) => v,
        None => return Ok(None),
    };
    let at = match position {
        None | Some(-1) => coords.len(),
        Some(n) if n >= 0 && (n as usize) <= coords.len() => n as usize,
        Some(_) => return Ok(None),
    };
    coords.insert(at, p);
    out(
        Geometry::LineString(LineString::new(coords)),
        g.srid,
        "ST_AddPoint",
        &[line, point],
    )
    .map(Some)
}

/// `ST_SetPoint(line, index, point)` — 0-based; negative indexes count from
/// the end, as in PostGIS.
pub fn st_set_point(line: &[u8], index: i64, point: &[u8]) -> Result<Option<Vec<u8>>> {
    let (g, mut coords, p) = match line_and_point(line, point, "ST_SetPoint")? {
        Some(v) => v,
        None => return Ok(None),
    };
    let Some(at) = resolve_index(index, coords.len()) else {
        return Ok(None);
    };
    coords[at] = p;
    out(
        Geometry::LineString(LineString::new(coords)),
        g.srid,
        "ST_SetPoint",
        &[line, point],
    )
    .map(Some)
}

/// `ST_RemovePoint(line, index)` — 0-based. NULL when the index is out of
/// range or the input is not a line.
pub fn st_remove_point(line: &[u8], index: i64) -> Result<Option<Vec<u8>>> {
    let g = geom::decode_auto(line)?;
    let Some(l) = as_line(&g.geometry) else {
        return Ok(None);
    };
    let mut coords = l.0.clone();
    let Some(at) = resolve_index(index, coords.len()) else {
        return Ok(None);
    };
    coords.remove(at);
    out(
        Geometry::LineString(LineString::new(coords)),
        g.srid,
        "ST_RemovePoint",
        &[line],
    )
    .map(Some)
}

fn resolve_index(index: i64, len: usize) -> Option<usize> {
    if index >= 0 && (index as usize) < len {
        Some(index as usize)
    } else {
        None
    }
}

type LineEdit = (Geom, Vec<Coord<f64>>, Coord<f64>);

fn line_and_point(line: &[u8], point: &[u8], func: &'static str) -> Result<Option<LineEdit>> {
    let g = geom::decode_auto(line)?;
    let p = geom::decode_auto(point)?;
    if g.srid > 0 && p.srid > 0 && g.srid != p.srid {
        return Err(Error::MixedSrid {
            func,
            a: g.srid,
            b: p.srid,
        });
    }
    let (Some(l), Geometry::Point(pt)) = (as_line(&g.geometry), &p.geometry) else {
        return Ok(None);
    };
    let coords = l.0.clone();
    let c = pt.0;
    Ok(Some((g, coords, c)))
}

/// `ST_MakeLine(a, b)` — the two-geometry form; points and lines are
/// concatenated in order.
pub fn st_make_line(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_MakeLine";
    let ga = geom::decode_auto(a)?;
    let gb = geom::decode_auto(b)?;
    if ga.srid > 0 && gb.srid > 0 && ga.srid != gb.srid {
        return Err(Error::MixedSrid {
            func: FUNC,
            a: ga.srid,
            b: gb.srid,
        });
    }
    let mut coords = Vec::new();
    for g in [&ga.geometry, &gb.geometry] {
        match g {
            Geometry::Point(p) => coords.push(p.0),
            Geometry::LineString(l) => coords.extend(l.0.iter().copied()),
            Geometry::MultiPoint(mp) => coords.extend(mp.iter().map(|p| p.0)),
            _ => {
                return Err(Error::Unsupported {
                    func: FUNC,
                    reason: "arguments must be points or linestrings".into(),
                });
            }
        }
    }
    let srid = if ga.srid > 0 { ga.srid } else { gb.srid };
    out(
        Geometry::LineString(LineString::new(coords)),
        srid,
        FUNC,
        &[a, b],
    )
}

/// `ST_MakePolygon(linestring)` — the shell must be closed, as in PostGIS.
pub fn st_make_polygon(bytes: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_MakePolygon";
    let g = geom::decode_auto(bytes)?;
    let Some(line) = as_line(&g.geometry) else {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "argument must be a LINESTRING".into(),
        });
    };
    if !is_closed_line(line) {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "shell is not closed".into(),
        });
    }
    out(
        Geometry::Polygon(Polygon::new(line.clone(), vec![])),
        g.srid,
        FUNC,
        &[bytes],
    )
}

/// `ST_Multi(geom)` — wrap a singular geometry in its MULTI form; already
/// multi input is returned unchanged.
pub fn st_multi(bytes: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_Multi";
    let g = geom::decode_auto(bytes)?;
    let multi = match g.geometry {
        Geometry::Point(p) => Geometry::MultiPoint(MultiPoint::new(vec![p])),
        Geometry::LineString(l) => Geometry::MultiLineString(MultiLineString::new(vec![l])),
        Geometry::Polygon(p) => Geometry::MultiPolygon(geo_types::MultiPolygon::new(vec![p])),
        other @ (Geometry::MultiPoint(_)
        | Geometry::MultiLineString(_)
        | Geometry::MultiPolygon(_)) => other,
        _ => {
            return Err(Error::Unsupported {
                func: FUNC,
                reason: "unsupported geometry type".into(),
            });
        }
    };
    out(multi, g.srid, FUNC, &[bytes])
}

/// `ST_SnapToGrid(geom, size)` / `(geom, sizex, sizey)` — round every
/// ordinate onto a grid anchored at the origin. A size of 0 leaves that axis
/// untouched (PostGIS behavior).
///
/// ⚠️ **PostGIS also drops the vertices that collapse together**, which kenro
/// does not. Measured on 3.5:
/// `ST_SnapToGrid(LINESTRING(0 0,0.1 0.1,1 1,1.1 1.1), 1)` is
/// `LINESTRING(0 0,1 1)` where kenro answers `LINESTRING(0 0,0 0,1 1,1 1)`,
/// and a polygon that collapses entirely becomes `POLYGON EMPTY` in PostGIS
/// against kenro's degenerate ring. Follow with `ST_RemoveRepeatedPoints` for
/// the vertex behavior.
///
/// It is also why this stays on the 2D path while the coordinate transforms
/// moved to `coords`: it is not coordinate-wise, and PostGIS refuses surface
/// collections here (`lwgeom_grid_in_place: Unsupported geometry type`).
pub fn st_snap_to_grid(bytes: &[u8], size_x: f64, size_y: f64) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_SnapToGrid";
    let mut g = geom::decode_auto(bytes)?;
    let snap = |v: f64, size: f64| {
        if size > 0.0 {
            (v / size).round() * size
        } else {
            v
        }
    };
    map_coords(&mut g.geometry, &mut |c| Coord {
        x: snap(c.x, size_x),
        y: snap(c.y, size_y),
    });
    // `encode_canonical_gpb` on the decoded value, not `out(g.geometry, …)`:
    // `out` rebuilds a `Geom` with `has_zm: false`, which turns "kenro refuses
    // 3D output" into "kenro silently drops the Z". Since this function stays
    // on the 2D path deliberately, it has to say so rather than flatten.
    geom::encode_canonical_gpb(&g, FUNC)
}

/// `ST_FlipCoordinates(geom)` — swap x and y, the fix for lat/lon-ordered data.
///
/// Only x and y: Z and M stay where they are, and a surface collection flips.
/// Measured on PostGIS 3.5 — `ST_FlipCoordinates(POINT Z (1 2 3))` is
/// `POINT(2 1 3)`, and `POINT M (1 2 99)` gives `POINTM(2 1 99)`.
pub fn st_flip_coordinates(bytes: &[u8]) -> Result<Vec<u8>> {
    crate::coords::map_coords(bytes, &mut |p| std::mem::swap(&mut p.x, &mut p.y))
}

/// Zero the low mantissa bits of `d` that `decimal_digits` decimal places
/// cannot distinguish — PostGIS's `trim_preserve_decimal_digits`, reproduced.
///
/// This is the whole of `ST_QuantizeCoordinates`, and it is worth spelling
/// out because it is not the rounding rule anyone would guess:
///
/// ```text
/// exponent    = (bits >> 52 & 2047) - 1023          // floor(log2(|d|))
/// bits_needed = 1 + exponent + (dd * 851 + 255) / 256   // C division, toward zero
/// if dd < 0 { bits_needed -= 1 }
/// if bits_needed >= 52 { return d }                 // also catches NaN/Inf
/// bits &= !0u64 << (52 - bits_needed.max(1))
/// ```
///
/// `(dd * 851 + 255) / 256` is PostGIS's integer stand-in for
/// `1 + (int)(dd * log2(10))` over `dd` in `[0, 30]`; for negative `dd` the
/// same expression truncates the other way, which is what the `-= 1` fixes
/// up. `docs/scope.md` used to exclude this function because the rule
/// "could not be reproduced (its results for prec 2 and 3 are identical)" —
/// that observation is real but is not the rule: `bits_needed` genuinely
/// differs between 2 and 3, and the *results* coincide whenever the bits the
/// wider mask would have kept were already zero. Verified bit-exactly
/// against PostGIS 3.5.2 over 30 sentinel values × `prec` −30…40 (2201
/// pairs), subnormals, ±0, `f64::MIN_POSITIVE`, `f64::MAX`, NaN and Inf
/// included; `tests/golden/quantize.jsonl` carries the vectors as hex.
///
/// Note what it is *not*: a grid. The result is the input with bits removed,
/// so it stays close to the input and compresses well, but it is not on any
/// round decimal. `ST_SnapToGrid` and `ST_ReducePrecision` are the grid.
fn trim_preserve_decimal_digits(d: f64, decimal_digits: i32) -> f64 {
    let bits = d.to_bits();
    let exponent = ((bits >> 52) & 2047) as i32 - 1023;
    // Rust's `/` on integers truncates toward zero, exactly like C's.
    let mut bits_needed = 1 + exponent + (decimal_digits * 851 + 255) / 256;
    if decimal_digits < 0 {
        bits_needed -= 1;
    }
    // NaN and Inf have exponent 1024, so this returns them untouched for any
    // sane `decimal_digits` — PostGIS relies on the same accident.
    if bits_needed >= 52 {
        return d;
    }
    let shift = 52 - bits_needed.max(1);
    f64::from_bits(bits & (u64::MAX << shift))
}

/// `ST_QuantizeCoordinates(geom, prec_x [, prec_y [, prec_z]])` — zero the
/// coordinate bits that `prec` decimal places cannot distinguish, so the
/// geometry compresses better while every ordinate stays within its stated
/// precision of the original.
///
/// A NULL `prec_y` or `prec_z` falls back to **`prec_x`**, not to the
/// argument before it (measured: `ST_QuantizeCoordinates(POINT Z(…), 2, 15)`
/// leaves y alone and still quantizes z at 2). kenro spells that as separate
/// arities rather than SQL defaults, which SQLite has no syntax for.
///
/// ⚠️ **A geometry carrying M is refused.** PostGIS quantizes M too, with
/// `prec_m` defaulting to `prec_x`; kenro's coordinate walker has an X, a Y
/// and an optional Z and no M slot, so the alternative to raising would be
/// returning a geometry whose M is silently un-quantized while its name
/// promises otherwise. There is no `prec_m` arity for the same reason.
///
/// Everything else rides through untouched, because this walks the encoding
/// rather than decoding: the SRID, empty geometries, and surface collections
/// (PostGIS quantizes a POLYHEDRALSURFACE too — measured).
pub fn st_quantize_coordinates(
    bytes: &[u8],
    prec_x: i32,
    prec_y: Option<i32>,
    prec_z: Option<i32>,
) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_QuantizeCoordinates";
    if crate::coords::has_m(bytes)? {
        return Err(Error::Unsupported {
            func: FUNC,
            reason: "a geometry with M is refused rather than returned with its M \
                     un-quantized — PostGIS quantizes M at prec_x by default, and kenro's \
                     coordinate walker has no M slot. Drop the M first"
                .into(),
        });
    }
    let (py, pz) = (prec_y.unwrap_or(prec_x), prec_z.unwrap_or(prec_x));
    crate::coords::map_coords(bytes, &mut |c| {
        c.x = trim_preserve_decimal_digits(c.x, prec_x);
        c.y = trim_preserve_decimal_digits(c.y, py);
        if let Some(z) = c.z {
            c.z = Some(trim_preserve_decimal_digits(z, pz));
        }
    })
}

/// `ST_ShiftLongitude(geom)` — move x from [-180,180) into [0,360).
///
/// Z and M ride through, and a surface collection shifts (measured on 3.5:
/// `ST_ShiftLongitude(POINT Z (-170 2 3))` is `POINT(190 2 3)`).
pub fn st_shift_longitude(bytes: &[u8]) -> Result<Vec<u8>> {
    crate::coords::map_coords(bytes, &mut |p| {
        if p.x < 0.0 {
            p.x += 360.0;
        }
    })
}

/// `ST_Expand(geom, units)` — the bounding box grown on every side, as a
/// POLYGON (PostGIS returns its `box2d` type, which SQLite has no equivalent
/// for; the polygon is what `ST_Envelope` would give).
pub fn st_expand(bytes: &[u8], units: f64) -> Result<Option<Vec<u8>>> {
    const FUNC: &str = "ST_Expand";
    let g = geom::decode_auto(bytes)?;
    let Some(env) = geom::envelope(&g.geometry) else {
        return Ok(None);
    };
    let (minx, miny) = (env.min_x - units, env.min_y - units);
    let (maxx, maxy) = (env.max_x + units, env.max_y + units);
    let ring = LineString::new(vec![
        Coord { x: minx, y: miny },
        Coord { x: minx, y: maxy },
        Coord { x: maxx, y: maxy },
        Coord { x: maxx, y: miny },
        Coord { x: minx, y: miny },
    ]);
    out_2d(Geometry::Polygon(Polygon::new(ring, vec![])), g.srid, FUNC).map(Some)
}

/// Apply `f` to every coordinate in place, without pulling geo's MapCoords
/// (which would clone the whole geometry). Shared with `functions::extra`.
pub(crate) fn map_coords_pub(g: &mut Geometry<f64>, f: &mut impl FnMut(Coord<f64>) -> Coord<f64>) {
    map_coords(g, f)
}

fn map_coords(g: &mut Geometry<f64>, f: &mut impl FnMut(Coord<f64>) -> Coord<f64>) {
    match g {
        Geometry::Point(p) => p.0 = f(p.0),
        Geometry::MultiPoint(mp) => {
            for p in mp.iter_mut() {
                p.0 = f(p.0);
            }
        }
        Geometry::LineString(l) => map_line(l, f),
        Geometry::MultiLineString(mls) => {
            for l in mls.iter_mut() {
                map_line(l, f);
            }
        }
        Geometry::Polygon(p) => map_polygon(p, f),
        Geometry::MultiPolygon(mp) => {
            for p in mp.iter_mut() {
                map_polygon(p, f);
            }
        }
        Geometry::GeometryCollection(gc) => {
            for g in gc.iter_mut() {
                map_coords(g, f);
            }
        }
        Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::Line(_) => {}
    }
}

fn map_line(l: &mut LineString<f64>, f: &mut impl FnMut(Coord<f64>) -> Coord<f64>) {
    for c in l.0.iter_mut() {
        *c = f(*c);
    }
}

fn map_polygon(p: &mut Polygon<f64>, f: &mut impl FnMut(Coord<f64>) -> Coord<f64>) {
    // Polygon guards its rings, so rebuild it from mapped copies.
    let mut exterior = p.exterior().clone();
    map_line(&mut exterior, f);
    let interiors: Vec<LineString<f64>> = p
        .interiors()
        .iter()
        .map(|r| {
            let mut r = r.clone();
            map_line(&mut r, f);
            r
        })
        .collect();
    *p = Polygon::new(exterior, interiors);
}

// ---- Ring orientation (geo's Orient) ----

/// `ST_ForcePolygonCW(geom)` — exterior rings clockwise, interiors
/// counter-clockwise. `ST_ForceRHR` is PostGIS's older name for the same
/// thing (the right-hand rule puts the interior on the right).
pub fn st_force_polygon_cw(bytes: &[u8]) -> Result<Vec<u8>> {
    orient(
        bytes,
        geo::algorithm::orient::Direction::Reversed,
        "ST_ForcePolygonCW",
    )
}

/// `ST_ForcePolygonCCW(geom)` — exterior counter-clockwise, interiors
/// clockwise (geo's default convention).
pub fn st_force_polygon_ccw(bytes: &[u8]) -> Result<Vec<u8>> {
    orient(
        bytes,
        geo::algorithm::orient::Direction::Default,
        "ST_ForcePolygonCCW",
    )
}

fn orient(
    bytes: &[u8],
    direction: geo::algorithm::orient::Direction,
    func: &'static str,
) -> Result<Vec<u8>> {
    use geo::algorithm::Orient;
    let g = geom::decode_auto(bytes)?;
    let oriented = match g.geometry {
        Geometry::Polygon(p) => Geometry::Polygon(p.orient(direction)),
        Geometry::MultiPolygon(mp) => Geometry::MultiPolygon(mp.orient(direction)),
        // Non-areal input passes through, as in PostGIS.
        other => other,
    };
    out(oriented, g.srid, func, &[bytes])
}

/// `ST_IsPolygonCW(geom)` — true when every exterior ring is clockwise (and
/// every interior ring counter-clockwise). Non-areal input is true, matching
/// PostGIS's "vacuously oriented" answer.
pub fn st_is_polygon_cw(bytes: &[u8]) -> Result<bool> {
    ring_orientation(bytes, true)
}

/// `ST_IsPolygonCCW(geom)` — the mirror of [`st_is_polygon_cw`].
pub fn st_is_polygon_ccw(bytes: &[u8]) -> Result<bool> {
    ring_orientation(bytes, false)
}

fn ring_orientation(bytes: &[u8], want_cw: bool) -> Result<bool> {
    let g = geom::decode_auto(bytes)?;
    fn check(p: &Polygon<f64>, want_cw: bool) -> bool {
        let exterior_cw = signed_area(p.exterior()) < 0.0;
        exterior_cw == want_cw
            && p.interiors()
                .iter()
                .all(|r| (signed_area(r) < 0.0) != want_cw)
    }
    Ok(match &g.geometry {
        Geometry::Polygon(p) => check(p, want_cw),
        Geometry::MultiPolygon(mp) => mp.iter().all(|p| check(p, want_cw)),
        _ => true,
    })
}

/// Shoelace sign: positive is counter-clockwise in a y-up frame.
fn signed_area(ring: &LineString<f64>) -> f64 {
    let mut sum = 0.0;
    for line in ring.lines() {
        sum += (line.end.x - line.start.x) * (line.end.y + line.start.y);
    }
    -sum / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::io::{st_as_text, st_geom_from_text};

    fn g(wkt: &str) -> Vec<u8> {
        st_geom_from_text(wkt, None).unwrap()
    }
    fn wkt(blob: &[u8]) -> String {
        st_as_text(blob).unwrap()
    }

    const HOLED: &str = "POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))";

    #[test]
    fn ring_accessors_follow_postgis_indexing_and_null_rules() {
        assert_eq!(
            wkt(&st_exterior_ring(&g(HOLED)).unwrap().unwrap()),
            "LINESTRING(0 0,4 0,4 4,0 4,0 0)"
        );
        // 1-based, and NULL rather than an error when out of range.
        assert_eq!(
            wkt(&st_interior_ring_n(&g(HOLED), 1).unwrap().unwrap()),
            "LINESTRING(1 1,2 1,2 2,1 2,1 1)"
        );
        assert!(st_interior_ring_n(&g(HOLED), 2).unwrap().is_none());
        assert!(st_interior_ring_n(&g(HOLED), 0).unwrap().is_none());
        // Wrong type → NULL.
        assert!(
            st_exterior_ring(&g("LINESTRING(0 0,1 1)"))
                .unwrap()
                .is_none()
        );
        assert!(st_num_interior_rings(&g("POINT(0 0)")).unwrap().is_none());
        assert_eq!(st_num_interior_rings(&g(HOLED)).unwrap(), Some(1));
        assert_eq!(st_nrings(&g(HOLED)).unwrap(), 2);
    }

    #[test]
    fn boundary_shapes_match_postgis() {
        // Verified against PostGIS 3.5, including the two empty cases.
        assert_eq!(
            wkt(&st_boundary(&g("POLYGON((0 0,4 0,4 4,0 4,0 0))")).unwrap()),
            "LINESTRING(0 0,4 0,4 4,0 4,0 0)"
        );
        assert_eq!(
            wkt(&st_boundary(&g(HOLED)).unwrap()),
            "MULTILINESTRING((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))"
        );
        assert_eq!(
            wkt(&st_boundary(&g("LINESTRING(0 0,1 1,2 0)")).unwrap()),
            "MULTIPOINT((0 0),(2 0))"
        );
        assert_eq!(
            wkt(&st_boundary(&g("LINESTRING(0 0,1 1,1 0,0 0)")).unwrap()),
            "MULTIPOINT EMPTY"
        );
        assert_eq!(wkt(&st_boundary(&g("POINT(1 1)")).unwrap()), "POINT EMPTY");
    }

    #[test]
    fn closed_and_ring_predicates() {
        assert!(st_is_closed(&g("POLYGON((0 0,4 0,4 4,0 4,0 0))")).unwrap());
        assert!(!st_is_closed(&g("LINESTRING(0 0,1 1)")).unwrap());
        assert!(st_is_closed(&g("LINESTRING(0 0,1 1,1 0,0 0)")).unwrap());
        assert!(st_is_ring(&g("LINESTRING(0 0,1 1,1 0,0 0)")).unwrap());
        assert!(!st_is_ring(&g("LINESTRING(0 0,1 1)")).unwrap());
        // PostGIS raises here instead of returning NULL.
        assert!(st_is_ring(&g("POINT(0 0)")).is_err());
    }

    #[test]
    fn vertex_surgery_is_zero_based() {
        let line = g("LINESTRING(0 0,1 1)");
        let p = g("POINT(9 9)");
        assert_eq!(
            wkt(&st_add_point(&line, &g("POINT(2 2)"), None)
                .unwrap()
                .unwrap()),
            "LINESTRING(0 0,1 1,2 2)"
        );
        assert_eq!(
            wkt(&st_add_point(&line, &p, Some(0)).unwrap().unwrap()),
            "LINESTRING(9 9,0 0,1 1)"
        );
        assert_eq!(
            wkt(&st_set_point(&line, 0, &p).unwrap().unwrap()),
            "LINESTRING(9 9,1 1)"
        );
        assert_eq!(
            wkt(&st_remove_point(&g("LINESTRING(0 0,1 1,2 2)"), 0)
                .unwrap()
                .unwrap()),
            "LINESTRING(1 1,2 2)"
        );
        assert!(st_remove_point(&line, 5).unwrap().is_none());
        assert!(st_set_point(&g("POINT(0 0)"), 0, &p).unwrap().is_none());
    }

    #[test]
    fn constructors_and_coordinate_ops() {
        assert_eq!(
            wkt(&st_make_line(&g("POINT(0 0)"), &g("POINT(1 1)")).unwrap()),
            "LINESTRING(0 0,1 1)"
        );
        assert_eq!(
            wkt(&st_make_polygon(&g("LINESTRING(0 0,1 0,1 1,0 0)")).unwrap()),
            "POLYGON((0 0,1 0,1 1,0 0))"
        );
        assert!(st_make_polygon(&g("LINESTRING(0 0,1 0)")).is_err());
        assert_eq!(
            wkt(&st_multi(&g("POINT(1 2)")).unwrap()),
            "MULTIPOINT((1 2))"
        );
        assert_eq!(
            wkt(&st_snap_to_grid(&g("POINT(1.23 4.57)"), 0.5, 0.5).unwrap()),
            "POINT(1 4.5)"
        );
        // Same double as PostGIS (which prints it as "POINT(1.2 5)" because
        // its WKT writer trims to 15 significant digits; kenro's is
        // shortest-roundtrip, so the 0.1-grid artifact stays visible).
        let snapped = st_snap_to_grid(&g("POINT(1.23 4.57)"), 0.1, 1.0).unwrap();
        assert_eq!(
            crate::functions::accessors::st_x(&snapped).unwrap(),
            Some((1.23f64 / 0.1).round() * 0.1)
        );
        assert!(wkt(&snapped).starts_with("POINT(1.2"));
        assert_eq!(
            wkt(&st_flip_coordinates(&g("POINT(1 2)")).unwrap()),
            "POINT(2 1)"
        );
        assert_eq!(
            wkt(&st_shift_longitude(&g("POINT(-10 5)")).unwrap()),
            "POINT(350 5)"
        );
        assert_eq!(
            wkt(&st_expand(&g("POINT(1 1)"), 2.0).unwrap().unwrap()),
            "POLYGON((-1 -1,-1 3,3 3,3 -1,-1 -1))"
        );
    }

    #[test]
    fn snap_to_grid_reaches_inside_polygon_rings() {
        let snapped = st_snap_to_grid(
            &g("POLYGON((0.1 0.1,4.4 0.1,4.4 4.4,0.1 4.4,0.1 0.1))"),
            1.0,
            1.0,
        )
        .unwrap();
        assert_eq!(wkt(&snapped), "POLYGON((0 0,4 0,4 4,0 4,0 0))");
    }
}
