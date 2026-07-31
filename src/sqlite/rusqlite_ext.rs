//! rusqlite binding: registers every kenro function on a connection.
//!
//! Nothing but value and error mapping lives here. All Stage-1 functions are
//! NULL-strict (any NULL argument → NULL result, matching PostGIS STRICT
//! functions) — that mapping happens in this layer only; core functions
//! never see "NULL". Stubs are the exception: they error regardless, since
//! their job is to be loud.

use rusqlite::Connection;
use rusqlite::functions::{Context, FunctionFlags};
use rusqlite::types::{Value, ValueRef};

use crate::error::Error;
use crate::functions::{io, predicates, rtree, stubs};

/// `SQLITE_INNOCUOUS` is load-bearing, not cosmetic: it lets these functions
/// run inside triggers under `PRAGMA trusted_schema=off`, which the
/// GeoPackage rtree maintenance triggers require.
const FLAGS: FunctionFlags = FunctionFlags::SQLITE_UTF8
    .union(FunctionFlags::SQLITE_DETERMINISTIC)
    .union(FunctionFlags::SQLITE_INNOCUOUS);

/// Register all kenro spatial functions on the connection.
pub fn register(conn: &Connection) -> rusqlite::Result<()> {
    // Constructors.
    conn.create_scalar_function("ST_GeomFromText", 1, FLAGS, |ctx| {
        let Some(wkt) = text_or_null(ctx, 0, "ST_GeomFromText")? else {
            return Ok(None);
        };
        blob(io::st_geom_from_text(wkt, None))
    })?;
    conn.create_scalar_function("ST_GeomFromText", 2, FLAGS, |ctx| {
        let (Some(wkt), Some(srid)) = (
            text_or_null(ctx, 0, "ST_GeomFromText")?,
            int_or_null(ctx, 1, "ST_GeomFromText")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_geom_from_text(wkt, Some(srid)))
    })?;
    conn.create_scalar_function("ST_GeomFromWKB", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_GeomFromWKB")? else {
            return Ok(None);
        };
        blob(io::st_geom_from_wkb(b, None))
    })?;
    conn.create_scalar_function("ST_GeomFromWKB", 2, FLAGS, |ctx| {
        let (Some(b), Some(srid)) = (
            blob_or_null(ctx, 0, "ST_GeomFromWKB")?,
            int_or_null(ctx, 1, "ST_GeomFromWKB")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_geom_from_wkb(b, Some(srid)))
    })?;
    conn.create_scalar_function("ST_GeomFromGPB", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_GeomFromGPB")? else {
            return Ok(None);
        };
        blob(io::st_geom_from_gpb(b))
    })?;

    // Output functions.
    conn.create_scalar_function("ST_AsText", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_AsText")? else {
            return Ok(None);
        };
        io::st_as_text(b)
            .map(|s| Some(Value::Text(s)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsBinary", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_AsBinary")? else {
            return Ok(None);
        };
        blob(io::st_as_binary(b))
    })?;
    conn.create_scalar_function("ST_AsGPB", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_AsGPB")? else {
            return Ok(None);
        };
        blob(io::st_as_gpb(b))
    })?;

    // Predicates.
    register_predicate(conn, "ST_Intersects", predicates::st_intersects)?;
    register_predicate(conn, "ST_Contains", predicates::st_contains)?;
    register_predicate(conn, "ST_Within", predicates::st_within)?;
    register_predicate(conn, "ST_Disjoint", predicates::st_disjoint)?;
    register_predicate(conn, "ST_Touches", predicates::st_touches)?;
    register_predicate(conn, "ST_Crosses", predicates::st_crosses)?;
    register_predicate(conn, "ST_Overlaps", predicates::st_overlaps)?;
    register_predicate(conn, "ST_Equals", predicates::st_equals)?;
    register_predicate(conn, "ST_Covers", predicates::st_covers)?;
    register_predicate(conn, "ST_CoveredBy", predicates::st_covered_by)?;
    conn.create_scalar_function("ST_Relate", 2, FLAGS, |ctx| {
        let (Some(a), Some(b)) = (
            blob_or_null(ctx, 0, "ST_Relate")?,
            blob_or_null(ctx, 1, "ST_Relate")?,
        ) else {
            return Ok(None);
        };
        predicates::st_relate(a, b)
            .map(|s| Some(Value::Text(s)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_Relate", 3, FLAGS, |ctx| {
        let (Some(a), Some(b), Some(pattern)) = (
            blob_or_null(ctx, 0, "ST_Relate")?,
            blob_or_null(ctx, 1, "ST_Relate")?,
            text_or_null(ctx, 2, "ST_Relate")?,
        ) else {
            return Ok(None);
        };
        predicates::st_relate_pattern(a, b, pattern)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_Distance", 2, FLAGS, |ctx| {
        let (Some(a), Some(b)) = (
            blob_or_null(ctx, 0, "ST_Distance")?,
            blob_or_null(ctx, 1, "ST_Distance")?,
        ) else {
            return Ok(None);
        };
        predicates::st_distance(a, b)
            .map(|d| d.map(Value::Real))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_DWithin", 3, FLAGS, |ctx| {
        let (Some(a), Some(b), Some(d)) = (
            blob_or_null(ctx, 0, "ST_DWithin")?,
            blob_or_null(ctx, 1, "ST_DWithin")?,
            real_or_null(ctx, 2, "ST_DWithin")?,
        ) else {
            return Ok(None);
        };
        predicates::st_dwithin(a, b, d)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;

    // Measures.
    {
        use crate::functions::measures;
        conn.create_scalar_function("ST_ClosestPoint", 2, FLAGS, |ctx| {
            let (Some(a), Some(b)) = (
                blob_or_null(ctx, 0, "ST_ClosestPoint")?,
                blob_or_null(ctx, 1, "ST_ClosestPoint")?,
            ) else {
                return Ok(None);
            };
            measures::st_closest_point(a, b)
                .map(|v| v.map(Value::Blob))
                .map_err(sql_err)
        })?;
        conn.create_scalar_function("ST_LineInterpolatePoint", 2, FLAGS, |ctx| {
            let (Some(a), Some(f)) = (
                blob_or_null(ctx, 0, "ST_LineInterpolatePoint")?,
                real_or_null(ctx, 1, "ST_LineInterpolatePoint")?,
            ) else {
                return Ok(None);
            };
            blob(measures::st_line_interpolate_point(a, f))
        })?;
        conn.create_scalar_function("ST_LineLocatePoint", 2, FLAGS, |ctx| {
            let (Some(a), Some(b)) = (
                blob_or_null(ctx, 0, "ST_LineLocatePoint")?,
                blob_or_null(ctx, 1, "ST_LineLocatePoint")?,
            ) else {
                return Ok(None);
            };
            measures::st_line_locate_point(a, b)
                .map(|v| Some(Value::Real(v)))
                .map_err(sql_err)
        })?;
        conn.create_scalar_function("ST_HausdorffDistance", 2, FLAGS, |ctx| {
            let (Some(a), Some(b)) = (
                blob_or_null(ctx, 0, "ST_HausdorffDistance")?,
                blob_or_null(ctx, 1, "ST_HausdorffDistance")?,
            ) else {
                return Ok(None);
            };
            measures::st_hausdorff_distance(a, b)
                .map(|v| Some(Value::Real(v)))
                .map_err(sql_err)
        })?;
        conn.create_scalar_function("ST_FrechetDistance", 2, FLAGS, |ctx| {
            let (Some(a), Some(b)) = (
                blob_or_null(ctx, 0, "ST_FrechetDistance")?,
                blob_or_null(ctx, 1, "ST_FrechetDistance")?,
            ) else {
                return Ok(None);
            };
            measures::st_frechet_distance(a, b)
                .map(|v| Some(Value::Real(v)))
                .map_err(sql_err)
        })?;
        conn.create_scalar_function("ST_Azimuth", 2, FLAGS, |ctx| {
            let (Some(a), Some(b)) = (
                blob_or_null(ctx, 0, "ST_Azimuth")?,
                blob_or_null(ctx, 1, "ST_Azimuth")?,
            ) else {
                return Ok(None);
            };
            measures::st_azimuth(a, b)
                .map(|v| v.map(Value::Real))
                .map_err(sql_err)
        })?;
    }

    // Overlay.
    {
        use crate::functions::overlay;
        register_geom2_to_blob(conn, "ST_Intersection", overlay::st_intersection)?;
        register_geom2_to_blob(conn, "ST_Difference", overlay::st_difference)?;
        register_geom2_to_blob(conn, "ST_SymDifference", overlay::st_sym_difference)?;
        register_geom2_to_blob(conn, "ST_Union", overlay::st_union)?;
        conn.create_scalar_function("ST_Buffer", 2, FLAGS, |ctx| {
            let (Some(b), Some(d)) = (
                blob_or_null(ctx, 0, "ST_Buffer")?,
                real_or_null(ctx, 1, "ST_Buffer")?,
            ) else {
                return Ok(None);
            };
            blob(overlay::st_buffer(b, d, None))
        })?;
        conn.create_scalar_function("ST_Buffer", 3, FLAGS, |ctx| {
            let (Some(b), Some(d), Some(opts)) = (
                blob_or_null(ctx, 0, "ST_Buffer")?,
                real_or_null(ctx, 1, "ST_Buffer")?,
                text_or_int_or_null(ctx, 2, "ST_Buffer")?,
            ) else {
                return Ok(None);
            };
            blob(overlay::st_buffer(b, d, Some(&opts)))
        })?;
    }

    // Processing + affine.
    {
        use crate::functions::{affine, processing};
        register_geom_to_blob(conn, "ST_ConvexHull", processing::st_convex_hull)?;
        register_geom_to_blob(conn, "ST_PointOnSurface", processing::st_point_on_surface)?;
        register_geom_to_blob(
            conn,
            "ST_RemoveRepeatedPoints",
            processing::st_remove_repeated_points,
        )?;
        register_geom_to_blob(
            conn,
            "ST_OrientedEnvelope",
            processing::st_oriented_envelope,
        )?;
        conn.create_scalar_function("ST_SimplifyVW", 2, FLAGS, |ctx| {
            let (Some(b), Some(tol)) = (
                blob_or_null(ctx, 0, "ST_SimplifyVW")?,
                real_or_null(ctx, 1, "ST_SimplifyVW")?,
            ) else {
                return Ok(None);
            };
            blob(processing::st_simplify_vw(b, tol))
        })?;
        conn.create_scalar_function("ST_ChaikinSmoothing", 1, FLAGS, |ctx| {
            let Some(b) = blob_or_null(ctx, 0, "ST_ChaikinSmoothing")? else {
                return Ok(None);
            };
            blob(processing::st_chaikin_smoothing(b, 1))
        })?;
        conn.create_scalar_function("ST_ChaikinSmoothing", 2, FLAGS, |ctx| {
            let (Some(b), Some(n)) = (
                blob_or_null(ctx, 0, "ST_ChaikinSmoothing")?,
                i64_or_null(ctx, 1, "ST_ChaikinSmoothing")?,
            ) else {
                return Ok(None);
            };
            blob(processing::st_chaikin_smoothing(b, n))
        })?;
        conn.create_scalar_function("ST_Rotate", 2, FLAGS, |ctx| {
            let (Some(b), Some(radians)) = (
                blob_or_null(ctx, 0, "ST_Rotate")?,
                real_or_null(ctx, 1, "ST_Rotate")?,
            ) else {
                return Ok(None);
            };
            blob(affine::st_rotate(b, radians))
        })?;
        conn.create_scalar_function("ST_Rotate", 4, FLAGS, |ctx| {
            let (Some(b), Some(radians), Some(x0), Some(y0)) = (
                blob_or_null(ctx, 0, "ST_Rotate")?,
                real_or_null(ctx, 1, "ST_Rotate")?,
                real_or_null(ctx, 2, "ST_Rotate")?,
                real_or_null(ctx, 3, "ST_Rotate")?,
            ) else {
                return Ok(None);
            };
            blob(affine::st_rotate_xy(b, radians, x0, y0))
        })?;
        conn.create_scalar_function("ST_Translate", 3, FLAGS, |ctx| {
            let (Some(b), Some(dx), Some(dy)) = (
                blob_or_null(ctx, 0, "ST_Translate")?,
                real_or_null(ctx, 1, "ST_Translate")?,
                real_or_null(ctx, 2, "ST_Translate")?,
            ) else {
                return Ok(None);
            };
            blob(affine::st_translate(b, dx, dy))
        })?;
        conn.create_scalar_function("ST_Scale", 3, FLAGS, |ctx| {
            let (Some(b), Some(xf), Some(yf)) = (
                blob_or_null(ctx, 0, "ST_Scale")?,
                real_or_null(ctx, 1, "ST_Scale")?,
                real_or_null(ctx, 2, "ST_Scale")?,
            ) else {
                return Ok(None);
            };
            blob(affine::st_scale(b, xf, yf))
        })?;
    }

    // R-tree helpers (GeoPackage Annex F.3 contract).
    register_rtree_minmax(conn, "ST_MinX", rtree::st_min_x)?;
    register_rtree_minmax(conn, "ST_MaxX", rtree::st_max_x)?;
    register_rtree_minmax(conn, "ST_MinY", rtree::st_min_y)?;
    register_rtree_minmax(conn, "ST_MaxY", rtree::st_max_y)?;
    conn.create_scalar_function("ST_IsEmpty", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_IsEmpty")? else {
            return Ok(None);
        };
        rtree::st_is_empty(b)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;

    // SRID management (byte-level, feature-independent).
    conn.create_scalar_function("ST_SetSRID", 2, FLAGS, |ctx| {
        let (Some(b), Some(srid)) = (
            blob_or_null(ctx, 0, "ST_SetSRID")?,
            int_or_null(ctx, 1, "ST_SetSRID")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_set_srid(b, srid))
    })?;
    conn.create_scalar_function("ST_SRID", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_SRID")? else {
            return Ok(None);
        };
        io::st_srid(b)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;

    register_transform(conn)?;
    register_h3(conn)?;
    register_geojson(conn)?;
    register_accessors(conn)?;

    // Stubs: known-but-unimplemented ST_ functions fail with a helpful
    // message instead of `no such function`.
    for stub in stubs::STUBS {
        register_stub(conn, stub)?;
    }

    Ok(())
}

#[cfg(feature = "transform")]
fn register_transform(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::transform;
    conn.create_scalar_function("ST_Transform", 2, FLAGS, |ctx| {
        let (Some(b), Some(srid)) = (
            blob_or_null(ctx, 0, "ST_Transform")?,
            int_or_null(ctx, 1, "ST_Transform")?,
        ) else {
            return Ok(None);
        };
        blob(transform::st_transform(b, srid))
    })
}

#[cfg(not(feature = "transform"))]
fn register_transform(conn: &Connection) -> rusqlite::Result<()> {
    register_stubs(conn, stubs::TRANSFORM_OFF)
}

#[cfg(feature = "h3")]
fn register_h3(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::h3;
    conn.create_scalar_function("h3_latlng_to_cell", 2, FLAGS, |ctx| {
        let (Some(b), Some(res)) = (
            blob_or_null(ctx, 0, "h3_latlng_to_cell")?,
            i64_or_null(ctx, 1, "h3_latlng_to_cell")?,
        ) else {
            return Ok(None);
        };
        h3::h3_latlng_to_cell(b, res)
            .map(|v| Some(Value::Integer(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("h3_cell_to_parent", 2, FLAGS, |ctx| {
        let (Some(cell), Some(res)) = (
            i64_or_null(ctx, 0, "h3_cell_to_parent")?,
            i64_or_null(ctx, 1, "h3_cell_to_parent")?,
        ) else {
            return Ok(None);
        };
        h3::h3_cell_to_parent(cell, res)
            .map(|v| Some(Value::Integer(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("h3_cell_to_string", 1, FLAGS, |ctx| {
        let Some(cell) = i64_or_null(ctx, 0, "h3_cell_to_string")? else {
            return Ok(None);
        };
        h3::h3_cell_to_string(cell)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("h3_string_to_cell", 1, FLAGS, |ctx| {
        let Some(s) = text_or_null(ctx, 0, "h3_string_to_cell")? else {
            return Ok(None);
        };
        h3::h3_string_to_cell(s)
            .map(|v| Some(Value::Integer(v)))
            .map_err(sql_err)
    })?;
    Ok(())
}

#[cfg(not(feature = "h3"))]
fn register_h3(conn: &Connection) -> rusqlite::Result<()> {
    register_stubs(conn, stubs::H3_OFF)
}

#[cfg(feature = "geojson")]
fn register_geojson(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::geojson;
    conn.create_scalar_function("ST_AsGeoJSON", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_AsGeoJSON")? else {
            return Ok(None);
        };
        geojson::st_as_geojson(b, None)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsGeoJSON", 2, FLAGS, |ctx| {
        let (Some(b), Some(digits)) = (
            blob_or_null(ctx, 0, "ST_AsGeoJSON")?,
            i64_or_null(ctx, 1, "ST_AsGeoJSON")?,
        ) else {
            return Ok(None);
        };
        geojson::st_as_geojson(b, Some(digits))
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_GeomFromGeoJSON", 1, FLAGS, |ctx| {
        let Some(s) = text_or_null(ctx, 0, "ST_GeomFromGeoJSON")? else {
            return Ok(None);
        };
        blob(geojson::st_geom_from_geojson(s))
    })?;
    Ok(())
}

#[cfg(not(feature = "geojson"))]
fn register_geojson(conn: &Connection) -> rusqlite::Result<()> {
    register_stubs(conn, stubs::GEOJSON_OFF)
}

fn register_accessors(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::accessors;
    register_geom_to_real(conn, "ST_Area", accessors::st_area)?;
    register_geom_to_real(conn, "ST_Perimeter", accessors::st_perimeter)?;
    conn.create_scalar_function("ST_NPoints", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_NPoints")? else {
            return Ok(None);
        };
        accessors::st_npoints(b)
            .map(|v| Some(Value::Integer(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_GeometryType", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_GeometryType")? else {
            return Ok(None);
        };
        accessors::st_geometry_type(b)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_NumGeometries", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_NumGeometries")? else {
            return Ok(None);
        };
        accessors::st_num_geometries(b)
            .map(|v| Some(Value::Integer(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_GeometryN", 2, FLAGS, |ctx| {
        let (Some(b), Some(n)) = (
            blob_or_null(ctx, 0, "ST_GeometryN")?,
            i64_or_null(ctx, 1, "ST_GeometryN")?,
        ) else {
            return Ok(None);
        };
        accessors::st_geometry_n(b, n)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    register_geom_to_opt_blob(conn, "ST_StartPoint", accessors::st_start_point)?;
    register_geom_to_opt_blob(conn, "ST_EndPoint", accessors::st_end_point)?;
    conn.create_scalar_function("ST_PointN", 2, FLAGS, |ctx| {
        let (Some(b), Some(n)) = (
            blob_or_null(ctx, 0, "ST_PointN")?,
            i64_or_null(ctx, 1, "ST_PointN")?,
        ) else {
            return Ok(None);
        };
        accessors::st_point_n(b, n)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    register_geom_to_blob(conn, "ST_Reverse", accessors::st_reverse)?;
    conn.create_scalar_function("ST_MakePoint", 2, FLAGS, |ctx| {
        let (Some(x), Some(y)) = (
            real_or_null(ctx, 0, "ST_MakePoint")?,
            real_or_null(ctx, 1, "ST_MakePoint")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_make_point(x, y))
    })?;
    conn.create_scalar_function("ST_Point", 2, FLAGS, |ctx| {
        let (Some(x), Some(y)) = (
            real_or_null(ctx, 0, "ST_Point")?,
            real_or_null(ctx, 1, "ST_Point")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_point(x, y, None))
    })?;
    conn.create_scalar_function("ST_Point", 3, FLAGS, |ctx| {
        let (Some(x), Some(y), Some(srid)) = (
            real_or_null(ctx, 0, "ST_Point")?,
            real_or_null(ctx, 1, "ST_Point")?,
            int_or_null(ctx, 2, "ST_Point")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_point(x, y, Some(srid)))
    })?;
    conn.create_scalar_function("ST_MakeEnvelope", 4, FLAGS, |ctx| {
        let (Some(xmin), Some(ymin), Some(xmax), Some(ymax)) = (
            real_or_null(ctx, 0, "ST_MakeEnvelope")?,
            real_or_null(ctx, 1, "ST_MakeEnvelope")?,
            real_or_null(ctx, 2, "ST_MakeEnvelope")?,
            real_or_null(ctx, 3, "ST_MakeEnvelope")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_make_envelope(xmin, ymin, xmax, ymax, None))
    })?;
    conn.create_scalar_function("ST_MakeEnvelope", 5, FLAGS, |ctx| {
        let (Some(xmin), Some(ymin), Some(xmax), Some(ymax), Some(srid)) = (
            real_or_null(ctx, 0, "ST_MakeEnvelope")?,
            real_or_null(ctx, 1, "ST_MakeEnvelope")?,
            real_or_null(ctx, 2, "ST_MakeEnvelope")?,
            real_or_null(ctx, 3, "ST_MakeEnvelope")?,
            int_or_null(ctx, 4, "ST_MakeEnvelope")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_make_envelope(xmin, ymin, xmax, ymax, Some(srid)))
    })?;
    conn.create_scalar_function("GPKG_IsAssignable", 2, FLAGS, |ctx| {
        let (Some(expected), Some(actual)) = (
            text_or_null(ctx, 0, "GPKG_IsAssignable")?,
            text_or_null(ctx, 1, "GPKG_IsAssignable")?,
        ) else {
            return Ok(None);
        };
        rtree::gpkg_is_assignable(expected, actual)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;
    register_geom_to_real(conn, "ST_Length", accessors::st_length)?;
    register_geom_to_blob(conn, "ST_Centroid", accessors::st_centroid)?;
    register_geom_to_blob(conn, "ST_Envelope", accessors::st_envelope)?;
    register_rtree_minmax(conn, "ST_X", accessors::st_x)?;
    register_rtree_minmax(conn, "ST_Y", accessors::st_y)?;
    conn.create_scalar_function("ST_NumPoints", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_NumPoints")? else {
            return Ok(None);
        };
        accessors::st_num_points(b)
            .map(|v| v.map(Value::Integer))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_IsValid", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_IsValid")? else {
            return Ok(None);
        };
        accessors::st_is_valid(b)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_Simplify", 2, FLAGS, |ctx| {
        let (Some(b), Some(tol)) = (
            blob_or_null(ctx, 0, "ST_Simplify")?,
            real_or_null(ctx, 1, "ST_Simplify")?,
        ) else {
            return Ok(None);
        };
        blob(accessors::st_simplify(b, tol))
    })?;
    Ok(())
}

fn register_stub(conn: &Connection, stub: &'static stubs::Stub) -> rusqlite::Result<()> {
    let (name, hint) = (stub.name, stub.hint);
    conn.create_scalar_function(
        name,
        -1,
        FLAGS,
        move |_ctx| -> rusqlite::Result<Option<Value>> {
            Err(sql_err(Error::Unimplemented { func: name, hint }))
        },
    )
}

#[allow(dead_code)]
fn register_stubs(conn: &Connection, list: &'static [stubs::Stub]) -> rusqlite::Result<()> {
    for stub in list {
        register_stub(conn, stub)?;
    }
    Ok(())
}

fn register_predicate(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8], &[u8]) -> crate::error::Result<bool>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
        let (Some(a), Some(b)) = (blob_or_null(ctx, 0, name)?, blob_or_null(ctx, 1, name)?) else {
            return Ok(None);
        };
        f(a, b)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })
}

fn register_geom_to_real(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8]) -> crate::error::Result<f64>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
        let Some(b) = blob_or_null(ctx, 0, name)? else {
            return Ok(None);
        };
        f(b).map(|v| Some(Value::Real(v))).map_err(sql_err)
    })
}

fn register_geom_to_blob(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8]) -> crate::error::Result<Vec<u8>>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
        let Some(b) = blob_or_null(ctx, 0, name)? else {
            return Ok(None);
        };
        blob(f(b))
    })
}

fn register_geom2_to_blob(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8], &[u8]) -> crate::error::Result<Vec<u8>>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
        let (Some(a), Some(b)) = (blob_or_null(ctx, 0, name)?, blob_or_null(ctx, 1, name)?) else {
            return Ok(None);
        };
        blob(f(a, b))
    })
}

fn register_geom_to_opt_blob(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8]) -> crate::error::Result<Option<Vec<u8>>>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
        let Some(b) = blob_or_null(ctx, 0, name)? else {
            return Ok(None);
        };
        f(b).map(|v| v.map(Value::Blob)).map_err(sql_err)
    })
}

fn register_rtree_minmax(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8]) -> crate::error::Result<Option<f64>>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
        let Some(b) = blob_or_null(ctx, 0, name)? else {
            return Ok(None);
        };
        f(b).map(|v| v.map(Value::Real)).map_err(sql_err)
    })
}

fn sql_err(e: Error) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(Box::new(e))
}

fn blob(r: crate::error::Result<Vec<u8>>) -> rusqlite::Result<Option<Value>> {
    r.map(|b| Some(Value::Blob(b))).map_err(sql_err)
}

fn blob_or_null<'a>(
    ctx: &'a Context<'_>,
    i: usize,
    func: &'static str,
) -> rusqlite::Result<Option<&'a [u8]>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Blob(b) => Ok(Some(b)),
        ValueRef::Text(_) => Err(sql_err(Error::Unsupported {
            func,
            reason: "got TEXT where a geometry BLOB was expected (did you mean ST_GeomFromText?)"
                .into(),
        })),
        other => Err(sql_err(Error::Unsupported {
            func,
            reason: format!("expected a geometry BLOB, got {}", other.data_type()),
        })),
    }
}

fn text_or_null<'a>(
    ctx: &'a Context<'_>,
    i: usize,
    func: &'static str,
) -> rusqlite::Result<Option<&'a str>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Text(t) => std::str::from_utf8(t)
            .map(Some)
            .map_err(|e| sql_err(Error::InvalidWkt(e.to_string()))),
        other => Err(sql_err(Error::Unsupported {
            func,
            reason: format!("expected TEXT, got {}", other.data_type()),
        })),
    }
}

/// `Kind::TextOrInt`: TEXT as-is; INTEGER n normalized to `quad_segs=n`
/// (ST_Buffer's PostGIS integer overload).
fn text_or_int_or_null(
    ctx: &Context<'_>,
    i: usize,
    func: &'static str,
) -> rusqlite::Result<Option<String>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Text(t) => std::str::from_utf8(t)
            .map(|s| Some(s.to_string()))
            .map_err(|e| sql_err(Error::InvalidWkt(e.to_string()))),
        ValueRef::Integer(n) => Ok(Some(format!("quad_segs={n}"))),
        other => Err(sql_err(Error::Unsupported {
            func,
            reason: format!(
                "expected TEXT options or INTEGER, got {}",
                other.data_type()
            ),
        })),
    }
}

#[allow(dead_code)]
fn i64_or_null(ctx: &Context<'_>, i: usize, func: &'static str) -> rusqlite::Result<Option<i64>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Integer(v) => Ok(Some(v)),
        other => Err(sql_err(Error::Unsupported {
            func,
            reason: format!("expected an INTEGER, got {}", other.data_type()),
        })),
    }
}

fn int_or_null(ctx: &Context<'_>, i: usize, func: &'static str) -> rusqlite::Result<Option<i32>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Integer(v) => Ok(Some(v as i32)),
        other => Err(sql_err(Error::Unsupported {
            func,
            reason: format!("expected an INTEGER srid, got {}", other.data_type()),
        })),
    }
}

fn real_or_null(ctx: &Context<'_>, i: usize, func: &'static str) -> rusqlite::Result<Option<f64>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Real(v) => Ok(Some(v)),
        ValueRef::Integer(v) => Ok(Some(v as f64)),
        other => Err(sql_err(Error::Unsupported {
            func,
            reason: format!("expected a numeric distance, got {}", other.data_type()),
        })),
    }
}
