//! Catalog of ST_ functions kenro knows about but does not implement.
//!
//! Each registers as a variadic SQL function whose body immediately raises
//! [`crate::Error::Unimplemented`] with a hint — so an AI or a human sees
//! "not implemented … use X instead" rather than `no such function`, and can
//! self-correct in one step. Two flavors of hint: "not yet" (planned) and
//! "never" (deliberately out of scope).

pub struct Stub {
    pub name: &'static str,
    pub hint: &'static str,
}

const GEOS_HINT: &str =
    "kenro deliberately excludes GEOS-class operations; use SpatiaLite or DuckDB spatial for this.";

pub const STUBS: &[Stub] = &[
    // Planned ("not yet").
    Stub {
        name: "ST_Transform",
        hint: "Planned for kenro 0.2 (proj4rs-based CRS transform).",
    },
    Stub {
        name: "ST_AsGeoJSON",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_GeomFromGeoJSON",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_Envelope",
        hint: "Planned for kenro 0.2. For R-tree maintenance use ST_MinX/ST_MaxX/ST_MinY/ST_MaxY.",
    },
    Stub {
        name: "ST_Centroid",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_Area",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_Length",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_Simplify",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_IsValid",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_NumPoints",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_SRID",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_X",
        hint: "Planned for kenro 0.2.",
    },
    Stub {
        name: "ST_Y",
        hint: "Planned for kenro 0.2.",
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
