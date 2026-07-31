package kenro

import (
	"database/sql"
	"fmt"
	"testing"
)

// The GeoPackage spec's rtree virtual table plus the six maintenance
// triggers (Annex F.3, `gpkg_rtree_index`), verbatim, for table `parks`.
// modernc.org/sqlite is compiled with SQLITE_ENABLE_RTREE, so the virtual
// table is real — the only missing piece is the ST_ functions, which is what
// kenro supplies.
const rtreeDDL = `
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

CREATE TRIGGER rtree_parks_geom_update2 AFTER UPDATE OF geom ON parks
  WHEN OLD.fid = NEW.fid AND
       (NEW.geom ISNULL OR ST_IsEmpty(NEW.geom))
BEGIN
  DELETE FROM rtree_parks_geom WHERE id = OLD.fid;
END;

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

CREATE TRIGGER rtree_parks_geom_update4 AFTER UPDATE ON parks
  WHEN OLD.fid != NEW.fid AND
       (NEW.geom ISNULL OR ST_IsEmpty(NEW.geom))
BEGIN
  DELETE FROM rtree_parks_geom WHERE id IN (OLD.fid, NEW.fid);
END;

CREATE TRIGGER rtree_parks_geom_delete AFTER DELETE ON parks
  WHEN old.geom NOT NULL
BEGIN
  DELETE FROM rtree_parks_geom WHERE id = OLD.fid;
END;
`

func setupGPKG(t *testing.T) *sql.DB {
	t.Helper()
	db := open(t)
	// A pooled connection would run the DDL and the inserts on different
	// connections; the R-tree needs the same one for the temp-file-free
	// in-memory database anyway.
	db.SetMaxOpenConns(1)
	if _, err := db.Exec(`CREATE TABLE parks (fid INTEGER PRIMARY KEY, name TEXT, geom BLOB);`); err != nil {
		t.Fatalf("create table: %v", err)
	}
	if _, err := db.Exec(rtreeDDL); err != nil {
		t.Fatalf("rtree DDL: %v", err)
	}
	return db
}

func insertPark(t *testing.T, db *sql.DB, fid int, wkt any) {
	t.Helper()
	var err error
	if wkt == nil {
		_, err = db.Exec(`INSERT INTO parks (fid, name, geom) VALUES (?, ?, NULL)`, fid, fmt.Sprintf("p%d", fid))
	} else {
		_, err = db.Exec(
			`INSERT INTO parks (fid, name, geom) VALUES (?, ?, ST_AsGPB(ST_GeomFromText(?, 4326)))`,
			fid, fmt.Sprintf("p%d", fid), wkt)
	}
	if err != nil {
		t.Fatalf("insert %d: %v", fid, err)
	}
}

func indexIDs(t *testing.T, db *sql.DB) []int {
	t.Helper()
	rows, err := db.Query(`SELECT id FROM rtree_parks_geom ORDER BY id`)
	if err != nil {
		t.Fatalf("scan index: %v", err)
	}
	defer rows.Close()
	var ids []int
	for rows.Next() {
		var id int
		if err := rows.Scan(&id); err != nil {
			t.Fatalf("scan: %v", err)
		}
		ids = append(ids, id)
	}
	return ids
}

func indexBounds(t *testing.T, db *sql.DB, fid int) [4]float64 {
	t.Helper()
	var b [4]float64
	err := db.QueryRow(`SELECT minx, maxx, miny, maxy FROM rtree_parks_geom WHERE id = ?`, fid).
		Scan(&b[0], &b[1], &b[2], &b[3])
	if err != nil {
		t.Fatalf("bounds %d: %v", fid, err)
	}
	return b
}

// The headline claim, on the pure-Go stack: with kenro registered, a plain
// modernc.org/sqlite build maintains a GeoPackage spatial index correctly.
func TestGeoPackageRtreeInsert(t *testing.T) {
	db := setupGPKG(t)
	insertPark(t, db, 1, "POINT(2 3)")
	insertPark(t, db, 2, "POLYGON((0 0,10 0,10 10,0 10,0 0))")
	insertPark(t, db, 3, "LINESTRING(-5 -5,5 5)")
	insertPark(t, db, 4, "POLYGON EMPTY") // empty: not indexed
	insertPark(t, db, 5, nil)             // NULL: not indexed

	if got := indexIDs(t, db); fmt.Sprint(got) != "[1 2 3]" {
		t.Fatalf("indexed ids = %v, want [1 2 3]", got)
	}
	for _, tc := range []struct {
		fid  int
		want [4]float64
	}{
		{1, [4]float64{2, 2, 3, 3}},
		{2, [4]float64{0, 10, 0, 10}},
		{3, [4]float64{-5, 5, -5, 5}},
	} {
		if got := indexBounds(t, db, tc.fid); got != tc.want {
			t.Errorf("bounds(%d) = %v, want %v", tc.fid, got, tc.want)
		}
	}
}

func TestGeoPackageRtreeUpdateDelete(t *testing.T) {
	db := setupGPKG(t)
	insertPark(t, db, 1, "POINT(2 3)")
	insertPark(t, db, 2, "POINT(4 5)")
	insertPark(t, db, 3, "POINT(6 7)")

	mustExec(t, db, `UPDATE parks SET geom = ST_AsGPB(ST_GeomFromText('POINT(100 200)', 4326)) WHERE fid = 1`)
	if got, want := indexBounds(t, db, 1), ([4]float64{100, 100, 200, 200}); got != want {
		t.Errorf("after update, bounds(1) = %v, want %v", got, want)
	}

	mustExec(t, db, `UPDATE parks SET geom = NULL WHERE fid = 2`)
	if got := indexIDs(t, db); fmt.Sprint(got) != "[1 3]" {
		t.Errorf("after NULL update, ids = %v, want [1 3]", got)
	}

	mustExec(t, db, `UPDATE parks SET fid = 30 WHERE fid = 3`)
	if got := indexIDs(t, db); fmt.Sprint(got) != "[1 30]" {
		t.Errorf("after rowid change, ids = %v, want [1 30]", got)
	}

	mustExec(t, db, `DELETE FROM parks WHERE fid = 1`)
	if got := indexIDs(t, db); fmt.Sprint(got) != "[30]" {
		t.Errorf("after delete, ids = %v, want [30]", got)
	}
}

// R-tree filter for the cheap bbox pass, then a precise predicate refine —
// the query shape kenro exists to make possible.
func TestGeoPackageRtreeFilterThenRefine(t *testing.T) {
	db := setupGPKG(t)
	insertPark(t, db, 1, "POLYGON((1 1,2 1,2 2,1 2,1 1))")
	insertPark(t, db, 2, "POLYGON((3 3,4 3,4 4,3 4,3 3))")
	insertPark(t, db, 3, "POINT(5 5)")
	insertPark(t, db, 4, "POLYGON((9 9,11 9,11 11,9 11,9 9))") // bbox hits, geometry not within
	insertPark(t, db, 5, "POLYGON((100 100,101 100,101 101,100 101,100 100))")

	rows, err := db.Query(`
		SELECT p.fid FROM parks p
		JOIN rtree_parks_geom r ON p.fid = r.id
		WHERE r.minx <= 10 AND r.maxx >= 0 AND r.miny <= 10 AND r.maxy >= 0
		  AND ST_Within(ST_GeomFromGPB(p.geom), ST_GeomFromText(?, 4326))
		ORDER BY p.fid`,
		"POLYGON((0 0,10 0,10 10,0 10,0 0))")
	if err != nil {
		t.Fatalf("query: %v", err)
	}
	defer rows.Close()
	var fids []int
	for rows.Next() {
		var fid int
		if err := rows.Scan(&fid); err != nil {
			t.Fatal(err)
		}
		fids = append(fids, fid)
	}
	if fmt.Sprint(fids) != "[1 2 3]" {
		t.Fatalf("fids = %v, want [1 2 3] (4 is a bbox hit the refine must reject)", fids)
	}
}

// modernc.org/sqlite's registration API exposes no SQLITE_INNOCUOUS flag, so
// kenro's functions are not callable from triggers once trusted_schema is
// turned off. This test pins that limitation so it cannot regress silently
// into "the index looks maintained but isn't".
func TestTrustedSchemaOffIsRejectedNotIgnored(t *testing.T) {
	db := setupGPKG(t)
	mustExec(t, db, `PRAGMA trusted_schema = off`)
	_, err := db.Exec(
		`INSERT INTO parks (fid, name, geom) VALUES (1, 'p1', ST_AsGPB(ST_GeomFromText('POINT(2 3)', 4326)))`)
	if err == nil {
		// If this ever starts passing, modernc gained INNOCUOUS support (or
		// changed its defaults) and the docs should say so.
		if got := indexIDs(t, db); fmt.Sprint(got) != "[1]" {
			t.Fatalf("insert succeeded but the index was not maintained: %v", got)
		}
		t.Skip("trusted_schema=off now works — modernc.org/sqlite must support SQLITE_INNOCUOUS; update the docs")
	}
	t.Logf("expected failure under trusted_schema=off: %v", err)
}

func mustExec(t *testing.T, db *sql.DB, q string, args ...any) {
	t.Helper()
	if _, err := db.Exec(q, args...); err != nil {
		t.Fatalf("%s: %v", q, err)
	}
}
