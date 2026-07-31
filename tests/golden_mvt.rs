//! Golden tests for the MVT functions vs the reference PostGIS.
//!
//! `asmvtgeom` vectors compare tile-space geometries with a ±1 integer
//! coordinate tolerance (PostGIS clips after grid snapping, kenro clips in
//! world space before it — corner pixels can differ by one). `asmvt`
//! vectors decode both tiles (PostGIS's via the independent python
//! `mapbox-vector-tile`, kenro's via `tests/common/mvt_decode.rs`) and
//! compare the normalized JSON with the same coordinate tolerance.

mod common;

use common::Vector;
use kenro::functions::{io, mvt};
use rusqlite::Connection;
use rusqlite::types::Value as SqlValue;
use serde_json::Value;

// ---- asmvtgeom comparison: type + vertex multiset within ±1 ----

fn wkt_coords(s: &str) -> Vec<(f64, f64)> {
    let nums: Vec<f64> = s
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    let mut pairs: Vec<(f64, f64)> = nums
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0], c[1]))
        .collect();
    pairs.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
    // Ring-closing duplicates land on different vertices depending on where
    // the ring starts; drop exact duplicates so they cannot misalign the
    // sorted pairing.
    pairs.dedup();
    pairs
}

fn mvt_geom_close(id: &str, got: &str, want: &str) {
    if got == want {
        return;
    }
    let type_of = |s: &str| s.split(['(', ' ']).next().unwrap_or("").to_string();
    assert_eq!(type_of(got), type_of(want), "{id}: got {got}, want {want}");
    let (cg, cw) = (wkt_coords(got), wkt_coords(want));
    assert_eq!(cg.len(), cw.len(), "{id}: got {got}, want {want}");
    for (p, q) in cg.iter().zip(&cw) {
        assert!(
            (p.0 - q.0).abs() <= 1.0 && (p.1 - q.1).abs() <= 1.0,
            "{id}: coordinate {p:?} vs {q:?}: got {got}, want {want}"
        );
    }
}

fn geom_args(v: &Vector) -> (Option<i32>, Option<i32>, Option<i32>) {
    let args = v.args.clone().unwrap_or_default();
    let get = |i: usize| args.get(i).map(|f| *f as i32);
    (get(0), get(1), get(2))
}

/// `dirty_*` vectors need the `full` tier's validity repair; the standard
/// tier (rect clipping, no repair) skips them by design.
fn skipped(v: &Vector) -> bool {
    v.id.starts_with("dirty_") && !cfg!(feature = "overlay")
}

fn check_asmvtgeom(v: &Vector, got: Option<String>) {
    match (got, v.effective()) {
        (None, Value::Null) => {}
        (Some(g), Value::String(w)) => mvt_geom_close(&v.id, &g, w),
        (got, want) => panic!("{}: got {got:?}, want {want}", v.id),
    }
}

// ---- asmvt comparison: normalized decoded tiles ----

/// Drop a ring's closing duplicate and rotate it to start at its smallest
/// vertex, so ring start (which PostGIS and kenro choose differently) does
/// not affect comparison.
fn canonical_ring(ring: &Value) -> Vec<Vec<f64>> {
    let mut pts: Vec<Vec<f64>> = ring
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            p.as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_f64().unwrap())
                .collect()
        })
        .collect();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    let min = (0..pts.len())
        .min_by(|&i, &j| {
            pts[i]
                .partial_cmp(&pts[j])
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    pts.rotate_left(min);
    pts
}

fn nums_close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn rings_close(id: &str, a: &Value, b: &Value) {
    let (ra, rb) = (canonical_ring(a), canonical_ring(b));
    assert_eq!(ra.len(), rb.len(), "{id}: ring length {ra:?} vs {rb:?}");
    for (p, q) in ra.iter().zip(&rb) {
        assert!(
            nums_close(p[0], q[0], 1.0) && nums_close(p[1], q[1], 1.0),
            "{id}: ring vertex {p:?} vs {q:?}"
        );
    }
}

fn coords_close(id: &str, ty: &str, got: &Value, want: &Value) {
    match ty {
        "Point" => {
            let (g, w) = (got.as_array().unwrap(), want.as_array().unwrap());
            assert!(
                nums_close(g[0].as_f64().unwrap(), w[0].as_f64().unwrap(), 1.0)
                    && nums_close(g[1].as_f64().unwrap(), w[1].as_f64().unwrap(), 1.0),
                "{id}: point {got} vs {want}"
            );
        }
        "MultiPoint" | "LineString" => {
            let (g, w) = (got.as_array().unwrap(), want.as_array().unwrap());
            assert_eq!(g.len(), w.len(), "{id}: {ty} length");
            for (p, q) in g.iter().zip(w) {
                coords_close(id, "Point", p, q);
            }
        }
        "MultiLineString" | "Polygon" => {
            let (g, w) = (got.as_array().unwrap(), want.as_array().unwrap());
            assert_eq!(g.len(), w.len(), "{id}: {ty} member count");
            for (p, q) in g.iter().zip(w) {
                if ty == "Polygon" {
                    rings_close(id, p, q);
                } else {
                    coords_close(id, "LineString", p, q);
                }
            }
        }
        "MultiPolygon" => {
            let (g, w) = (got.as_array().unwrap(), want.as_array().unwrap());
            assert_eq!(g.len(), w.len(), "{id}: polygon count");
            for (p, q) in g.iter().zip(w) {
                coords_close(id, "Polygon", p, q);
            }
        }
        other => panic!("{id}: unknown geometry type {other}"),
    }
}

fn props_equal(id: &str, got: &Value, want: &Value) {
    let (g, w) = (got.as_object().unwrap(), want.as_object().unwrap());
    assert_eq!(
        g.keys().collect::<Vec<_>>(),
        w.keys().collect::<Vec<_>>(),
        "{id}: property keys"
    );
    for (k, wv) in w {
        let gv = &g[k];
        let ok = match (gv.as_f64(), wv.as_f64()) {
            (Some(a), Some(b)) => nums_close(a, b, 1e-9),
            _ => gv == wv,
        };
        assert!(ok, "{id}: property {k}: got {gv}, want {wv}");
    }
}

fn tiles_close(id: &str, got: &Value, want: &Value) {
    assert_eq!(got["name"], want["name"], "{id}: layer name");
    assert_eq!(
        got["extent"].as_u64(),
        want["extent"].as_u64(),
        "{id}: extent"
    );
    let (gf, wf) = (
        got["features"].as_array().unwrap(),
        want["features"].as_array().unwrap(),
    );
    assert_eq!(gf.len(), wf.len(), "{id}: feature count");
    for (i, (g, w)) in gf.iter().zip(wf).enumerate() {
        let fid = format!("{id}[{i}]");
        assert_eq!(g["type"], w["type"], "{fid}: geometry type");
        props_equal(&fid, &g["properties"], &w["properties"]);
        coords_close(
            &fid,
            w["type"].as_str().unwrap(),
            &g["coordinates"],
            &w["coordinates"],
        );
    }
}

fn asmvt_rows(v: &Vector) -> Vec<(String, Option<String>)> {
    v.rows
        .as_ref()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let row = row.as_array().unwrap();
            let props = match &row[1] {
                Value::Null => None,
                obj => Some(obj.to_string()),
            };
            (row[0].as_str().unwrap().to_string(), props)
        })
        .collect()
}

// ---- harnesses ----

#[test]
fn golden_mvt_through_pure_functions() {
    for v in common::load("mvt") {
        if skipped(&v) {
            continue;
        }
        match v.func.as_str() {
            "asmvtgeom" => {
                let a = io::st_geom_from_text(v.a.as_ref().unwrap(), None).unwrap();
                let b = io::st_geom_from_text(v.b.as_ref().unwrap(), None).unwrap();
                let (extent, buffer, clip) = geom_args(&v);
                let result = mvt::st_as_mvt_geom(&a, &b, extent, buffer, clip);
                if v.expects_error() {
                    assert!(result.is_err(), "{}: expected an error", v.id);
                    continue;
                }
                let got = result
                    .unwrap_or_else(|e| panic!("{}: {e}", v.id))
                    .map(|blob| io::st_as_text(&blob).unwrap());
                check_asmvtgeom(&v, got);
            }
            "asmvt" => {
                let bounds = io::st_geom_from_text(v.b.as_ref().unwrap(), None).unwrap();
                let name = v.arg_text.as_ref().unwrap();
                let extent = v.arg.unwrap() as i32;
                let mut acc = mvt::MvtAggregate::new();
                for (wkt, props) in asmvt_rows(&v) {
                    let geom = io::st_geom_from_text(&wkt, None).unwrap();
                    let Some(tile_geom) =
                        mvt::st_as_mvt_geom(&geom, &bounds, Some(extent), Some(0), None).unwrap()
                    else {
                        continue; // clipped away, like PostGIS's NULL-row skip
                    };
                    acc.step(&tile_geom, Some(name), Some(extent), props.as_deref())
                        .unwrap_or_else(|e| panic!("{}: {e}", v.id));
                }
                let tile = acc
                    .finish()
                    .unwrap_or_else(|e| panic!("{}: {e}", v.id))
                    .unwrap_or_else(|| panic!("{}: aggregate returned NULL", v.id));
                tiles_close(
                    &v.id,
                    &common::mvt_decode::decode_tile(&tile),
                    v.effective(),
                );
            }
            other => panic!("{}: unknown fn {other}", v.id),
        }
    }
}

#[test]
fn golden_mvt_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    for v in common::load("mvt") {
        if skipped(&v) {
            continue;
        }
        match v.func.as_str() {
            "asmvtgeom" => {
                let (extent, buffer, clip) = geom_args(&v);
                let mut call = vec!["ST_GeomFromText(?1)".into(), "ST_GeomFromText(?2)".into()];
                for opt in [extent, buffer, clip].into_iter().flatten() {
                    call.push(opt.to_string());
                }
                let sql = format!("SELECT ST_AsMVTGeom({})", call.join(", "));
                let result: rusqlite::Result<SqlValue> =
                    conn.query_row(&sql, [v.a.as_ref().unwrap(), v.b.as_ref().unwrap()], |r| {
                        r.get(0)
                    });
                if v.expects_error() {
                    assert!(result.is_err(), "{}: expected an error", v.id);
                    continue;
                }
                let got = match result.unwrap_or_else(|e| panic!("{}: {e}", v.id)) {
                    SqlValue::Null => None,
                    SqlValue::Blob(b) => Some(io::st_as_text(&b).unwrap()),
                    other => panic!("{}: unexpected SQL value {other:?}", v.id),
                };
                check_asmvtgeom(&v, got);
            }
            "asmvt" => {
                let name = v.arg_text.as_ref().unwrap();
                let extent = v.arg.unwrap() as i32;
                conn.execute_batch(
                    "DROP TABLE IF EXISTS mvt_rows; CREATE TABLE mvt_rows (wkt TEXT, props TEXT);",
                )
                .unwrap();
                for (wkt, props) in asmvt_rows(&v) {
                    conn.execute(
                        "INSERT INTO mvt_rows VALUES (?1, ?2)",
                        rusqlite::params![wkt, props.unwrap_or_else(|| "{}".into())],
                    )
                    .unwrap();
                }
                let sql = format!(
                    "SELECT ST_AsMVT(ST_AsMVTGeom(ST_GeomFromText(wkt), ST_GeomFromText(?1), \
                     {extent}, 0), ?2, {extent}, props) FROM mvt_rows"
                );
                let tile: Vec<u8> = conn
                    .query_row(&sql, rusqlite::params![v.b.as_ref().unwrap(), name], |r| {
                        r.get(0)
                    })
                    .unwrap_or_else(|e| panic!("{}: {e}", v.id));
                tiles_close(
                    &v.id,
                    &common::mvt_decode::decode_tile(&tile),
                    v.effective(),
                );
            }
            other => panic!("{}: unknown fn {other}", v.id),
        }
    }
}
