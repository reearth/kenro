package kenro

import (
	"context"
	"database/sql"
	"fmt"
	"math"
	"slices"
	"strings"
	"testing"

	_ "modernc.org/sqlite"
)

// Geometry fixtures shared by the smoke cases.
const (
	poly  = `ST_GeomFromText('POLYGON((0 0,2 0,2 3,0 3,0 0))', 4326)`
	poly2 = `ST_GeomFromText('POLYGON((1 0,3 0,3 3,1 3,1 0))', 4326)`
	line  = `ST_GeomFromText('LINESTRING(0 0,1 1,2 2)', 4326)`
	pt    = `ST_GeomFromText('POINT(1 2)', 4326)`
	pt2   = `ST_GeomFromText('POINT(4 6)', 4326)`
)

func open(t *testing.T) *sql.DB {
	t.Helper()
	if err := Register(); err != nil {
		t.Fatalf("Register: %v", err)
	}
	db, err := sql.Open("sqlite", ":memory:")
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	t.Cleanup(func() { db.Close() })
	return db
}

func value(t *testing.T, db *sql.DB, query string) any {
	t.Helper()
	var v any
	if err := db.QueryRow("SELECT " + query).Scan(&v); err != nil {
		t.Fatalf("%s: %v", query, err)
	}
	return v
}

// A case per SQL function kenro registers; TestSmokeCoversTheManifest checks
// that this list stays exhaustive.
type smokeCase struct {
	fn   string
	sql  string
	want any // nil = only assert that it runs and is non-NULL
}

var smokeCases = []smokeCase{
	// I/O
	{"ST_GeomFromText", `ST_AsText(` + pt + `)`, "POINT(1 2)"},
	{"ST_AsText", `ST_AsText(` + line + `)`, "LINESTRING(0 0,1 1,2 2)"},
	{"ST_AsBinary", `length(ST_AsBinary(` + pt + `))`, int64(21)},
	{"ST_GeomFromWKB", `ST_AsText(ST_GeomFromWKB(ST_AsBinary(` + pt + `), 4326))`, "POINT(1 2)"},
	{"ST_AsGPB", `length(ST_AsGPB(` + pt + `)) > 0`, int64(1)},
	{"ST_GeomFromGPB", `ST_AsText(ST_GeomFromGPB(ST_AsGPB(` + pt + `)))`, "POINT(1 2)"},
	{"ST_SetSRID", `ST_SRID(ST_SetSRID(` + pt + `, 3857))`, int64(3857)},
	{"ST_SRID", `ST_SRID(` + pt + `)`, int64(4326)},

	// Predicates
	{"ST_Intersects", `ST_Intersects(` + poly + `, ` + pt + `)`, int64(1)},
	{"ST_Contains", `ST_Contains(` + poly + `, ` + pt + `)`, int64(1)},
	{"ST_Within", `ST_Within(` + pt + `, ` + poly + `)`, int64(1)},
	{"ST_Disjoint", `ST_Disjoint(` + poly + `, ` + pt2 + `)`, int64(1)},
	{"ST_Touches", `ST_Touches(` + poly + `, ` + pt + `)`, int64(0)},
	{"ST_Crosses", `ST_Crosses(` + line + `, ` + poly + `)`, nil},
	{"ST_Overlaps", `ST_Overlaps(` + poly + `, ` + poly2 + `)`, int64(1)},
	{"ST_Equals", `ST_Equals(` + poly + `, ` + poly + `)`, int64(1)},
	{"ST_Covers", `ST_Covers(` + poly + `, ` + pt + `)`, int64(1)},
	{"ST_CoveredBy", `ST_CoveredBy(` + pt + `, ` + poly + `)`, int64(1)},
	{"ST_Relate", `length(ST_Relate(` + poly + `, ` + poly2 + `))`, int64(9)},
	{"ST_Distance", `ST_Distance(` + poly + `, ` + pt2 + `)`, nil},
	{"ST_DWithin", `ST_DWithin(` + pt + `, ` + pt2 + `, 100.0)`, int64(1)},

	// GeoPackage R-tree helpers
	{"ST_MinX", `ST_MinX(` + poly + `)`, 0.0},
	{"ST_MaxX", `ST_MaxX(` + poly + `)`, 2.0},
	{"ST_MinY", `ST_MinY(` + poly + `)`, 0.0},
	{"ST_MaxY", `ST_MaxY(` + poly + `)`, 3.0},
	{"ST_IsEmpty", `ST_IsEmpty(` + poly + `)`, int64(0)},
	{"GPKG_IsAssignable", `GPKG_IsAssignable('GEOMETRY', 'POINT')`, int64(1)},

	// CRS / GeoJSON / H3
	{"ST_Transform", `ST_AsText(ST_Transform(` + pt + `, 3857)) LIKE 'POINT(111319%'`, int64(1)},
	{"ST_AsGeoJSON", `ST_AsGeoJSON(` + pt + `)`, `{"type":"Point","coordinates":[1,2]}`},
	{"ST_GeomFromGeoJSON", `ST_AsText(ST_GeomFromGeoJSON('{"type":"Point","coordinates":[1,2]}'))`, "POINT(1 2)"},
	{"h3_latlng_to_cell", `h3_latlng_to_cell(` + pt + `, 9) != 0`, int64(1)},
	{"h3_cell_to_parent", `h3_cell_to_parent(h3_latlng_to_cell(` + pt + `, 9), 5) != 0`, int64(1)},
	{"h3_cell_to_string", `length(h3_cell_to_string(h3_latlng_to_cell(` + pt + `, 9)))`, int64(15)},
	{"h3_string_to_cell", `h3_string_to_cell(h3_cell_to_string(h3_latlng_to_cell(` + pt + `, 9))) = h3_latlng_to_cell(` + pt + `, 9)`, int64(1)},

	// Constructors
	{"ST_MakePoint", `ST_AsText(ST_MakePoint(1, 2))`, "POINT(1 2)"},
	{"ST_Point", `ST_SRID(ST_Point(1, 2, 4326))`, int64(4326)},
	{"ST_MakeEnvelope", `ST_AsText(ST_MakeEnvelope(0, 0, 1, 1, 4326))`, "POLYGON((0 0,0 1,1 1,1 0,0 0))"},

	// Measures
	{"ST_ClosestPoint", `ST_AsText(ST_ClosestPoint(` + line + `, ` + pt + `))`, nil},
	{"ST_LineInterpolatePoint", `ST_AsText(ST_LineInterpolatePoint(` + line + `, 0.5))`, "POINT(1 1)"},
	{"ST_LineLocatePoint", `ST_LineLocatePoint(` + line + `, ` + pt + `)`, nil},
	{"ST_HausdorffDistance", `ST_HausdorffDistance(` + line + `, ` + line + `)`, 0.0},
	{"ST_FrechetDistance", `ST_FrechetDistance(` + line + `, ` + line + `)`, 0.0},
	{"ST_Azimuth", `ST_Azimuth(` + pt + `, ` + pt2 + `)`, nil},

	// Overlay
	{"ST_Intersection", `ST_Area(ST_Intersection(` + poly + `, ` + poly2 + `))`, 3.0},
	{"ST_Difference", `ST_Area(ST_Difference(` + poly + `, ` + poly2 + `))`, 3.0},
	{"ST_SymDifference", `ST_Area(ST_SymDifference(` + poly + `, ` + poly2 + `))`, 6.0},
	{"ST_Buffer", `ST_Area(ST_Buffer(` + pt + `, 1)) > 3`, int64(1)},
	{"ST_MakeValid", `ST_Area(ST_MakeValid(` + poly + `))`, 6.0},

	// MVT
	{"ST_AsMVTGeom", `ST_AsMVTGeom(` + poly + `, ST_MakeEnvelope(0, 0, 4, 4, 4326)) IS NOT NULL`, int64(1)},

	// Processing
	{"ST_ConvexHull", `ST_Area(ST_ConvexHull(` + poly + `))`, 6.0},
	{"ST_PointOnSurface", `ST_Intersects(` + poly + `, ST_PointOnSurface(` + poly + `))`, int64(1)},
	{"ST_SimplifyVW", `ST_AsText(ST_SimplifyVW(` + line + `, 0.0001))`, nil},
	{"ST_ChaikinSmoothing", `ST_AsText(ST_ChaikinSmoothing(` + line + `, 1))`, nil},
	{"ST_RemoveRepeatedPoints", `ST_AsText(ST_RemoveRepeatedPoints(` + line + `))`, "LINESTRING(0 0,1 1,2 2)"},
	{"ST_OrientedEnvelope", `ST_Area(ST_OrientedEnvelope(` + poly + `))`, 6.0},

	// Affine
	{"ST_Rotate", `ST_AsText(ST_Rotate(` + pt + `, 0))`, "POINT(1 2)"},
	{"ST_Translate", `ST_AsText(ST_Translate(` + pt + `, 1, 1))`, "POINT(2 3)"},
	{"ST_Scale", `ST_AsText(ST_Scale(` + pt + `, 2, 2))`, "POINT(2 4)"},

	// Accessors
	{"ST_Area", `ST_Area(` + poly + `)`, 6.0},
	{"ST_NPoints", `ST_NPoints(` + poly + `)`, int64(5)},
	{"ST_Perimeter", `ST_Perimeter(` + poly + `)`, 10.0},
	{"ST_GeometryType", `ST_GeometryType(` + poly + `)`, "ST_Polygon"},
	{"ST_NumGeometries", `ST_NumGeometries(` + poly + `)`, int64(1)},
	{"ST_GeometryN", `ST_AsText(ST_GeometryN(` + poly + `, 1))`, nil},
	{"ST_StartPoint", `ST_AsText(ST_StartPoint(` + line + `))`, "POINT(0 0)"},
	{"ST_EndPoint", `ST_AsText(ST_EndPoint(` + line + `))`, "POINT(2 2)"},
	{"ST_PointN", `ST_AsText(ST_PointN(` + line + `, 2))`, "POINT(1 1)"},
	{"ST_Reverse", `ST_AsText(ST_Reverse(` + line + `))`, "LINESTRING(2 2,1 1,0 0)"},
	{"ST_Length", `ST_Length(` + line + `) > 2`, int64(1)},
	{"ST_Centroid", `ST_AsText(ST_Centroid(` + poly + `))`, "POINT(1 1.5)"},
	{"ST_Envelope", `ST_Area(ST_Envelope(` + poly + `))`, 6.0},
	{"ST_X", `ST_X(` + pt + `)`, 1.0},
	{"ST_Y", `ST_Y(` + pt + `)`, 2.0},
	{"ST_NumPoints", `ST_NumPoints(` + line + `)`, int64(3)},
	{"ST_IsValid", `ST_IsValid(` + poly + `)`, int64(1)},
	{"ST_Simplify", `ST_AsText(ST_Simplify(` + line + `, 0.0001))`, nil},

	// Aggregates (ST_Union's scalar form is covered in TestUnionMode).
	{"ST_Union", `(SELECT ST_Area(ST_Union(g)) FROM (SELECT ` + poly + ` AS g UNION ALL SELECT ` + poly2 + `))`, 9.0},
	{"ST_AsMVT", `(SELECT length(ST_AsMVT(ST_AsMVTGeom(g, ST_MakeEnvelope(0, 0, 4, 4, 4326)), 'layer')) > 0 FROM (SELECT ` + poly + ` AS g))`, int64(1)},
}

func TestSmoke(t *testing.T) {
	db := open(t)
	for _, c := range smokeCases {
		t.Run(c.fn, func(t *testing.T) {
			got := value(t, db, c.sql)
			if c.want == nil {
				if got == nil {
					t.Fatalf("%s: got NULL", c.sql)
				}
				return
			}
			if f, ok := c.want.(float64); ok {
				g, ok := got.(float64)
				if !ok {
					t.Fatalf("%s: got %T(%v), want float64", c.sql, got, got)
				}
				if math.Abs(g-f) > 1e-9 {
					t.Fatalf("%s: got %v, want %v", c.sql, g, f)
				}
				return
			}
			if fmt.Sprint(got) != fmt.Sprint(c.want) {
				t.Fatalf("%s: got %v (%T), want %v", c.sql, got, got, c.want)
			}
		})
	}
}

// TestSmokeCoversTheManifest fails when a function is added to kenro but not
// exercised here — the manifest is the source of truth on both sides.
func TestSmokeCoversTheManifest(t *testing.T) {
	covered := map[string]bool{}
	for _, c := range smokeCases {
		covered[c.fn] = true
	}
	names := manifestNames(t)
	for _, name := range names {
		if !covered[name] {
			t.Errorf("%s is registered but has no smoke case", name)
		}
	}
	for name := range covered {
		if !slices.Contains(names, name) {
			t.Errorf("%s has a smoke case but is not in the manifest", name)
		}
	}
}

func manifestNames(t *testing.T) []string {
	t.Helper()
	ctx := context.Background()
	rt, err := newRuntime(ctx, defaultModule())
	if err != nil {
		t.Fatalf("newRuntime: %v", err)
	}
	defer rt.close(ctx)
	m, err := (&binding{rt: rt, cfg: &config{}}).manifest(ctx)
	if err != nil {
		t.Fatalf("manifest: %v", err)
	}
	var names []string
	for _, e := range m.Functions {
		if !slices.Contains(names, e.SQLName) {
			names = append(names, e.SQLName)
		}
	}
	for _, e := range m.Aggregates {
		if !slices.Contains(names, e.SQLName) {
			names = append(names, e.SQLName)
		}
	}
	return names
}

func TestNullStrictness(t *testing.T) {
	db := open(t)
	for _, q := range []string{
		`ST_Area(NULL)`,
		`ST_Intersects(NULL, ` + poly + `)`,
		`ST_Intersects(` + poly + `, NULL)`,
		`ST_GeomFromText(NULL, 4326)`,
		`ST_Buffer(` + pt + `, NULL)`,
	} {
		if got := value(t, db, q); got != nil {
			t.Errorf("%s: got %v, want NULL", q, got)
		}
	}
}

func TestErrorsCarryKenroPrefix(t *testing.T) {
	db := open(t)
	for _, tc := range []struct{ query, want string }{
		{`ST_GeomFromText('NOT A GEOMETRY')`, "kenro: invalid WKT"},
		{`ST_Area('a string')`, "did you mean ST_GeomFromText?"},
		{`ST_Collect(` + pt + `)`, "is not implemented in kenro"},
		{`ST_Area(` + pt + `, 1)`, "takes 1 argument(s), got 2"},
	} {
		var v any
		err := db.QueryRow("SELECT " + tc.query).Scan(&v)
		if err == nil {
			t.Errorf("%s: expected an error, got %v", tc.query, v)
			continue
		}
		if !strings.Contains(err.Error(), tc.want) {
			t.Errorf("%s: error %q does not contain %q", tc.query, err, tc.want)
		}
	}
}

func TestAggregateOverEmptySetIsNull(t *testing.T) {
	db := open(t)
	var v any
	if err := db.QueryRow(`SELECT ST_Union(g) FROM (SELECT ` + pt + ` AS g WHERE 0)`).Scan(&v); err != nil {
		t.Fatalf("empty aggregate: %v", err)
	}
	if v != nil {
		t.Fatalf("got %v, want NULL", v)
	}
}

func TestAggregateSkipsNullRows(t *testing.T) {
	db := open(t)
	got := value(t, db, `(SELECT ST_Area(ST_Union(g)) FROM (SELECT `+poly+` AS g UNION ALL SELECT NULL))`)
	if f, ok := got.(float64); !ok || math.Abs(f-6) > 1e-9 {
		t.Fatalf("got %v, want 6", got)
	}
}

// The scalar/aggregate collision is a driver limitation, not a kenro one:
// whichever form is not registered must fail loudly and name the way out.
func TestUnionScalarFormIsALoudError(t *testing.T) {
	db := open(t)
	var v any
	err := db.QueryRow(`SELECT ST_Union(` + poly + `, ` + poly2 + `)`).Scan(&v)
	if err == nil {
		t.Fatalf("expected an error for the scalar form under UnionAggregate, got %v", v)
	}
	if !strings.Contains(err.Error(), "argument") {
		t.Fatalf("unhelpful error: %v", err)
	}
}

func TestConcurrentQueries(t *testing.T) {
	db := open(t)
	db.SetMaxOpenConns(8)
	errs := make(chan error, 64)
	for range 64 {
		go func() {
			var area float64
			errs <- db.QueryRow(`SELECT ST_Area(ST_Buffer(` + pt + `, 1))`).Scan(&area)
		}()
	}
	for i := range 64 {
		if err := <-errs; err != nil {
			t.Fatalf("concurrent query %d: %v", i, err)
		}
	}
}
