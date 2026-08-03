//! Grid generators: `ST_SquareGrid` and `ST_HexagonGrid`.
//!
//! These were listed under "Set-returning functions" as out of scope because
//! PostGIS's return `SETOF record` and kenro registers no table-valued
//! functions. What that reasoning missed is that kenro had **already made the
//! same accommodation under the same name**: `ST_Subdivide` is `SETOF
//! geometry` in PostGIS and a MULTI\* here. Grids are the same trade.
//!
//! SpatiaLite corroborates it independently — its `ST_SquareGrid`,
//! `ST_HexagonalGrid` and `ST_TriangularGrid` are scalars returning a
//! MULTIPOLYGON by default, so a scalar grid is not an invention.
//!
//! ⚠️ **The argument order is PostGIS's, which is the reverse of
//! SpatiaLite's**: `ST_SquareGrid(size, bounds)` here and in PostGIS,
//! `ST_SquareGrid(geom, size)` in SpatiaLite. Pasted SpatiaLite SQL fails on
//! the argument type rather than silently gridding something else, which is
//! the only reason this is safe to do.
//!
//! Everything about the cell layout below was measured against PostGIS 3.5.

use geo_types::{Geometry, LineString, MultiPolygon, Polygon, coord};

use crate::error::{Error, Result};
use crate::geom::{self, Geom};

/// kenro materialises the whole grid into one MULTIPOLYGON where PostGIS
/// streams rows, so an over-large request has to be refused rather than
/// swallowing memory. A cell budget is the honest place to draw that line.
const MAX_CELLS: i64 = 100_000;

/// Does a cell spanning `lo..hi` on one axis belong in the grid?
///
/// PostGIS's rule is **asymmetric**, which is the single most surprising thing
/// measured here: a cell whose low edge sits exactly on the bounds' maximum is
/// kept, and a cell whose high edge sits exactly on the bounds' minimum is
/// dropped. `ST_SquareGrid(1, ST_MakeEnvelope(0,0,3,2))` returns the column
/// starting at `maxx = 3`; `ST_HexagonGrid(1, ST_MakeEnvelope(0,0,3,3))` drops
/// the staggered cell whose top edge is exactly `miny = 0`. Treating either
/// end as a plain intersection gets one of those two wrong.
fn cell_in(lo: f64, hi: f64, min: f64, max: f64) -> bool {
    lo <= max && hi > min
}

/// The square cell index range along one axis.
///
/// Cell `i` spans `[i·size, (i+1)·size]`, so [`cell_in`] reduces to
/// `floor(min/size) ..= floor(max/size)` — including for negative coordinates
/// (`-1.5 → -2`). Verified against the same measurements.
fn cell_range(min: f64, max: f64, size: f64) -> (i64, i64) {
    ((min / size).floor() as i64, (max / size).floor() as i64)
}

fn bounds_of(bounds: &[u8], func: &'static str) -> Result<Option<(Geom, [f64; 4])>> {
    use geo::BoundingRect;
    let g = geom::decode_auto(bounds)?;
    // A non-rectangular bounds argument is used by its envelope, as in
    // PostGIS — a triangle over 0..3 grids the same 16 cells as the box does.
    let Some(r) = g.geometry.bounding_rect() else {
        return Ok(None);
    };
    if ![r.min().x, r.min().y, r.max().x, r.max().y]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(Error::Unsupported {
            func,
            reason: "the bounds have a non-finite coordinate".into(),
        });
    }
    Ok(Some((g, [r.min().x, r.min().y, r.max().x, r.max().y])))
}

/// Named `out_2d`, not `out`: everywhere else in the tree `out` means "restore
/// the Z from the inputs" and `out_2d` means "2D on purpose". A grid is
/// generated rather than derived — there are no input vertices to take a height
/// from — and PostGIS's grids are 2D too (measured on 3.5: `ST_SquareGrid` and
/// `ST_HexagonGrid` over a `POLYGON Z` both answer `ST_NDims = 2`).
fn out_2d(cells: Vec<Polygon<f64>>, srid: i32, func: &'static str) -> Result<Vec<u8>> {
    geom::encode_canonical_gpb(
        &Geom {
            geometry: Geometry::MultiPolygon(MultiPolygon::new(cells)),
            srid,
            has_zm: false,
        },
        func,
    )
}

fn too_many(func: &'static str, wanted: i64) -> Error {
    Error::Unsupported {
        func,
        reason: format!(
            "that is {wanted} cells, over kenro's {MAX_CELLS} limit; PostGIS streams grid rows \
             but kenro returns one MULTIPOLYGON, so enlarge the size or shrink the bounds"
        ),
    }
}

/// `ST_SquareGrid(size, bounds)` — a square tiling covering `bounds`.
///
/// The grid is **anchored at the origin**, not at the bounds: cell `(i, j)`
/// is always `[i·size, (i+1)·size] × [j·size, (j+1)·size]`, so two calls with
/// different bounds produce cells that line up. Measured, and the reason a
/// bounds of `0.5 0.5 → 1.6 1.4` returns the four cells covering `0..2 × 0..2`
/// rather than four cells starting at `0.5`.
///
/// ⚠️ **Divergences.** PostGIS returns one row per cell with `i`/`j` columns;
/// kenro returns a **MULTIPOLYGON**, as `ST_Subdivide` already does, and the
/// indices are not carried — recover them from a cell's own corner
/// (`ST_MinX(cell) / size`) if they matter. The cell *order* is PostGIS's
/// (i-major, then j). A `size` of zero or less yields an empty result rather
/// than an error, matching PostGIS's zero rows. Over `MAX_CELLS` cells is an
/// error, because kenro materialises what PostGIS streams.
pub fn st_square_grid(size: f64, bounds: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_SquareGrid";
    let Some((g, [minx, miny, maxx, maxy])) = bounds_of(bounds, FUNC)? else {
        return out_2d(vec![], geom::decode_auto(bounds)?.srid, FUNC);
    };
    // NaN is caught by is_finite, so the comparison never sees it.
    if !size.is_finite() || size <= 0.0 {
        // PostGIS answers zero rows for size <= 0, not an error.
        return out_2d(vec![], g.srid, FUNC);
    }
    let (i0, i1) = cell_range(minx, maxx, size);
    let (j0, j1) = cell_range(miny, maxy, size);
    let count = (i1 - i0 + 1).saturating_mul(j1 - j0 + 1);
    if count > MAX_CELLS {
        return Err(too_many(FUNC, count));
    }
    let mut cells = Vec::with_capacity(count.max(0) as usize);
    for i in i0..=i1 {
        for j in j0..=j1 {
            let (x0, y0) = (i as f64 * size, j as f64 * size);
            let (x1, y1) = (x0 + size, y0 + size);
            // PostGIS's own vertex order: lower-left, up, right, down, close.
            cells.push(Polygon::new(
                LineString::new(vec![
                    coord! { x: x0, y: y0 },
                    coord! { x: x0, y: y1 },
                    coord! { x: x1, y: y1 },
                    coord! { x: x1, y: y0 },
                    coord! { x: x0, y: y0 },
                ]),
                vec![],
            ));
        }
    }
    out_2d(cells, g.srid, FUNC)
}

/// `ST_HexagonGrid(size, bounds)` — a hexagonal tiling covering `bounds`.
///
/// `size` is the **circumradius**: cell `(0, 0)` is centred on the origin with
/// vertices at `(±size, 0)`, so it is flat-topped and `2·size` wide by
/// `√3·size` tall. Centres step `1.5·size` in x, `√3·size` in y, and odd
/// columns are staggered up by `√3·size/2` — all read off PostGIS 3.5 rather
/// than derived, because the alternative conventions (pointy-top, `size` as
/// the inradius or the width) are all equally plausible and all wrong here.
///
/// A cell is emitted when its bounding box intersects the bounds, which is why
/// the column count and the row count per column differ by parity.
///
/// ⚠️ Same divergences as `ST_SquareGrid`: a MULTIPOLYGON rather than rows
/// with `i`/`j`, and a cell budget. SpatiaLite spells this
/// `ST_HexagonalGrid`, and its hexagons are laid out differently again;
/// kenro follows PostGIS.
pub fn st_hexagon_grid(size: f64, bounds: &[u8]) -> Result<Vec<u8>> {
    const FUNC: &str = "ST_HexagonGrid";
    let Some((g, [minx, miny, maxx, maxy])) = bounds_of(bounds, FUNC)? else {
        return out_2d(vec![], geom::decode_auto(bounds)?.srid, FUNC);
    };
    if !size.is_finite() || size <= 0.0 {
        return out_2d(vec![], g.srid, FUNC);
    }
    let half_h = size * 3.0_f64.sqrt() / 2.0; // flat-to-flat, halved
    let row = half_h * 2.0; // √3·size

    // Enumerate one column and one row wider than needed, then let `cell_in`
    // decide — the inequality is asymmetric enough (see its doc comment) that
    // closed-form bounds would be all edge case and no clarity.
    let i0 = ((minx - size) / (1.5 * size)).floor() as i64 - 1;
    let i1 = ((maxx + size) / (1.5 * size)).ceil() as i64 + 1;

    let mut cells = Vec::new();
    for i in i0..=i1 {
        let cx = 1.5 * size * i as f64;
        if !cell_in(cx - size, cx + size, minx, maxx) {
            continue;
        }
        let stagger = if i.rem_euclid(2) == 1 { half_h } else { 0.0 };
        let j0 = ((miny - stagger - half_h) / row).floor() as i64 - 1;
        let j1 = ((maxy - stagger + half_h) / row).ceil() as i64 + 1;
        for j in j0..=j1 {
            let cy = row * j as f64 + stagger;
            if !cell_in(cy - half_h, cy + half_h, miny, maxy) {
                continue;
            }
            if cells.len() as i64 >= MAX_CELLS {
                return Err(too_many(FUNC, MAX_CELLS + 1));
            }
            cells.push(Polygon::new(
                LineString::new(vec![
                    coord! { x: cx - size,       y: cy },
                    coord! { x: cx - size / 2.0, y: cy - half_h },
                    coord! { x: cx + size / 2.0, y: cy - half_h },
                    coord! { x: cx + size,       y: cy },
                    coord! { x: cx + size / 2.0, y: cy + half_h },
                    coord! { x: cx - size / 2.0, y: cy + half_h },
                    coord! { x: cx - size,       y: cy },
                ]),
                vec![],
            ));
        }
    }
    out_2d(cells, g.srid, FUNC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::accessors::st_num_geometries;
    use crate::functions::io::{st_as_text, st_geom_from_text, st_set_srid, st_srid};

    fn env(minx: f64, miny: f64, maxx: f64, maxy: f64) -> Vec<u8> {
        st_geom_from_text(
            &format!(
                "POLYGON(({minx} {miny},{maxx} {miny},{maxx} {maxy},{minx} {maxy},{minx} {miny}))"
            ),
            None,
        )
        .unwrap()
    }

    /// Every number here was read off PostGIS 3.5.
    #[test]
    fn the_square_grid_is_anchored_at_the_origin_like_postgis() {
        // 0..3 × 0..2 at size 1 gives 12 cells spanning 0..4 × 0..3 — the
        // column starting exactly at maxx = 3 is included, because touching
        // counts as intersecting.
        let grid = st_square_grid(1.0, &env(0.0, 0.0, 3.0, 2.0)).unwrap();
        assert_eq!(st_num_geometries(&grid).unwrap(), 12);

        // The first two cells, verbatim, including PostGIS's vertex order.
        let small = st_as_text(&st_square_grid(1.0, &env(0.0, 0.0, 2.0, 1.0)).unwrap()).unwrap();
        assert!(
            small.starts_with("MULTIPOLYGON(((0 0,0 1,1 1,1 0,0 0)),((0 1,0 2,1 2,1 1,0 1))"),
            "{small}"
        );
        // …which also pins the cell order: i-major, then j.
        assert_eq!(
            st_num_geometries(&st_square_grid(1.0, &env(0.0, 0.0, 2.0, 1.0)).unwrap()).unwrap(),
            6
        );

        // Anchored at the origin, not the bounds: an offset window still
        // returns the cells of the global grid it overlaps.
        let offset = st_as_text(&st_square_grid(1.0, &env(0.5, 0.5, 1.6, 1.4)).unwrap()).unwrap();
        assert_eq!(
            offset,
            "MULTIPOLYGON(((0 0,0 1,1 1,1 0,0 0)),((0 1,0 2,1 2,1 1,0 1)),\
             ((1 0,1 1,2 1,2 0,1 0)),((1 1,1 2,2 2,2 1,1 1)))"
        );
        // Negative coordinates floor the way PostGIS's do (-1.5 → -2).
        let neg = st_as_text(&st_square_grid(1.0, &env(-1.5, -1.5, -0.4, -0.4)).unwrap()).unwrap();
        assert!(
            neg.starts_with("MULTIPOLYGON(((-2 -2,-2 -1,-1 -1,-1 -2,-2 -2))"),
            "{neg}"
        );
        assert_eq!(
            st_num_geometries(&st_square_grid(1.0, &env(-1.5, -1.5, -0.4, -0.4)).unwrap()).unwrap(),
            4
        );
    }

    #[test]
    fn the_hexagon_layout_is_postgis_s() {
        // Cell (0,0) verbatim: circumradius 1, centred on the origin, flat
        // top and bottom.
        let one = st_as_text(&st_hexagon_grid(1.0, &env(0.0, 0.0, 0.1, 0.1)).unwrap()).unwrap();
        assert_eq!(
            one,
            "MULTIPOLYGON(((-1 0,-0.5 -0.8660254037844386,0.5 -0.8660254037844386,1 0,\
             0.5 0.8660254037844386,-0.5 0.8660254037844386,-1 0)))"
        );
        // 0..3 × 0..3 gives 8 cells: i = 0..2, with j = 0..2 on even columns
        // and 0..1 on the odd one, because the stagger moves the odd column
        // out of reach at the top.
        assert_eq!(
            st_num_geometries(&st_hexagon_grid(1.0, &env(0.0, 0.0, 3.0, 3.0)).unwrap()).unwrap(),
            8
        );
        // The small cases that pin the parity rule.
        assert_eq!(
            st_num_geometries(&st_hexagon_grid(1.0, &env(0.0, 0.0, 1.0, 1.0)).unwrap()).unwrap(),
            3
        );
        assert_eq!(
            st_num_geometries(&st_hexagon_grid(1.0, &env(0.0, 0.0, 6.0, 2.0)).unwrap()).unwrap(),
            10
        );
    }

    #[test]
    fn the_edges_behave_as_postgis_does() {
        // size <= 0 is zero rows there, so an empty multi here — not an error.
        assert_eq!(
            st_as_text(&st_square_grid(0.0, &env(0.0, 0.0, 2.0, 2.0)).unwrap()).unwrap(),
            "MULTIPOLYGON EMPTY"
        );
        assert_eq!(
            st_as_text(&st_hexagon_grid(-1.0, &env(0.0, 0.0, 2.0, 2.0)).unwrap()).unwrap(),
            "MULTIPOLYGON EMPTY"
        );
        // A non-rectangular bounds is used by its envelope: a triangle over
        // 0..3 grids the same 16 cells the box does.
        let tri = st_geom_from_text("POLYGON((0 0,3 0,0 3,0 0))", None).unwrap();
        assert_eq!(
            st_num_geometries(&st_square_grid(1.0, &tri).unwrap()).unwrap(),
            16
        );
        // A point bounds still lands in one cell.
        let pt = st_geom_from_text("POINT(0.5 0.5)", None).unwrap();
        assert_eq!(
            st_num_geometries(&st_square_grid(1.0, &pt).unwrap()).unwrap(),
            1
        );
        // The SRID comes from the bounds.
        let labelled = st_set_srid(&env(0.0, 0.0, 2.0, 2.0), 4326).unwrap();
        assert_eq!(
            st_srid(&st_square_grid(1.0, &labelled).unwrap()).unwrap(),
            4326
        );
        // The cell budget is a loud error, since PostGIS would have streamed.
        let err = st_square_grid(1.0, &env(0.0, 0.0, 1000.0, 1000.0))
            .unwrap_err()
            .to_string();
        assert!(err.contains("cells"), "{err}");
    }
}
