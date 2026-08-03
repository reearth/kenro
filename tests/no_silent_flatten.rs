//! A source-level tripwire: `has_zm: false` may only be written where someone
//! decided it, on purpose, with the measurement to back it up.
//!
//! `has_zm` is the only field `geom::reject_zm` consults, so writing `false`
//! into a freshly-built `Geom` is how a function silently drops a Z. That
//! happened 45 times (`tmp/out-audit.md`), and the fix was to route derived
//! geometries through `geom::encode_derived` instead.
//!
//! Routing was not enough on its own. The fix gave each module an `out()` that
//! *requires* naming the source geometries, so the compiler would ask the
//! question at every existing call site — but a later module wrote its own
//! helper and never met that question. It reached the right answer (PostGIS's
//! grids really are 2D, measured) under a name that means the opposite
//! elsewhere, which is a trap rather than a bug.
//!
//! So the guard is structural instead of type-driven: the set of functions
//! allowed to write `has_zm: false` is enumerated here. Adding one means
//! deciding, and saying why in a comment next to it.
//!
//! **If this test fails**, the new site is one of:
//!
//! - a *derived* geometry — the output came from some input's coordinates. Use
//!   `geom::encode_derived(geometry, srid, func, &[sources…])`; it carries the Z
//!   where it can and refuses where it cannot.
//! - a genuinely 2D answer — PostGIS returns 2D too. **Measure it**, name the
//!   helper `*_2d`, put the measurement in its doc comment, and add it below.
//! - a constructor from numbers or text, with no geometry input to lose a Z
//!   from. Same treatment.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Functions permitted to build a `Geom` with `has_zm: false`.
///
/// Grouped by why. Every entry is a deliberate 2D answer, not an oversight.
const ALLOWED: &[&str] = &[
    // The deliberate-2D encoders. Each module's doc comment records that
    // PostGIS answers 2D for its callers too.
    "out_2d",
    "encode_2d",
    "point_geom_2d",
    // geom.rs itself: the low-level encoders, and `encode_derived`'s own
    // no-Z-to-carry and empty-result branches.
    "encode_derived",
    // Reading a format that has no Z on the way in, or drops it by definition.
    "decode_wkt",
    "st_geom_from_geojson",
    "write_geometry",
    "st_geom_from_gml",
    // Constructors from numbers: no geometry input to lose a Z from.
    "st_point",
    "st_make_envelope",
    // Deliberate flatteners and 2D-by-definition outputs.
    "st_force_2d",
    "st_as_mvt_geom",
    "encode", // functions::surface — ST_PatchN and ST_Force2D of a surface
    // PostGIS answers 2D here too, each measured on 3.5.
    "st_centroid",
    "st_envelope",
    "st_project", // only its 2D branch; the 3D one asserts a Z
    "st_clip_by_box_2d",
];

/// Both spellings of the same claim: the struct-literal field and the
/// assignment. Missing the second one is how `ST_Force2D` hid from an earlier
/// draft of this test.
fn writes_false_has_zm(line: &str) -> bool {
    if line.trim_start().starts_with("//") {
        return false;
    }
    line.contains("has_zm: false") || line.contains("has_zm = false")
}

fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            source_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The nearest `fn name(` at or above `line` (0-based).
fn enclosing_fn(lines: &[&str], line: usize) -> String {
    for candidate in lines[..=line].iter().rev() {
        let trimmed = candidate.trim_start();
        let rest = trimmed
            .strip_prefix("pub fn ")
            .or_else(|| trimmed.strip_prefix("fn "))
            .or_else(|| trimmed.strip_prefix("pub(crate) fn "));
        if let Some(rest) = rest {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return name;
            }
        }
    }
    "<top level>".to_string()
}

#[test]
fn every_has_zm_false_is_an_allowed_deliberate_2d_answer() {
    let mut files = Vec::new();
    source_files(Path::new("src"), &mut files);
    files.sort();
    assert!(files.len() > 20, "expected to find the source tree");

    let allowed: BTreeSet<&str> = ALLOWED.iter().copied().collect();
    let mut offenders = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("read source");
        let lines: Vec<&str> = text.lines().collect();
        // The test module of each file is allowed anything: a fixture that
        // hand-builds a Geom is not a code path users reach.
        let tests_start = lines
            .iter()
            .position(|l| l.trim_start().starts_with("mod tests"))
            .unwrap_or(lines.len());
        for (i, line) in lines.iter().enumerate() {
            if i >= tests_start {
                break;
            }
            if !writes_false_has_zm(line) {
                continue;
            }
            let name = enclosing_fn(&lines, i);
            if !allowed.contains(name.as_str()) {
                offenders.push(format!("{}:{} in fn {name}", file.display(), i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "`has_zm: false` written outside the allowed set — see this file's \
         module comment for the three options:\n  {}",
        offenders.join("\n  ")
    );
}

/// The allowlist must not rot either: an entry that no longer matches anything
/// is a decision nobody is making any more, and reading it would mislead.
#[test]
fn the_allowlist_has_no_dead_entries() {
    let mut files = Vec::new();
    source_files(Path::new("src"), &mut files);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("read source");
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if writes_false_has_zm(line) {
                seen.insert(enclosing_fn(&lines, i));
            }
        }
    }
    let dead: Vec<&&str> = ALLOWED.iter().filter(|a| !seen.contains(**a)).collect();
    assert!(
        dead.is_empty(),
        "these allowlist entries no longer write `has_zm: false` — drop them: {dead:?}"
    );
}
