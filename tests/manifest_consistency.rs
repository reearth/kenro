//! The manifest (`kenro::functions::manifest`) must describe exactly what
//! `kenro::register` puts on a connection — this is what keeps the WASM
//! adapters, the loadable extension, and the rusqlite binding from
//! drifting apart.

use std::collections::BTreeSet;

use kenro::functions::manifest;
use rusqlite::Connection;

/// (lowercased name, narg) pairs currently registered on a connection.
fn function_set(conn: &Connection) -> BTreeSet<(String, i32)> {
    let mut stmt = conn
        .prepare("SELECT name, narg FROM pragma_function_list")
        .unwrap();
    stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?.to_lowercase(), r.get(1)?))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[test]
fn registration_matches_the_manifest_exactly() {
    let baseline = function_set(&Connection::open_in_memory().unwrap());
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    let registered: BTreeSet<(String, i32)> =
        function_set(&conn).difference(&baseline).cloned().collect();

    let mut expected = BTreeSet::new();
    for entry in manifest::active_functions() {
        expected.insert((entry.sql_name.to_lowercase(), entry.args.len() as i32));
    }
    for entry in manifest::active_aggregates() {
        expected.insert((entry.sql_name.to_lowercase(), entry.args.len() as i32));
    }
    for stub in manifest::active_stubs() {
        expected.insert((stub.name.to_lowercase(), -1));
    }

    let missing: Vec<_> = expected.difference(&registered).collect();
    let extra: Vec<_> = registered.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "manifest drift — missing from registration: {missing:?}; registered but not in manifest: {extra:?}"
    );
}

#[test]
fn every_stub_has_concrete_arities_for_non_variadic_hosts() {
    for stub in manifest::active_stubs() {
        let arities = manifest::stub_arities(stub.name);
        assert!(!arities.is_empty(), "{}", stub.name);
        assert!(
            arities.iter().all(|a| *a >= 0),
            "{}: concrete arities must be non-negative",
            stub.name
        );
    }
}
