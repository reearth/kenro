//! Measures kenro's (proj4rs-based) ST_Transform against the reference
//! PostGIS/PROJ lattice in scripts/accuracy/reference.jsonl and prints the
//! error table for docs/accuracy.md.
//!
//! `cargo run --example accuracy_report`            — print the table
//! `cargo run --example accuracy_report -- --check` — assert every pair
//! stays under its documented threshold (CI regression gate for proj4rs
//! upgrades; no Docker needed).

use std::collections::BTreeMap;

use geo_types::{Geometry, Point};

/// Documented maximum error per pair, in meters (docs/accuracy.md). The
/// `--check` mode fails if a measured max exceeds these.
const THRESHOLDS_M: &[(&str, f64)] = &[
    ("4326->3857", 1e-7),
    ("4326->32633", 1e-6),
    ("4326->32633_wide", 1e-6),
    ("4326->32756", 1e-6),
    ("32633->4326", 1e-6),
];

fn geographic(srid: i32) -> bool {
    matches!(srid, 4326)
}

struct Stats {
    errors: Vec<f64>,
}

impl Stats {
    fn max(&self) -> f64 {
        self.errors.iter().copied().fold(0.0, f64::max)
    }
    fn mean(&self) -> f64 {
        self.errors.iter().sum::<f64>() / self.errors.len() as f64
    }
    fn p99(&self) -> f64 {
        let mut sorted = self.errors.clone();
        sorted.sort_by(f64::total_cmp);
        sorted[((sorted.len() - 1) as f64 * 0.99) as usize]
    }
}

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/accuracy/reference.jsonl"
    );
    let content = std::fs::read_to_string(path).expect("run scripts/accuracy/generate.sh first");

    let mut provenance = String::from("unknown reference");
    let mut stats: BTreeMap<String, Stats> = BTreeMap::new();

    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line).expect(line);
        if let Some(g) = row.get("_generated_by") {
            provenance = g.as_str().unwrap().to_string();
            continue;
        }
        let pair = row["pair"].as_str().unwrap();
        let (src, dst) = parse_pair(pair);
        let (x, y) = (row["x"].as_f64().unwrap(), row["y"].as_f64().unwrap());
        let (ex, ey) = (row["ex"].as_f64().unwrap(), row["ey"].as_f64().unwrap());

        let mut g: Geometry<f64> = Geometry::Point(Point::new(x, y));
        kenro::crs::transform_geometry("accuracy", &mut g, src, dst)
            .unwrap_or_else(|e| panic!("{pair} ({x}, {y}): {e}"));
        let Geometry::Point(p) = g else {
            unreachable!()
        };

        let (dx_m, dy_m) = if geographic(dst) {
            let lat = ey.to_radians();
            (
                (p.x() - ex) * 111_320.0 * lat.cos(),
                (p.y() - ey) * 110_540.0,
            )
        } else {
            (p.x() - ex, p.y() - ey)
        };
        let err = (dx_m * dx_m + dy_m * dy_m).sqrt();
        stats
            .entry(pair.to_string())
            .or_insert(Stats { errors: vec![] })
            .errors
            .push(err);
    }

    println!("# Transform accuracy: kenro (proj4rs) vs {provenance}\n");
    println!("| pair | points | max (m) | mean (m) | p99 (m) |");
    println!("|---|---|---|---|---|");
    let mut failures = Vec::new();
    for (pair, s) in &stats {
        println!(
            "| {pair} | {} | {:.3e} | {:.3e} | {:.3e} |",
            s.errors.len(),
            s.max(),
            s.mean(),
            s.p99()
        );
        if check {
            let threshold = THRESHOLDS_M
                .iter()
                .find(|(p, _)| p == pair)
                .unwrap_or_else(|| panic!("no threshold documented for pair {pair}"))
                .1;
            if s.max() > threshold {
                failures.push(format!(
                    "{pair}: max error {:.3e} m exceeds documented {threshold:.3e} m",
                    s.max()
                ));
            }
        }
    }
    if check {
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
        println!("\nall pairs within documented thresholds ✓");
    }
}

fn parse_pair(pair: &str) -> (i32, i32) {
    let (src, rest) = pair.split_once("->").expect(pair);
    let dst: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (src.parse().expect(pair), dst.parse().expect(pair))
}
