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
use crate::functions::{compat, io, predicates, rtree, stubs};

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
    #[cfg(feature = "overlay")]
    {
        use crate::functions::overlay;
        register_geom2_to_blob(conn, "ST_Intersection", overlay::st_intersection)?;
        register_geom2_to_blob(conn, "ST_Difference", overlay::st_difference)?;
        register_geom2_to_blob(conn, "ST_SymDifference", overlay::st_sym_difference)?;
        // PostGIS accepts both spellings; so does kenro.
        register_geom2_to_blob(conn, "ST_SymmetricDifference", overlay::st_sym_difference)?;
        register_geom2_to_blob(conn, "ST_Union", overlay::st_union)?;
        register_geom_to_blob(conn, "ST_UnaryUnion", overlay::st_unary_union)?;
        register_geom2_to_blob(conn, "ST_ClipByBox2D", overlay::st_clip_by_box_2d)?;
        register_geom2_to_blob(conn, "ST_Split", crate::functions::lines::st_split)?;
        conn.create_scalar_function("ST_Subdivide", 2, FLAGS, |ctx| {
            let (Some(g), Some(max)) = (
                blob_or_null(ctx, 0, "ST_Subdivide")?,
                i64_or_null(ctx, 1, "ST_Subdivide")?,
            ) else {
                return Ok(None);
            };
            blob(overlay::st_subdivide(g, max))
        })?;
        register_geom_to_blob(conn, "ST_MakeValid", overlay::st_make_valid)?;
        conn.create_scalar_function("ST_Buffer", 2, FLAGS, |ctx| {
            let (Some(b), Some(d)) = (
                blob_or_null(ctx, 0, "ST_Buffer")?,
                real_or_null(ctx, 1, "ST_Buffer")?,
            ) else {
                return Ok(None);
            };
            blob(overlay::st_buffer(b, d, None))
        })?;
        // 1-arg aggregate ST_Union(geom): NULL rows skipped (PostGIS
        // aggregate semantics), zero rows → NULL.
        struct UnionAgg;
        impl rusqlite::functions::Aggregate<overlay::UnionAggregate, Option<Value>> for UnionAgg {
            fn init(&self, _: &mut Context<'_>) -> rusqlite::Result<overlay::UnionAggregate> {
                Ok(overlay::UnionAggregate::new())
            }
            fn step(
                &self,
                ctx: &mut Context<'_>,
                acc: &mut overlay::UnionAggregate,
            ) -> rusqlite::Result<()> {
                match blob_or_null(ctx, 0, "ST_Union")? {
                    None => Ok(()),
                    Some(b) => acc.step(b).map_err(sql_err),
                }
            }
            fn finalize(
                &self,
                _: &mut Context<'_>,
                acc: Option<overlay::UnionAggregate>,
            ) -> rusqlite::Result<Option<Value>> {
                match acc {
                    None => Ok(None),
                    Some(agg) => agg.finish().map(|o| o.map(Value::Blob)).map_err(sql_err),
                }
            }
        }
        conn.create_aggregate_function("ST_Union", 1, FLAGS, UnionAgg)?;
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

    // MVT.
    #[cfg(feature = "mvt")]
    {
        use crate::functions::mvt as fmvt;
        // ST_AsMVTGeom /2../5: trailing Int args are optional.
        for arity in 2..=5 {
            conn.create_scalar_function("ST_AsMVTGeom", arity, FLAGS, move |ctx| {
                const NAME: &str = "ST_AsMVTGeom";
                let (Some(g), Some(bounds)) =
                    (blob_or_null(ctx, 0, NAME)?, blob_or_null(ctx, 1, NAME)?)
                else {
                    return Ok(None);
                };
                let mut opts = [None, None, None];
                for (slot, opt) in opts.iter_mut().enumerate().take(arity as usize - 2) {
                    let Some(v) = int_or_null(ctx, slot + 2, NAME)? else {
                        return Ok(None);
                    };
                    *opt = Some(v);
                }
                fmvt::st_as_mvt_geom(g, bounds, opts[0], opts[1], opts[2])
                    .map(|v| v.map(Value::Blob))
                    .map_err(sql_err)
            })?;
        }
        // ST_AsMVT /1../4 aggregate: (geom [, name [, extent [, props_json]]]).
        struct MvtAgg;
        impl rusqlite::functions::Aggregate<fmvt::MvtAggregate, Option<Value>> for MvtAgg {
            fn init(&self, _: &mut Context<'_>) -> rusqlite::Result<fmvt::MvtAggregate> {
                Ok(fmvt::MvtAggregate::new())
            }
            fn step(
                &self,
                ctx: &mut Context<'_>,
                acc: &mut fmvt::MvtAggregate,
            ) -> rusqlite::Result<()> {
                const NAME: &str = "ST_AsMVT";
                // Any NULL argument skips the row (aggregate convention,
                // identical across all bindings).
                let Some(geom) = blob_or_null(ctx, 0, NAME)? else {
                    return Ok(());
                };
                let name = if ctx.len() > 1 {
                    match text_or_null(ctx, 1, NAME)? {
                        None => return Ok(()),
                        some => some,
                    }
                } else {
                    None
                };
                let extent = if ctx.len() > 2 {
                    match int_or_null(ctx, 2, NAME)? {
                        None => return Ok(()),
                        some => some,
                    }
                } else {
                    None
                };
                let props = if ctx.len() > 3 {
                    match text_or_null(ctx, 3, NAME)? {
                        None => return Ok(()),
                        some => some,
                    }
                } else {
                    None
                };
                acc.step(geom, name, extent, props).map_err(sql_err)
            }
            fn finalize(
                &self,
                _: &mut Context<'_>,
                acc: Option<fmvt::MvtAggregate>,
            ) -> rusqlite::Result<Option<Value>> {
                match acc {
                    None => Ok(None),
                    Some(agg) => agg.finish().map(|o| o.map(Value::Blob)).map_err(sql_err),
                }
            }
        }
        for arity in 1..=4 {
            conn.create_aggregate_function("ST_AsMVT", arity, FLAGS, MvtAgg)?;
        }
    }

    // Routing aggregates: kenro_dijkstra(id, source, target, cost,
    // start_vid, end_vid [, reverse_cost]) and kenro_dijkstra_cost without
    // the id column. reverse_cost trails deliberately — see
    // functions::routing's module doc.
    #[cfg(feature = "routing")]
    {
        use crate::functions::routing;
        struct DijkstraAgg;
        impl rusqlite::functions::Aggregate<routing::DijkstraAggregate, Option<Value>> for DijkstraAgg {
            fn init(&self, _: &mut Context<'_>) -> rusqlite::Result<routing::DijkstraAggregate> {
                Ok(routing::DijkstraAggregate::new())
            }
            fn step(
                &self,
                ctx: &mut Context<'_>,
                acc: &mut routing::DijkstraAggregate,
            ) -> rusqlite::Result<()> {
                const NAME: &str = "kenro_dijkstra";
                // Any NULL argument skips the row (aggregate convention,
                // identical across all bindings).
                let (Some(id), Some(source), Some(target), Some(cost), Some(start), Some(end)) = (
                    int_or_null(ctx, 0, NAME)?,
                    int_or_null(ctx, 1, NAME)?,
                    int_or_null(ctx, 2, NAME)?,
                    real_or_null(ctx, 3, NAME)?,
                    int_or_null(ctx, 4, NAME)?,
                    int_or_null(ctx, 5, NAME)?,
                ) else {
                    return Ok(());
                };
                let reverse = if ctx.len() > 6 {
                    match real_or_null(ctx, 6, NAME)? {
                        None => return Ok(()),
                        some => some,
                    }
                } else {
                    None
                };
                acc.step(id, source, target, cost, start, end, reverse)
                    .map_err(sql_err)
            }
            fn finalize(
                &self,
                _: &mut Context<'_>,
                acc: Option<routing::DijkstraAggregate>,
            ) -> rusqlite::Result<Option<Value>> {
                match acc {
                    None => Ok(None),
                    Some(agg) => agg.finish().map(|o| o.map(Value::Text)).map_err(sql_err),
                }
            }
        }
        for arity in 6..=7 {
            conn.create_aggregate_function("kenro_dijkstra", arity, FLAGS, DijkstraAgg)?;
        }

        struct DijkstraCostAgg;
        impl rusqlite::functions::Aggregate<routing::DijkstraCostAggregate, Option<Value>>
            for DijkstraCostAgg
        {
            fn init(
                &self,
                _: &mut Context<'_>,
            ) -> rusqlite::Result<routing::DijkstraCostAggregate> {
                Ok(routing::DijkstraCostAggregate::new())
            }
            fn step(
                &self,
                ctx: &mut Context<'_>,
                acc: &mut routing::DijkstraCostAggregate,
            ) -> rusqlite::Result<()> {
                const NAME: &str = "kenro_dijkstra_cost";
                let (Some(source), Some(target), Some(cost), Some(start), Some(end)) = (
                    int_or_null(ctx, 0, NAME)?,
                    int_or_null(ctx, 1, NAME)?,
                    real_or_null(ctx, 2, NAME)?,
                    int_or_null(ctx, 3, NAME)?,
                    int_or_null(ctx, 4, NAME)?,
                ) else {
                    return Ok(());
                };
                let reverse = if ctx.len() > 5 {
                    match real_or_null(ctx, 5, NAME)? {
                        None => return Ok(()),
                        some => some,
                    }
                } else {
                    None
                };
                acc.step(source, target, cost, start, end, reverse)
                    .map_err(sql_err)
            }
            fn finalize(
                &self,
                _: &mut Context<'_>,
                acc: Option<routing::DijkstraCostAggregate>,
            ) -> rusqlite::Result<Option<Value>> {
                match acc {
                    None => Ok(None),
                    Some(agg) => agg.finish().map(|o| o.map(Value::Real)).map_err(sql_err),
                }
            }
        }
        for arity in 5..=6 {
            conn.create_aggregate_function("kenro_dijkstra_cost", arity, FLAGS, DijkstraCostAgg)?;
        }

        struct DrivingDistAgg;
        impl rusqlite::functions::Aggregate<routing::DrivingDistanceAggregate, Option<Value>>
            for DrivingDistAgg
        {
            fn init(
                &self,
                _: &mut Context<'_>,
            ) -> rusqlite::Result<routing::DrivingDistanceAggregate> {
                Ok(routing::DrivingDistanceAggregate::new())
            }
            fn step(
                &self,
                ctx: &mut Context<'_>,
                acc: &mut routing::DrivingDistanceAggregate,
            ) -> rusqlite::Result<()> {
                const NAME: &str = "kenro_drivingdistance";
                let (Some(id), Some(source), Some(target), Some(cost), Some(start), Some(limit)) = (
                    int_or_null(ctx, 0, NAME)?,
                    int_or_null(ctx, 1, NAME)?,
                    int_or_null(ctx, 2, NAME)?,
                    real_or_null(ctx, 3, NAME)?,
                    int_or_null(ctx, 4, NAME)?,
                    real_or_null(ctx, 5, NAME)?,
                ) else {
                    return Ok(());
                };
                let reverse = if ctx.len() > 6 {
                    match real_or_null(ctx, 6, NAME)? {
                        None => return Ok(()),
                        some => some,
                    }
                } else {
                    None
                };
                acc.step(id, source, target, cost, start, limit, reverse)
                    .map_err(sql_err)
            }
            fn finalize(
                &self,
                _: &mut Context<'_>,
                acc: Option<routing::DrivingDistanceAggregate>,
            ) -> rusqlite::Result<Option<Value>> {
                match acc {
                    None => Ok(None),
                    Some(agg) => agg.finish().map(|o| o.map(Value::Text)).map_err(sql_err),
                }
            }
        }
        for arity in 6..=7 {
            conn.create_aggregate_function("kenro_drivingdistance", arity, FLAGS, DrivingDistAgg)?;
        }
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
    register_box_accessor(conn, "ST_MinX", rtree::st_min_x)?;
    register_box_accessor(conn, "ST_MaxX", rtree::st_max_x)?;
    register_box_accessor(conn, "ST_MinY", rtree::st_min_y)?;
    register_box_accessor(conn, "ST_MaxY", rtree::st_max_y)?;
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
    register_compat(conn)?;
    register_edit(conn)?;
    register_geodesic_and_linear(conn)?;
    register_extra(conn)?;
    register_hull(conn)?;
    register_misc(conn)?;
    register_threed(conn)?;
    register_surface(conn)?;
    register_lines(conn)?;
    register_grid(conn)?;
    #[cfg(feature = "gml")]
    register_gml(conn)?;
    #[cfg(feature = "text-encodings")]
    register_text_encodings(conn)?;

    // Stubs: known-but-unimplemented ST_ functions fail with a helpful
    // message instead of `no such function`.
    for stub in stubs::STUBS {
        register_stub(conn, stub)?;
    }
    #[cfg(not(feature = "overlay"))]
    register_stubs(conn, stubs::OVERLAY_OFF)?;
    #[cfg(not(feature = "mvt"))]
    register_stubs(conn, stubs::MVT_OFF)?;
    #[cfg(not(feature = "spheroid"))]
    register_stubs(conn, stubs::SPHEROID_OFF)?;
    #[cfg(not(feature = "concave-hull"))]
    register_stubs(conn, stubs::CONCAVE_HULL_OFF)?;
    #[cfg(not(feature = "text-encodings"))]
    register_stubs(conn, stubs::TEXT_ENCODINGS_OFF)?;
    #[cfg(not(feature = "voronoi"))]
    register_stubs(conn, stubs::VORONOI_OFF)?;
    #[cfg(not(feature = "delaunay"))]
    register_stubs(conn, stubs::DELAUNAY_OFF)?;
    #[cfg(not(feature = "gml"))]
    register_stubs(conn, stubs::GML_OFF)?;
    #[cfg(not(feature = "routing"))]
    register_stubs(conn, stubs::ROUTING_OFF)?;

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

/// PostGIS name compatibility (see `functions::compat`): alternative
/// spellings for functions already registered above, plus the EWKT/EWKB pair
/// and the typed constructors.
fn register_compat(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::compat::Expect;

    // Same code, PostGIS's spelling.
    register_box_accessor(conn, "ST_XMin", rtree::st_min_x)?;
    register_box_accessor(conn, "ST_XMax", rtree::st_max_x)?;
    register_box_accessor(conn, "ST_YMin", rtree::st_min_y)?;
    register_box_accessor(conn, "ST_YMax", rtree::st_max_y)?;
    register_geom_to_real(conn, "ST_Area2D", crate::functions::accessors::st_area)?;
    register_geom_to_real(
        conn,
        "ST_Perimeter2D",
        crate::functions::accessors::st_perimeter,
    )?;
    register_geom_to_real(conn, "ST_Length2D", crate::functions::accessors::st_length)?;
    conn.create_scalar_function("ST_GeometryFromText", 1, FLAGS, |ctx| {
        let Some(wkt) = text_or_null(ctx, 0, "ST_GeometryFromText")? else {
            return Ok(None);
        };
        blob(io::st_geom_from_text(wkt, None))
    })?;
    conn.create_scalar_function("ST_GeometryFromText", 2, FLAGS, |ctx| {
        let (Some(wkt), Some(srid)) = (
            text_or_null(ctx, 0, "ST_GeometryFromText")?,
            int_or_null(ctx, 1, "ST_GeometryFromText")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_geom_from_text(wkt, Some(srid)))
    })?;
    conn.create_scalar_function("ST_GeomFromEWKB", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_GeomFromEWKB")? else {
            return Ok(None);
        };
        blob(io::st_geom_from_wkb(b, None))
    })?;

    // New code, all of it small.
    // ---- the two SFCGAL measurements (functions::threed_solid) ----
    use crate::functions::threed_solid;
    register_geom_to_real(conn, "ST_3DArea", threed_solid::st_3d_area)?;
    conn.create_scalar_function("kenro_volume", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "kenro_volume")? else {
            return Ok(None);
        };
        threed_solid::kenro_volume(b)
            .map(|v| v.map(Value::Real))
            .map_err(sql_err)
    })?;

    // ---- the core-PostGIS 3D metric family (functions::threed_metric) ----
    use crate::functions::threed_metric as m3;
    for (name, f) in [
        (
            "ST_3DDistance",
            m3::st_3d_distance as fn(&[u8], &[u8]) -> crate::error::Result<Option<f64>>,
        ),
        ("ST_3DMaxDistance", m3::st_3d_max_distance),
    ] {
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(a), Some(b)) = (blob_or_null(ctx, 0, name)?, blob_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            f(a, b).map(|v| v.map(Value::Real)).map_err(sql_err)
        })?;
    }
    conn.create_scalar_function("ST_3DIntersects", 2, FLAGS, |ctx| {
        let (Some(a), Some(b)) = (
            blob_or_null(ctx, 0, "ST_3DIntersects")?,
            blob_or_null(ctx, 1, "ST_3DIntersects")?,
        ) else {
            return Ok(None);
        };
        m3::st_3d_intersects(a, b)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;
    for (name, f) in [
        (
            "ST_3DDWithin",
            m3::st_3d_dwithin as fn(&[u8], &[u8], f64) -> crate::error::Result<bool>,
        ),
        ("ST_3DDFullyWithin", m3::st_3d_dfully_within),
    ] {
        conn.create_scalar_function(name, 3, FLAGS, move |ctx| {
            let (Some(a), Some(b), Some(d)) = (
                blob_or_null(ctx, 0, name)?,
                blob_or_null(ctx, 1, name)?,
                real_or_null(ctx, 2, name)?,
            ) else {
                return Ok(None);
            };
            f(a, b, d)
                .map(|v| Some(Value::Integer(v as i64)))
                .map_err(sql_err)
        })?;
    }
    for (name, f) in [
        (
            "ST_3DClosestPoint",
            m3::st_3d_closest_point as fn(&[u8], &[u8]) -> crate::error::Result<Option<Vec<u8>>>,
        ),
        ("ST_3DShortestLine", m3::st_3d_shortest_line),
        ("ST_3DLongestLine", m3::st_3d_longest_line),
    ] {
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(a), Some(b)) = (blob_or_null(ctx, 0, name)?, blob_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            f(a, b).map(|v| v.map(Value::Blob)).map_err(sql_err)
        })?;
    }
    conn.create_scalar_function("ST_3DLineInterpolatePoint", 2, FLAGS, |ctx| {
        let (Some(g), Some(f)) = (
            blob_or_null(ctx, 0, "ST_3DLineInterpolatePoint")?,
            real_or_null(ctx, 1, "ST_3DLineInterpolatePoint")?,
        ) else {
            return Ok(None);
        };
        blob(m3::st_3d_line_interpolate_point(g, f))
    })?;
    register_geom_to_blob(conn, "ST_Force2D", compat::st_force_2d)?;
    // ST_Force3D / ST_Force3DZ, one and two arguments. The default zvalue is 0,
    // as in PostGIS.
    for name in ["ST_Force3D", "ST_Force3DZ"] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(g) = blob_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            blob(compat::st_force_3d(g, 0.0))
        })?;
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(g), Some(z)) = (blob_or_null(ctx, 0, name)?, real_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            blob(compat::st_force_3d(g, z))
        })?;
    }
    conn.create_scalar_function("ST_MakePoint", 3, FLAGS, |ctx| {
        let (Some(x), Some(y), Some(z)) = (
            real_or_null(ctx, 0, "ST_MakePoint")?,
            real_or_null(ctx, 1, "ST_MakePoint")?,
            real_or_null(ctx, 2, "ST_MakePoint")?,
        ) else {
            return Ok(None);
        };
        blob(io::st_make_point_z(x, y, z))
    })?;
    conn.create_scalar_function("ST_AsEWKT", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_AsEWKT")? else {
            return Ok(None);
        };
        compat::st_as_ewkt(b)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsHexEWKB", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_AsHexEWKB")? else {
            return Ok(None);
        };
        compat::st_as_hex_ewkb(b)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    register_geom_to_blob(conn, "ST_AsEWKB", compat::st_as_ewkb)?;
    conn.create_scalar_function("ST_GeomFromEWKT", 1, FLAGS, |ctx| {
        let Some(t) = text_or_null(ctx, 0, "ST_GeomFromEWKT")? else {
            return Ok(None);
        };
        blob(compat::st_geom_from_ewkt(t))
    })?;

    for (name, expect) in [
        ("ST_PointFromText", Expect::Point),
        ("ST_LineFromText", Expect::LineString),
        ("ST_LineStringFromText", Expect::LineString),
        ("ST_PolyFromText", Expect::Polygon),
        ("ST_PolygonFromText", Expect::Polygon),
        ("ST_MPointFromText", Expect::MultiPoint),
        ("ST_MLineFromText", Expect::MultiLineString),
        ("ST_MPolyFromText", Expect::MultiPolygon),
    ] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(t) = text_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            compat::from_text_typed(t, None, expect)
                .map(|v| v.map(Value::Blob))
                .map_err(sql_err)
        })?;
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(t), Some(srid)) = (text_or_null(ctx, 0, name)?, int_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            compat::from_text_typed(t, Some(srid), expect)
                .map(|v| v.map(Value::Blob))
                .map_err(sql_err)
        })?;
    }

    for (name, expect) in [
        ("ST_PointFromWKB", Expect::Point),
        ("ST_LineFromWKB", Expect::LineString),
        ("ST_PolyFromWKB", Expect::Polygon),
    ] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(b) = blob_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            compat::from_wkb_typed(b, None, expect)
                .map(|v| v.map(Value::Blob))
                .map_err(sql_err)
        })?;
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(b), Some(srid)) = (blob_or_null(ctx, 0, name)?, int_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            compat::from_wkb_typed(b, Some(srid), expect)
                .map(|v| v.map(Value::Blob))
                .map_err(sql_err)
        })?;
    }
    Ok(())
}

/// Structural accessors and geometry editing (see `functions::edit`).
fn register_edit(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::edit;

    register_geom_to_opt_blob(conn, "ST_ExteriorRing", edit::st_exterior_ring)?;
    register_geom_to_blob(conn, "ST_Boundary", edit::st_boundary)?;
    register_geom_to_blob(conn, "ST_MakePolygon", edit::st_make_polygon)?;
    register_geom_to_blob(conn, "ST_Multi", edit::st_multi)?;
    register_geom_to_blob(conn, "ST_FlipCoordinates", edit::st_flip_coordinates)?;
    register_geom_to_blob(conn, "ST_ShiftLongitude", edit::st_shift_longitude)?;
    register_predicate_1(conn, "ST_IsClosed", edit::st_is_closed)?;
    register_predicate_1(conn, "ST_IsRing", edit::st_is_ring)?;

    conn.create_scalar_function("ST_InteriorRingN", 2, FLAGS, |ctx| {
        let (Some(b), Some(n)) = (
            blob_or_null(ctx, 0, "ST_InteriorRingN")?,
            i64_or_null(ctx, 1, "ST_InteriorRingN")?,
        ) else {
            return Ok(None);
        };
        edit::st_interior_ring_n(b, n)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    for name in ["ST_NumInteriorRings", "ST_NumInteriorRing"] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(b) = blob_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            edit::st_num_interior_rings(b)
                .map(|v| v.map(Value::Integer))
                .map_err(sql_err)
        })?;
    }
    conn.create_scalar_function("ST_NRings", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_NRings")? else {
            return Ok(None);
        };
        edit::st_nrings(b)
            .map(|v| Some(Value::Integer(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AddPoint", 2, FLAGS, |ctx| {
        let (Some(l), Some(p)) = (
            blob_or_null(ctx, 0, "ST_AddPoint")?,
            blob_or_null(ctx, 1, "ST_AddPoint")?,
        ) else {
            return Ok(None);
        };
        edit::st_add_point(l, p, None)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AddPoint", 3, FLAGS, |ctx| {
        let (Some(l), Some(p), Some(at)) = (
            blob_or_null(ctx, 0, "ST_AddPoint")?,
            blob_or_null(ctx, 1, "ST_AddPoint")?,
            i64_or_null(ctx, 2, "ST_AddPoint")?,
        ) else {
            return Ok(None);
        };
        edit::st_add_point(l, p, Some(at))
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_SetPoint", 3, FLAGS, |ctx| {
        let (Some(l), Some(n), Some(p)) = (
            blob_or_null(ctx, 0, "ST_SetPoint")?,
            i64_or_null(ctx, 1, "ST_SetPoint")?,
            blob_or_null(ctx, 2, "ST_SetPoint")?,
        ) else {
            return Ok(None);
        };
        edit::st_set_point(l, n, p)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_RemovePoint", 2, FLAGS, |ctx| {
        let (Some(l), Some(n)) = (
            blob_or_null(ctx, 0, "ST_RemovePoint")?,
            i64_or_null(ctx, 1, "ST_RemovePoint")?,
        ) else {
            return Ok(None);
        };
        edit::st_remove_point(l, n)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_MakeLine", 2, FLAGS, |ctx| {
        let (Some(a), Some(b)) = (
            blob_or_null(ctx, 0, "ST_MakeLine")?,
            blob_or_null(ctx, 1, "ST_MakeLine")?,
        ) else {
            return Ok(None);
        };
        blob(edit::st_make_line(a, b))
    })?;
    conn.create_scalar_function("ST_SnapToGrid", 2, FLAGS, |ctx| {
        let (Some(b), Some(size)) = (
            blob_or_null(ctx, 0, "ST_SnapToGrid")?,
            real_or_null(ctx, 1, "ST_SnapToGrid")?,
        ) else {
            return Ok(None);
        };
        blob(edit::st_snap_to_grid(b, size, size))
    })?;
    conn.create_scalar_function("ST_SnapToGrid", 3, FLAGS, |ctx| {
        let (Some(b), Some(sx), Some(sy)) = (
            blob_or_null(ctx, 0, "ST_SnapToGrid")?,
            real_or_null(ctx, 1, "ST_SnapToGrid")?,
            real_or_null(ctx, 2, "ST_SnapToGrid")?,
        ) else {
            return Ok(None);
        };
        blob(edit::st_snap_to_grid(b, sx, sy))
    })?;
    conn.create_scalar_function("ST_Expand", 2, FLAGS, |ctx| {
        let (Some(b), Some(units)) = (
            blob_or_null(ctx, 0, "ST_Expand")?,
            real_or_null(ctx, 1, "ST_Expand")?,
        ) else {
            return Ok(None);
        };
        edit::st_expand(b, units)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    Ok(())
}

/// One-argument boolean predicate (ST_IsClosed, ST_IsRing).
fn register_predicate_1(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8]) -> crate::error::Result<bool>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
        let Some(b) = blob_or_null(ctx, 0, name)? else {
            return Ok(None);
        };
        f(b).map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })
}

/// Sphere/spheroid measures, dimension reporting, ring orientation and
/// linear referencing (`functions::geodesic`, `edit`, `linear`).
fn register_geodesic_and_linear(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::{accessors, edit, geodesic, linear};

    register_geom2_to_real(conn, "ST_DistanceSphere", geodesic::st_distance_sphere)?;
    #[cfg(feature = "spheroid")]
    register_geom2_to_real(conn, "ST_DistanceSpheroid", geodesic::st_distance_spheroid)?;
    #[cfg(feature = "spheroid")]
    conn.create_scalar_function("ST_DistanceSpheroid", 3, FLAGS, |ctx| {
        let (Some(a), Some(b), Some(s)) = (
            blob_or_null(ctx, 0, "ST_DistanceSpheroid")?,
            blob_or_null(ctx, 1, "ST_DistanceSpheroid")?,
            text_or_null(ctx, 2, "ST_DistanceSpheroid")?,
        ) else {
            return Ok(None);
        };
        geodesic::st_distance_spheroid_on(a, b, s)
            .map(|v| Some(Value::Real(v)))
            .map_err(sql_err)
    })?;
    #[cfg(feature = "spheroid")]
    for name in ["ST_LengthSpheroid", "ST_Length2DSpheroid"] {
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(g), Some(s)) = (blob_or_null(ctx, 0, name)?, text_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            geodesic::st_length_spheroid(g, s)
                .map(|v| Some(Value::Real(v)))
                .map_err(sql_err)
        })?;
    }
    conn.create_scalar_function("ST_Project", 3, FLAGS, |ctx| {
        let (Some(g), Some(d), Some(a)) = (
            blob_or_null(ctx, 0, "ST_Project")?,
            real_or_null(ctx, 1, "ST_Project")?,
            real_or_null(ctx, 2, "ST_Project")?,
        ) else {
            return Ok(None);
        };
        blob(geodesic::st_project(g, d, a))
    })?;

    for (name, f) in [
        (
            "ST_Dimension",
            accessors::st_dimension as fn(&[u8]) -> crate::error::Result<i64>,
        ),
        // Dimensionality comes from the encoding, not from the decoded
        // (always-2D) value — see functions::threed.
        (
            "ST_CoordDim",
            crate::functions::threed::st_coord_dim as fn(&[u8]) -> crate::error::Result<i64>,
        ),
        ("ST_NDims", crate::functions::threed::st_coord_dim),
    ] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(b) = blob_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            f(b).map(|v| Some(Value::Integer(v))).map_err(sql_err)
        })?;
    }
    conn.create_scalar_function("ST_IsValidReason", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_IsValidReason")? else {
            return Ok(None);
        };
        accessors::st_is_valid_reason(b)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;

    register_geom_to_blob(conn, "ST_ForcePolygonCW", edit::st_force_polygon_cw)?;
    register_geom_to_blob(conn, "ST_ForceRHR", edit::st_force_polygon_cw)?;
    register_geom_to_blob(conn, "ST_ForcePolygonCCW", edit::st_force_polygon_ccw)?;
    register_predicate_1(conn, "ST_IsPolygonCW", edit::st_is_polygon_cw)?;
    register_predicate_1(conn, "ST_IsPolygonCCW", edit::st_is_polygon_ccw)?;

    conn.create_scalar_function("ST_Segmentize", 2, FLAGS, |ctx| {
        let (Some(g), Some(max)) = (
            blob_or_null(ctx, 0, "ST_Segmentize")?,
            real_or_null(ctx, 1, "ST_Segmentize")?,
        ) else {
            return Ok(None);
        };
        blob(linear::st_segmentize(g, max))
    })?;
    conn.create_scalar_function("ST_LineSubstring", 3, FLAGS, |ctx| {
        let (Some(g), Some(from), Some(to)) = (
            blob_or_null(ctx, 0, "ST_LineSubstring")?,
            real_or_null(ctx, 1, "ST_LineSubstring")?,
            real_or_null(ctx, 2, "ST_LineSubstring")?,
        ) else {
            return Ok(None);
        };
        linear::st_line_substring(g, from, to)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    for (name, f) in [
        (
            "ST_ShortestLine",
            linear::st_shortest_line as fn(&[u8], &[u8]) -> crate::error::Result<Option<Vec<u8>>>,
        ),
        ("ST_LongestLine", linear::st_longest_line),
    ] {
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(a), Some(b)) = (blob_or_null(ctx, 0, name)?, blob_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            f(a, b).map(|v| v.map(Value::Blob)).map_err(sql_err)
        })?;
    }
    conn.create_scalar_function("ST_MinimumBoundingRadius", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_MinimumBoundingRadius")? else {
            return Ok(None);
        };
        linear::st_minimum_bounding_radius(b)
            .map(|v| v.map(Value::Real))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_MinimumBoundingCircle", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_MinimumBoundingCircle")? else {
            return Ok(None);
        };
        linear::st_minimum_bounding_circle(b, 48)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_MinimumBoundingCircle", 2, FLAGS, |ctx| {
        let (Some(b), Some(segs)) = (
            blob_or_null(ctx, 0, "ST_MinimumBoundingCircle")?,
            i64_or_null(ctx, 1, "ST_MinimumBoundingCircle")?,
        ) else {
            return Ok(None);
        };
        linear::st_minimum_bounding_circle(b, segs)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_MaxDistance", 2, FLAGS, |ctx| {
        let (Some(a), Some(b)) = (
            blob_or_null(ctx, 0, "ST_MaxDistance")?,
            blob_or_null(ctx, 1, "ST_MaxDistance")?,
        ) else {
            return Ok(None);
        };
        linear::st_max_distance(a, b)
            .map(|v| v.map(Value::Real))
            .map_err(sql_err)
    })?;
    Ok(())
}

/// Two geometries in, one REAL out.
fn register_geom2_to_real(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8], &[u8]) -> crate::error::Result<f64>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
        let (Some(a), Some(b)) = (blob_or_null(ctx, 0, name)?, blob_or_null(ctx, 1, name)?) else {
            return Ok(None);
        };
        f(a, b).map(|v| Some(Value::Real(v))).map_err(sql_err)
    })
}

/// The remainder of the reachable PostGIS surface (see `functions::extra`),
/// including the `ST_Extent` aggregate.
fn register_extra(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::extra;

    register_predicate(conn, "ST_ContainsProperly", extra::st_contains_properly)?;
    register_predicate(conn, "ST_OrderingEquals", extra::st_ordering_equals)?;
    register_geom_to_blob(conn, "ST_Points", extra::st_points)?;
    register_geom_to_opt_blob(conn, "ST_BoundingDiagonal", extra::st_bounding_diagonal)?;

    conn.create_scalar_function("ST_DFullyWithin", 3, FLAGS, |ctx| {
        let (Some(a), Some(b), Some(d)) = (
            blob_or_null(ctx, 0, "ST_DFullyWithin")?,
            blob_or_null(ctx, 1, "ST_DFullyWithin")?,
            real_or_null(ctx, 2, "ST_DFullyWithin")?,
        ) else {
            return Ok(None);
        };
        extra::st_d_fully_within(a, b, d)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_RelateMatch", 2, FLAGS, |ctx| {
        let (Some(m), Some(p)) = (
            text_or_null(ctx, 0, "ST_RelateMatch")?,
            text_or_null(ctx, 1, "ST_RelateMatch")?,
        ) else {
            return Ok(None);
        };
        extra::st_relate_match(m, p)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_Affine", 7, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_Affine")? else {
            return Ok(None);
        };
        let mut args = [0.0f64; 6];
        for (i, slot) in args.iter_mut().enumerate() {
            let Some(v) = real_or_null(ctx, i + 1, "ST_Affine")? else {
                return Ok(None);
            };
            *slot = v;
        }
        blob(extra::st_affine(
            g, args[0], args[1], args[2], args[3], args[4], args[5],
        ))
    })?;
    conn.create_scalar_function("ST_Affine", 13, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_Affine")? else {
            return Ok(None);
        };
        let mut args = [0.0f64; 12];
        for (i, slot) in args.iter_mut().enumerate() {
            let Some(v) = real_or_null(ctx, i + 1, "ST_Affine")? else {
                return Ok(None);
            };
            *slot = v;
        }
        blob(extra::st_affine_3d(
            g, args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8],
            args[9], args[10], args[11],
        ))
    })?;
    conn.create_scalar_function("ST_TransScale", 5, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_TransScale")? else {
            return Ok(None);
        };
        let mut args = [0.0f64; 4];
        for (i, slot) in args.iter_mut().enumerate() {
            let Some(v) = real_or_null(ctx, i + 1, "ST_TransScale")? else {
                return Ok(None);
            };
            *slot = v;
        }
        blob(extra::st_trans_scale(g, args[0], args[1], args[2], args[3]))
    })?;
    conn.create_scalar_function("ST_ReducePrecision", 2, FLAGS, |ctx| {
        let (Some(g), Some(grid)) = (
            blob_or_null(ctx, 0, "ST_ReducePrecision")?,
            real_or_null(ctx, 1, "ST_ReducePrecision")?,
        ) else {
            return Ok(None);
        };
        blob(extra::st_reduce_precision(g, grid))
    })?;
    conn.create_scalar_function("ST_Angle", 3, FLAGS, |ctx| {
        let (Some(a), Some(b), Some(c)) = (
            blob_or_null(ctx, 0, "ST_Angle")?,
            blob_or_null(ctx, 1, "ST_Angle")?,
            blob_or_null(ctx, 2, "ST_Angle")?,
        ) else {
            return Ok(None);
        };
        extra::st_angle_3(a, b, c)
            .map(|v| v.map(Value::Real))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_Angle", 4, FLAGS, |ctx| {
        let (Some(a), Some(b), Some(c), Some(d)) = (
            blob_or_null(ctx, 0, "ST_Angle")?,
            blob_or_null(ctx, 1, "ST_Angle")?,
            blob_or_null(ctx, 2, "ST_Angle")?,
            blob_or_null(ctx, 3, "ST_Angle")?,
        ) else {
            return Ok(None);
        };
        extra::st_angle_4(a, b, c, d)
            .map(|v| v.map(Value::Real))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_LineInterpolatePoints", 2, FLAGS, |ctx| {
        let (Some(g), Some(f)) = (
            blob_or_null(ctx, 0, "ST_LineInterpolatePoints")?,
            real_or_null(ctx, 1, "ST_LineInterpolatePoints")?,
        ) else {
            return Ok(None);
        };
        extra::st_line_interpolate_points(g, f)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_GeoHash", 1, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_GeoHash")? else {
            return Ok(None);
        };
        extra::st_geohash(g, None)
            .map(|v| v.map(Value::Text))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_GeoHash", 2, FLAGS, |ctx| {
        let (Some(g), Some(n)) = (
            blob_or_null(ctx, 0, "ST_GeoHash")?,
            i64_or_null(ctx, 1, "ST_GeoHash")?,
        ) else {
            return Ok(None);
        };
        extra::st_geohash(g, Some(n))
            .map(|v| v.map(Value::Text))
            .map_err(sql_err)
    })?;

    // ST_Extent(geom): NULL rows skipped, an all-NULL group yields NULL.
    struct ExtentAgg;
    impl rusqlite::functions::Aggregate<extra::ExtentAggregate, Option<Value>> for ExtentAgg {
        fn init(&self, _: &mut Context<'_>) -> rusqlite::Result<extra::ExtentAggregate> {
            Ok(extra::ExtentAggregate::new())
        }
        fn step(
            &self,
            ctx: &mut Context<'_>,
            acc: &mut extra::ExtentAggregate,
        ) -> rusqlite::Result<()> {
            match blob_or_null(ctx, 0, "ST_Extent")? {
                None => Ok(()),
                Some(b) => acc.step(b).map_err(sql_err),
            }
        }
        fn finalize(
            &self,
            _: &mut Context<'_>,
            acc: Option<extra::ExtentAggregate>,
        ) -> rusqlite::Result<Option<Value>> {
            match acc {
                None => Ok(None),
                Some(agg) => agg.finish().map(|o| o.map(Value::Blob)).map_err(sql_err),
            }
        }
    }
    conn.create_aggregate_function("ST_Extent", 1, FLAGS, ExtentAgg)?;

    // ST_3DExtent(geom): same semantics, but the result is TEXT — SQLite has
    // no box3d type, and kenro cannot write a 3D geometry to stand in.
    struct Extent3DAgg;
    impl rusqlite::functions::Aggregate<extra::Extent3DAggregate, Option<Value>> for Extent3DAgg {
        fn init(&self, _: &mut Context<'_>) -> rusqlite::Result<extra::Extent3DAggregate> {
            Ok(extra::Extent3DAggregate::new())
        }
        fn step(
            &self,
            ctx: &mut Context<'_>,
            acc: &mut extra::Extent3DAggregate,
        ) -> rusqlite::Result<()> {
            match blob_or_null(ctx, 0, "ST_3DExtent")? {
                None => Ok(()),
                Some(b) => acc.step(b).map_err(sql_err),
            }
        }
        fn finalize(
            &self,
            _: &mut Context<'_>,
            acc: Option<extra::Extent3DAggregate>,
        ) -> rusqlite::Result<Option<Value>> {
            match acc {
                None => Ok(None),
                Some(agg) => agg.finish().map(|o| o.map(Value::Text)).map_err(sql_err),
            }
        }
    }
    conn.create_aggregate_function("ST_3DExtent", 1, FLAGS, Extent3DAgg)?;
    Ok(())
}

/// The two size-gated algorithms (see `functions::hull`).
#[allow(unused_variables)]
fn register_hull(conn: &Connection) -> rusqlite::Result<()> {
    #[cfg(feature = "concave-hull")]
    conn.create_scalar_function("ST_ConcaveHull", 2, FLAGS, |ctx| {
        let (Some(g), Some(target)) = (
            blob_or_null(ctx, 0, "ST_ConcaveHull")?,
            real_or_null(ctx, 1, "ST_ConcaveHull")?,
        ) else {
            return Ok(None);
        };
        blob(crate::functions::hull::st_concave_hull(g, target))
    })?;
    #[cfg(feature = "delaunay")]
    register_geom_to_blob(
        conn,
        "ST_DelaunayTriangles",
        crate::functions::hull::st_delaunay_triangles,
    )?;
    #[cfg(feature = "delaunay")]
    register_geom_to_blob(
        conn,
        "ST_TriangulatePolygon",
        crate::functions::hull::st_triangulate_polygon,
    )?;
    #[cfg(feature = "voronoi")]
    for (name, f) in [
        (
            "ST_VoronoiPolygons",
            crate::functions::hull::st_voronoi_polygons
                as fn(&[u8], Option<f64>, Option<&[u8]>) -> crate::error::Result<Vec<u8>>,
        ),
        ("ST_VoronoiLines", crate::functions::hull::st_voronoi_lines),
    ] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(g) = blob_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            blob(f(g, None, None))
        })?;
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(g), Some(tol)) = (blob_or_null(ctx, 0, name)?, real_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            blob(f(g, Some(tol), None))
        })?;
        conn.create_scalar_function(name, 3, FLAGS, move |ctx| {
            let (Some(g), Some(tol), Some(e)) = (
                blob_or_null(ctx, 0, name)?,
                real_or_null(ctx, 1, name)?,
                blob_or_null(ctx, 2, name)?,
            ) else {
                return Ok(None);
            };
            blob(f(g, Some(tol), Some(e)))
        })?;
    }
    Ok(())
}

/// Grid generators (see `functions::grid`).
fn register_grid(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::grid;

    // Note the argument order: PostGIS's (size, bounds), which is the reverse
    // of SpatiaLite's (geom, size). A pasted SpatiaLite call hits the type
    // check on argument 0 rather than gridding the wrong thing.
    for (name, f) in [
        (
            "ST_SquareGrid",
            grid::st_square_grid as fn(f64, &[u8]) -> crate::error::Result<Vec<u8>>,
        ),
        ("ST_HexagonGrid", grid::st_hexagon_grid),
    ] {
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(size), Some(bounds)) =
                (real_or_null(ctx, 0, name)?, blob_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            blob(f(size, bounds))
        })?;
    }
    Ok(())
}

/// Line structure: simplicity and merging (see `functions::lines`).
/// `ST_Split` needs the overlay engine and is registered with it.
fn register_lines(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::lines;

    register_predicate_1(conn, "ST_IsSimple", lines::st_is_simple)?;
    register_geom_to_blob(conn, "ST_LineMerge", lines::st_line_merge)?;
    // PostGIS's second argument is a boolean; SQLite spells it 0/1, and
    // `true`/`false` are integer literals there too.
    conn.create_scalar_function("ST_LineMerge", 2, FLAGS, |ctx| {
        let (Some(g), Some(directed)) = (
            blob_or_null(ctx, 0, "ST_LineMerge")?,
            bool_or_null(ctx, 1, "ST_LineMerge")?,
        ) else {
            return Ok(None);
        };
        blob(lines::st_line_merge_directed(g, directed))
    })?;
    Ok(())
}

/// The tail of the PostGIS surface (see `functions::misc`).
fn register_misc(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::misc;

    // Aliases: same code, PostGIS's other spelling.
    conn.create_scalar_function("ST_RotateZ", 2, FLAGS, |ctx| {
        let (Some(g), Some(rad)) = (
            blob_or_null(ctx, 0, "ST_RotateZ")?,
            real_or_null(ctx, 1, "ST_RotateZ")?,
        ) else {
            return Ok(None);
        };
        blob(crate::functions::affine::st_rotate(g, rad))
    })?;
    for (name, expect) in [
        ("ST_MultiPointFromText", compat::Expect::MultiPoint),
        (
            "ST_MultiLineStringFromText",
            compat::Expect::MultiLineString,
        ),
        ("ST_MultiPolygonFromText", compat::Expect::MultiPolygon),
    ] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(t) = text_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            compat::from_text_typed(t, None, expect)
                .map(|v| v.map(Value::Blob))
                .map_err(sql_err)
        })?;
    }
    for (name, expect) in [
        ("ST_PolygonFromWKB", compat::Expect::Polygon),
        ("ST_LineStringFromWKB", compat::Expect::LineString),
        ("ST_MPointFromWKB", compat::Expect::MultiPoint),
        ("ST_MLineFromWKB", compat::Expect::MultiLineString),
        ("ST_MPolyFromWKB", compat::Expect::MultiPolygon),
        ("ST_MultiPointFromWKB", compat::Expect::MultiPoint),
        ("ST_MultiLineFromWKB", compat::Expect::MultiLineString),
        ("ST_MultiPolyFromWKB", compat::Expect::MultiPolygon),
    ] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(b) = blob_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            compat::from_wkb_typed(b, None, expect)
                .map(|v| v.map(Value::Blob))
                .map_err(sql_err)
        })?;
        // The srid form, which these eight were missing while their three
        // shorter siblings had it — the wasm and ABI layers had generated it
        // all along, so it was unreachable code rather than absent code.
        conn.create_scalar_function(name, 2, FLAGS, move |ctx| {
            let (Some(b), Some(srid)) = (blob_or_null(ctx, 0, name)?, int_or_null(ctx, 1, name)?)
            else {
                return Ok(None);
            };
            compat::from_wkb_typed(b, Some(srid), expect)
                .map(|v| v.map(Value::Blob))
                .map_err(sql_err)
        })?;
    }

    conn.create_scalar_function("ST_Polygon", 2, FLAGS, |ctx| {
        let (Some(g), Some(srid)) = (
            blob_or_null(ctx, 0, "ST_Polygon")?,
            int_or_null(ctx, 1, "ST_Polygon")?,
        ) else {
            return Ok(None);
        };
        blob(misc::st_polygon(g, srid))
    })?;
    register_geom_to_opt_blob(conn, "ST_LineFromMultiPoint", misc::st_line_from_multipoint)?;
    conn.create_scalar_function("ST_LineExtend", 2, FLAGS, |ctx| {
        let (Some(g), Some(f)) = (
            blob_or_null(ctx, 0, "ST_LineExtend")?,
            real_or_null(ctx, 1, "ST_LineExtend")?,
        ) else {
            return Ok(None);
        };
        misc::st_line_extend(g, f, 0.0)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_LineExtend", 3, FLAGS, |ctx| {
        let (Some(g), Some(f), Some(b)) = (
            blob_or_null(ctx, 0, "ST_LineExtend")?,
            real_or_null(ctx, 1, "ST_LineExtend")?,
            real_or_null(ctx, 2, "ST_LineExtend")?,
        ) else {
            return Ok(None);
        };
        misc::st_line_extend(g, f, b)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_PointInsideCircle", 4, FLAGS, |ctx| {
        let (Some(g), Some(cx), Some(cy), Some(r)) = (
            blob_or_null(ctx, 0, "ST_PointInsideCircle")?,
            real_or_null(ctx, 1, "ST_PointInsideCircle")?,
            real_or_null(ctx, 2, "ST_PointInsideCircle")?,
            real_or_null(ctx, 3, "ST_PointInsideCircle")?,
        ) else {
            return Ok(None);
        };
        misc::st_point_inside_circle(g, cx, cy, r)
            .map(|v| Some(Value::Integer(v as i64)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_WrapX", 3, FLAGS, |ctx| {
        let (Some(g), Some(wrap), Some(amount)) = (
            blob_or_null(ctx, 0, "ST_WrapX")?,
            real_or_null(ctx, 1, "ST_WrapX")?,
            real_or_null(ctx, 2, "ST_WrapX")?,
        ) else {
            return Ok(None);
        };
        blob(misc::st_wrap_x(g, wrap, amount))
    })?;
    conn.create_scalar_function("ST_MakeBox2D", 2, FLAGS, |ctx| {
        let (Some(a), Some(b)) = (
            blob_or_null(ctx, 0, "ST_MakeBox2D")?,
            blob_or_null(ctx, 1, "ST_MakeBox2D")?,
        ) else {
            return Ok(None);
        };
        blob(misc::st_make_box_2d(a, b))
    })?;
    for name in ["ST_GeomFromGeoHash", "ST_Box2dFromGeoHash"] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(h) = text_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            blob(misc::st_geom_from_geohash(h, None))
        })?;
    }
    conn.create_scalar_function("ST_GeomFromGeoHash", 2, FLAGS, |ctx| {
        let (Some(h), Some(p)) = (
            text_or_null(ctx, 0, "ST_GeomFromGeoHash")?,
            i64_or_null(ctx, 1, "ST_GeomFromGeoHash")?,
        ) else {
            return Ok(None);
        };
        blob(misc::st_geom_from_geohash(h, Some(p)))
    })?;
    conn.create_scalar_function("ST_PointFromGeoHash", 1, FLAGS, |ctx| {
        let Some(h) = text_or_null(ctx, 0, "ST_PointFromGeoHash")? else {
            return Ok(None);
        };
        blob(misc::st_point_from_geohash(h, None))
    })?;
    conn.create_scalar_function("ST_PointFromGeoHash", 2, FLAGS, |ctx| {
        let (Some(h), Some(p)) = (
            text_or_null(ctx, 0, "ST_PointFromGeoHash")?,
            i64_or_null(ctx, 1, "ST_PointFromGeoHash")?,
        ) else {
            return Ok(None);
        };
        blob(misc::st_point_from_geohash(h, Some(p)))
    })?;
    conn.create_scalar_function("ST_GeometricMedian", 1, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_GeometricMedian")? else {
            return Ok(None);
        };
        misc::st_geometric_median(g, None)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_GeometricMedian", 2, FLAGS, |ctx| {
        let (Some(g), Some(tol)) = (
            blob_or_null(ctx, 0, "ST_GeometricMedian")?,
            real_or_null(ctx, 1, "ST_GeometricMedian")?,
        ) else {
            return Ok(None);
        };
        misc::st_geometric_median(g, Some(tol))
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_LineCrossingDirection", 2, FLAGS, |ctx| {
        let (Some(a), Some(b)) = (
            blob_or_null(ctx, 0, "ST_LineCrossingDirection")?,
            blob_or_null(ctx, 1, "ST_LineCrossingDirection")?,
        ) else {
            return Ok(None);
        };
        misc::st_line_crossing_direction(a, b)
            .map(|v| Some(Value::Integer(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_Summary", 1, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_Summary")? else {
            return Ok(None);
        };
        misc::st_summary(g)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_MemSize", 1, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_MemSize")? else {
            return Ok(None);
        };
        misc::st_mem_size(g)
            .map(|v| Some(Value::Integer(v)))
            .map_err(sql_err)
    })?;
    register_geom_to_blob(conn, "ST_Normalize", misc::st_normalize)?;
    Ok(())
}

/// 3D pass-through (see `functions::threed`).
fn register_threed(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::threed;

    register_predicate_1(conn, "ST_HasZ", threed::st_has_z)?;
    register_predicate_1(conn, "ST_HasM", threed::st_has_m)?;
    register_rtree_minmax(conn, "ST_Z", threed::st_z)?;
    register_rtree_minmax(conn, "ST_M", threed::st_m)?;
    register_box_accessor(conn, "ST_ZMin", threed::st_zmin)?;
    register_box_accessor(conn, "ST_ZMax", threed::st_zmax)?;
    Ok(())
}

/// KML and SVG output (see `functions::kml`, `functions::svg`).
#[cfg(feature = "text-encodings")]
fn register_text_encodings(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::{kml, svg};

    conn.create_scalar_function("ST_AsKML", 1, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_AsKML")? else {
            return Ok(None);
        };
        kml::st_as_kml(g, None, None)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsKML", 2, FLAGS, |ctx| {
        let (Some(g), Some(digits)) = (
            blob_or_null(ctx, 0, "ST_AsKML")?,
            int_or_null(ctx, 1, "ST_AsKML")?,
        ) else {
            return Ok(None);
        };
        kml::st_as_kml(g, Some(digits as i64), None)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsKML", 3, FLAGS, |ctx| {
        let (Some(g), Some(digits), Some(prefix)) = (
            blob_or_null(ctx, 0, "ST_AsKML")?,
            int_or_null(ctx, 1, "ST_AsKML")?,
            text_or_null(ctx, 2, "ST_AsKML")?,
        ) else {
            return Ok(None);
        };
        kml::st_as_kml(g, Some(digits as i64), Some(prefix))
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsSVG", 1, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_AsSVG")? else {
            return Ok(None);
        };
        svg::st_as_svg(g, None, None)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsSVG", 2, FLAGS, |ctx| {
        let (Some(g), Some(rel)) = (
            blob_or_null(ctx, 0, "ST_AsSVG")?,
            int_or_null(ctx, 1, "ST_AsSVG")?,
        ) else {
            return Ok(None);
        };
        svg::st_as_svg(g, Some(rel as i64), None)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsSVG", 3, FLAGS, |ctx| {
        let (Some(g), Some(rel), Some(digits)) = (
            blob_or_null(ctx, 0, "ST_AsSVG")?,
            int_or_null(ctx, 1, "ST_AsSVG")?,
            int_or_null(ctx, 2, "ST_AsSVG")?,
        ) else {
            return Ok(None);
        };
        svg::st_as_svg(g, Some(rel as i64), Some(digits as i64))
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    Ok(())
}

/// GML 2/3 I/O (see `functions::gml`).
#[cfg(feature = "gml")]
fn register_gml(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::gml;

    // PostGIS's default version is 2; the leading integer selects it.
    conn.create_scalar_function("ST_AsGML", 1, FLAGS, |ctx| {
        let Some(g) = blob_or_null(ctx, 0, "ST_AsGML")? else {
            return Ok(None);
        };
        gml::st_as_gml(g, 2, None)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsGML", 2, FLAGS, |ctx| {
        let (Some(version), Some(g)) = (
            int_or_null(ctx, 0, "ST_AsGML")?,
            blob_or_null(ctx, 1, "ST_AsGML")?,
        ) else {
            return Ok(None);
        };
        gml::st_as_gml(g, version as i64, None)
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_AsGML", 3, FLAGS, |ctx| {
        let (Some(version), Some(g), Some(digits)) = (
            int_or_null(ctx, 0, "ST_AsGML")?,
            blob_or_null(ctx, 1, "ST_AsGML")?,
            int_or_null(ctx, 2, "ST_AsGML")?,
        ) else {
            return Ok(None);
        };
        gml::st_as_gml(g, version as i64, Some(digits as i64))
            .map(|v| Some(Value::Text(v)))
            .map_err(sql_err)
    })?;
    for name in ["ST_GeomFromGML", "ST_GMLToSQL"] {
        conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
            let Some(t) = text_or_null(ctx, 0, name)? else {
                return Ok(None);
            };
            blob(gml::st_geom_from_gml(t, None))
        })?;
    }
    conn.create_scalar_function("ST_GeomFromGML", 2, FLAGS, |ctx| {
        let (Some(t), Some(srid)) = (
            text_or_null(ctx, 0, "ST_GeomFromGML")?,
            int_or_null(ctx, 1, "ST_GeomFromGML")?,
        ) else {
            return Ok(None);
        };
        blob(gml::st_geom_from_gml(t, Some(srid)))
    })?;
    Ok(())
}

/// Surface collections (see `functions::surface`).
fn register_surface(conn: &Connection) -> rusqlite::Result<()> {
    use crate::functions::surface;

    conn.create_scalar_function("ST_NumPatches", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "ST_NumPatches")? else {
            return Ok(None);
        };
        surface::st_num_patches(b)
            .map(|v| v.map(Value::Integer))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("ST_PatchN", 2, FLAGS, |ctx| {
        let (Some(b), Some(n)) = (
            blob_or_null(ctx, 0, "ST_PatchN")?,
            i64_or_null(ctx, 1, "ST_PatchN")?,
        ) else {
            return Ok(None);
        };
        surface::st_patch_n(b, n)
            .map(|v| v.map(Value::Blob))
            .map_err(sql_err)
    })?;
    conn.create_scalar_function("kenro_gpkg_extension_required", 1, FLAGS, |ctx| {
        let Some(b) = blob_or_null(ctx, 0, "kenro_gpkg_extension_required")? else {
            return Ok(None);
        };
        surface::extension_required(b)
            .map(|v| v.map(Value::Text))
            .map_err(sql_err)
    })?;
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

#[cfg(feature = "overlay")]
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

/// The six box accessors (`Kind::BlobOrText`): identical to
/// `register_rtree_minmax` but for the argument helper, so `BOX3D(…)` text
/// reaches the pure function as bytes and the geometry path is unchanged.
fn register_box_accessor(
    conn: &Connection,
    name: &'static str,
    f: fn(&[u8]) -> crate::error::Result<Option<f64>>,
) -> rusqlite::Result<()> {
    conn.create_scalar_function(name, 1, FLAGS, move |ctx| {
        let Some(b) = blob_or_text_or_null(ctx, 0, name)? else {
            return Ok(None);
        };
        f(b).map(|v| v.map(Value::Real)).map_err(sql_err)
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

/// `Kind::BlobOrText`: a geometry BLOB, or box text (`BOX3D(…)` / `BOX(…)`)
/// handed on as its UTF-8 bytes.
///
/// There is no discriminator to pass: `functions::box3d::is_box_text` tells
/// the two apart by content, which is unambiguous because a geometry
/// encoding never starts with `B`. The BLOB path is byte-identical to
/// `blob_or_null`'s — the GeoPackage R-tree triggers call `ST_MinX` on every
/// row and must not notice this exists. What they lose is only the TEXT
/// *rejection*; `box3d`'s parse error carries the same
/// "did you mean ST_GeomFromText?" help.
fn blob_or_text_or_null<'a>(
    ctx: &'a Context<'_>,
    i: usize,
    func: &'static str,
) -> rusqlite::Result<Option<&'a [u8]>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Blob(b) => Ok(Some(b)),
        ValueRef::Text(t) => Ok(Some(t)),
        other => Err(sql_err(Error::Unsupported {
            func,
            reason: format!(
                "expected a geometry BLOB or box text, got {}",
                other.data_type()
            ),
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
#[cfg(feature = "overlay")]
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

/// SQLite has no boolean type: 0 and 1 are the only accepted spellings, and
/// `true`/`false` in SQL are integer literals for exactly those.
fn bool_or_null(ctx: &Context<'_>, i: usize, func: &'static str) -> rusqlite::Result<Option<bool>> {
    match ctx.get_raw(i) {
        ValueRef::Null => Ok(None),
        ValueRef::Integer(0) => Ok(Some(false)),
        ValueRef::Integer(1) => Ok(Some(true)),
        other => Err(sql_err(Error::Unsupported {
            func,
            reason: format!("expected a boolean (0 or 1), got {}", other.data_type()),
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
