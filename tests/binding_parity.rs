//! The two wasm bindings must call the same kenro function for the same SQL
//! name.
//!
//! `kenro-abi` (Go, Cloudflare) and `kenro-wasm` (browser) are written by hand
//! from the same manifest, so nothing structural stopped them drifting. One had:
//!
//! ```text
//! k_stCoordDim  ->  accessors::st_coord_dim   // a stale `Ok(2)`
//! stCoordDim    ->  threed::st_coord_dim      // reads the encoding
//! ```
//!
//! `ST_NDims` therefore answered 2 for a genuine 3D geometry on every Go and
//! Cloudflare host, while rusqlite and the browser answered 3. Both bindings
//! registered, every smoke case passed, and the manifest agreed with itself —
//! the mismatch was one identifier deep and nothing was looking.
//!
//! The comparison is textual: the first `module::function(` in each export's
//! body, which is what those bodies consist of. Exports that reach for a local
//! helper have no pair to compare and are skipped, so this is a drift detector
//! rather than a proof. Hand-rolled rather than regex — a dev-dependency for one
//! test is a poor trade in a crate this deliberate about its dependency list.

use std::collections::BTreeMap;

/// The first `module::function(` in `body`, ignoring plumbing modules that some
/// bodies touch before the call that matters.
fn forwarded_call(body: &str) -> Option<String> {
    const PLUMBING: &[&str] = &["std", "core", "kenro", "gpb", "geom", "api", "manifest"];
    let bytes = body.as_bytes();
    let mut i = 0;
    while let Some(found) = body[i..].find("::") {
        let sep = i + found;
        // Walk back over the module identifier.
        let mut start = sep;
        while start > 0 && {
            let c = bytes[start - 1];
            c.is_ascii_alphanumeric() || c == b'_'
        } {
            start -= 1;
        }
        // Walk forward over the function identifier, which must be followed by
        // an opening paren for this to be a call rather than a path.
        let mut end = sep + 2;
        while end < bytes.len() && {
            let c = bytes[end];
            c.is_ascii_alphanumeric() || c == b'_'
        } {
            end += 1;
        }
        let module = &body[start..sep];
        let function = &body[sep + 2..end];
        let is_call = bytes.get(end) == Some(&b'(');
        let lowercase = |s: &str| {
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        if is_call
            && !module.is_empty()
            && !function.is_empty()
            && lowercase(module)
            && lowercase(function)
            && !PLUMBING.contains(&module)
        {
            return Some(format!("{module}::{function}"));
        }
        i = sep + 2;
    }
    None
}

/// Scan `source` for exports, given how one is introduced and how its signature
/// ends. Returns `export name -> module::function`.
fn forwards(source: &str, marker: &Marker) -> BTreeMap<String, String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(name) = marker.export_name(&lines, i) else {
            i += 1;
            continue;
        };
        // The body runs from the line after the signature's `{` to the first
        // line that is exactly `}` at column zero.
        let mut j = i;
        while j < lines.len() && !lines[j].ends_with('{') {
            j += 1;
        }
        let mut body = String::new();
        let mut k = j + 1;
        while k < lines.len() && lines[k] != "}" {
            body.push_str(lines[k]);
            body.push('\n');
            k += 1;
        }
        if let Some(call) = forwarded_call(&body) {
            out.insert(name, call);
        }
        i = k.max(i + 1);
    }
    out
}

enum Marker {
    /// `pub extern "C" fn k_<name>(`
    Abi,
    /// `#[wasm_bindgen(js_name = <name>)]`
    Wasm,
}

impl Marker {
    fn export_name(&self, lines: &[&str], i: usize) -> Option<String> {
        let line = lines[i];
        match self {
            Marker::Abi => {
                let rest = line.strip_prefix("pub extern \"C\" fn k_")?;
                Some(rest.split('(').next()?.to_string())
            }
            Marker::Wasm => {
                let rest = line
                    .trim_start()
                    .strip_prefix("#[wasm_bindgen(js_name = ")?
                    .strip_suffix(")]")?;
                Some(rest.to_string())
            }
        }
    }
}

#[test]
fn abi_and_wasm_exports_call_the_same_implementation() {
    let abi_source =
        std::fs::read_to_string("crates/kenro-abi/src/lib.rs").expect("read kenro-abi");
    let wasm_source =
        std::fs::read_to_string("crates/kenro-wasm/src/lib.rs").expect("read kenro-wasm");

    let abi = forwards(&abi_source, &Marker::Abi);
    let wasm = forwards(&wasm_source, &Marker::Wasm);

    assert!(
        abi.len() > 150 && wasm.len() > 150,
        "the scanners stopped matching ({} abi, {} wasm) — fix them rather than \
         letting this test pass vacuously",
        abi.len(),
        wasm.len()
    );

    let mismatches: Vec<String> = abi
        .iter()
        .filter_map(|(name, abi_impl)| {
            let wasm_impl = wasm.get(name)?;
            (abi_impl != wasm_impl).then(|| {
                format!("{name}: kenro-abi calls {abi_impl}, kenro-wasm calls {wasm_impl}")
            })
        })
        .collect();

    assert!(
        mismatches.is_empty(),
        "the two wasm bindings disagree about which function implements a name. \
         One of them is stale:\n  {}",
        mismatches.join("\n  ")
    );
}
