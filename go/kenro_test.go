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

	// --- the PostGIS surface added in the T1-T4 phases ---
	// SQL is shared with the JS smoke catalog, so the two bindings exercise
	// the same expressions; tolerance-based checks assert only non-NULL here.
	{"ST_AddPoint", `ST_AsText(ST_AddPoint(ST_GeomFromText('LINESTRING(0 0,1 1)'), ST_GeomFromText('POINT(2 2)')))`, "LINESTRING(0 0,1 1,2 2)"},
	{"ST_Affine", `ST_AsText(ST_Affine(ST_GeomFromText('LINESTRING(1 2,3 4)'), 2,0,0,2,10,20))`, "LINESTRING(12 24,16 28)"},
	{"ST_Angle", `ST_Angle(ST_GeomFromText('POINT(1 0)'), ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(0 1)'))`, nil},
	{"ST_Area2D", `ST_Area2D(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))`, int64(4)},
	{"ST_AsEWKB", `ST_SRID(ST_GeomFromEWKB(ST_AsEWKB(ST_GeomFromText('POINT(1 2)', 4326))))`, int64(4326)},
	{"ST_AsEWKT", `ST_AsEWKT(ST_GeomFromText('POINT(1 2)', 4326))`, "SRID=4326;POINT(1 2)"},
	{"ST_AsHexEWKB", `ST_AsHexEWKB(ST_GeomFromText('POINT(1 2)', 4326))`, "0101000020E6100000000000000000F03F0000000000000040"},
	{"ST_Boundary", `ST_AsText(ST_Boundary(ST_GeomFromText('LINESTRING(0 0,1 1,2 0)')))`, "MULTIPOINT((0 0),(2 0))"},
	{"ST_BoundingDiagonal", `ST_AsText(ST_BoundingDiagonal(ST_GeomFromText('LINESTRING(1 2,5 9)')))`, "LINESTRING(1 2,5 9)"},
	{"ST_ClipByBox2D", `ST_Area(ST_ClipByBox2D(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_MakeEnvelope(2,2,5,5)))`, nil},
	{"ST_ConcaveHull", `ST_Area(ST_ConcaveHull(ST_GeomFromText('MULTIPOINT(0 0,4 0,4 4,0 4)'), 1.0))`, nil},
	{"ST_ContainsProperly", `ST_ContainsProperly(ST_GeomFromText('POLYGON((0 0,3 0,3 3,0 3,0 0))'), ST_GeomFromText('POINT(1 1)'))`, int64(1)},
	{"ST_CoordDim", `ST_CoordDim(ST_GeomFromText('POINT(1 2)'))`, int64(2)},
	{"ST_DFullyWithin", `ST_DFullyWithin(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)'), 3)`, int64(1)},
	{"ST_DelaunayTriangles", `ST_NumGeometries(ST_DelaunayTriangles(ST_GeomFromText('MULTIPOINT(0 0,4 0,4 4,0 4)')))`, int64(2)},
	{"ST_Dimension", `ST_Dimension(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 0))'))`, int64(2)},
	{"ST_DistanceSphere", `ST_DistanceSphere(ST_GeomFromText('POINT(0 0)', 4326), ST_GeomFromText('POINT(1 0)', 4326))`, nil},
	{"ST_DistanceSpheroid", `ST_DistanceSpheroid(ST_GeomFromText('POINT(0 0)', 4326), ST_GeomFromText('POINT(1 0)', 4326))`, nil},
	{"ST_Expand", `ST_AsText(ST_Expand(ST_GeomFromText('POINT(1 1)'), 2))`, "POLYGON((-1 -1,-1 3,3 3,3 -1,-1 -1))"},
	{"ST_ExteriorRing", `ST_AsText(ST_ExteriorRing(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))')))`, "LINESTRING(0 0,4 0,4 4,0 4,0 0)"},
	{"ST_FlipCoordinates", `ST_AsText(ST_FlipCoordinates(ST_GeomFromText('POINT(1 2)')))`, "POINT(2 1)"},
	{"ST_Force2D", `ST_AsText(ST_Force2D(ST_GeomFromText('POINT(1 2)')))`, "POINT(1 2)"},
	{"ST_ForcePolygonCCW", `ST_IsPolygonCCW(ST_ForcePolygonCCW(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')))`, int64(1)},
	{"ST_ForcePolygonCW", `ST_IsPolygonCW(ST_ForcePolygonCW(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')))`, int64(1)},
	{"ST_ForceRHR", `ST_IsPolygonCW(ST_ForceRHR(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')))`, int64(1)},
	{"ST_GeoHash", `ST_GeoHash(ST_GeomFromText('POINT(139.7 35.68)', 4326))`, "xn76fzq7jfn42q30gmb9"},
	{"ST_GeomFromEWKB", `ST_AsText(ST_GeomFromEWKB(ST_AsEWKB(ST_GeomFromText('POINT(1 2)', 4326))))`, "POINT(1 2)"},
	{"ST_GeomFromEWKT", `ST_AsEWKT(ST_GeomFromEWKT('SRID=3857;POINT(1 2)'))`, "SRID=3857;POINT(1 2)"},
	{"ST_GeometryFromText", `ST_AsText(ST_GeometryFromText('POINT(1 2)'))`, "POINT(1 2)"},
	{"ST_InteriorRingN", `ST_AsText(ST_InteriorRingN(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))'), 1))`, "LINESTRING(1 1,2 1,2 2,1 2,1 1)"},
	{"ST_IsClosed", `ST_IsClosed(ST_GeomFromText('LINESTRING(0 0,1 1,1 0,0 0)'))`, int64(1)},
	{"ST_IsPolygonCCW", `ST_IsPolygonCCW(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))`, int64(1)},
	{"ST_IsPolygonCW", `ST_IsPolygonCW(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))`, int64(0)},
	{"ST_IsRing", `ST_IsRing(ST_GeomFromText('LINESTRING(0 0,1 1,1 0,0 0)'))`, int64(1)},
	{"ST_IsValidReason", `ST_IsValidReason(ST_GeomFromText('POINT(1 1)'))`, "Valid Geometry"},
	{"ST_Length2D", `ST_Length2D(ST_GeomFromText('LINESTRING(0 0,3 4)'))`, int64(5)},
	{"ST_Length2DSpheroid", `ST_Length2DSpheroid(ST_GeomFromText('LINESTRING(0 0,1 0)', 4326), 'SPHEROID["WGS 84",6378137,298.257223563]')`, nil},
	{"ST_LengthSpheroid", `ST_LengthSpheroid(ST_GeomFromText('LINESTRING(0 0,1 0)', 4326), 'SPHEROID["WGS 84",6378137,298.257223563]')`, nil},
	{"ST_LineFromText", `ST_AsText(ST_LineFromText('LINESTRING(0 0,1 1)'))`, "LINESTRING(0 0,1 1)"},
	{"ST_LineFromWKB", `ST_AsText(ST_LineFromWKB(ST_AsBinary(ST_GeomFromText('LINESTRING(0 0,1 1)'))))`, "LINESTRING(0 0,1 1)"},
	{"ST_LineInterpolatePoints", `ST_AsText(ST_LineInterpolatePoints(ST_GeomFromText('LINESTRING(0 0,10 0)'), 0.25))`, "MULTIPOINT((2.5 0),(5 0),(7.5 0),(10 0))"},
	{"ST_LineStringFromText", `ST_AsText(ST_LineStringFromText('LINESTRING(0 0,1 1)'))`, "LINESTRING(0 0,1 1)"},
	{"ST_LineSubstring", `ST_AsText(ST_LineSubstring(ST_GeomFromText('LINESTRING(0 0,10 0)'), 0.3, 0.7))`, "LINESTRING(3 0,7 0)"},
	{"ST_LongestLine", `ST_NPoints(ST_LongestLine(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)')))`, int64(2)},
	{"ST_MLineFromText", `ST_AsText(ST_MLineFromText('MULTILINESTRING((0 0,1 1))'))`, "MULTILINESTRING((0 0,1 1))"},
	{"ST_MPointFromText", `ST_AsText(ST_MPointFromText('MULTIPOINT((1 2),(3 4))'))`, "MULTIPOINT((1 2),(3 4))"},
	{"ST_MPolyFromText", `ST_AsText(ST_MPolyFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))`, "MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))"},
	{"ST_MakeLine", `ST_AsText(ST_MakeLine(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(1 1)')))`, "LINESTRING(0 0,1 1)"},
	{"ST_MakePolygon", `ST_AsText(ST_MakePolygon(ST_GeomFromText('LINESTRING(0 0,1 0,1 1,0 0)')))`, "POLYGON((0 0,1 0,1 1,0 0))"},
	{"ST_MaxDistance", `ST_MaxDistance(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)'))`, nil},
	{"ST_MinimumBoundingCircle", `ST_Covers(ST_MinimumBoundingCircle(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))')), ST_GeomFromText('POINT(4 4)'))`, int64(1)},
	{"ST_MinimumBoundingRadius", `ST_MinimumBoundingRadius(ST_GeomFromText('LINESTRING(0 0,4 0)'))`, nil},
	{"ST_Multi", `ST_AsText(ST_Multi(ST_GeomFromText('POINT(1 2)')))`, "MULTIPOINT((1 2))"},
	{"ST_NDims", `ST_NDims(ST_GeomFromText('POINT(1 2)'))`, int64(2)},
	{"ST_NRings", `ST_NRings(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))'))`, int64(2)},
	{"ST_NumInteriorRing", `ST_NumInteriorRing(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))'))`, int64(1)},
	{"ST_NumInteriorRings", `ST_NumInteriorRings(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))'))`, int64(1)},
	{"ST_OrderingEquals", `ST_OrderingEquals(ST_GeomFromText('LINESTRING(0 0,1 1)'), ST_GeomFromText('LINESTRING(1 1,0 0)'))`, int64(0)},
	{"ST_Perimeter2D", `ST_Perimeter2D(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'))`, int64(8)},
	{"ST_PointFromText", `ST_AsText(ST_PointFromText('POINT(1 2)'))`, "POINT(1 2)"},
	{"ST_PointFromWKB", `ST_AsText(ST_PointFromWKB(ST_AsBinary(ST_GeomFromText('POINT(1 2)'))))`, "POINT(1 2)"},
	{"ST_Points", `ST_AsText(ST_Points(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 0))')))`, "MULTIPOINT((0 0),(1 0),(1 1),(0 0))"},
	{"ST_PolyFromText", `ST_AsText(ST_PolyFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))`, "POLYGON((0 0,1 0,1 1,0 1,0 0))"},
	{"ST_PolyFromWKB", `ST_AsText(ST_PolyFromWKB(ST_AsBinary(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))))`, "POLYGON((0 0,1 0,1 1,0 1,0 0))"},
	{"ST_PolygonFromText", `ST_AsText(ST_PolygonFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))`, "POLYGON((0 0,1 0,1 1,0 1,0 0))"},
	{"ST_Project", `ST_X(ST_Project(ST_GeomFromText('POINT(0 0)'), 100, 1.5707963267948966))`, nil},
	{"ST_ReducePrecision", `ST_X(ST_ReducePrecision(ST_GeomFromText('POINT(1.234 5.678)'), 0.1))`, nil},
	{"ST_RelateMatch", `ST_RelateMatch('101202FFF', 'TTTTTTFFF')`, int64(1)},
	{"ST_RemovePoint", `ST_AsText(ST_RemovePoint(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'), 0))`, "LINESTRING(1 1,2 2)"},
	{"ST_Segmentize", `ST_NPoints(ST_Segmentize(ST_GeomFromText('LINESTRING(0 0,10 0)'), 4))`, int64(4)},
	{"ST_SetPoint", `ST_AsText(ST_SetPoint(ST_GeomFromText('LINESTRING(0 0,1 1)'), 0, ST_GeomFromText('POINT(9 9)')))`, "LINESTRING(9 9,1 1)"},
	{"ST_ShiftLongitude", `ST_AsText(ST_ShiftLongitude(ST_GeomFromText('POINT(-10 5)')))`, "POINT(350 5)"},
	{"ST_ShortestLine", `ST_AsText(ST_ShortestLine(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('LINESTRING(2 -1,2 1)')))`, "LINESTRING(0 0,2 0)"},
	{"ST_SnapToGrid", `ST_AsText(ST_SnapToGrid(ST_GeomFromText('POINT(1.23 4.57)'), 0.5))`, "POINT(1 4.5)"},
	{"ST_Subdivide", `ST_Area(ST_Subdivide(ST_Segmentize(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), 2), 8))`, nil},
	{"ST_SymmetricDifference", `ST_Area(ST_SymmetricDifference(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), ST_GeomFromText('POLYGON((5 5,15 5,15 15,5 15,5 5))')))`, nil},
	{"ST_TransScale", `ST_AsText(ST_TransScale(ST_GeomFromText('POINT(1 2)'), 1, 2, 3, 4))`, "POINT(6 16)"},
	{"ST_UnaryUnion", `ST_Area(ST_UnaryUnion(ST_GeomFromText('MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((1 1,3 1,3 3,1 3,1 1)))')))`, nil},
	{"ST_XMax", `ST_XMax(ST_GeomFromText('LINESTRING(1 2,3 4)'))`, int64(3)},
	{"ST_XMin", `ST_XMin(ST_GeomFromText('LINESTRING(1 2,3 4)'))`, int64(1)},
	{"ST_YMax", `ST_YMax(ST_GeomFromText('LINESTRING(1 2,3 4)'))`, int64(4)},
	{"ST_YMin", `ST_YMin(ST_GeomFromText('LINESTRING(1 2,3 4)'))`, int64(2)},
	// ST_Extent is an aggregate, so it needs a table rather than one row.
	{"ST_Extent", `(SELECT ST_AsText(ST_Extent(g)) FROM (SELECT ST_GeomFromText('POINT(1 2)') AS g UNION ALL SELECT ST_GeomFromText('POINT(5 0)')))`, "POLYGON((1 0,1 2,5 2,5 0,1 0))"},

	// --- the tail (functions::misc) ---
	{"ST_Box2dFromGeoHash", `ST_GeometryType(ST_Box2dFromGeoHash('xn76f'))`, "ST_Polygon"},
	{"ST_GeomFromGeoHash", `ST_GeometryType(ST_GeomFromGeoHash('xn76f'))`, "ST_Polygon"},
	{"ST_GeometricMedian", `ST_X(ST_GeometricMedian(ST_GeomFromText('MULTIPOINT((0 0),(4 0),(0 4),(4 4))')))`, nil},
	{"ST_LineCrossingDirection", `ST_LineCrossingDirection(ST_GeomFromText('LINESTRING(0 0,2 2)'), ST_GeomFromText('LINESTRING(0 2,2 0)'))`, int64(1)},
	{"ST_LineExtend", `ST_AsText(ST_LineExtend(ST_GeomFromText('LINESTRING(0 0,1 0)'), 1))`, "LINESTRING(0 0,1 0,2 0)"},
	{"ST_LineFromMultiPoint", `ST_AsText(ST_LineFromMultiPoint(ST_GeomFromText('MULTIPOINT((0 0),(1 1),(2 2))')))`, "LINESTRING(0 0,1 1,2 2)"},
	{"ST_LineStringFromWKB", `ST_AsText(ST_LineStringFromWKB(ST_AsBinary(ST_GeomFromText('LINESTRING(0 0,1 1)'))))`, "LINESTRING(0 0,1 1)"},
	{"ST_MLineFromWKB", `ST_AsText(ST_MLineFromWKB(ST_AsBinary(ST_GeomFromText('MULTILINESTRING((0 0,1 1))'))))`, "MULTILINESTRING((0 0,1 1))"},
	{"ST_MPointFromWKB", `ST_AsText(ST_MPointFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOINT((1 2),(3 4))'))))`, "MULTIPOINT((1 2),(3 4))"},
	{"ST_MPolyFromWKB", `ST_GeometryType(ST_MPolyFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))))`, "ST_MultiPolygon"},
	{"ST_MakeBox2D", `ST_AsText(ST_MakeBox2D(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)')))`, "POLYGON((0 0,0 4,3 4,3 0,0 0))"},
	{"ST_MemSize", `ST_MemSize(ST_GeomFromText('POINT(1 2)')) > 0`, int64(1)},
	{"ST_MultiLineFromWKB", `ST_AsText(ST_MultiLineFromWKB(ST_AsBinary(ST_GeomFromText('MULTILINESTRING((0 0,1 1))'))))`, "MULTILINESTRING((0 0,1 1))"},
	{"ST_MultiLineStringFromText", `ST_AsText(ST_MultiLineStringFromText('MULTILINESTRING((0 0,1 1))'))`, "MULTILINESTRING((0 0,1 1))"},
	{"ST_MultiPointFromText", `ST_AsText(ST_MultiPointFromText('MULTIPOINT((1 2),(3 4))'))`, "MULTIPOINT((1 2),(3 4))"},
	{"ST_MultiPointFromWKB", `ST_AsText(ST_MultiPointFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOINT((1 2))'))))`, "MULTIPOINT((1 2))"},
	{"ST_MultiPolyFromWKB", `ST_GeometryType(ST_MultiPolyFromWKB(ST_AsBinary(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))))`, "ST_MultiPolygon"},
	{"ST_MultiPolygonFromText", `ST_AsText(ST_MultiPolygonFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))`, nil},
	{"ST_Normalize", `ST_IsPolygonCW(ST_Normalize(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')))`, int64(1)},
	{"ST_PointFromGeoHash", `ST_AsText(ST_PointFromGeoHash('xn76f'))`, "POINT(139.68017578125 35.66162109375)"},
	{"ST_PointInsideCircle", `ST_PointInsideCircle(ST_GeomFromText('POINT(1 1)'), 0, 0, 2)`, int64(1)},
	{"ST_Polygon", `ST_SRID(ST_Polygon(ST_GeomFromText('LINESTRING(0 0,1 0,1 1,0 0)'), 4326))`, int64(4326)},
	{"ST_PolygonFromWKB", `ST_AsText(ST_PolygonFromWKB(ST_AsBinary(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))))`, "POLYGON((0 0,1 0,1 1,0 1,0 0))"},
	{"ST_RotateZ", `ST_AsText(ST_RotateZ(ST_GeomFromText('POINT(1 0)'), 1.5707963267948966))`, nil},
	{"ST_Summary", `ST_Summary(ST_GeomFromText('POINT(1 2)', 4326))`, nil},
	{"ST_WrapX", `ST_AsText(ST_WrapX(ST_GeomFromText('LINESTRING(-170 0,170 0)'), 0, 360))`, "LINESTRING(190 0,170 0)"},

	// --- 3D pass-through: a raw WKB blob, as a GDAL-written column would be ---
	{"ST_HasM", `ST_HasM(x'01e9030000000000000000f03f00000000000000400000000000000840')`, int64(0)},
	{"ST_HasZ", `ST_HasZ(x'01e9030000000000000000f03f00000000000000400000000000000840')`, int64(1)},
	{"ST_M", `ST_M(x'01e9030000000000000000f03f00000000000000400000000000000840') IS NULL`, int64(1)},
	{"ST_Z", `ST_Z(x'01e9030000000000000000f03f00000000000000400000000000000840')`, int64(3)},
	{"ST_ZMax", `ST_ZMax(x'01ea03000002000000000000000000000000000000000000000000000000002440000000000000f03f000000000000f03f0000000000003e40')`, int64(30)},
	{"ST_ZMin", `ST_ZMin(x'01ea03000002000000000000000000000000000000000000000000000000002440000000000000f03f000000000000f03f0000000000003e40')`, int64(10)},

	// --- GML 2/3 I/O ---
	{"ST_AsGML", `ST_AsGML(ST_GeomFromText('POINT(1 2)', 4326))`, nil},
	{"ST_GMLToSQL", `ST_AsText(ST_GMLToSQL('<gml:Point><gml:pos>1 2</gml:pos></gml:Point>'))`, "POINT(1 2)"},
	{"ST_GeomFromGML", `ST_AsText(ST_GeomFromGML('<gml:Point><gml:pos>1 2</gml:pos></gml:Point>'))`, "POINT(1 2)"},

	// --- surface collections: PostGIS's own POLYHEDRALSURFACE bytes ---
	{"ST_NumPatches", `ST_NumPatches(x'01f70300000100000001eb03000001000000050000000000000000000000000000000000000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f0000000000000000000000000000f03f00000000000000000000000000000000000000000000000000000000000000000000000000000000')`, int64(1)},
	{"ST_PatchN", `ST_AsText(ST_PatchN(x'01f70300000100000001eb03000001000000050000000000000000000000000000000000000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f0000000000000000000000000000f03f00000000000000000000000000000000000000000000000000000000000000000000000000000000', 1))`, "POLYGON((0 0,0 1,1 1,1 0,0 0))"},
	{"kenro_gpkg_extension_required", `kenro_gpkg_extension_required(x'01f70300000100000001eb03000001000000050000000000000000000000000000000000000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f0000000000000000000000000000f03f00000000000000000000000000000000000000000000000000000000000000000000000000000000')`, "gpkg_geom_POLYHEDRALSURFACE"},
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
