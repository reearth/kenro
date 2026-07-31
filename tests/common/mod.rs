//! Shared helpers for the golden-test harnesses.
#![allow(dead_code)]

pub mod mvt_decode;

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
    pub mode: Option<String>,
    /// MVT aggregate input rows: `[[wkt, props|null], ...]`.
    #[serde(default)]
    pub rows: Option<serde_json::Value>,
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

/// Rotation/direction-insensitive comparison for convex hulls and oriented
/// envelopes: same type prefix and the same coordinate multiset. Valid
/// because a convex ring is determined by its vertex set.
pub fn geoms_same_vertex_set(a: &str, b: &str, tol: f64) -> bool {
    if a == b {
        return true;
    }
    let type_of = |s: &str| s.split(['(', ' ']).next().unwrap_or("").to_string();
    if type_of(a) != type_of(b) {
        return false;
    }
    let coords = |s: &str| {
        let mut pairs: Vec<(f64, f64)> = Vec::new();
        let nums: Vec<f64> = s
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
            .filter(|t| !t.is_empty())
            .filter_map(|t| t.parse().ok())
            .collect();
        for chunk in nums.chunks(2) {
            if chunk.len() == 2 {
                pairs.push((chunk[0], chunk[1]));
            }
        }
        pairs.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
        pairs.dedup_by(|p, q| (p.0 - q.0).abs() <= tol && (p.1 - q.1).abs() <= tol);
        pairs
    };
    let (ca, cb) = (coords(a), coords(b));
    ca.len() == cb.len()
        && ca.iter().zip(&cb).all(|(p, q)| {
            let t = |v: f64| tol * v.abs().max(1.0);
            (p.0 - q.0).abs() <= t(q.0) && (p.1 - q.1).abs() <= t(q.1)
        })
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
