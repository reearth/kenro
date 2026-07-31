//! Shared helpers for the golden-test harnesses.
#![allow(dead_code)]

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Vector {
    pub id: String,
    #[serde(default)]
    pub a: Option<String>,
    #[serde(default)]
    pub b: Option<String>,
    #[serde(rename = "fn")]
    pub func: String,
    #[serde(default)]
    pub arg: Option<f64>,
    #[serde(default)]
    pub arg_text: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<f64>>,
    #[serde(default)]
    pub cell: Option<i64>,
    #[serde(default)]
    pub src_srid: Option<i32>,
    #[serde(default)]
    pub to_srid: Option<i32>,
    #[serde(default)]
    pub srid: Option<i32>,
    #[serde(default)]
    pub expected_srid: Option<i32>,
    pub expected: serde_json::Value,
    #[serde(default)]
    pub kenro_expected: Option<serde_json::Value>,
    #[serde(default)]
    pub note: Option<String>,
}

impl Vector {
    /// The value kenro must produce: the documented-divergence override if
    /// present, otherwise the reference value.
    pub fn effective(&self) -> &serde_json::Value {
        self.kenro_expected.as_ref().unwrap_or(&self.expected)
    }

    pub fn expects_error(&self) -> bool {
        self.effective().get("error").is_some()
    }
}

pub fn load(suite: &str) -> Vec<Vector> {
    let path = format!("{}/tests/golden/{suite}.jsonl", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e} — run the generator in scripts/golden/"));
    let mut vectors = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line).expect(line);
        if value.get("fn").is_none() {
            continue; // provenance header record
        }
        vectors.push(serde_json::from_value(value).expect(line));
    }
    assert!(!vectors.is_empty(), "{suite}: no vectors");
    vectors
}

pub fn assert_number(id: &str, got: f64, want: f64) {
    let tol = 1e-12 * want.abs().max(1.0);
    assert!(
        (got - want).abs() <= tol,
        "{id}: got {got}, want {want} (tolerance {tol})"
    );
}

/// Geometric comparison of two WKT strings with a per-coordinate tolerance
/// (scaled by magnitude). Identical strings short-circuit, which also covers
/// `POINT EMPTY` (unparseable by kenro's WKT reader by design).
pub fn geoms_approx_equal(a: &str, b: &str, tol: f64) -> bool {
    use geo::CoordsIter;
    if a == b {
        return true;
    }
    let (Ok(ga), Ok(gb)) = (kenro::geom::decode_wkt(a, 0), kenro::geom::decode_wkt(b, 0)) else {
        return false;
    };
    if std::mem::discriminant(&ga.geometry) != std::mem::discriminant(&gb.geometry) {
        return false;
    }
    let ca: Vec<_> = ga.geometry.coords_iter().collect();
    let cb: Vec<_> = gb.geometry.coords_iter().collect();
    ca.len() == cb.len()
        && ca.iter().zip(&cb).all(|(p, q)| {
            let t = |v: f64| tol * v.abs().max(1.0);
            (p.x - q.x).abs() <= t(q.x) && (p.y - q.y).abs() <= t(q.y)
        })
}
