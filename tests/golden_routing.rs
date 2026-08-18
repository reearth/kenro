//! Routing tests: SQL-level semantics of the routing aggregates.
//!
//! The golden vectors generated against pgRouting land here too (see
//! `scripts/golden/routing_generate.sh`).

use rusqlite::Connection;
use rusqlite::types::Value as SqlValue;

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
