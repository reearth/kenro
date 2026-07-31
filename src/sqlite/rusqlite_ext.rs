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
