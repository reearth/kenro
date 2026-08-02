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
    /// Nullable small integer (`Option<i64>` → SQL NULL) that does *not*
    /// need a 64-bit path — ring counts and the like. Kept apart from
    /// `OptI64` so the "only h3 crosses 64 bits" invariant stays checkable,
    /// and so sql.js (no int64) can register these normally. Return-only.
    OptInt,
    /// Nullable geometry return (`Option<Vec<u8>>` → SQL NULL). Return-only.
    OptBlob,
    /// Nullable text return (`Option<String>` → SQL NULL). Return-only.
    OptText,
    /// TEXT accepted as-is; INTEGER n normalized to `quad_segs=n` by the
    /// binding layer (ST_Buffer's third argument). Argument-only.
    TextOrInt,
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
    // PostGIS spellings for the same code. Aliases reuse the wasm export, so
    // they cost nothing beyond a registration.
    entry!("ST_XMin", "stMinX", [Blob], OptReal, None),
    entry!("ST_XMax", "stMaxX", [Blob], OptReal, None),
    entry!("ST_YMin", "stMinY", [Blob], OptReal, None),
    entry!("ST_YMax", "stMaxY", [Blob], OptReal, None),
    entry!("ST_GeometryFromText", "stGeomFromText", [Text], Blob, None),
    entry!(
        "ST_GeometryFromText",
        "stGeomFromTextSrid",
        [Text, Int],
        Blob,
        None
    ),
    entry!("ST_GeomFromEWKB", "stGeomFromWkb", [Blob], Blob, None),
    entry!("ST_Area2D", "stArea", [Blob], Real, None),
    entry!("ST_Perimeter2D", "stPerimeter", [Blob], Real, None),
    entry!("ST_Length2D", "stLength", [Blob], Real, None),
    // EWKT/EWKB and the flattening that lets 3D input reach an encoder.
    entry!("ST_Force2D", "stForce2d", [Blob], Blob, None),
    entry!("ST_AsEWKT", "stAsEwkt", [Blob], Text, None),
    entry!("ST_GeomFromEWKT", "stGeomFromEwkt", [Text], Blob, None),
    entry!("ST_AsEWKB", "stAsEwkb", [Blob], Blob, None),
    entry!("ST_AsHexEWKB", "stAsHexEwkb", [Blob], Text, None),
    // Typed constructors: NULL (not an error) when the input parses but is
    // another type, as PostGIS does.
    entry!("ST_PointFromText", "stPointFromText", [Text], OptBlob, None),
    entry!(
        "ST_PointFromText",
        "stPointFromTextSrid",
        [Text, Int],
        OptBlob,
        None
    ),
    entry!("ST_LineFromText", "stLineFromText", [Text], OptBlob, None),
    entry!(
        "ST_LineFromText",
        "stLineFromTextSrid",
        [Text, Int],
        OptBlob,
        None
    ),
    entry!("ST_PolyFromText", "stPolyFromText", [Text], OptBlob, None),
    entry!(
        "ST_PolyFromText",
        "stPolyFromTextSrid",
        [Text, Int],
        OptBlob,
        None
    ),
    entry!(
        "ST_MPointFromText",
        "stMPointFromText",
        [Text],
        OptBlob,
        None
    ),
    entry!(
        "ST_MPointFromText",
        "stMPointFromTextSrid",
        [Text, Int],
        OptBlob,
        None
    ),
    entry!("ST_MLineFromText", "stMLineFromText", [Text], OptBlob, None),
    entry!(
        "ST_MLineFromText",
        "stMLineFromTextSrid",
        [Text, Int],
        OptBlob,
        None
    ),
    entry!("ST_MPolyFromText", "stMPolyFromText", [Text], OptBlob, None),
    entry!(
        "ST_MPolyFromText",
        "stMPolyFromTextSrid",
        [Text, Int],
        OptBlob,
        None
    ),
    entry!(
        "ST_PolygonFromText",
        "stPolyFromText",
        [Text],
        OptBlob,
        None
    ),
    entry!(
        "ST_PolygonFromText",
        "stPolyFromTextSrid",
        [Text, Int],
        OptBlob,
        None
    ),
    entry!(
        "ST_LineStringFromText",
        "stLineFromText",
        [Text],
        OptBlob,
        None
    ),
    entry!(
        "ST_LineStringFromText",
        "stLineFromTextSrid",
        [Text, Int],
        OptBlob,
        None
    ),
    entry!("ST_PointFromWKB", "stPointFromWkb", [Blob], OptBlob, None),
    entry!(
        "ST_PointFromWKB",
        "stPointFromWkbSrid",
        [Blob, Int],
        OptBlob,
        None
    ),
    entry!("ST_LineFromWKB", "stLineFromWkb", [Blob], OptBlob, None),
    entry!(
        "ST_LineFromWKB",
        "stLineFromWkbSrid",
        [Blob, Int],
        OptBlob,
        None
    ),
    // Structural accessors and geometry editing (functions::edit).
    entry!("ST_ExteriorRing", "stExteriorRing", [Blob], OptBlob, None),
    entry!(
        "ST_InteriorRingN",
        "stInteriorRingN",
        [Blob, Int],
        OptBlob,
        None
    ),
    entry!(
        "ST_NumInteriorRings",
        "stNumInteriorRings",
        [Blob],
        OptInt,
        None
    ),
    entry!(
        "ST_NumInteriorRing",
        "stNumInteriorRings",
        [Blob],
        OptInt,
        None
    ),
    entry!("ST_NRings", "stNRings", [Blob], Int, None),
    entry!("ST_Boundary", "stBoundary", [Blob], Blob, None),
    entry!("ST_IsClosed", "stIsClosed", [Blob], Bool, None),
    entry!("ST_IsRing", "stIsRing", [Blob], Bool, None),
    entry!("ST_AddPoint", "stAddPoint", [Blob, Blob], OptBlob, None),
    entry!(
        "ST_AddPoint",
        "stAddPointAt",
        [Blob, Blob, Int],
        OptBlob,
        None
    ),
    entry!(
        "ST_SetPoint",
        "stSetPoint",
        [Blob, Int, Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_RemovePoint",
        "stRemovePoint",
        [Blob, Int],
        OptBlob,
        None
    ),
    entry!("ST_MakeLine", "stMakeLine", [Blob, Blob], Blob, None),
    entry!("ST_MakePolygon", "stMakePolygon", [Blob], Blob, None),
    entry!("ST_Multi", "stMulti", [Blob], Blob, None),
    entry!("ST_SnapToGrid", "stSnapToGrid", [Blob, Real], Blob, None),
    entry!(
        "ST_SnapToGrid",
        "stSnapToGridXy",
        [Blob, Real, Real],
        Blob,
        None
    ),
    entry!(
        "ST_FlipCoordinates",
        "stFlipCoordinates",
        [Blob],
        Blob,
        None
    ),
    entry!("ST_ShiftLongitude", "stShiftLongitude", [Blob], Blob, None),
    entry!("ST_Expand", "stExpand", [Blob, Real], OptBlob, None),
    // Sphere/spheroid measures (functions::geodesic) — the answer to
    // ST_Distance being planar on EPSG:4326 data.
    entry!(
        "ST_DistanceSphere",
        "stDistanceSphere",
        [Blob, Blob],
        Real,
        None
    ),
    entry!(
        "ST_DistanceSpheroid",
        "stDistanceSpheroid",
        [Blob, Blob],
        Real,
        Some("spheroid")
    ),
    entry!(
        "ST_DistanceSpheroid",
        "stDistanceSpheroidOn",
        [Blob, Blob, Text],
        Real,
        Some("spheroid")
    ),
    entry!(
        "ST_LengthSpheroid",
        "stLengthSpheroid",
        [Blob, Text],
        Real,
        Some("spheroid")
    ),
    entry!(
        "ST_Length2DSpheroid",
        "stLengthSpheroid",
        [Blob, Text],
        Real,
        Some("spheroid")
    ),
    entry!("ST_Project", "stProject", [Blob, Real, Real], Blob, None),
    // Dimension and validity reporting.
    entry!("ST_Dimension", "stDimension", [Blob], Int, None),
    entry!("ST_CoordDim", "stCoordDim", [Blob], Int, None),
    entry!("ST_NDims", "stCoordDim", [Blob], Int, None),
    // 3D pass-through (functions::threed): kenro computes in 2D but reads
    // and reports the ordinates the stored geometry actually has.
    // GML 2/3 I/O (functions::gml).
    entry!("ST_AsGML", "stAsGml", [Blob], Text, Some("gml")),
    entry!("ST_AsGML", "stAsGmlVersion", [Int, Blob], Text, Some("gml")),
    entry!(
        "ST_AsGML",
        "stAsGmlDigits",
        [Int, Blob, Int],
        Text,
        Some("gml")
    ),
    entry!("ST_GeomFromGML", "stGeomFromGml", [Text], Blob, Some("gml")),
    entry!(
        "ST_GeomFromGML",
        "stGeomFromGmlSrid",
        [Text, Int],
        Blob,
        Some("gml")
    ),
    entry!("ST_GMLToSQL", "stGeomFromGml", [Text], Blob, Some("gml")),
    // Surface collections (functions::surface): read, measured and
    // flattened, never computed with directly.
    entry!("ST_NumPatches", "stNumPatches", [Blob], OptInt, None),
    entry!("ST_PatchN", "stPatchN", [Blob, Int], OptBlob, None),
    entry!(
        "kenro_gpkg_extension_required",
        "kenroGpkgExtensionRequired",
        [Blob],
        OptText,
        None
    ),
    entry!("ST_HasZ", "stHasZ", [Blob], Bool, None),
    entry!("ST_HasM", "stHasM", [Blob], Bool, None),
    entry!("ST_Z", "stZ", [Blob], OptReal, None),
    entry!("ST_M", "stM", [Blob], OptReal, None),
    entry!("ST_ZMin", "stZMin", [Blob], OptReal, None),
    entry!("ST_ZMax", "stZMax", [Blob], OptReal, None),
    entry!("ST_IsValidReason", "stIsValidReason", [Blob], Text, None),
    // Ring orientation.
    entry!("ST_ForcePolygonCW", "stForcePolygonCw", [Blob], Blob, None),
    entry!(
        "ST_ForcePolygonCCW",
        "stForcePolygonCcw",
        [Blob],
        Blob,
        None
    ),
    entry!("ST_ForceRHR", "stForcePolygonCw", [Blob], Blob, None),
    entry!("ST_IsPolygonCW", "stIsPolygonCw", [Blob], Bool, None),
    entry!("ST_IsPolygonCCW", "stIsPolygonCcw", [Blob], Bool, None),
    // Linear referencing and distance geometry (functions::linear).
    entry!("ST_Segmentize", "stSegmentize", [Blob, Real], Blob, None),
    entry!(
        "ST_LineSubstring",
        "stLineSubstring",
        [Blob, Real, Real],
        OptBlob,
        None
    ),
    entry!(
        "ST_ShortestLine",
        "stShortestLine",
        [Blob, Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_LongestLine",
        "stLongestLine",
        [Blob, Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_MaxDistance",
        "stMaxDistance",
        [Blob, Blob],
        OptReal,
        None
    ),
    // Smallest enclosing circle — no overlay engine needed.
    entry!(
        "ST_MinimumBoundingRadius",
        "stMinimumBoundingRadius",
        [Blob],
        OptReal,
        None
    ),
    entry!(
        "ST_MinimumBoundingCircle",
        "stMinimumBoundingCircle",
        [Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_MinimumBoundingCircle",
        "stMinimumBoundingCircleSegs",
        [Blob, Int],
        OptBlob,
        None
    ),
    // Areal operations that go through the overlay mesh.
    entry!(
        "ST_UnaryUnion",
        "stUnaryUnion",
        [Blob],
        Blob,
        Some("overlay")
    ),
    entry!(
        "ST_ClipByBox2D",
        "stClipByBox2d",
        [Blob, Blob],
        Blob,
        Some("overlay")
    ),
    entry!(
        "ST_Subdivide",
        "stSubdivide",
        [Blob, Int],
        Blob,
        Some("overlay")
    ),
    // The rest of the reachable PostGIS surface (functions::extra).
    entry!(
        "ST_ContainsProperly",
        "stContainsProperly",
        [Blob, Blob],
        Bool,
        None
    ),
    entry!(
        "ST_DFullyWithin",
        "stDfullyWithin",
        [Blob, Blob, Real],
        Bool,
        None
    ),
    entry!("ST_RelateMatch", "stRelateMatch", [Text, Text], Bool, None),
    entry!(
        "ST_Affine",
        "stAffine",
        [Blob, Real, Real, Real, Real, Real, Real],
        Blob,
        None
    ),
    entry!(
        "ST_TransScale",
        "stTransScale",
        [Blob, Real, Real, Real, Real],
        Blob,
        None
    ),
    entry!(
        "ST_ReducePrecision",
        "stReducePrecision",
        [Blob, Real],
        Blob,
        None
    ),
    entry!("ST_Angle", "stAngle3", [Blob, Blob, Blob], OptReal, None),
    entry!(
        "ST_Angle",
        "stAngle4",
        [Blob, Blob, Blob, Blob],
        OptReal,
        None
    ),
    entry!(
        "ST_LineInterpolatePoints",
        "stLineInterpolatePoints",
        [Blob, Real],
        OptBlob,
        None
    ),
    entry!("ST_Points", "stPoints", [Blob], Blob, None),
    entry!(
        "ST_BoundingDiagonal",
        "stBoundingDiagonal",
        [Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_OrderingEquals",
        "stOrderingEquals",
        [Blob, Blob],
        Bool,
        None
    ),
    entry!("ST_GeoHash", "stGeohash", [Blob], OptText, None),
    entry!("ST_GeoHash", "stGeohashChars", [Blob, Int], OptText, None),
    // The tail: alternative spellings and small constructors
    // (functions::misc). Aliases reuse an existing export.
    entry!("ST_RotateZ", "stRotate", [Blob, Real], Blob, None),
    entry!(
        "ST_MultiPointFromText",
        "stMPointFromText",
        [Text],
        OptBlob,
        None
    ),
    entry!(
        "ST_MultiLineStringFromText",
        "stMLineFromText",
        [Text],
        OptBlob,
        None
    ),
    entry!(
        "ST_MultiPolygonFromText",
        "stMPolyFromText",
        [Text],
        OptBlob,
        None
    ),
    entry!("ST_PolygonFromWKB", "stPolyFromWkb", [Blob], OptBlob, None),
    entry!(
        "ST_LineStringFromWKB",
        "stLineFromWkb",
        [Blob],
        OptBlob,
        None
    ),
    entry!("ST_MPointFromWKB", "stMPointFromWkb", [Blob], OptBlob, None),
    entry!("ST_MLineFromWKB", "stMLineFromWkb", [Blob], OptBlob, None),
    entry!("ST_MPolyFromWKB", "stMPolyFromWkb", [Blob], OptBlob, None),
    entry!(
        "ST_MultiPointFromWKB",
        "stMPointFromWkb",
        [Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_MultiLineFromWKB",
        "stMLineFromWkb",
        [Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_MultiPolyFromWKB",
        "stMPolyFromWkb",
        [Blob],
        OptBlob,
        None
    ),
    entry!("ST_Polygon", "stPolygon", [Blob, Int], Blob, None),
    entry!(
        "ST_LineFromMultiPoint",
        "stLineFromMultipoint",
        [Blob],
        OptBlob,
        None
    ),
    entry!("ST_LineExtend", "stLineExtend", [Blob, Real], OptBlob, None),
    entry!(
        "ST_LineExtend",
        "stLineExtendBoth",
        [Blob, Real, Real],
        OptBlob,
        None
    ),
    entry!(
        "ST_PointInsideCircle",
        "stPointInsideCircle",
        [Blob, Real, Real, Real],
        Bool,
        None
    ),
    entry!("ST_WrapX", "stWrapX", [Blob, Real, Real], Blob, None),
    entry!("ST_MakeBox2D", "stMakeBox2d", [Blob, Blob], Blob, None),
    entry!(
        "ST_GeomFromGeoHash",
        "stGeomFromGeohash",
        [Text],
        Blob,
        None
    ),
    entry!(
        "ST_GeomFromGeoHash",
        "stGeomFromGeohashPrec",
        [Text, Int],
        Blob,
        None
    ),
    entry!(
        "ST_Box2dFromGeoHash",
        "stGeomFromGeohash",
        [Text],
        Blob,
        None
    ),
    entry!(
        "ST_PointFromGeoHash",
        "stPointFromGeohash",
        [Text],
        Blob,
        None
    ),
    entry!(
        "ST_PointFromGeoHash",
        "stPointFromGeohashPrec",
        [Text, Int],
        Blob,
        None
    ),
    entry!(
        "ST_GeometricMedian",
        "stGeometricMedian",
        [Blob],
        OptBlob,
        None
    ),
    entry!(
        "ST_GeometricMedian",
        "stGeometricMedianTol",
        [Blob, Real],
        OptBlob,
        None
    ),
    entry!(
        "ST_LineCrossingDirection",
        "stLineCrossingDirection",
        [Blob, Blob],
        Int,
        None
    ),
    entry!("ST_Summary", "stSummary", [Blob], Text, None),
    entry!("ST_MemSize", "stMemSize", [Blob], Int, None),
    entry!("ST_Normalize", "stNormalize", [Blob], Blob, None),
    // The two size-gated algorithms (functions::hull).
    entry!(
        "ST_ConcaveHull",
        "stConcaveHull",
        [Blob, Real],
        Blob,
        Some("concave-hull")
    ),
    entry!(
        "ST_DelaunayTriangles",
        "stDelaunayTriangles",
        [Blob],
        Blob,
        Some("delaunay")
    ),
    entry!(
        "ST_TriangulatePolygon",
        "stTriangulatePolygon",
        [Blob],
        Blob,
        Some("delaunay")
    ),
    entry!("ST_PolyFromWKB", "stPolyFromWkb", [Blob], OptBlob, None),
    entry!(
        "ST_PolyFromWKB",
        "stPolyFromWkbSrid",
        [Blob, Int],
        OptBlob,
        None
    ),
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
    // Line structure (functions::lines).
    entry!("ST_IsSimple", "stIsSimple", [Blob], Bool, None),
    entry!("ST_LineMerge", "stLineMerge", [Blob], Blob, None),
    entry!(
        "ST_LineMerge",
        "stLineMergeDirected",
        [Blob, Bool],
        Blob,
        None
    ),
    // Overlay.
    entry!(
        "ST_Intersection",
        "stIntersection",
        [Blob, Blob],
        Blob,
        Some("overlay")
    ),
    entry!(
        "ST_Difference",
        "stDifference",
        [Blob, Blob],
        Blob,
        Some("overlay")
    ),
    entry!(
        "ST_SymmetricDifference",
        "stSymDifference",
        [Blob, Blob],
        Blob,
        Some("overlay")
    ),
    entry!(
        "ST_SymDifference",
        "stSymDifference",
        [Blob, Blob],
        Blob,
        Some("overlay")
    ),
    entry!("ST_Union", "stUnion", [Blob, Blob], Blob, Some("overlay")),
    entry!("ST_Split", "stSplit", [Blob, Blob], Blob, Some("overlay")),
    entry!("ST_Buffer", "stBuffer", [Blob, Real], Blob, Some("overlay")),
    entry!("ST_MakeValid", "stMakeValid", [Blob], Blob, Some("overlay")),
    entry!(
        "ST_Buffer",
        "stBufferOpts",
        [Blob, Real, TextOrInt],
        Blob,
        Some("overlay")
    ),
    // MVT.
    entry!(
        "ST_AsMVTGeom",
        "stAsMvtGeom",
        [Blob, Blob],
        OptBlob,
        Some("mvt")
    ),
    entry!(
        "ST_AsMVTGeom",
        "stAsMvtGeomExtent",
        [Blob, Blob, Int],
        OptBlob,
        Some("mvt")
    ),
    entry!(
        "ST_AsMVTGeom",
        "stAsMvtGeomBuffer",
        [Blob, Blob, Int, Int],
        OptBlob,
        Some("mvt")
    ),
    entry!(
        "ST_AsMVTGeom",
        "stAsMvtGeomClip",
        [Blob, Blob, Int, Int, Int],
        OptBlob,
        Some("mvt")
    ),
    // Processing.
    entry!("ST_ConvexHull", "stConvexHull", [Blob], Blob, None),
    entry!("ST_PointOnSurface", "stPointOnSurface", [Blob], Blob, None),
    entry!("ST_SimplifyVW", "stSimplifyVw", [Blob, Real], Blob, None),
    entry!(
        "ST_ChaikinSmoothing",
        "stChaikinSmoothing",
        [Blob],
        Blob,
        None
    ),
    entry!(
        "ST_ChaikinSmoothing",
        "stChaikinSmoothingN",
        [Blob, Int],
        Blob,
        None
    ),
    entry!(
        "ST_RemoveRepeatedPoints",
        "stRemoveRepeatedPoints",
        [Blob],
        Blob,
        None
    ),
    entry!(
        "ST_OrientedEnvelope",
        "stOrientedEnvelope",
        [Blob],
        Blob,
        None
    ),
    // Affine.
    entry!("ST_Rotate", "stRotate", [Blob, Real], Blob, None),
    entry!(
        "ST_Rotate",
        "stRotateXY",
        [Blob, Real, Real, Real],
        Blob,
        None
    ),
    entry!(
        "ST_Translate",
        "stTranslate",
        [Blob, Real, Real],
        Blob,
        None
    ),
    entry!("ST_Scale", "stScale", [Blob, Real, Real], Blob, None),
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
    entry!("ST_NumPoints", "stNumPoints", [Blob], OptInt, None),
    entry!("ST_IsValid", "stIsValid", [Blob], Bool, None),
    entry!("ST_Simplify", "stSimplify", [Blob, Real], Blob, None),
];

/// Aggregate functions: registered with xStep/xFinal on every host. Their
/// NULL handling differs from the scalar rule — NULL rows are SKIPPED
/// (PostGIS aggregate semantics), enforced by the binding layers.
pub struct AggEntry {
    pub sql_name: &'static str,
    /// wasm-bindgen accumulator class name in kenro-wasm.
    pub ctor_export: &'static str,
    pub args: &'static [Kind],
    pub feature: Option<&'static str>,
}

pub const AGGREGATES: &[AggEntry] = &[
    AggEntry {
        sql_name: "ST_Union",
        ctor_export: "UnionAgg",
        args: &[Kind::Blob],
        feature: Some("overlay"),
    },
    // ST_AsMVT's signature deliberately diverges from PostGIS (SQLite has no
    // record type): (geom [, name [, extent [, props_json]]]).
    AggEntry {
        sql_name: "ST_Extent",
        ctor_export: "ExtentAgg",
        args: &[Kind::Blob],
        feature: None,
    },
    AggEntry {
        sql_name: "ST_AsMVT",
        ctor_export: "MvtAgg",
        args: &[Kind::Blob],
        feature: Some("mvt"),
    },
    AggEntry {
        sql_name: "ST_AsMVT",
        ctor_export: "MvtAgg",
        args: &[Kind::Blob, Kind::Text],
        feature: Some("mvt"),
    },
    AggEntry {
        sql_name: "ST_AsMVT",
        ctor_export: "MvtAgg",
        args: &[Kind::Blob, Kind::Text, Kind::Int],
        feature: Some("mvt"),
    },
    AggEntry {
        sql_name: "ST_AsMVT",
        ctor_export: "MvtAgg",
        args: &[Kind::Blob, Kind::Text, Kind::Int, Kind::Text],
        feature: Some("mvt"),
    },
];

/// The aggregate entries active under the current feature set.
pub fn active_aggregates() -> impl Iterator<Item = &'static AggEntry> {
    AGGREGATES.iter().filter(|e| match e.feature {
        None => true,
        Some("transform") => cfg!(feature = "transform"),
        Some("h3") => cfg!(feature = "h3"),
        Some("geojson") => cfg!(feature = "geojson"),
        Some("overlay") => cfg!(feature = "overlay"),
        Some("mvt") => cfg!(feature = "mvt"),
        Some("spheroid") => cfg!(feature = "spheroid"),
        Some("concave-hull") => cfg!(feature = "concave-hull"),
        Some("delaunay") => cfg!(feature = "delaunay"),
        Some("gml") => cfg!(feature = "gml"),
        Some(_) => false,
    })
}

/// Concrete arities for stubs, for hosts that cannot register variadic
/// (`n_args = -1`) functions. Names not listed here register at every arity
/// in `DEFAULT_STUB_ARITIES`.
pub const STUB_ARITIES: &[(&str, &[i32])] = &[
    // Feature-off fallbacks.
    ("ST_MakeValid", &[1]),
    ("ST_Intersection", &[2]),
    ("ST_Difference", &[2]),
    ("ST_SymDifference", &[2]),
    ("ST_SymmetricDifference", &[2]),
    ("ST_DistanceSpheroid", &[2, 3]),
    ("ST_LengthSpheroid", &[2]),
    ("ST_Length2DSpheroid", &[2]),
    ("ST_UnaryUnion", &[1]),
    ("ST_ClipByBox2D", &[2]),
    ("ST_Subdivide", &[2]),
    ("ST_ConcaveHull", &[2]),
    ("ST_DelaunayTriangles", &[1]),
    ("ST_TriangulatePolygon", &[1]),
    ("ST_IsSimple", &[1]),
    ("ST_LineMerge", &[1, 2]),
    ("ST_Split", &[2]),
    ("ST_AsGML", &[1, 2, 3]),
    ("ST_GeomFromGML", &[1, 2]),
    ("ST_GMLToSQL", &[1]),
    ("ST_Union", &[1, 2]),
    ("ST_Buffer", &[2, 3]),
    ("ST_AsMVTGeom", &[2, 3, 4, 5]),
    ("ST_AsMVT", &[1, 2, 3, 4]),
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
        Some("overlay") => cfg!(feature = "overlay"),
        Some("mvt") => cfg!(feature = "mvt"),
        Some("spheroid") => cfg!(feature = "spheroid"),
        Some("concave-hull") => cfg!(feature = "concave-hull"),
        Some("delaunay") => cfg!(feature = "delaunay"),
        Some("gml") => cfg!(feature = "gml"),
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
    if !cfg!(feature = "spheroid") {
        stubs.extend(super::stubs::SPHEROID_OFF.iter());
    }
    if !cfg!(feature = "concave-hull") {
        stubs.extend(super::stubs::CONCAVE_HULL_OFF.iter());
    }
    if !cfg!(feature = "delaunay") {
        stubs.extend(super::stubs::DELAUNAY_OFF.iter());
    }
    if !cfg!(feature = "gml") {
        stubs.extend(super::stubs::GML_OFF.iter());
    }
    if !cfg!(feature = "h3") {
        stubs.extend(super::stubs::H3_OFF.iter());
    }
    if !cfg!(feature = "geojson") {
        stubs.extend(super::stubs::GEOJSON_OFF.iter());
    }
    if !cfg!(feature = "overlay") {
        stubs.extend(super::stubs::OVERLAY_OFF.iter());
    }
    if !cfg!(feature = "mvt") {
        stubs.extend(super::stubs::MVT_OFF.iter());
    }
    stubs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_export_implements_one_signature() {
        // Exports are deliberately shared by alias spellings — `ST_XMin` is
        // `ST_MinX`'s export, which is why an alias costs no wasm at all. What
        // must never happen is one export serving two different signatures,
        // which would mean a copy-paste error in an entry.
        let mut by_export: Vec<(&str, usize, Kind)> = FUNCTIONS
            .iter()
            .map(|e| (e.export, e.args.len(), e.ret))
            .collect();
        by_export.sort_unstable_by_key(|(export, arity, _)| (*export, *arity));
        by_export.dedup();
        for pair in by_export.windows(2) {
            assert_ne!(
                pair[0].0, pair[1].0,
                "export {} is used for two different signatures",
                pair[0].0
            );
        }
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
            .filter(|e| e.args.contains(&Kind::I64) || matches!(e.ret, Kind::I64 | Kind::OptI64))
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
