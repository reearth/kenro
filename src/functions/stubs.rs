//! Catalog of ST_ functions kenro knows about but does not implement.
//!
//! Each registers as a variadic SQL function whose body immediately raises
//! [`crate::Error::Unimplemented`] with a hint — so an AI or a human sees
//! "not implemented … use X instead" rather than `no such function`, and can
//! self-correct in one step. Two flavors of hint: "not yet" (planned) and
//! "never" (deliberately out of scope).
//!
//! Functions gated behind a disabled cargo feature also register as stubs
//! (with a hint naming the missing feature); that wiring lives in the
//! binding layer.

pub struct Stub {
    pub name: &'static str,
    pub hint: &'static str,
}

pub const STUBS: &[Stub] = &[
    // Deliberately excluded.
    Stub {
        name: "ST_Collect",
        hint: "kenro never produces GeometryCollection values; for areal dissolve use \
               the ST_Union aggregate, otherwise collect rows on the application side.",
    },
];

/// Stubs installed when a cargo feature is compiled out, so the failure mode
/// stays a helpful error rather than `no such function`.
pub const TRANSFORM_OFF: &[Stub] = &[Stub {
    name: "ST_Transform",
    hint: "kenro was built without the `transform` cargo feature.",
}];

pub const H3_OFF: &[Stub] = &[
    Stub {
        name: "h3_latlng_to_cell",
        hint: "kenro was built without the `h3` cargo feature.",
    },
    Stub {
        name: "h3_cell_to_parent",
        hint: "kenro was built without the `h3` cargo feature.",
    },
    Stub {
        name: "h3_cell_to_string",
        hint: "kenro was built without the `h3` cargo feature.",
    },
    Stub {
        name: "h3_string_to_cell",
        hint: "kenro was built without the `h3` cargo feature.",
    },
];

pub const OVERLAY_OFF: &[Stub] = &[
    Stub {
        name: "ST_MakeValid",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_Intersection",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_Difference",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_SymDifference",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_SymmetricDifference",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_Union",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_UnaryUnion",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_ClipByBox2D",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_Subdivide",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_Split",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
    Stub {
        name: "ST_Buffer",
        hint: "kenro was built without the `overlay` cargo feature.",
    },
];

pub const MVT_OFF: &[Stub] = &[
    Stub {
        name: "ST_AsMVTGeom",
        hint: "kenro was built without the `mvt` cargo feature.",
    },
    Stub {
        name: "ST_AsMVT",
        hint: "kenro was built without the `mvt` cargo feature.",
    },
];

pub const ROUTING_OFF: &[Stub] = &[
    Stub {
        name: "kenro_dijkstra",
        hint: "kenro was built without the `routing` cargo feature.",
    },
    Stub {
        name: "kenro_dijkstra_cost",
        hint: "kenro was built without the `routing` cargo feature.",
    },
    Stub {
        name: "kenro_drivingdistance",
        hint: "kenro was built without the `routing` cargo feature.",
    },
];

pub const GEOJSON_OFF: &[Stub] = &[
    Stub {
        name: "ST_AsGeoJSON",
        hint: "kenro was built without the `geojson` cargo feature.",
    },
    Stub {
        name: "ST_GeomFromGeoJSON",
        hint: "kenro was built without the `geojson` cargo feature.",
    },
];

/// Registered when the `spheroid` feature is off: the ellipsoidal measures.
/// The spherical `ST_DistanceSphere` is always available.
pub static SPHEROID_OFF: &[Stub] = &[
    Stub {
        name: "ST_DistanceSpheroid",
        hint: "kenro was built without the `spheroid` cargo feature; \
               ST_DistanceSphere (spherical) is available in every build.",
    },
    Stub {
        name: "ST_LengthSpheroid",
        hint: "kenro was built without the `spheroid` cargo feature.",
    },
    Stub {
        name: "ST_Length2DSpheroid",
        hint: "kenro was built without the `spheroid` cargo feature.",
    },
];

/// Registered when `concave-hull` is off (+41 KB of wasm when on).
pub static CONCAVE_HULL_OFF: &[Stub] = &[Stub {
    name: "ST_ConcaveHull",
    hint: "kenro was built without the `concave-hull` cargo feature; \
           ST_ConvexHull is available in every build.",
}];

/// Registered when `delaunay` is off (+81 KB of wasm when on).
pub static DELAUNAY_OFF: &[Stub] = &[
    Stub {
        name: "ST_DelaunayTriangles",
        hint: "kenro was built without the `delaunay` cargo feature.",
    },
    Stub {
        name: "ST_TriangulatePolygon",
        hint: "kenro was built without the `delaunay` cargo feature.",
    },
];

/// Registered when `voronoi` is off. The cells need `delaunay` (spade) *and*
/// `overlay` (i_overlay clips them), which is why the feature names both.
pub static VORONOI_OFF: &[Stub] = &[
    Stub {
        name: "ST_VoronoiPolygons",
        hint: "kenro was built without the `voronoi` cargo feature.",
    },
    Stub {
        name: "ST_VoronoiLines",
        hint: "kenro was built without the `voronoi` cargo feature.",
    },
];

/// Registered when `text-encodings` is off. Both are pure string formatting;
/// the feature exists because `ST_AsKML` reprojects to WGS84 and so pulls
/// `transform` in with it.
pub static TEXT_ENCODINGS_OFF: &[Stub] = &[
    Stub {
        name: "ST_AsKML",
        hint: "kenro was built without the `text-encodings` cargo feature.",
    },
    Stub {
        name: "ST_AsSVG",
        hint: "kenro was built without the `text-encodings` cargo feature.",
    },
];

/// Registered when `gml` is off (+13 KB of wasm when on, for quick-xml).
pub static GML_OFF: &[Stub] = &[
    Stub {
        name: "ST_AsGML",
        hint: "kenro was built without the `gml` cargo feature.",
    },
    Stub {
        name: "ST_GeomFromGML",
        hint: "kenro was built without the `gml` cargo feature.",
    },
    Stub {
        name: "ST_GMLToSQL",
        hint: "kenro was built without the `gml` cargo feature.",
    },
];
