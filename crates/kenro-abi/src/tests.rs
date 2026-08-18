//! Native-target tests for the C ABI itself. The wasm artifact is checked
//! from the other side too: the Go binding looks up every export named by the
//! manifest, so a manifest entry with no thunk fails there.

use super::*;

use std::sync::{Mutex, MutexGuard};

/// The ABI's OUT buffer and return slots are process-wide statics — correct
/// for a single-threaded wasm instance, but Rust's test harness is
/// multi-threaded, so tests take turns.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn out() -> Vec<u8> {
    OUT.get().clone()
}

fn out_str() -> String {
    String::from_utf8(out()).unwrap()
}

fn wkt_to_geom(wkt: &str) -> Vec<u8> {
    assert_eq!(k_stGeomFromText(wkt.as_ptr(), wkt.len() as u32), OK);
    out()
}

#[test]
fn roundtrips_wkt_through_the_abi() {
    let _g = serial();
    let geom = wkt_to_geom("POINT(1 2)");
    assert_eq!(k_stAsText(geom.as_ptr(), geom.len() as u32), OK);
    assert_eq!(out_str(), "POINT(1 2)");
}

#[test]
fn scalar_returns_land_in_the_right_slot() {
    let _g = serial();
    let poly = wkt_to_geom("POLYGON((0 0,2 0,2 3,0 3,0 0))");
    assert_eq!(k_stArea(poly.as_ptr(), poly.len() as u32), OK);
    assert_eq!(kenro_ret_f64(), 6.0);

    assert_eq!(k_stNPoints(poly.as_ptr(), poly.len() as u32), OK);
    assert_eq!(kenro_ret_i64(), 5);

    let pt = wkt_to_geom("POINT(1 2)");
    assert_eq!(
        k_stIntersects(
            poly.as_ptr(),
            poly.len() as u32,
            pt.as_ptr(),
            pt.len() as u32
        ),
        OK
    );
    assert_eq!(kenro_ret_i64(), 1);
}

#[test]
fn null_result_is_its_own_status() {
    let _g = serial();
    // ST_NumPoints is NULL for anything that is not a LINESTRING.
    let poly = wkt_to_geom("POLYGON((0 0,2 0,2 3,0 3,0 0))");
    assert_eq!(k_stNumPoints(poly.as_ptr(), poly.len() as u32), NULL);

    let line = wkt_to_geom("LINESTRING(0 0,1 1,2 2)");
    assert_eq!(k_stNumPoints(line.as_ptr(), line.len() as u32), OK);
    assert_eq!(kenro_ret_i64(), 3);

    let pt = wkt_to_geom("POINT(1 2)");
    assert_eq!(k_stX(pt.as_ptr(), pt.len() as u32), OK);
    assert_eq!(kenro_ret_f64(), 1.0);
}

#[test]
fn errors_carry_the_kenro_prefixed_message() {
    let _g = serial();
    let bad = "NOT A GEOMETRY";
    assert_eq!(k_stGeomFromText(bad.as_ptr(), bad.len() as u32), ERR);
    assert!(out_str().starts_with("kenro: "), "got {:?}", out_str());
}

#[test]
fn invalid_utf8_text_argument_fails_loudly() {
    let _g = serial();
    let bytes = [0xffu8, 0xfe];
    assert_eq!(k_stGeomFromText(bytes.as_ptr(), 2), ERR);
    assert!(out_str().contains("invalid UTF-8"), "got {:?}", out_str());
}

#[test]
fn alloc_free_roundtrip() {
    let _g = serial();
    let p = kenro_alloc(64);
    assert!(!p.is_null());
    kenro_free(p, 64);
}

#[cfg(feature = "overlay")]
#[test]
fn union_aggregate_handle_lifecycle() {
    let _g = serial();
    let h = k_agg_new(AGG_UNION);
    assert!(h >= 0);
    let a = wkt_to_geom("POLYGON((0 0,2 0,2 2,0 2,0 0))");
    let b = wkt_to_geom("POLYGON((1 0,3 0,3 2,1 2,1 0))");
    assert_eq!(k_agg_union_step(h, a.as_ptr(), a.len() as u32), OK);
    assert_eq!(k_agg_union_step(h, b.as_ptr(), b.len() as u32), OK);
    assert_eq!(k_agg_finish(h), OK);
    let unioned = out();
    assert_eq!(k_stArea(unioned.as_ptr(), unioned.len() as u32), OK);
    assert_eq!(kenro_ret_f64(), 6.0);

    // The handle is released, and reusing it is a loud error, not a panic.
    assert_eq!(k_agg_finish(h), ERR);
    assert!(out_str().contains("already finished"));
}

#[cfg(feature = "overlay")]
#[test]
fn aggregate_handles_are_recycled() {
    let _g = serial();
    let h = k_agg_new(AGG_UNION);
    k_agg_drop(h);
    assert_eq!(k_agg_new(AGG_UNION), h, "a dropped slot should be reused");
    k_agg_drop(h);
}

#[cfg(feature = "routing")]
#[test]
fn dijkstra_aggregate_handle_lifecycle() {
    let _g = serial();
    let h = k_agg_new(AGG_DIJKSTRA);
    assert!(h >= 0);
    // 1 →(1.1) 2 →(0.7) 3, no reverse_cost: the 6-argument call form.
    assert_eq!(k_agg_dijkstra_step(h, 10, 1, 2, 1.1, 1, 3, 0, 0.0), OK);
    assert_eq!(k_agg_dijkstra_step(h, 11, 2, 3, 0.7, 1, 3, 0, 0.0), OK);
    assert_eq!(k_agg_finish(h), OK);
    let path = out_str();
    assert!(path.contains("\"edge\":10"), "{path}");
    assert!(path.contains("\"agg_cost\":1.8"), "{path}");
    assert_eq!(k_agg_finish(h), ERR);
    assert!(out_str().contains("already finished"));

    // The cost twin, this time with a reverse_cost present: 3 → 1 the long
    // way back, which only exists because has_rev is set.
    let h = k_agg_new(AGG_DIJKSTRA_COST);
    assert_eq!(k_agg_dijkstra_cost_step(h, 1, 2, 1.1, 3, 1, 1, 2.5), OK);
    assert_eq!(k_agg_dijkstra_cost_step(h, 2, 3, 0.7, 3, 1, 1, 4.0), OK);
    assert_eq!(k_agg_finish(h), OK);
    assert_eq!(kenro_ret_f64(), 6.5);

    // No path, and no rows at all, are both SQL NULL.
    let h = k_agg_new(AGG_DIJKSTRA);
    assert_eq!(k_agg_dijkstra_step(h, 10, 1, 2, 1.1, 1, 9, 0, 0.0), OK);
    assert_eq!(k_agg_finish(h), NULL);
    let h = k_agg_new(AGG_DIJKSTRA_COST);
    assert_eq!(k_agg_finish(h), NULL);
}

#[test]
fn zero_row_aggregate_is_null() {
    let _g = serial();
    #[cfg(feature = "overlay")]
    {
        let h = k_agg_new(AGG_UNION);
        assert_eq!(k_agg_finish(h), NULL);
    }
}

#[test]
fn manifest_json_covers_every_active_function() {
    let _g = serial();
    assert_eq!(k_manifest(), OK);
    let json = out_str();
    for e in manifest::active_functions() {
        let needle = format!("\"export\":\"k_{}\"", e.export);
        assert!(json.contains(&needle), "manifest JSON is missing {needle}");
    }
    for e in manifest::active_aggregates() {
        let needle = format!("\"sql_name\":\"{}\"", e.sql_name);
        assert!(json.contains(&needle), "manifest JSON is missing {needle}");
    }
    // Feature-gated entries must not leak in when the feature is off.
    #[cfg(not(feature = "overlay"))]
    assert!(!json.contains("k_stIntersection"));
}

#[test]
fn manifest_json_escapes_stub_hints() {
    let _g = serial();
    assert_eq!(k_manifest(), OK);
    let json = out_str();
    // Stub hints are prose and may contain quotes; the hand-rolled encoder
    // has to survive that.
    assert!(json.contains("\"stubs\":["));
    assert!(!json.contains("\n"), "raw newlines would break the JSON");
}
