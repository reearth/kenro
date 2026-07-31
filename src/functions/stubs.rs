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

const GEOS_HINT: &str =
    "kenro deliberately excludes GEOS-class operations; use SpatiaLite or DuckDB spatial for this.";

pub const STUBS: &[Stub] = &[
    // Planned ("not yet").
    Stub {
        name: "ST_NPoints",
        hint: "Planned for a future release. For linestrings, ST_NumPoints works today \
               (note the two are different functions in PostGIS too).",
    },
    Stub {
        name: "ST_Perimeter",
        hint: "Planned for a future release. Note ST_Length returns 0 for polygons, \
               matching PostGIS.",
    },
    // Deliberately out of scope ("never").
    Stub {
        name: "ST_Buffer",
        hint: GEOS_HINT,
    },
    Stub {
        name: "ST_Union",
        hint: GEOS_HINT,
    },
    Stub {
        name: "ST_Intersection",
        hint: GEOS_HINT,
    },
    Stub {
        name: "ST_Difference",
        hint: GEOS_HINT,
    },
    Stub {
        name: "ST_SymDifference",
        hint: GEOS_HINT,
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
