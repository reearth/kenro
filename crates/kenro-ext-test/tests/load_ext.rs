//! Loads the built kenro-ext cdylib into a bundled SQLite and exercises one
//! function from every module — through the LOADED extension, not linked
//! kenro. Build the artifact first: `cargo build -p kenro-ext --release`.

use std::path::PathBuf;

use rusqlite::{Connection, LoadExtensionGuard};

fn artifact() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KENRO_EXT_DYLIB") {
        return Some(p.into());
    }
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let name = format!(
        "{}kenro_ext{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    ["release", "debug"]
        .iter()
        .map(|profile| target.join(profile).join(&name))
        .find(|p| p.exists())
}

/// None = artifact not built (skipped locally; CI sets KENRO_EXT_REQUIRE=1
/// to turn the skip into a failure).
fn load(entry: Option<&str>) -> Option<Connection> {
    let Some(path) = artifact() else {
        if std::env::var_os("KENRO_EXT_REQUIRE").is_some() {
            panic!("kenro-ext cdylib missing; run `cargo build -p kenro-ext --release` first");
        }
        eprintln!("SKIP: kenro-ext cdylib not built (cargo build -p kenro-ext --release)");
        return None;
    };
    let conn = Connection::open_in_memory().unwrap();
    unsafe {
        let _guard = LoadExtensionGuard::new(&conn).unwrap();
        conn.load_extension(&path, entry).unwrap();
    }
    Some(conn)
}

fn query<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str) -> T {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn every_module_works_through_the_loaded_extension() {
    let Some(conn) = load(None) else { return };

    // io roundtrip.
    let wkt: String = query(&conn, "SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))");
    assert_eq!(wkt, "POINT(1 2)");

    // transform.
    let srid: i64 = query(
        &conn,
        "SELECT ST_SRID(ST_Transform(ST_GeomFromText('POINT(139.767 35.681)', 4326), 6677))",
    );
    assert_eq!(srid, 6677);
    let x: f64 = query(
        &conn,
        "SELECT ST_X(ST_Transform(ST_GeomFromText('POINT(139.767 35.681)', 4326), 6677))",
    );
    assert!((-10_000.0..0.0).contains(&x), "easting {x}");

    // h3.
    let cell: String = query(
        &conn,
        "SELECT h3_cell_to_string(h3_latlng_to_cell(ST_GeomFromText('POINT(139.767 35.681)'), 9))",
    );
    assert!(!cell.is_empty());

    // geojson.
    let gj: String = query(&conn, "SELECT ST_AsGeoJSON(ST_GeomFromText('POINT(1 2)'))");
    assert_eq!(gj, r#"{"type":"Point","coordinates":[1,2]}"#);

    // NULL-strictness.
    let is_null: i64 = query(&conn, "SELECT ST_AsText(NULL) IS NULL");
    assert_eq!(is_null, 1);

    // Stub error message.
    let err = conn
        .query_row(
            "SELECT ST_MakeValid(ST_GeomFromText('POINT(0 0)'))",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("not implemented in kenro"), "{err}");
}

#[test]
fn gpkg_rtree_triggers_run_under_trusted_schema_off() {
    let Some(conn) = load(None) else { return };
    conn.pragma_update(None, "trusted_schema", false).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE parks (fid INTEGER PRIMARY KEY, geom BLOB);
        CREATE VIRTUAL TABLE rtree_parks_geom USING rtree(id, minx, maxx, miny, maxy);
        CREATE TRIGGER rtree_parks_geom_insert AFTER INSERT ON parks
          WHEN (new.geom NOT NULL AND NOT ST_IsEmpty(NEW.geom))
        BEGIN
          INSERT OR REPLACE INTO rtree_parks_geom VALUES (
            NEW.fid,
            ST_MinX(NEW.geom), ST_MaxX(NEW.geom),
            ST_MinY(NEW.geom), ST_MaxY(NEW.geom)
          );
        END;
        INSERT INTO parks (fid, geom)
          VALUES (1, ST_AsGPB(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))', 4326)));
        "#,
    )
    .unwrap();
    let indexed: i64 = conn
        .query_row("SELECT count(*) FROM rtree_parks_geom", [], |r| r.get(0))
        .unwrap();
    assert_eq!(indexed, 1);
}

#[test]
fn explicit_entry_point_aliases_load() {
    for entry in ["sqlite3_kenroext_init", "sqlite3_kenro_init"] {
        let Some(conn) = load(Some(entry)) else {
            return;
        };
        let wkt: String = query(&conn, "SELECT ST_AsText(ST_GeomFromText('POINT(3 4)'))");
        assert_eq!(wkt, "POINT(3 4)", "{entry}");
    }
}
