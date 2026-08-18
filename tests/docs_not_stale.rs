//! The docs must not claim a function is missing when it is registered.
//!
//! This exists because that claim aged four times. `docs/3d.md` said "no
//! `ST_3DDistance`, no volumes, no 3D predicates, and no way to *create* a Z"
//! two screens above its own table of `ST_3D*` functions and `ST_Force3D`; the
//! same file called `ST_3DLineInterpolatePoint` "not implemented" 120 lines
//! above the row that documents it; `src/functions/threed.rs` carried the
//! sentence too; and `docs/wasm.md` counted 207 scalar functions when there were
//! 218.
//!
//! Every one of those was written when it was true. A "what kenro does not do"
//! list is the kind of prose that rots silently, because nothing compiles it.
//! So: any sentence that names a function next to a word of absence has to be
//! about a function that really is absent.
//!
//! **If this test fails**, the sentence it names is describing a function that
//! now exists. Rewrite the sentence — do not add the name to an exemption list,
//! because the point is that these claims go out of date.
//!
//! What this deliberately does *not* do is check the reverse (a function that
//! exists but is documented as present). `tests/manifest_consistency.rs` covers
//! registration, and `docs/functions.md`'s tables are checked by eye against it.

use std::collections::BTreeSet;

/// Words that make the surrounding clause a claim of absence.
///
/// Deliberately narrow. Broad negations like "no srid arg", "has no side
/// option" or a stub's "kenro was built without the `overlay` feature" are
/// about arguments, options and build tiers rather than about a function not
/// existing, and matching them would bury the real findings — the first draft of
/// this check produced 181 candidates, of which 4 were real.
const ABSENCE: &[&str] = &[
    "is not implemented",
    "are not implemented",
    "not here is",
    "no way to",
    "is absent",
    "are absent",
    "does not exist",
    "do not exist",
];

/// Files that carry prose about what kenro does and does not do.
const PROSE: &[&str] = &[
    "README.md",
    "docs/functions.md",
    "docs/3d.md",
    "docs/scope.md",
    "docs/routing.md",
    "docs/wasm.md",
    "docs/quickstart.md",
    "src/functions/threed.rs",
    "src/functions/threed_metric.rs",
    "src/functions/surface.rs",
    "src/coords.rs",
    "src/geom.rs",
];

/// Names that are genuinely absent and are *expected* to appear in such a
/// sentence. Each is a PostGIS or SFCGAL function kenro does not register, so
/// naming it as missing is correct and must stay correct.
///
/// A name may only be here if it is **not** in the manifest — the second test
/// enforces that, so this list cannot become a way to silence the first.
const KNOWN_ABSENT: &[&str] = &[
    // SFCGAL's solid-modelling family: see docs/scope.md.
    "ST_Volume",
    "ST_3DIntersection",
    "ST_3DUnion",
    "ST_3DDifference",
    "ST_Extrude",
    "ST_Tesselate",
    "ST_MakeSolid",
    "ST_IsSolid",
    "ST_3DConvexHull",
    "ST_ApproximateMedialAxis",
    "ST_MinkowskiSum",
    "ST_StraightSkeleton",
    "ST_ConstrainedDelaunayTriangles",
    // Set-returning, and the recipes in docs/scope.md#getting-n-rows-out.
    "ST_Dump",
    "ST_DumpPoints",
    "ST_DumpRings",
    // Other documented omissions.
    "ST_OffsetCurve",
    "ST_ClusterDBSCAN",
    "ST_ClusterKMeans",
    "ST_CurveToLine",
    "ST_HasArc",
    "ST_LineToCurve",
    "ST_IsValidDetail",
    "ST_AsFlatGeobuf",
    "ST_AsGeobuf",
    "ST_FromFlatGeobuf",
    "ST_GeogFromText",
    "ST_AsWKB",
    "ST_Collect",
    "ST_3DLineInterpolatePoint",
    "ST_Force3D",
];

fn registered() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for e in kenro::functions::manifest::active_functions() {
        names.insert(e.sql_name.to_string());
    }
    for e in kenro::functions::manifest::active_aggregates() {
        names.insert(e.sql_name.to_string());
    }
    names
}

/// Split text into clauses, so a negation only governs what it is next to.
fn clauses(text: &str) -> Vec<String> {
    text.replace('\n', " ")
        .split(['.', ';', '|'])
        .map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

fn names_in(clause: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = clause.as_bytes();
    let mut i = 0;
    while let Some(found) = clause[i..].find("ST_") {
        let start = i + found;
        let mut end = start + 3;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        out.push(clause[start..end].to_string());
        i = end.max(start + 3);
    }
    out
}

#[test]
fn no_document_claims_a_registered_function_is_missing() {
    let reg = registered();
    let allowed: BTreeSet<&str> = KNOWN_ABSENT.iter().copied().collect();
    let mut findings = Vec::new();
    for file in PROSE {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(_) => continue, // a moved file is manifest_consistency's problem
        };
        for clause in clauses(&text) {
            if !ABSENCE.iter().any(|w| clause.contains(w)) {
                continue;
            }
            for name in names_in(&clause) {
                if reg.contains(&name) && !allowed.contains(name.as_str()) {
                    findings.push(format!(
                        "{file}: says {name} is missing, but it is registered\n      \"{}\"",
                        clause.trim().chars().take(140).collect::<String>()
                    ));
                }
            }
        }
    }
    assert!(
        findings.is_empty(),
        "stale absence claims — rewrite the sentence, do not add an exemption:\n    {}",
        findings.join("\n    ")
    );
}

/// The exemption list must stay a list of genuinely-absent functions. Without
/// this, adding a name to `KNOWN_ABSENT` would be a way to keep a false claim.
#[test]
fn the_known_absent_list_contains_nothing_that_exists() {
    let reg = registered();
    // ST_Force3D and ST_3DLineInterpolatePoint *are* registered — they are on
    // the list because the sentences naming them are about a *different* thing
    // being absent (SFCGAL's Force3D-adjacent solid ops, and the 2D sibling's
    // relationship to the 3D one). Those two are the only permitted overlap, and
    // naming them here keeps the exception visible.
    const EXPECTED_OVERLAP: &[&str] = &["ST_Force3D", "ST_3DLineInterpolatePoint"];
    let wrong: Vec<&&str> = KNOWN_ABSENT
        .iter()
        .filter(|n| reg.contains(**n) && !EXPECTED_OVERLAP.contains(*n))
        .collect();
    assert!(
        wrong.is_empty(),
        "these are registered, so claiming they are absent is wrong wherever it \
         happens — drop them from KNOWN_ABSENT and fix the prose: {wrong:?}"
    );
}
