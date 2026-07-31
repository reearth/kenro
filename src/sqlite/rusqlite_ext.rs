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

    // Stubs: known-but-unimplemented ST_ functions fail with a helpful
    // message instead of `no such function`.
    for stub in stubs::STUBS {
        let (name, hint) = (stub.name, stub.hint);
        conn.create_scalar_function(
            name,
            -1,
            FLAGS,
            move |_ctx| -> rusqlite::Result<Option<Value>> {
                Err(sql_err(Error::Unimplemented { func: name, hint }))
            },
        )?;
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
