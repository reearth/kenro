//! A real GeoPackage with a POLYHEDRALSURFACE column, built by SQLite with
//! kenro registered and nothing else.
//!
//! `tests/gpkg_rtree.rs` proves the R-tree triggers work for the core
//! geometry types. This file is the same proof for the extended type kenro
//! reads but cannot decode — the one whose legality depends on a row in
//! `gpkg_extensions` (Annex F.1), which is exactly what
//! `kenro_gpkg_extension_required` names and deliberately does not write.
//!
//! Three things get pinned here:
//!
//! 1. **The declared file works.** With `geometry_type_name =
//!    'POLYHEDRALSURFACE'` and the `gpkg_geom_POLYHEDRALSURFACE` extension
//!    row present, inserts/updates/deletes maintain the R-tree, and the
//!    2D bbox the index carries is the surface's patch extent.
//! 2. **The undeclared file behaves as documented.** kenro does *not*
//!    enforce Annex F.1 — a reader that refused would be refusing data GDAL
//!    happily writes. `kenro_gpkg_extension_required` reports what is
//!    missing; the test asserts that report, not an enforcement kenro does
//!    not have.
//! 3. **GDAL agrees the declared file is a GeoPackage.** That check needs
//!    Docker, which CI never has, so it lives in
//!    `scripts/gpkg-ogrinfo-check.sh` next to the golden harness. Set
//!    `KENRO_GPKG_OUT` to a directory and these tests write both fixtures
//!    there for the script to feed `ogrinfo`; the measured verdict is
//!    recorded in the script, including the correction it forced — GDAL
//!    3.11 validates the `gpkg_extensions` *name* and is silent about a
//!    wrong `definition` URL or a missing row entirely, which is the
//!    opposite of what the design note assumed and the reason point 2 above
//!    is the right contract.

use rusqlite::{Connection, params};

/// GeoPackage 1.2 `application_id` ("GPKG") and `user_version` (10200).
const APPLICATION_ID: i32 = 0x4750_4B47;
const USER_VERSION: i32 = 10200;

/// The extension row Annex F.1 requires for an extended geometry type, and
/// the one the R-tree needs. The `definition` string is the part a
/// hand-written file gets wrong — `scripts/gpkg-ogrinfo-check.sh` is what
/// catches that, because GDAL is the thing that reads it.
const EXTENSION_DEFINITION: &str = "http://www.geopackage.org/spec120/#extension_geometry_types";
const RTREE_DEFINITION: &str = "http://www.geopackage.org/spec120/#extension_rtree";

/// ISO WKB for a POLYHEDRALSURFACE Z, little-endian: type 15 + 1000.
fn polyhedral_surface(patches: &[&[[f64; 3]]]) -> Vec<u8> {
    let mut out = vec![0x01u8];
    out.extend_from_slice(&1015u32.to_le_bytes());
    out.extend_from_slice(&(patches.len() as u32).to_le_bytes());
    for patch in patches {
        out.push(0x01);
        out.extend_from_slice(&1003u32.to_le_bytes()); // POLYGON Z
        out.extend_from_slice(&1u32.to_le_bytes()); // one ring
        out.extend_from_slice(&(patch.len() as u32).to_le_bytes());
        for c in *patch {
            for v in c {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

/// A closed unit box translated to `(x, y)` and standing `height` tall:
/// six patches, so the 2D footprint the R-tree indexes is
/// `[x, x+1] × [y, y+1]` however the Z moves.
fn building(x: f64, y: f64, height: f64) -> Vec<u8> {
    let (x1, y1, z1) = (x + 1.0, y + 1.0, height);
    let ring = |c: [[f64; 3]; 4]| vec![c[0], c[1], c[2], c[3], c[0]];
    let patches = [
        ring([[x, y, 0.], [x, y1, 0.], [x1, y1, 0.], [x1, y, 0.]]),
        ring([[x, y, z1], [x1, y, z1], [x1, y1, z1], [x, y1, z1]]),
        ring([[x, y, 0.], [x1, y, 0.], [x1, y, z1], [x, y, z1]]),
        ring([[x1, y, 0.], [x1, y1, 0.], [x1, y1, z1], [x1, y, z1]]),
        ring([[x1, y1, 0.], [x, y1, 0.], [x, y1, z1], [x1, y1, z1]]),
        ring([[x, y1, 0.], [x, y, 0.], [x, y, z1], [x, y1, z1]]),
    ];
    let refs: Vec<&[[f64; 3]]> = patches.iter().map(|p| p.as_slice()).collect();
    polyhedral_surface(&refs)
}

/// The GeoPackage core tables, verbatim from the spec's table definitions
/// (the same shapes GDAL writes — compare `tests/fixtures/mini.gpkg`).
const CORE_DDL: &str = r#"
CREATE TABLE gpkg_spatial_ref_sys (
  srs_name TEXT NOT NULL, srs_id INTEGER NOT NULL PRIMARY KEY,
  organization TEXT NOT NULL, organization_coordsys_id INTEGER NOT NULL,
  definition TEXT NOT NULL, description TEXT);
CREATE TABLE gpkg_contents (
  table_name TEXT NOT NULL PRIMARY KEY, data_type TEXT NOT NULL,
  identifier TEXT UNIQUE, description TEXT DEFAULT '',
  last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE, srs_id INTEGER,
  CONSTRAINT fk_gc_r_srs_id FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id));
CREATE TABLE gpkg_geometry_columns (
  table_name TEXT NOT NULL, column_name TEXT NOT NULL,
  geometry_type_name TEXT NOT NULL, srs_id INTEGER NOT NULL,
  z TINYINT NOT NULL, m TINYINT NOT NULL,
  CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name),
  CONSTRAINT uk_gc_table_name UNIQUE (table_name),
  CONSTRAINT fk_gc_tn FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name),
  CONSTRAINT fk_gc_srs FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys (srs_id));
CREATE TABLE gpkg_extensions (
  table_name TEXT, column_name TEXT, extension_name TEXT NOT NULL,
  definition TEXT NOT NULL, scope TEXT NOT NULL,
  CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name));

INSERT INTO gpkg_spatial_ref_sys VALUES
  ('Undefined cartesian SRS', -1, 'NONE', -1, 'undefined', 'undefined cartesian coordinate reference system'),
  ('Undefined geographic SRS', 0, 'NONE', 0, 'undefined', 'undefined geographic coordinate reference system'),
  ('WGS 84 geodetic', 4326, 'EPSG', 4326,
   'GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]]',
   'longitude/latitude coordinates in decimal degrees on the WGS 84 spheroid');

CREATE TABLE buildings (fid INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, geom GEOMETRY, name TEXT);
INSERT INTO gpkg_contents (table_name, data_type, identifier, srs_id, min_x, min_y, max_x, max_y)
  VALUES ('buildings', 'features', 'buildings', 4326, 0, 0, 11, 11);
INSERT INTO gpkg_geometry_columns VALUES ('buildings', 'geom', 'POLYHEDRALSURFACE', 4326, 1, 0);
"#;

/// Annex F.3's R-tree virtual table and the six maintenance triggers, spelled
/// for `buildings`/`fid`/`geom`. They call `ST_IsEmpty`, `ST_MinX`, `ST_MaxX`,
/// `ST_MinY` and `ST_MaxY` — the five functions that have to answer for a
/// surface collection rather than raising, or a surface column is unindexable.
const RTREE_DDL: &str = r#"
CREATE VIRTUAL TABLE rtree_buildings_geom USING rtree(id, minx, maxx, miny, maxy);

CREATE TRIGGER rtree_buildings_geom_insert AFTER INSERT ON buildings
  WHEN (NEW.geom NOT NULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN
  INSERT OR REPLACE INTO rtree_buildings_geom VALUES (
    NEW.fid, ST_MinX(NEW.geom), ST_MaxX(NEW.geom), ST_MinY(NEW.geom), ST_MaxY(NEW.geom));
END;

CREATE TRIGGER rtree_buildings_geom_update1 AFTER UPDATE OF geom ON buildings
  WHEN OLD.fid = NEW.fid AND (NEW.geom NOTNULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN
  INSERT OR REPLACE INTO rtree_buildings_geom VALUES (
    NEW.fid, ST_MinX(NEW.geom), ST_MaxX(NEW.geom), ST_MinY(NEW.geom), ST_MaxY(NEW.geom));
END;

CREATE TRIGGER rtree_buildings_geom_update2 AFTER UPDATE OF geom ON buildings
  WHEN OLD.fid = NEW.fid AND (NEW.geom ISNULL OR ST_IsEmpty(NEW.geom))
BEGIN
  DELETE FROM rtree_buildings_geom WHERE id = OLD.fid;
END;

CREATE TRIGGER rtree_buildings_geom_update3 AFTER UPDATE ON buildings
  WHEN OLD.fid != NEW.fid AND (NEW.geom NOTNULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN
  DELETE FROM rtree_buildings_geom WHERE id = OLD.fid;
  INSERT OR REPLACE INTO rtree_buildings_geom VALUES (
    NEW.fid, ST_MinX(NEW.geom), ST_MaxX(NEW.geom), ST_MinY(NEW.geom), ST_MaxY(NEW.geom));
END;

CREATE TRIGGER rtree_buildings_geom_update4 AFTER UPDATE ON buildings
  WHEN OLD.fid != NEW.fid AND (NEW.geom ISNULL OR ST_IsEmpty(NEW.geom))
BEGIN
  DELETE FROM rtree_buildings_geom WHERE id IN (OLD.fid, NEW.fid);
END;

CREATE TRIGGER rtree_buildings_geom_delete AFTER DELETE ON buildings
  WHEN OLD.geom NOT NULL
BEGIN
  DELETE FROM rtree_buildings_geom WHERE id = OLD.fid;
END;
"#;

/// Build the file. `declare_extension` is the whole experiment: with it the
/// file conforms to Annex F.1, without it the POLYHEDRALSURFACE column is
/// undeclared and `kenro_gpkg_extension_required` has something to report.
fn build(path: &std::path::Path, declare_extension: bool) -> Connection {
    let _ = std::fs::remove_file(path);
    let conn = Connection::open(path).unwrap();
    kenro::register(&conn).unwrap();
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    conn.pragma_update(None, "user_version", USER_VERSION)
        .unwrap();
    conn.execute_batch(CORE_DDL).unwrap();
    conn.execute(
        "INSERT INTO gpkg_extensions VALUES ('buildings','geom','gpkg_rtree_index',?1,'write-only')",
        [RTREE_DEFINITION],
    )
    .unwrap();
    if declare_extension {
        conn.execute(
            "INSERT INTO gpkg_extensions VALUES \
             ('buildings','geom','gpkg_geom_POLYHEDRALSURFACE',?1,'read-write')",
            [EXTENSION_DEFINITION],
        )
        .unwrap();
    }
    conn.execute_batch(RTREE_DDL).unwrap();
    // trusted_schema off is the hostile setting the triggers must survive,
    // and the one `tests/gpkg_rtree.rs` uses.
    conn.pragma_update(None, "trusted_schema", false).unwrap();
    conn
}

fn insert(conn: &Connection, name: &str, geom: &[u8]) {
    conn.execute(
        "INSERT INTO buildings (name, geom) VALUES (?1, ST_AsGPB(ST_SetSRID(?2, 4326)))",
        params![name, geom],
    )
    .unwrap();
}

fn tmp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("kenro_ps_{}_{tag}.gpkg", std::process::id()))
}

fn index_rows(conn: &Connection) -> Vec<(i64, f64, f64, f64, f64)> {
    let mut stmt = conn
        .prepare("SELECT id, minx, maxx, miny, maxy FROM rtree_buildings_geom ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[test]
fn a_declared_polyhedralsurface_column_indexes_and_round_trips() {
    let path = tmp_path("declared");
    let conn = build(&path, true);

    insert(&conn, "a", &building(0.0, 0.0, 12.0));
    insert(&conn, "b", &building(5.0, 5.0, 30.0));
    insert(&conn, "c", &building(10.0, 10.0, 3.0));

    // The R-tree carries the 2D footprint, walked from the patches — the Z
    // is not in it, and must not be.
    assert_eq!(
        index_rows(&conn),
        vec![
            (1, 0.0, 1.0, 0.0, 1.0),
            (2, 5.0, 6.0, 5.0, 6.0),
            (3, 10.0, 11.0, 10.0, 11.0),
        ]
    );

    // The surface survives storage: still a POLYHEDRALSURFACE, still six
    // patches, still carrying its heights.
    let (kind, patches, zmin, zmax): (String, i64, f64, f64) = conn
        .query_row(
            "SELECT ST_GeometryType(geom), ST_NumPatches(geom), ST_ZMin(geom), ST_ZMax(geom) \
             FROM buildings WHERE name = 'b'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        (kind.as_str(), patches, zmin, zmax),
        ("ST_PolyhedralSurface", 6, 0.0, 30.0)
    );

    // The declaration in the file matches what kenro says is required.
    let required: String = conn
        .query_row(
            "SELECT kenro_gpkg_extension_required(geom) FROM buildings LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(required, "gpkg_geom_POLYHEDRALSURFACE");
    let declared: i64 = conn
        .query_row(
            "SELECT count(*) FROM gpkg_extensions
             WHERE table_name = 'buildings' AND column_name = 'geom'
               AND extension_name = ?1 AND definition = ?2 AND scope = 'read-write'",
            params![required, EXTENSION_DEFINITION],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(declared, 1);

    // gpkg_geometry_columns keeps the extended type name — nothing kenro
    // does rewrites it to a core one.
    let (type_name, z): (String, i64) = conn
        .query_row(
            "SELECT geometry_type_name, z FROM gpkg_geometry_columns WHERE table_name='buildings'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((type_name.as_str(), z), ("POLYHEDRALSURFACE", 1));

    export_for_ogrinfo(conn, &path, "declared.gpkg");
}

/// `scripts/gpkg-ogrinfo-check.sh` sets `KENRO_GPKG_OUT` to a directory and
/// then feeds what lands there to GDAL. Without it the fixtures are
/// temporary files these tests clean up.
fn export_for_ogrinfo(conn: Connection, path: &std::path::Path, name: &str) {
    let out = std::env::var("KENRO_GPKG_OUT").ok();
    drop(conn);
    if let Some(dir) = out {
        std::fs::copy(path, std::path::Path::new(&dir).join(name)).unwrap();
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn updates_and_deletes_track_the_index_for_surfaces() {
    let path = tmp_path("update");
    let conn = build(&path, true);

    insert(&conn, "a", &building(0.0, 0.0, 12.0));
    insert(&conn, "b", &building(5.0, 5.0, 30.0));
    insert(&conn, "c", &building(10.0, 10.0, 3.0));

    // Move a building: the footprint follows, the height does not leak in.
    conn.execute(
        "UPDATE buildings SET geom = ST_AsGPB(ST_SetSRID(?1, 4326)) WHERE name = 'a'",
        params![building(100.0, 200.0, 999.0)],
    )
    .unwrap();
    assert_eq!(index_rows(&conn)[0], (1, 100.0, 101.0, 200.0, 201.0));

    // NULL the geometry: the row leaves the index (update2).
    conn.execute("UPDATE buildings SET geom = NULL WHERE name = 'b'", [])
        .unwrap();
    assert_eq!(
        index_rows(&conn).iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![1, 3]
    );

    // Change the row id: the index follows it (update3).
    conn.execute("UPDATE buildings SET fid = 33 WHERE fid = 3", [])
        .unwrap();
    assert_eq!(
        index_rows(&conn).iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![1, 33]
    );

    // Delete: the row leaves.
    conn.execute("DELETE FROM buildings WHERE fid = 1", [])
        .unwrap();
    assert_eq!(
        index_rows(&conn).iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![33]
    );

    // And the index still answers a bbox query joined back to the table.
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM buildings b JOIN rtree_buildings_geom r ON b.fid = r.id
             WHERE r.minx <= 11 AND r.maxx >= 10 AND r.miny <= 11 AND r.maxy >= 10",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);

    drop(conn);
    let _ = std::fs::remove_file(&path);
}

/// The same file **without** the `gpkg_extensions` row.
///
/// kenro does not enforce Annex F.1 and this test does not ask it to: a
/// reader that refused an undeclared surface column would refuse files GDAL
/// writes. What the docs promise is that the requirement is *detectable*, so
/// that is what is asserted — `kenro_gpkg_extension_required` names the row,
/// and comparing its answer against `gpkg_extensions` is the one-query
/// conformance check a caller can run for themselves.
#[test]
fn an_undeclared_surface_column_still_works_and_is_detectable() {
    let path = tmp_path("undeclared");
    let conn = build(&path, false);
    insert(&conn, "a", &building(0.0, 0.0, 12.0));

    // Everything still works. This is the deliberate part.
    assert_eq!(index_rows(&conn), vec![(1, 0.0, 1.0, 0.0, 1.0)]);
    let patches: i64 = conn
        .query_row("SELECT ST_NumPatches(geom) FROM buildings", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(patches, 6);

    // And the gap is reportable, in one query, without kenro writing a thing.
    let missing: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT kenro_gpkg_extension_required(geom) AS required
                 FROM buildings
                 WHERE required IS NOT NULL
                   AND required NOT IN (SELECT extension_name FROM gpkg_extensions
                                        WHERE table_name='buildings' AND column_name='geom')",
            )
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(missing, vec!["gpkg_geom_POLYHEDRALSURFACE".to_string()]);

    // A core-typed geometry needs nothing, so it never shows up as missing.
    let none: Option<String> = conn
        .query_row(
            "SELECT kenro_gpkg_extension_required(ST_GeomFromText('POINT(1 2)'))",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(none, None);

    export_for_ogrinfo(conn, &path, "undeclared.gpkg");
}
