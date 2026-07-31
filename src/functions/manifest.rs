//! Machine-readable catalog of every SQL function kenro registers: the
//! single source of truth the rusqlite binding is tested against
//! (`tests/manifest_consistency.rs`) and the WASM adapters are generated
//! from. Dependency-free on purpose.

/// Value kinds crossing the SQL ↔ kenro boundary.
///
/// Adapters apply the same mapping rules everywhere: all functions are
/// NULL-strict (any SQL NULL argument → NULL result, applied before kenro is
/// called), deterministic and innocuous; `Blob` arguments must be BLOBs
/// (TEXT gets the "did you mean ST_GeomFromText?" error); `Int`/`I64` must
/// be integral; `Real` accepts INTEGER or REAL. `I64` marks values that need
/// true 64-bit integers — hosts without an int64 path must register those
/// functions as loud errors instead of silently losing precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Blob,
    Text,
    Int,
    I64,
    Real,
    Bool,
    OptReal,
    OptI64,
    /// Nullable geometry return (`Option<Vec<u8>>` → SQL NULL). Return-only.
    OptBlob,
}

pub struct FnEntry {
    pub sql_name: &'static str,
    /// wasm-bindgen export name in kenro-wasm (camelCase).
    pub export: &'static str,
    /// Argument kinds; the SQL arity is `args.len()`.
    pub args: &'static [Kind],
    pub ret: Kind,
    /// Cargo feature gating this function, if any.
    pub feature: Option<&'static str>,
}

macro_rules! entry {
    ($sql:literal, $export:literal, [$($arg:ident),*], $ret:ident, $feature:expr) => {
        FnEntry {
            sql_name: $sql,
            export: $export,
            args: &[$(Kind::$arg),*],
            ret: Kind::$ret,
            feature: $feature,
        }
    };
}

/// Every implemented SQL function, one entry per (name, arity).
pub const FUNCTIONS: &[FnEntry] = &[
    // Geometry I/O.
    entry!("ST_GeomFromText", "stGeomFromText", [Text], Blob, None),
    entry!(
        "ST_GeomFromText",
        "stGeomFromTextSrid",
        [Text, Int],
        Blob,
        None
    ),
    entry!("ST_GeomFromWKB", "stGeomFromWkb", [Blob], Blob, None),
    entry!(
        "ST_GeomFromWKB",
        "stGeomFromWkbSrid",
        [Blob, Int],
        Blob,
        None
    ),
    entry!("ST_GeomFromGPB", "stGeomFromGpb", [Blob], Blob, None),
    entry!("ST_AsText", "stAsText", [Blob], Text, None),
    entry!("ST_AsBinary", "stAsBinary", [Blob], Blob, None),
    entry!("ST_AsGPB", "stAsGpb", [Blob], Blob, None),
    // SRID.
    entry!("ST_SetSRID", "stSetSrid", [Blob, Int], Blob, None),
    entry!("ST_SRID", "stSrid", [Blob], Int, None),
    // Predicates & measures.
    entry!("ST_Intersects", "stIntersects", [Blob, Blob], Bool, None),
    entry!("ST_Contains", "stContains", [Blob, Blob], Bool, None),
    entry!("ST_Within", "stWithin", [Blob, Blob], Bool, None),
    entry!("ST_Disjoint", "stDisjoint", [Blob, Blob], Bool, None),
    entry!("ST_Touches", "stTouches", [Blob, Blob], Bool, None),
    entry!("ST_Crosses", "stCrosses", [Blob, Blob], Bool, None),
    entry!("ST_Overlaps", "stOverlaps", [Blob, Blob], Bool, None),
    entry!("ST_Equals", "stEquals", [Blob, Blob], Bool, None),
    entry!("ST_Covers", "stCovers", [Blob, Blob], Bool, None),
    entry!("ST_CoveredBy", "stCoveredBy", [Blob, Blob], Bool, None),
    entry!("ST_Relate", "stRelate", [Blob, Blob], Text, None),
    entry!(
        "ST_Relate",
        "stRelatePattern",
        [Blob, Blob, Text],
        Bool,
        None
    ),
    entry!("ST_Distance", "stDistance", [Blob, Blob], OptReal, None),
    entry!("ST_DWithin", "stDwithin", [Blob, Blob, Real], Bool, None),
    // GeoPackage R-tree.
    entry!("ST_MinX", "stMinX", [Blob], OptReal, None),
    entry!("ST_MaxX", "stMaxX", [Blob], OptReal, None),
    entry!("ST_MinY", "stMinY", [Blob], OptReal, None),
    entry!("ST_MaxY", "stMaxY", [Blob], OptReal, None),
    entry!("ST_IsEmpty", "stIsEmpty", [Blob], Bool, None),
    // CRS transform.
    entry!(
        "ST_Transform",
        "stTransform",
        [Blob, Int],
        Blob,
        Some("transform")
    ),
    // GeoJSON.
    entry!("ST_AsGeoJSON", "stAsGeojson", [Blob], Text, Some("geojson")),
    entry!(
        "ST_AsGeoJSON",
        "stAsGeojsonDigits",
        [Blob, Int],
        Text,
        Some("geojson")
    ),
    entry!(
        "ST_GeomFromGeoJSON",
        "stGeomFromGeojson",
        [Text],
        Blob,
        Some("geojson")
    ),
    // H3 cells (need true int64).
    entry!(
        "h3_latlng_to_cell",
        "h3LatlngToCell",
        [Blob, Int],
        I64,
        Some("h3")
    ),
    entry!(
        "h3_cell_to_parent",
        "h3CellToParent",
        [I64, Int],
        I64,
        Some("h3")
    ),
    entry!(
        "h3_cell_to_string",
        "h3CellToString",
        [I64],
        Text,
        Some("h3")
    ),
    entry!(
        "h3_string_to_cell",
        "h3StringToCell",
        [Text],
        I64,
        Some("h3")
    ),
    // Constructors.
    entry!("ST_MakePoint", "stMakePoint", [Real, Real], Blob, None),
    entry!("ST_Point", "stPoint", [Real, Real], Blob, None),
    entry!("ST_Point", "stPointSrid", [Real, Real, Int], Blob, None),
    entry!(
        "ST_MakeEnvelope",
        "stMakeEnvelope",
        [Real, Real, Real, Real],
        Blob,
        None
    ),
    entry!(
        "ST_MakeEnvelope",
        "stMakeEnvelopeSrid",
        [Real, Real, Real, Real, Int],
        Blob,
        None
    ),
    // GeoPackage geometry-type-trigger support (extension F.4).
    entry!(
        "GPKG_IsAssignable",
        "gpkgIsAssignable",
        [Text, Text],
        Bool,
        None
    ),
    // Measures.
    entry!(
        "ST_ClosestPoint",
        "stClosestPoint",
        [Blob, Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_LineInterpolatePoint",
        "stLineInterpolatePoint",
        [Blob, Real],
        Blob,
        None
    ),
    entry!(
        "ST_LineLocatePoint",
        "stLineLocatePoint",
        [Blob, Blob],
        Real,
        None
    ),
    entry!(
        "ST_HausdorffDistance",
        "stHausdorffDistance",
        [Blob, Blob],
        Real,
        None
    ),
    entry!(
        "ST_FrechetDistance",
        "stFrechetDistance",
        [Blob, Blob],
        Real,
        None
    ),
    entry!("ST_Azimuth", "stAzimuth", [Blob, Blob], OptReal, None),
    // Accessors.
    entry!("ST_Area", "stArea", [Blob], Real, None),
    entry!("ST_NPoints", "stNPoints", [Blob], Int, None),
    entry!("ST_Perimeter", "stPerimeter", [Blob], Real, None),
    entry!("ST_GeometryType", "stGeometryType", [Blob], Text, None),
    entry!("ST_NumGeometries", "stNumGeometries", [Blob], Int, None),
    entry!("ST_GeometryN", "stGeometryN", [Blob, Int], OptBlob, None),
    entry!("ST_StartPoint", "stStartPoint", [Blob], OptBlob, None),
    entry!("ST_EndPoint", "stEndPoint", [Blob], OptBlob, None),
    entry!("ST_PointN", "stPointN", [Blob, Int], OptBlob, None),
    entry!("ST_Reverse", "stReverse", [Blob], Blob, None),
    entry!("ST_Length", "stLength", [Blob], Real, None),
    entry!("ST_Centroid", "stCentroid", [Blob], Blob, None),
    entry!("ST_Envelope", "stEnvelope", [Blob], Blob, None),
    entry!("ST_X", "stX", [Blob], OptReal, None),
    entry!("ST_Y", "stY", [Blob], OptReal, None),
    entry!("ST_NumPoints", "stNumPoints", [Blob], OptI64, None),
    entry!("ST_IsValid", "stIsValid", [Blob], Bool, None),
    entry!("ST_Simplify", "stSimplify", [Blob, Real], Blob, None),
];

/// Concrete arities for stubs, for hosts that cannot register variadic
/// (`n_args = -1`) functions. Names not listed here register at every arity
/// in `DEFAULT_STUB_ARITIES`.
pub const STUB_ARITIES: &[(&str, &[i32])] = &[
    ("ST_Buffer", &[2, 3]),
    ("ST_Union", &[1, 2]),
    ("ST_Intersection", &[2]),
    ("ST_Difference", &[2]),
    ("ST_SymDifference", &[2]),
    ("ST_AsMVT", &[1, 2, 3, 4]),
    ("ST_AsMVTGeom", &[2, 3, 4, 5]),
    // Feature-off fallbacks.
    ("ST_Transform", &[2]),
    ("ST_AsGeoJSON", &[1, 2]),
    ("ST_GeomFromGeoJSON", &[1]),
    ("h3_latlng_to_cell", &[2]),
    ("h3_cell_to_parent", &[2]),
    ("h3_cell_to_string", &[1]),
    ("h3_string_to_cell", &[1]),
];

pub const DEFAULT_STUB_ARITIES: &[i32] = &[1, 2];

pub fn stub_arities(name: &str) -> &'static [i32] {
    STUB_ARITIES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, a)| *a)
        .unwrap_or(DEFAULT_STUB_ARITIES)
}

/// The function entries active under the current feature set.
pub fn active_functions() -> impl Iterator<Item = &'static FnEntry> {
    FUNCTIONS.iter().filter(|e| match e.feature {
        None => true,
        Some("transform") => cfg!(feature = "transform"),
        Some("h3") => cfg!(feature = "h3"),
        Some("geojson") => cfg!(feature = "geojson"),
        Some(_) => false,
    })
}

/// The stub entries active under the current feature set: the permanent
/// catalog plus feature-off fallbacks.
pub fn active_stubs() -> Vec<&'static super::stubs::Stub> {
    let mut stubs: Vec<&'static super::stubs::Stub> = super::stubs::STUBS.iter().collect();
    if !cfg!(feature = "transform") {
        stubs.extend(super::stubs::TRANSFORM_OFF.iter());
    }
    if !cfg!(feature = "h3") {
        stubs.extend(super::stubs::H3_OFF.iter());
    }
    if !cfg!(feature = "geojson") {
        stubs.extend(super::stubs::GEOJSON_OFF.iter());
    }
    stubs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_names_are_unique() {
        let mut exports: Vec<_> = FUNCTIONS.iter().map(|e| e.export).collect();
        exports.sort_unstable();
        let len = exports.len();
        exports.dedup();
        assert_eq!(exports.len(), len);
    }

    #[test]
    fn sql_name_arity_pairs_are_unique() {
        let mut pairs: Vec<_> = FUNCTIONS
            .iter()
            .map(|e| (e.sql_name.to_ascii_lowercase(), e.args.len()))
            .collect();
        pairs.sort();
        let len = pairs.len();
        pairs.dedup();
        assert_eq!(pairs.len(), len);
    }

    #[test]
    fn i64_functions_are_exactly_the_h3_family() {
        let uses_i64: Vec<_> = FUNCTIONS
            .iter()
            .filter(|e| e.args.contains(&Kind::I64) || matches!(e.ret, Kind::I64))
            .map(|e| e.sql_name)
            .collect();
        assert_eq!(
            uses_i64,
            [
                "h3_latlng_to_cell",
                "h3_cell_to_parent",
                "h3_cell_to_string",
                "h3_string_to_cell"
            ]
        );
    }
}
