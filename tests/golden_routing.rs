//! Routing tests: the golden vectors generated against real pgRouting (see
//! `scripts/golden/routing_generate.sh`), plus the SQL-level semantics that
//! have no pgRouting counterpart — NULL rows, zero rows, GROUP BY, and the
//! constant-argument rule.
//!
//! Every vector is self-contained: `rows` is the whole edge table, `args` is
//! `[start_vid, end_vid]`, and `expected` is what pgRouting answered (an
//! empty result set becomes `null`, which is the aggregate's NULL). Vectors
//! run twice, once against the accumulator directly and once through SQL.
//!
//! `seq`/`node`/`edge` are compared exactly; `cost`/`agg_cost` go through
//! `common::assert_number`, because both sides accumulate the same additions
//! in the same order but neither promises the same last bit.

mod common;

use common::Vector;
use kenro::functions::routing::{DijkstraAggregate, DijkstraCostAggregate};
use rusqlite::Connection;
use rusqlite::types::Value as SqlValue;
use serde_json::Value;

// ---- vector plumbing ----

/// `[id, source, target, cost]` or `[id, source, target, cost, reverse_cost]`
/// — four elements means the 6-argument call form, five the 7-argument one.
struct EdgeRow {
    id: i32,
    source: i32,
    target: i32,
    cost: f64,
    reverse_cost: Option<f64>,
}

fn edge_rows(v: &Vector) -> Vec<EdgeRow> {
    v.rows
        .as_ref()
        .unwrap_or_else(|| panic!("{}: no rows", v.id))
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let r = r.as_array().unwrap();
            let num = |i: usize| r[i].as_f64().unwrap();
            EdgeRow {
                id: num(0) as i32,
                source: num(1) as i32,
                target: num(2) as i32,
                cost: num(3),
                reverse_cost: r.get(4).map(|c| c.as_f64().unwrap()),
            }
        })
        .collect()
}

fn endpoints(v: &Vector) -> (i32, i32) {
    let a = v
        .args
        .as_ref()
        .unwrap_or_else(|| panic!("{}: no args", v.id));
    (a[0] as i32, a[1] as i32)
}

fn has_reverse(rows: &[EdgeRow]) -> bool {
    rows.first().is_some_and(|r| r.reverse_cost.is_some())
}

/// Compare a produced path against the pgRouting rows the vector recorded.
fn check_path(id: &str, got: Option<&str>, want: &Value) {
    match (got, want) {
        (None, Value::Null) => {}
        (Some(g), Value::Array(rows)) => {
            let got: Value = serde_json::from_str(g).unwrap();
            let got = got.as_array().unwrap();
            assert_eq!(got.len(), rows.len(), "{id}: row count: {g} vs {want}");
            for (i, (g, w)) in got.iter().zip(rows).enumerate() {
                for key in ["seq", "node", "edge"] {
                    assert_eq!(
                        g[key].as_i64(),
                        w[key].as_i64(),
                        "{id}[{i}]: {key}: {g} vs {w}"
                    );
                }
                for key in ["cost", "agg_cost"] {
                    common::assert_number(
                        &format!("{id}[{i}].{key}"),
                        g[key].as_f64().unwrap(),
                        w[key].as_f64().unwrap(),
                    );
                }
            }
        }
        (got, want) => panic!("{id}: got {got:?}, want {want}"),
    }
}

fn check_cost(id: &str, got: Option<f64>, want: &Value) {
    match (got, want) {
        (None, Value::Null) => {}
        (Some(g), w) if w.is_number() => common::assert_number(id, g, w.as_f64().unwrap()),
        (got, want) => panic!("{id}: got {got:?}, want {want}"),
    }
}

// ---- harnesses ----

#[test]
fn golden_routing_through_pure_functions() {
    for v in common::load("routing") {
        let rows = edge_rows(&v);
        let (start, end) = endpoints(&v);
        match v.func.as_str() {
            "dijkstra" => {
                let mut acc = DijkstraAggregate::new();
                for r in &rows {
                    acc.step(r.id, r.source, r.target, r.cost, start, end, r.reverse_cost)
                        .unwrap_or_else(|e| panic!("{}: {e}", v.id));
                }
                let got = acc.finish().unwrap_or_else(|e| panic!("{}: {e}", v.id));
                check_path(&v.id, got.as_deref(), v.effective());
            }
            "dijkstra_cost" => {
                let mut acc = DijkstraCostAggregate::new();
                for r in &rows {
                    acc.step(r.source, r.target, r.cost, start, end, r.reverse_cost)
                        .unwrap_or_else(|e| panic!("{}: {e}", v.id));
                }
                let got = acc.finish().unwrap_or_else(|e| panic!("{}: {e}", v.id));
                check_cost(&v.id, got, v.effective());
            }
            other => panic!("{}: unknown fn {other}", v.id),
        }
    }
}

#[test]
fn golden_routing_through_sql() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn.execute_batch(
        "CREATE TABLE e (id INTEGER, source INTEGER, target INTEGER, cost REAL, rcost REAL);",
    )
    .unwrap();
    for v in common::load("routing") {
        let rows = edge_rows(&v);
        let (start, end) = endpoints(&v);
        conn.execute("DELETE FROM e", []).unwrap();
        for r in &rows {
            conn.execute(
                "INSERT INTO e VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![r.id, r.source, r.target, r.cost, r.reverse_cost],
            )
            .unwrap();
        }
        let rev = if has_reverse(&rows) { ", rcost" } else { "" };
        match v.func.as_str() {
            "dijkstra" => {
                let sql = format!(
                    "SELECT kenro_dijkstra(id, source, target, cost, {start}, {end}{rev}) FROM e"
                );
                let got: Option<String> = conn
                    .query_row(&sql, [], |r| r.get(0))
                    .unwrap_or_else(|e| panic!("{}: {e}", v.id));
                check_path(&v.id, got.as_deref(), v.effective());
            }
            "dijkstra_cost" => {
                let sql = format!(
                    "SELECT kenro_dijkstra_cost(source, target, cost, {start}, {end}{rev}) FROM e"
                );
                let got: Option<f64> = conn
                    .query_row(&sql, [], |r| r.get(0))
                    .unwrap_or_else(|e| panic!("{}: {e}", v.id));
                check_cost(&v.id, got, v.effective());
            }
            other => panic!("{}: unknown fn {other}", v.id),
        }
    }
}

// ---- semantics with no pgRouting counterpart ----

fn conn_with_edges(rows: &[(Option<i32>, i32, i32, Option<f64>)]) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn.execute_batch(
        "CREATE TABLE e (id INTEGER, source INTEGER, target INTEGER, cost REAL, grp INTEGER);",
    )
    .unwrap();
    for (id, source, target, cost) in rows {
        conn.execute(
            "INSERT INTO e (id, source, target, cost, grp) VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![id, source, target, cost],
        )
        .unwrap();
    }
    conn
}

fn nodes(json: &str) -> Vec<i64> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|r| r["node"].as_i64().unwrap())
        .collect()
}

#[test]
fn a_null_in_any_column_skips_the_row() {
    // The 1→2→3 shortcut is spelled with a NULL id, so it must not exist as
    // far as the aggregate is concerned: the answer is the long way round.
    let conn = conn_with_edges(&[
        (Some(1), 1, 2, Some(1.1)),
        (None, 2, 3, Some(0.2)),
        (Some(3), 2, 4, Some(0.7)),
        (Some(4), 4, 3, Some(0.9)),
    ]);
    let path: String = conn
        .query_row(
            "SELECT kenro_dijkstra(id, source, target, cost, 1, 3) FROM e",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(nodes(&path), vec![1, 2, 4, 3]);
}

#[test]
fn zero_rows_are_null() {
    let conn = conn_with_edges(&[]);
    let got: SqlValue = conn
        .query_row(
            "SELECT kenro_dijkstra(id, source, target, cost, 1, 3) FROM e",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(got, SqlValue::Null);
    let got: SqlValue = conn
        .query_row(
            "SELECT kenro_dijkstra_cost(source, target, cost, 1, 3) FROM e",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(got, SqlValue::Null);
}

#[test]
fn a_where_clause_is_the_edge_query() {
    let conn = conn_with_edges(&[
        (Some(1), 1, 2, Some(1.1)),
        (Some(2), 2, 3, Some(0.7)),
        (Some(3), 1, 3, Some(3.3)),
    ]);
    // Restricting the rows restricts the graph, exactly as pgRouting's SQL
    // string argument would.
    let path: String = conn
        .query_row(
            "SELECT kenro_dijkstra(id, source, target, cost, 1, 3) FROM e WHERE id <> 2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(nodes(&path), vec![1, 3]);
}

#[test]
fn group_by_routes_each_group_separately() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn.execute_batch(
        "CREATE TABLE e (id INTEGER, source INTEGER, target INTEGER, cost REAL, grp INTEGER);
         INSERT INTO e VALUES (1, 1, 2, 1.1, 10), (2, 2, 3, 0.7, 10),
                              (3, 1, 3, 4.2, 20);",
    )
    .unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT grp, kenro_dijkstra_cost(source, target, cost, 1, 3) FROM e \
             GROUP BY grp ORDER BY grp",
        )
        .unwrap();
    let got: Vec<(i64, f64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].0, 10);
    assert!((got[0].1 - 1.8).abs() < 1e-12, "{:?}", got[0]);
    assert_eq!(got[1], (20, 4.2));
}

#[test]
fn endpoints_must_be_constant_within_a_group() {
    let conn = conn_with_edges(&[(Some(1), 1, 2, Some(1.1)), (Some(2), 2, 3, Some(0.7))]);
    let err = conn
        .query_row(
            "SELECT kenro_dijkstra(id, source, target, cost, 1, id + 2) FROM e",
            [],
            |r| r.get::<_, SqlValue>(0),
        )
        .unwrap_err();
    assert!(err.to_string().contains("constant"), "{err}");
}

#[test]
fn a_non_finite_cost_errors() {
    let conn = conn_with_edges(&[(Some(1), 1, 2, Some(f64::INFINITY))]);
    let err = conn
        .query_row(
            "SELECT kenro_dijkstra(id, source, target, cost, 1, 2) FROM e",
            [],
            |r| r.get::<_, SqlValue>(0),
        )
        .unwrap_err();
    assert!(err.to_string().contains("finite"), "{err}");
}

#[test]
fn the_seven_argument_form_carries_reverse_cost() {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn.execute_batch(
        "CREATE TABLE e (id INTEGER, source INTEGER, target INTEGER, cost REAL, rcost REAL);
         INSERT INTO e VALUES (1, 1, 2, 1.1, 5.5);",
    )
    .unwrap();
    let forward: f64 = conn
        .query_row(
            "SELECT kenro_dijkstra_cost(source, target, cost, 1, 2, rcost) FROM e",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let backward: f64 = conn
        .query_row(
            "SELECT kenro_dijkstra_cost(source, target, cost, 2, 1, rcost) FROM e",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((forward - 1.1).abs() < 1e-12);
    assert!((backward - 5.5).abs() < 1e-12);
    // Without the reverse_cost argument the edge is one-way.
    let none: SqlValue = conn
        .query_row(
            "SELECT kenro_dijkstra_cost(source, target, cost, 2, 1) FROM e",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(none, SqlValue::Null);
}

#[test]
fn json_each_expands_the_path_into_rows() {
    // The documented recipe in docs/routing.md.
    let conn = conn_with_edges(&[
        (Some(1), 1, 2, Some(1.1)),
        (Some(2), 2, 3, Some(0.7)),
        (Some(3), 3, 4, Some(2.9)),
    ]);
    let mut stmt = conn
        .prepare(
            "WITH p(j) AS (SELECT kenro_dijkstra(id, source, target, cost, 1, 4) FROM e) \
             SELECT json_extract(value, '$.seq'), json_extract(value, '$.node'), \
                    json_extract(value, '$.edge') \
             FROM p, json_each(p.j) ORDER BY 1",
        )
        .unwrap();
    let rows: Vec<(i64, i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows, vec![(1, 1, 1), (2, 2, 2), (3, 3, 3), (4, 4, -1)]);
}

#[test]
fn the_documented_topology_recipe_builds_a_routable_edge_table() {
    // docs/routing.md's pgr_createTopology replacement, verbatim enough that
    // a rename of any function it uses fails here rather than in a reader's
    // terminal. Three roads: 0→1→2 in two hops, or 0→2 in one dear one.
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn.execute_batch(
        "CREATE TABLE roads (id INTEGER, geom BLOB);
         INSERT INTO roads VALUES
           (1, ST_GeomFromText('LINESTRING(0 0,1 0)')),
           (2, ST_GeomFromText('LINESTRING(1 0,3 0)')),
           (3, ST_GeomFromText('LINESTRING(0 0,3 0)'));
         CREATE TEMP TABLE ends AS
         SELECT id, 'start' AS which,
                ST_AsBinary(ST_SnapToGrid(ST_StartPoint(geom), 0.001)) AS pt
         FROM roads
         UNION ALL
         SELECT id, 'end', ST_AsBinary(ST_SnapToGrid(ST_EndPoint(geom), 0.001))
         FROM roads;
         CREATE TEMP TABLE vertices AS
         SELECT pt, DENSE_RANK() OVER (ORDER BY pt) AS vid
         FROM (SELECT DISTINCT pt FROM ends);
         CREATE TABLE edges AS
         SELECT r.id, vs.vid AS source, ve.vid AS target, ST_Length(r.geom) AS cost
         FROM roads r
         JOIN ends es ON es.id = r.id AND es.which = 'start'
         JOIN ends ee ON ee.id = r.id AND ee.which = 'end'
         JOIN vertices vs ON vs.pt = es.pt
         JOIN vertices ve ON ve.pt = ee.pt;",
    )
    .unwrap();
    // Three lines, three distinct endpoints, so three vertices.
    let n: i64 = conn
        .query_row("SELECT count(*) FROM vertices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 3);
    // The two-hop route and the direct one cost the same 3.0 here, so ask
    // for the cost rather than a path — see the tie note in docs/routing.md.
    let (start, end): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT vid FROM vertices WHERE pt = ST_AsBinary(ST_GeomFromText('POINT(0 0)'))),
                    (SELECT vid FROM vertices WHERE pt = ST_AsBinary(ST_GeomFromText('POINT(3 0)')))",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let sql =
        format!("SELECT kenro_dijkstra_cost(source, target, cost, {start}, {end}) FROM edges");
    let cost: f64 = conn.query_row(&sql, [], |r| r.get(0)).unwrap();
    assert!((cost - 3.0).abs() < 1e-12, "{cost}");
}
