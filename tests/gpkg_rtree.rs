//! The headline test: with kenro registered, a plain SQLite build maintains
//! a GeoPackage R-tree spatial index correctly, using the virtual table and
//! trigger DDL from the GeoPackage spec (Annex F.3, `gpkg_rtree_index`)
//! verbatim.

use rusqlite::{Connection, params};

/// The spec's rtree virtual table plus the six maintenance triggers, for
/// table `parks`, id column `fid`, geometry column `geom`.
const RTREE_DDL: &str = r#"
CREATE VIRTUAL TABLE rtree_parks_geom USING rtree(id, minx, maxx, miny, maxy);

/* Conditions: Insertion of non-empty geometry
   Actions   : Insert record into rtree */
CREATE TRIGGER rtree_parks_geom_insert AFTER INSERT ON parks
  WHEN (new.geom NOT NULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN
  INSERT OR REPLACE INTO rtree_parks_geom VALUES (
    NEW.fid,
    ST_MinX(NEW.geom), ST_MaxX(NEW.geom),
    ST_MinY(NEW.geom), ST_MaxY(NEW.geom)
  );
END;

/* Conditions: Update of geometry column to non-empty geometry
               No row ID change
   Actions   : Update record in rtree */
CREATE TRIGGER rtree_parks_geom_update1 AFTER UPDATE OF geom ON parks
  WHEN OLD.fid = NEW.fid AND
       (NEW.geom NOTNULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN
  INSERT OR REPLACE INTO rtree_parks_geom VALUES (
    NEW.fid,
    ST_MinX(NEW.geom), ST_MaxX(NEW.geom),
    ST_MinY(NEW.geom), ST_MaxY(NEW.geom)
  );
END;

/* Conditions: Update of geometry column to empty geometry
               No row ID change
   Actions   : Remove record from rtree */
CREATE TRIGGER rtree_parks_geom_update2 AFTER UPDATE OF geom ON parks
  WHEN OLD.fid = NEW.fid AND
       (NEW.geom ISNULL OR ST_IsEmpty(NEW.geom))
BEGIN
  DELETE FROM rtree_parks_geom WHERE id = OLD.fid;
END;

/* Conditions: Update of any column
               Row ID change
               Non-empty geometry
   Actions   : Remove record from rtree for old rowid
               Insert record into rtree for new rowid */
CREATE TRIGGER rtree_parks_geom_update3 AFTER UPDATE ON parks
  WHEN OLD.fid != NEW.fid AND
       (NEW.geom NOTNULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN
  DELETE FROM rtree_parks_geom WHERE id = OLD.fid;
  INSERT OR REPLACE INTO rtree_parks_geom VALUES (
    NEW.fid,
    ST_MinX(NEW.geom), ST_MaxX(NEW.geom),
    ST_MinY(NEW.geom), ST_MaxY(NEW.geom)
  );
END;

/* Conditions: Update of any column
               Row ID change
               Empty geometry
   Actions   : Remove record from rtree for old and new rowid */
CREATE TRIGGER rtree_parks_geom_update4 AFTER UPDATE ON parks
  WHEN OLD.fid != NEW.fid AND
       (NEW.geom ISNULL OR ST_IsEmpty(NEW.geom))
BEGIN
  DELETE FROM rtree_parks_geom WHERE id IN (OLD.fid, NEW.fid);
END;

/* Conditions: Row deleted
   Actions   : Remove record from rtree */
CREATE TRIGGER rtree_parks_geom_delete AFTER DELETE ON parks
  WHEN old.geom NOT NULL
BEGIN
  DELETE FROM rtree_parks_geom WHERE id = OLD.fid;
END;
"#;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn.pragma_update(None, "trusted_schema", false).unwrap();
    conn.execute_batch("CREATE TABLE parks (fid INTEGER PRIMARY KEY, name TEXT, geom BLOB);")
        .unwrap();
    conn.execute_batch(RTREE_DDL).unwrap();
    conn
}

fn insert(conn: &Connection, fid: i64, wkt: Option<&str>) {
    match wkt {
        Some(wkt) => conn.execute(
            "INSERT INTO parks (fid, name, geom) VALUES (?1, ?2, ST_AsGPB(ST_GeomFromText(?3, 4326)))",
            params![fid, format!("p{fid}"), wkt],
        ),
        None => conn.execute(
            "INSERT INTO parks (fid, name, geom) VALUES (?1, ?2, NULL)",
            params![fid, format!("p{fid}")],
        ),
    }
    .unwrap();
}

fn index_ids(conn: &Connection) -> Vec<i64> {
    let mut stmt = conn
        .prepare("SELECT id FROM rtree_parks_geom ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn index_bounds(conn: &Connection, fid: i64) -> (f64, f64, f64, f64) {
    conn.query_row(
        "SELECT minx, maxx, miny, maxy FROM rtree_parks_geom WHERE id = ?1",
        [fid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap()
}

#[test]
fn insert_maintains_index() {
    let conn = setup();
    insert(&conn, 1, Some("POINT(2 3)"));
    insert(&conn, 2, Some("POLYGON((0 0,10 0,10 10,0 10,0 0))"));
    insert(&conn, 3, Some("LINESTRING(-5 -5,5 5)"));
    insert(&conn, 4, Some("POLYGON EMPTY")); // empty: not indexed
    insert(&conn, 5, None); // NULL: not indexed

    assert_eq!(index_ids(&conn), vec![1, 2, 3]);
    assert_eq!(index_bounds(&conn, 1), (2.0, 2.0, 3.0, 3.0));
    assert_eq!(index_bounds(&conn, 2), (0.0, 10.0, 0.0, 10.0));
    assert_eq!(index_bounds(&conn, 3), (-5.0, 5.0, -5.0, 5.0));
}

#[test]
fn update_and_delete_track_the_index() {
    let conn = setup();
    insert(&conn, 1, Some("POINT(2 3)"));
    insert(&conn, 2, Some("POINT(4 5)"));
    insert(&conn, 3, Some("POINT(6 7)"));

    // Geometry update → bounds follow.
    conn.execute(
        "UPDATE parks SET geom = ST_AsGPB(ST_GeomFromText('POINT(100 200)', 4326)) WHERE fid = 1",
        [],
    )
    .unwrap();
    assert_eq!(index_bounds(&conn, 1), (100.0, 100.0, 200.0, 200.0));

    // Update to NULL → row leaves the index.
    conn.execute("UPDATE parks SET geom = NULL WHERE fid = 2", [])
        .unwrap();
    assert_eq!(index_ids(&conn), vec![1, 3]);

    // Row-id change → index follows the new id.
    conn.execute("UPDATE parks SET fid = 30 WHERE fid = 3", [])
        .unwrap();
    assert_eq!(index_ids(&conn), vec![1, 30]);

    // Delete → row leaves the index.
    conn.execute("DELETE FROM parks WHERE fid = 1", []).unwrap();
    assert_eq!(index_ids(&conn), vec![30]);
}

#[test]
fn headline_query_rtree_filter_plus_predicate_refine() {
    let conn = setup();
    // Three parks inside the query window, one straddling its edge, one far away.
    insert(&conn, 1, Some("POLYGON((1 1,2 1,2 2,1 2,1 1))"));
    insert(&conn, 2, Some("POLYGON((3 3,4 3,4 4,3 4,3 3))"));
    insert(&conn, 3, Some("POINT(5 5)"));
    insert(&conn, 4, Some("POLYGON((9 9,11 9,11 11,9 11,9 9))")); // bbox hits, geometry not within
    insert(
        &conn,
        5,
        Some("POLYGON((100 100,101 100,101 101,100 101,100 100))"),
    );

    let window = "POLYGON((0 0,10 0,10 10,0 10,0 0))";
    let mut stmt = conn
        .prepare(
            "SELECT p.fid FROM parks p
             JOIN rtree_parks_geom r ON p.fid = r.id
             WHERE r.minx <= 10 AND r.maxx >= 0 AND r.miny <= 10 AND r.maxy >= 0
               AND ST_Within(ST_GeomFromGPB(p.geom), ST_GeomFromText(?1, 4326))
             ORDER BY p.fid",
        )
        .unwrap();
    let fids: Vec<i64> = stmt
        .query_map([window], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(fids, vec![1, 2, 3]);
}

#[test]
fn geometry_type_and_srs_id_triggers() {
    // The GeoPackage geometry-type-trigger (extension F.4) and SRS-ID
    // trigger (F.5) patterns: GPKG_IsAssignable + ST_GeometryType +
    // ST_SRID guarding inserts against gpkg_geometry_columns. kenro's
    // GPKG_IsAssignable normalizes both the gpkg ('POLYGON') and PostGIS
    // ('ST_Polygon') spellings, so the spec pattern works with kenro's
    // PostGIS-style ST_GeometryType output.
    let conn = Connection::open_in_memory().unwrap();
    kenro::register(&conn).unwrap();
    conn.pragma_update(None, "trusted_schema", false).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE parks (fid INTEGER PRIMARY KEY, geom BLOB);
        CREATE TABLE gpkg_geometry_columns (
          table_name TEXT, column_name TEXT, geometry_type_name TEXT,
          srs_id INTEGER, z TINYINT, m TINYINT);
        INSERT INTO gpkg_geometry_columns VALUES ('parks','geom','POLYGON',4326,0,0);

        CREATE TRIGGER "fgti_parks_geom" BEFORE INSERT ON "parks"
        FOR EACH ROW
        BEGIN
          SELECT RAISE (ABORT, 'insert on table parks violates constraint: geometry type is not assignable')
          WHERE NEW.geom IS NOT NULL AND (
            SELECT GPKG_IsAssignable(geometry_type_name, ST_GeometryType(NEW.geom))
            FROM gpkg_geometry_columns
            WHERE Lower(table_name) = Lower('parks') AND Lower(column_name) = Lower('geom')) = 0;
        END;

        CREATE TRIGGER "fgsi_parks_geom" BEFORE INSERT ON "parks"
        FOR EACH ROW
        BEGIN
          SELECT RAISE (ABORT, 'insert on table parks violates constraint: srs_id does not match')
          WHERE NEW.geom IS NOT NULL AND (
            SELECT srs_id FROM gpkg_geometry_columns
            WHERE Lower(table_name) = Lower('parks') AND Lower(column_name) = Lower('geom'))
            <> ST_SRID(NEW.geom);
        END;
        "#,
    )
    .unwrap();

    // Matching type + srid inserts fine.
    conn.execute(
        "INSERT INTO parks (geom) VALUES (ST_AsGPB(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))', 4326)))",
        [],
    )
    .unwrap();
    // Wrong geometry type aborts.
    let err = conn
        .execute(
            "INSERT INTO parks (geom) VALUES (ST_AsGPB(ST_GeomFromText('POINT(1 2)', 4326)))",
            [],
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("geometry type"), "{err}");
    // Wrong SRID aborts.
    let err = conn
        .execute(
            "INSERT INTO parks (geom) VALUES (ST_AsGPB(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))', 6677)))",
            [],
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("srs_id"), "{err}");
    let count: i64 = conn
        .query_row("SELECT count(*) FROM parks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn externally_written_gpkg_interoperates() {
    // tests/fixtures/mini.gpkg is written by GDAL (ogr2ogr; generation
    // command in tests/fixtures/README.md): GDAL-written GPB headers carry
    // envelopes, exercising the header fast path, and the file ships GDAL's
    // own rtree triggers.
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mini.gpkg");
    let tmp = std::env::temp_dir().join(format!("kenro_rtree_{}.gpkg", std::process::id()));
    std::fs::copy(fixture, &tmp).unwrap();

    let conn = Connection::open(&tmp).unwrap();
    kenro::register(&conn).unwrap();
    conn.pragma_update(None, "trusted_schema", false).unwrap();

    // Reading GDAL-written GPB works, and the envelope fast path agrees
    // with WKB-computed values (ST_GeomFromGPB strips the envelope, forcing
    // the fallback).
    let (minx, minx_slow): (f64, f64) = conn
        .query_row(
            "SELECT ST_MinX(geom), ST_MinX(ST_GeomFromGPB(geom)) FROM parks LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(minx, minx_slow);

    // Inserting through SQL maintains GDAL's own rtree via its triggers.
    let before: i64 = conn
        .query_row("SELECT count(*) FROM rtree_parks_geom", [], |r| r.get(0))
        .unwrap();
    conn.execute(
        "INSERT INTO parks (name, geom) VALUES ('added', ST_AsGPB(ST_GeomFromText('POINT(139.7 35.7)', 4326)))",
        [],
    )
    .unwrap();
    let after: i64 = conn
        .query_row("SELECT count(*) FROM rtree_parks_geom", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, before + 1);

    // The new row is findable through the index + predicate.
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM parks p JOIN rtree_parks_geom r ON p.fid = r.id
             WHERE r.minx <= 140 AND r.maxx >= 139 AND r.miny <= 36 AND r.maxy >= 35
               AND ST_Within(ST_GeomFromGPB(p.geom),
                             ST_GeomFromText('POLYGON((139 35,140 35,140 36,139 36,139 35))', 4326))",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(n >= 1);

    drop(conn);
    let _ = std::fs::remove_file(&tmp);
}
