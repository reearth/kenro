//! Surface collections — POLYHEDRALSURFACE, TIN, TRIANGLE — read through
//! their encoding rather than decoded into a value.
//!
//! `geo_types` has no variant for a surface collection, and giving kenro a
//! second geometry model would mean two representations that can disagree
//! (see `tmp/3d-geometry-design.md`). So these functions walk the WKB: they
//! answer structural questions, measure patch by patch, and hand the whole
//! thing to the 2D world through `ST_Force2D`.
//!
//! Everything else — every predicate, every overlay — refuses surface input
//! at a single guard in `geom::wkb_to_geo`, with a message naming
//! `ST_Force2D`. Silently flattening a building into overlapping faces would
//! be the same class of mistake as writing 2D where 3D went in.
//!
//! The encoding, measured from PostGIS 3.5: type codes 15/16/17 with the ISO
//! `+1000` Z convention, and **each patch is a complete nested WKB
//! geometry**, header and all.

use geo_types::{Coord, Geometry, LineString, MultiPolygon, Polygon};

use crate::error::{Error, Result};
use crate::geom::{self, Geom, SurfaceKind};
use crate::gpb::{self, GpbHeader};

/// A surface collection, borrowed from its blob.
pub struct Surfaces<'a> {
    kind: SurfaceKind,
    srid: i32,
    /// Byte ranges of the patches, each a complete nested WKB geometry.
    patches: Vec<&'a [u8]>,
}

impl Surfaces<'_> {
    pub fn kind(&self) -> SurfaceKind {
        self.kind
    }

    pub fn srid(&self) -> i32 {
        self.srid
    }

    pub fn len(&self) -> usize {
        self.patches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    /// Patch `n` (0-based) as a 2D polygon — Z dropped, as everywhere else.
    pub fn patch(&self, n: usize) -> Result<Option<Polygon<f64>>> {
        let Some(bytes) = self.patches.get(n) else {
            return Ok(None);
        };
        Ok(Some(patch_polygon(bytes)?))
    }

    /// Patch `n` with its Z ordinates, for area and closure work.
    pub fn patch_3d(&self, n: usize) -> Result<Option<Vec<[f64; 3]>>> {
        let Some(bytes) = self.patches.get(n) else {
            return Ok(None);
        };
        Ok(Some(patch_ring_3d(bytes)?))
    }

    fn polygons(&self) -> Result<Vec<Polygon<f64>>> {
        self.patches.iter().map(|b| patch_polygon(b)).collect()
    }
}

/// Read a blob as a surface collection, or `None` when it is not one.
pub fn surfaces(bytes: &[u8]) -> Result<Option<Surfaces<'_>>> {
    let Some(kind) = geom::surface_kind(bytes) else {
        return Ok(None);
    };
    let (wkb, srid) = if gpb::is_gpb(bytes) {
        let header = GpbHeader::parse(bytes)?;
        (&bytes[header.wkb_offset..], header.srid)
    } else {
        (bytes, 0)
    };
    let mut cursor = Cursor::new(wkb)?;
    let patches = match kind {
        // A Triangle is its own single patch.
        SurfaceKind::Triangle => vec![wkb],
        _ => {
            let count = cursor.u32()? as usize;
            let mut out = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                out.push(cursor.nested_geometry()?);
            }
            out
        }
    };
    Ok(Some(Surfaces {
        kind,
        srid,
        patches,
    }))
}

/// A little-or-big-endian reader positioned just past a WKB header.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
    le: bool,
    dims: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < 5 {
            return Err(short());
        }
        let le = match bytes[0] {
            0 => false,
            1 => true,
            b => {
                return Err(Error::InvalidWkb(format!(
                    "invalid byte-order marker {b:#04x}"
                )));
            }
        };
        let raw: [u8; 4] = bytes[1..5].try_into().expect("length checked");
        let ty = if le {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        };
        let mut pos = 5;
        if ty & 0x2000_0000 != 0 {
            pos += 4; // EWKB SRID
        }
        let dims = match (ty & 0x0000_FFFF) / 1000 {
            1 | 2 => 3,
            3 => 4,
            _ => 2 + usize::from(ty & 0x8000_0000 != 0) + usize::from(ty & 0x4000_0000 != 0),
        };
        Ok(Cursor {
            bytes,
            pos,
            le,
            dims,
        })
    }

    fn u32(&mut self) -> Result<u32> {
        let end = self.pos.checked_add(4).ok_or_else(short)?;
        let raw: [u8; 4] = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(short)?
            .try_into()
            .map_err(|_| short())?;
        self.pos = end;
        Ok(if self.le {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        })
    }

    /// The next complete nested WKB geometry, returned as a slice.
    fn nested_geometry(&mut self) -> Result<&'a [u8]> {
        let start = self.pos;
        let mut inner = Cursor::new(self.bytes.get(start..).ok_or_else(short)?)?;
        inner.skip_rings()?;
        let len = inner.pos;
        self.pos = start.checked_add(len).ok_or_else(short)?;
        self.bytes.get(start..self.pos).ok_or_else(short)
    }

    /// Skip a Polygon/Triangle body (ring count, then point counts).
    fn skip_rings(&mut self) -> Result<()> {
        let rings = self.u32()? as usize;
        for _ in 0..rings {
            let points = self.u32()? as usize;
            let bytes = points
                .checked_mul(8 * self.dims)
                .ok_or_else(|| Error::InvalidWkb("count overflow".into()))?;
            self.pos = self.pos.checked_add(bytes).ok_or_else(short)?;
            if self.pos > self.bytes.len() {
                return Err(short());
            }
        }
        Ok(())
    }

    fn f64(&mut self) -> Result<f64> {
        let end = self.pos.checked_add(8).ok_or_else(short)?;
        let raw: [u8; 8] = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(short)?
            .try_into()
            .map_err(|_| short())?;
        self.pos = end;
        Ok(if self.le {
            f64::from_le_bytes(raw)
        } else {
            f64::from_be_bytes(raw)
        })
    }
}

fn short() -> Error {
    Error::InvalidWkb("truncated inside a surface patch".into())
}

/// One patch → a 2D polygon.
fn patch_polygon(bytes: &[u8]) -> Result<Polygon<f64>> {
    let mut c = Cursor::new(bytes)?;
    let rings = c.u32()? as usize;
    let mut collected: Vec<LineString<f64>> = Vec::with_capacity(rings);
    for _ in 0..rings {
        let points = c.u32()? as usize;
        let mut ring = Vec::with_capacity(points.min(4096));
        for _ in 0..points {
            let x = c.f64()?;
            let y = c.f64()?;
            for _ in 2..c.dims {
                c.f64()?; // Z/M, dropped
            }
            ring.push(Coord { x, y });
        }
        collected.push(LineString::new(ring));
    }
    if collected.is_empty() {
        return Ok(Polygon::new(LineString::new(vec![]), vec![]));
    }
    let exterior = collected.remove(0);
    Ok(Polygon::new(exterior, collected))
}

/// One patch's exterior ring, Z included.
fn patch_ring_3d(bytes: &[u8]) -> Result<Vec<[f64; 3]>> {
    let mut c = Cursor::new(bytes)?;
    let rings = c.u32()? as usize;
    if rings == 0 {
        return Ok(Vec::new());
    }
    let points = c.u32()? as usize;
    let mut out = Vec::with_capacity(points.min(4096));
    for _ in 0..points {
        let x = c.f64()?;
        let y = c.f64()?;
        let z = if c.dims > 2 { c.f64()? } else { 0.0 };
        for _ in 3..c.dims {
            c.f64()?;
        }
        out.push([x, y, z]);
    }
    Ok(out)
}

// ---- SQL functions ----

/// `ST_NumPatches(geom)` — patch count, NULL for anything else (PostGIS).
pub fn st_num_patches(bytes: &[u8]) -> Result<Option<i64>> {
    Ok(surfaces(bytes)?.map(|s| s.len() as i64))
}

/// `ST_PatchN(geom, n)` — patch `n` as a POLYGON, **1-based** like
/// `ST_GeometryN`. NULL when out of range or not a surface.
pub fn st_patch_n(bytes: &[u8], n: i64) -> Result<Option<Vec<u8>>> {
    let Some(s) = surfaces(bytes)? else {
        return Ok(None);
    };
    if n < 1 {
        return Ok(None);
    }
    let Some(poly) = s.patch((n - 1) as usize)? else {
        return Ok(None);
    };
    encode(Geometry::Polygon(poly), s.srid(), "ST_PatchN").map(Some)
}

/// `ST_Force2D` for surfaces — the bridge into every 2D function.
///
/// ⚠️ A closed solid becomes a MULTIPOLYGON of overlapping coplanar faces.
/// Geometrically correct, visually surprising, and what PostGIS does.
pub fn force_2d(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let Some(s) = surfaces(bytes)? else {
        return Ok(None);
    };
    encode(
        Geometry::MultiPolygon(MultiPolygon::new(s.polygons()?)),
        s.srid(),
        "ST_Force2D",
    )
    .map(Some)
}

/// `ST_Area` / `ST_Perimeter` for surfaces: summed patch by patch, which is
/// what PostGIS reports (a planar sum, not a 3D surface area).
pub fn area(bytes: &[u8]) -> Result<Option<f64>> {
    use geo::algorithm::Area;
    let Some(s) = surfaces(bytes)? else {
        return Ok(None);
    };
    Ok(Some(s.polygons()?.iter().map(|p| p.unsigned_area()).sum()))
}

pub fn perimeter(bytes: &[u8]) -> Result<Option<f64>> {
    use geo::algorithm::line_measures::{Euclidean, Length};
    let Some(s) = surfaces(bytes)? else {
        return Ok(None);
    };
    Ok(Some(
        s.polygons()?
            .iter()
            .map(|p| {
                Euclidean.length(p.exterior())
                    + p.interiors()
                        .iter()
                        .map(|r| Euclidean.length(r))
                        .sum::<f64>()
            })
            .sum(),
    ))
}

/// `ST_IsClosed` for surfaces — is this a closed shell?
///
/// The test is combinatorial, not geometric: in a closed polyhedron every
/// edge is shared by exactly two faces. Runs on the 3D coordinates, so a
/// cube is closed and a cube missing a face is not.
pub fn is_closed(bytes: &[u8]) -> Result<Option<bool>> {
    let Some(s) = surfaces(bytes)? else {
        return Ok(None);
    };
    if s.is_empty() {
        return Ok(Some(false));
    }
    let mut edges: std::collections::HashMap<([u64; 3], [u64; 3]), usize> =
        std::collections::HashMap::new();
    for i in 0..s.len() {
        let Some(ring) = s.patch_3d(i)? else { continue };
        if ring.len() < 2 {
            return Ok(Some(false));
        }
        // The ring repeats its first vertex; walk the segments once.
        for pair in ring.windows(2) {
            let (a, b) = (key(pair[0]), key(pair[1]));
            if a == b {
                continue; // degenerate
            }
            // Undirected: the two orientations are the same edge.
            let edge = if a <= b { (a, b) } else { (b, a) };
            *edges.entry(edge).or_insert(0) += 1;
        }
    }
    Ok(Some(!edges.is_empty() && edges.values().all(|n| *n == 2)))
}

/// Bit patterns, so -0.0 and 0.0 hash alike and NaN never matches.
fn key(c: [f64; 3]) -> [u64; 3] {
    [
        (c[0] + 0.0).to_bits(),
        (c[1] + 0.0).to_bits(),
        (c[2] + 0.0).to_bits(),
    ]
}

/// The 2D bounding box of every patch, for the R-tree accessors.
pub fn envelope(bytes: &[u8]) -> Result<Option<(f64, f64, f64, f64)>> {
    let Some(s) = surfaces(bytes)? else {
        return Ok(None);
    };
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for poly in s.polygons()? {
        for c in poly.exterior() {
            bounds = Some(match bounds {
                None => (c.x, c.y, c.x, c.y),
                Some((minx, miny, maxx, maxy)) => {
                    (minx.min(c.x), miny.min(c.y), maxx.max(c.x), maxy.max(c.y))
                }
            });
        }
    }
    Ok(bounds)
}

/// The Z extent across every patch, for `ST_ZMin`/`ST_ZMax`.
pub fn z_extent(bytes: &[u8]) -> Result<Option<(f64, f64)>> {
    let Some(s) = surfaces(bytes)? else {
        return Ok(None);
    };
    let mut bounds: Option<(f64, f64)> = None;
    for i in 0..s.len() {
        let Some(ring) = s.patch_3d(i)? else { continue };
        for c in ring {
            bounds = Some(match bounds {
                None => (c[2], c[2]),
                Some((lo, hi)) => (lo.min(c[2]), hi.max(c[2])),
            });
        }
    }
    Ok(bounds)
}

/// `kenro_gpkg_extension_required(geom)` — the `gpkg_extensions` row a
/// GeoPackage needs before it may store this value, or NULL when none is.
///
/// GeoPackage Annex F.1 makes an extended geometry type legal only if the
/// file declares it: one row per (table, column) with `extension_name`
/// `gpkg_geom_<TYPE>`, `definition`
/// `http://www.geopackage.org/spec120/#extension_geometry_types` and `scope`
/// `read-write`. kenro does not write that row — it registers functions, it
/// does not manage schemas, the same reason `InitSpatialMetadata` is out of
/// scope — but silence would leave the requirement as folklore, so this
/// names it.
///
/// Deliberately **not** called `GPKG_*`: the spec reserves the `gpkg` author
/// prefix for OGC-adopted extension *names*, and while it says nothing about
/// SQL function names, a `GPKG_` function would read as one the standard
/// defines. This one is kenro's.
pub fn extension_required(bytes: &[u8]) -> Result<Option<String>> {
    Ok(geom::surface_kind(bytes).map(|kind| format!("gpkg_geom_{}", kind.name())))
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

#[cfg(test)]
pub(crate) mod fixtures {
    /// `POLYHEDRALSURFACE Z(((0 0 0,0 1 0,1 1 0,1 0 0,0 0 0)))` as PostGIS
    /// 3.5 encodes it — the bytes in the design note.
    pub fn unit_square_z() -> Vec<u8> {
        // Verbatim from `SELECT encode(ST_AsBinary(…),'hex')` on PostGIS 3.5.
        hex(
            "01f70300000100000001eb03000001000000050000000000000000000000000000000000000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f0000000000000000000000000000f03f00000000000000000000000000000000000000000000000000000000000000000000000000000000",
        )
    }

    /// A closed unit cube, six quads, all Z-bearing.
    pub fn cube(faces: usize) -> Vec<u8> {
        let quads: [[[f64; 3]; 4]; 6] = [
            [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]], // bottom
            [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]], // top
            [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
            [[1., 1., 0.], [0., 1., 0.], [0., 1., 1.], [1., 1., 1.]],
            [[0., 1., 0.], [0., 0., 0.], [0., 0., 1.], [0., 1., 1.]],
        ];
        let mut out = vec![0x01];
        out.extend_from_slice(&1015u32.to_le_bytes());
        out.extend_from_slice(&(faces as u32).to_le_bytes());
        for quad in quads.iter().take(faces) {
            out.push(0x01);
            out.extend_from_slice(&1003u32.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&5u32.to_le_bytes());
            for v in quad.iter().chain(std::iter::once(&quad[0])) {
                for ordinate in v {
                    out.extend_from_slice(&ordinate.to_le_bytes());
                }
            }
        }
        out
    }

    fn hex(s: &str) -> Vec<u8> {
        let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        (0..clean.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).unwrap())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{cube, unit_square_z};
    use super::*;
    use crate::functions::io::st_as_text;

    #[test]
    fn reads_the_bytes_postgis_writes() {
        let blob = unit_square_z();
        let s = surfaces(&blob).unwrap().unwrap();
        assert_eq!(s.kind(), SurfaceKind::PolyhedralSurface);
        // PostGIS 3.5: ST_NumPatches = 1, ST_Area = 1.
        assert_eq!(s.len(), 1);
        assert_eq!(st_num_patches(&unit_square_z()).unwrap(), Some(1));
        assert!((area(&unit_square_z()).unwrap().unwrap() - 1.0).abs() < 1e-12);
        assert!((perimeter(&unit_square_z()).unwrap().unwrap() - 4.0).abs() < 1e-12);
    }

    #[test]
    fn patches_come_out_as_2d_polygons() {
        let patch = st_patch_n(&unit_square_z(), 1).unwrap().unwrap();
        assert_eq!(
            st_as_text(&patch).unwrap(),
            "POLYGON((0 0,0 1,1 1,1 0,0 0))"
        );
        // 1-based, like ST_GeometryN; out of range is NULL.
        assert!(st_patch_n(&unit_square_z(), 0).unwrap().is_none());
        assert!(st_patch_n(&unit_square_z(), 2).unwrap().is_none());
        // Not a surface at all.
        let point = crate::functions::io::st_geom_from_text("POINT(1 2)", None).unwrap();
        assert!(st_num_patches(&point).unwrap().is_none());
        assert!(st_patch_n(&point, 1).unwrap().is_none());
    }

    #[test]
    fn force_2d_is_the_bridge_to_every_2d_function() {
        let flat = force_2d(&cube(6)).unwrap().unwrap();
        // Six faces, and the overlapping-coplanar-faces caveat is visible:
        // the flattened cube is six unit squares stacked in 2D.
        assert_eq!(
            crate::functions::accessors::st_num_geometries(&flat).unwrap(),
            6
        );
        // From here, ordinary 2D work resumes.
        let window =
            crate::functions::io::st_geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None)
                .unwrap();
        assert!(crate::functions::predicates::st_intersects(&flat, &window).unwrap());
    }

    #[test]
    fn is_closed_counts_edges_not_geometry() {
        // A cube: every edge shared by exactly two faces.
        assert_eq!(is_closed(&cube(6)).unwrap(), Some(true));
        // Take one face away and the shell is open.
        assert_eq!(is_closed(&cube(5)).unwrap(), Some(false));
        // A single square is not a shell either.
        assert_eq!(is_closed(&unit_square_z()).unwrap(), Some(false));
    }

    #[test]
    fn extents_cover_every_patch() {
        assert_eq!(envelope(&cube(6)).unwrap(), Some((0.0, 0.0, 1.0, 1.0)));
        assert_eq!(z_extent(&cube(6)).unwrap(), Some((0.0, 1.0)));
        assert_eq!(z_extent(&unit_square_z()).unwrap(), Some((0.0, 0.0)));
    }

    #[test]
    fn the_shared_guard_names_the_way_out() {
        // Anything that needs a geo_types value stops here, once.
        let err = crate::geom::decode_auto(&cube(6)).unwrap_err().to_string();
        assert!(err.contains("POLYHEDRALSURFACE"), "{err}");
        assert!(err.contains("ST_Force2D"), "{err}");
    }

    #[test]
    fn the_geopackage_obligation_is_named_not_hidden() {
        assert_eq!(
            extension_required(&cube(6)).unwrap().as_deref(),
            Some("gpkg_geom_POLYHEDRALSURFACE")
        );
        // An ordinary geometry needs no extension row.
        let point = crate::functions::io::st_geom_from_text("POINT(1 2)", None).unwrap();
        assert_eq!(extension_required(&point).unwrap(), None);
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        let full = cube(6);
        for cut in [6, 20, 60, full.len() - 1] {
            let slice = &full[..cut];
            // Whatever it returns, it must not panic and must not hang.
            if let Ok(Some(s)) = surfaces(slice) {
                let _ = s.polygons();
            }
        }
    }
}
