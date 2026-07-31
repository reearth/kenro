package kenro_test

import (
	"database/sql"
	"fmt"
	"log"

	kenro "github.com/reearth/kenro/go"
	_ "modernc.org/sqlite"
)

// Register once at start-up, then use ST_ functions from any connection.
func Example() {
	if err := kenro.Register(); err != nil {
		log.Fatal(err)
	}

	db, err := sql.Open("sqlite", ":memory:")
	if err != nil {
		log.Fatal(err)
	}
	defer db.Close()

	var wkt string
	err = db.QueryRow(`
		SELECT ST_AsText(ST_Centroid(ST_GeomFromText(?, 4326)))`,
		"POLYGON((0 0,4 0,4 2,0 2,0 0))").Scan(&wkt)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(wkt)
	// Output: POINT(2 1)
}

// Geometries cross the boundary as GeoPackage blobs, so they can be stored in
// a column and handed straight back to kenro — this is what a GeoPackage file
// written by GDAL or QGIS already contains.
func Example_storeAndQuery() {
	db := openDB()
	defer db.Close()

	mustExec(db, `CREATE TABLE parks (fid INTEGER PRIMARY KEY, name TEXT, geom BLOB)`)
	for _, p := range []struct {
		name string
		wkt  string
	}{
		{"Ueno", "POLYGON((0 0,2 0,2 2,0 2,0 0))"},
		{"Yoyogi", "POLYGON((5 5,7 5,7 7,5 7,5 5))"},
	} {
		mustExec(db,
			`INSERT INTO parks (name, geom) VALUES (?, ST_AsGPB(ST_GeomFromText(?, 4326)))`,
			p.name, p.wkt)
	}

	// ST_GeomFromGPB unwraps the stored blob; everything else is PostGIS.
	rows, err := db.Query(`
		SELECT name, ST_Area(ST_GeomFromGPB(geom))
		FROM parks
		WHERE ST_Intersects(ST_GeomFromGPB(geom), ST_GeomFromText(?, 4326))
		ORDER BY name`,
		"POLYGON((1 1,6 1,6 6,1 6,1 1))")
	if err != nil {
		log.Fatal(err)
	}
	defer rows.Close()
	for rows.Next() {
		var name string
		var area float64
		if err := rows.Scan(&name, &area); err != nil {
			log.Fatal(err)
		}
		fmt.Printf("%s %.0f\n", name, area)
	}
	// Output:
	// Ueno 4
	// Yoyogi 4
}

// The query shape kenro exists for: let the R-tree throw away most rows on
// bounding boxes, then refine the survivors with a precise predicate. The
// index is maintained by the GeoPackage spec's own triggers — kenro only
// supplies the ST_ functions they call.
func Example_spatialIndex() {
	db := openDB()
	defer db.Close()

	mustExec(db, `CREATE TABLE parks (fid INTEGER PRIMARY KEY, geom BLOB)`)
	mustExec(db, `CREATE VIRTUAL TABLE rtree_parks_geom USING rtree(id, minx, maxx, miny, maxy)`)
	mustExec(db, `
		CREATE TRIGGER rtree_parks_geom_insert AFTER INSERT ON parks
		  WHEN (NEW.geom NOT NULL AND NOT ST_IsEmpty(NEW.geom))
		BEGIN
		  INSERT OR REPLACE INTO rtree_parks_geom VALUES (
		    NEW.fid,
		    ST_MinX(NEW.geom), ST_MaxX(NEW.geom),
		    ST_MinY(NEW.geom), ST_MaxY(NEW.geom));
		END`)

	for _, wkt := range []string{
		"POLYGON((1 1,2 1,2 2,1 2,1 1))",           // inside the window
		"POLYGON((9 9,11 9,11 11,9 11,9 9))",       // bbox overlaps, geometry does not fit
		"POLYGON((50 50,51 50,51 51,50 51,50 50))", // far away, the index skips it
	} {
		mustExec(db, `INSERT INTO parks (geom) VALUES (ST_AsGPB(ST_GeomFromText(?, 4326)))`, wkt)
	}

	var n int
	err := db.QueryRow(`
		SELECT count(*) FROM parks p
		JOIN rtree_parks_geom r ON p.fid = r.id
		WHERE r.minx <= 10 AND r.maxx >= 0 AND r.miny <= 10 AND r.maxy >= 0
		  AND ST_Within(ST_GeomFromGPB(p.geom), ST_GeomFromText(?, 4326))`,
		"POLYGON((0 0,10 0,10 10,0 10,0 0))").Scan(&n)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("parks fully inside the window:", n)
	// Output: parks fully inside the window: 1
}

// ST_Union(geom) dissolves a group of geometries into one — the aggregate
// form, which is what kenro registers by default. See [kenro.UnionMode] for
// the scalar ST_Union(a, b) and why the two cannot coexist on this driver.
func Example_dissolve() {
	db := openDB()
	defer db.Close()

	mustExec(db, `CREATE TABLE plots (ward TEXT, geom BLOB)`)
	for _, p := range []struct{ ward, wkt string }{
		{"north", "POLYGON((0 0,2 0,2 2,0 2,0 0))"},
		{"north", "POLYGON((1 0,3 0,3 2,1 2,1 0))"}, // overlaps the first
		{"south", "POLYGON((0 5,1 5,1 6,0 6,0 5))"},
	} {
		mustExec(db, `INSERT INTO plots VALUES (?, ST_GeomFromText(?, 4326))`, p.ward, p.wkt)
	}

	rows, err := db.Query(`
		SELECT ward, ST_Area(ST_Union(geom)) FROM plots GROUP BY ward ORDER BY ward`)
	if err != nil {
		log.Fatal(err)
	}
	defer rows.Close()
	for rows.Next() {
		var ward string
		var area float64
		if err := rows.Scan(&ward, &area); err != nil {
			log.Fatal(err)
		}
		fmt.Printf("%s %.0f\n", ward, area)
	}
	// Output:
	// north 6
	// south 1
}

// Reproject with ST_Transform: WGS84, Web Mercator and every UTM zone are
// built in.
func Example_reproject() {
	db := openDB()
	defer db.Close()

	var x, y float64
	err := db.QueryRow(`
		SELECT ST_X(g), ST_Y(g)
		FROM (SELECT ST_Transform(ST_GeomFromText(?, 4326), 3857) AS g)`,
		"POINT(139.767 35.681)").Scan(&x, &y)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("%.0f %.0f\n", x, y)
	// Output: 15558791 4256816
}

// A function kenro does not implement fails with a hint instead of
// "no such function", so the fix is one step away. (The driver wraps the
// message in its own "SQL logic error: … (1)".)
func Example_unimplemented() {
	db := openDB()
	defer db.Close()

	var v any
	err := db.QueryRow(`SELECT ST_Collect(ST_GeomFromText('POINT(1 2)'))`).Scan(&v)
	fmt.Println(err)
	// Output: SQL logic error: kenro: ST_Collect is not implemented in kenro. kenro never produces GeometryCollection values; for areal dissolve use the ST_Union aggregate, otherwise collect rows on the application side. (1)
}

func openDB() *sql.DB {
	if err := kenro.Register(); err != nil {
		log.Fatal(err)
	}
	db, err := sql.Open("sqlite", ":memory:")
	if err != nil {
		log.Fatal(err)
	}
	// One connection: an in-memory database is per-connection.
	db.SetMaxOpenConns(1)
	return db
}

func mustExec(db *sql.DB, query string, args ...any) {
	if _, err := db.Exec(query, args...); err != nil {
		log.Fatalf("%s: %v", query, err)
	}
}
