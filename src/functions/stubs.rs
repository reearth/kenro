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
        name: "ST_Union",
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
