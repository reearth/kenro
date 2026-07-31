//! Property tests over the SQLite-free core: roundtrips, determinism, and
//! cross-function invariants. Strategies generate geometries that are valid
//! by construction (points, rects, star-convex polygons) rather than
//! fighting proptest into arbitrary valid polygons.

use proptest::prelude::*;

use kenro::functions::{io, mvt, overlay, predicates, rtree};
use kenro::geom;
use kenro::gpb::{self, GpbHeader};

fn coord() -> impl Strategy<Value = f64> {
    // Finite, sane magnitudes; exercises negatives and fractions.
    prop_oneof![
        -1e6..1e6f64,
        -180.0..180.0f64,
        Just(0.0),
        Just(-0.0),
        Just(123.456789012345),
    ]
}

prop_compose! {
    fn point_wkt()(x in coord(), y in coord()) -> String {
        format!("POINT({x:?} {y:?})")
    }
}

prop_compose! {
    fn rect_wkt()(x in coord(), y in coord(), w in 0.001..1000.0f64, h in 0.001..1000.0f64) -> String {
        let (x2, y2) = (x + w, y + h);
        format!("POLYGON(({x:?} {y:?},{x2:?} {y:?},{x2:?} {y2:?},{x:?} {y2:?},{x:?} {y:?}))")
    }
}

prop_compose! {
    /// Star-convex polygon around a center: valid by construction.
    fn star_wkt()(cx in -1000.0..1000.0f64, cy in -1000.0..1000.0f64,
                  radii in prop::collection::vec(0.1..100.0f64, 4..12)) -> String {
        let n = radii.len();
        let mut pts: Vec<String> = radii
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let angle = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
                let (x, y) = (cx + r * angle.cos(), cy + r * angle.sin());
                format!("{x:?} {y:?}")
            })
            .collect();
        pts.push(pts[0].clone());
        format!("POLYGON(({}))", pts.join(","))
    }
}

fn geom_wkt() -> impl Strategy<Value = String> {
    prop_oneof![point_wkt(), rect_wkt(), star_wkt()]
}

proptest! {
    #[test]
    fn wkb_roundtrip_is_byte_identical(wkt in geom_wkt()) {
        let blob = io::st_geom_from_text(&wkt, None).unwrap();
        let wkb = io::st_as_binary(&blob).unwrap();
        let blob2 = io::st_geom_from_wkb(&wkb, None).unwrap();
        let wkb2 = io::st_as_binary(&blob2).unwrap();
        prop_assert_eq!(wkb, wkb2);
    }

    #[test]
    fn gpb_roundtrip_preserves_geometry_srid_and_flags(
        wkt in geom_wkt(),
        srid in prop_oneof![Just(0), Just(4326), Just(3857), -1..100_000i32],
    ) {
        let blob = io::st_geom_from_text(&wkt, Some(srid)).unwrap();
        let header = GpbHeader::parse(&blob).unwrap();
        prop_assert_eq!(header.srid, srid);
        // Storage form roundtrips through the validator/normalizer.
        let stored = io::st_as_gpb(&blob).unwrap();
        let normalized = io::st_geom_from_gpb(&stored).unwrap();
        prop_assert_eq!(&blob, &normalized);
    }

    #[test]
    fn header_parse_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
        let _ = GpbHeader::parse(&bytes); // Ok or Err, never panic
        let _ = gpb::is_gpb(&bytes);
    }

    #[test]
    fn decode_auto_never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let _ = geom::decode_auto(&bytes);
        let _ = rtree::st_is_empty(&bytes);
        let _ = rtree::st_min_x(&bytes);
    }

    #[test]
    fn functions_are_deterministic(wkt_a in geom_wkt(), wkt_b in geom_wkt()) {
        let a = io::st_geom_from_text(&wkt_a, Some(4326)).unwrap();
        let b = io::st_geom_from_text(&wkt_b, Some(4326)).unwrap();
        prop_assert_eq!(io::st_geom_from_text(&wkt_a, Some(4326)).unwrap(), a.clone());
        prop_assert_eq!(io::st_as_text(&a).unwrap(), io::st_as_text(&a).unwrap());
        prop_assert_eq!(
            predicates::st_intersects(&a, &b).unwrap(),
            predicates::st_intersects(&a, &b).unwrap()
        );
        prop_assert_eq!(
            predicates::st_distance(&a, &b).unwrap(),
            predicates::st_distance(&a, &b).unwrap()
        );
    }

    #[test]
    fn within_is_contains_flipped(wkt_a in geom_wkt(), wkt_b in geom_wkt()) {
        let a = io::st_geom_from_text(&wkt_a, None).unwrap();
        let b = io::st_geom_from_text(&wkt_b, None).unwrap();
        prop_assert_eq!(
            predicates::st_within(&a, &b).unwrap(),
            predicates::st_contains(&b, &a).unwrap()
        );
    }

    #[test]
    fn dwithin_zero_agrees_with_distance_zero(wkt_a in geom_wkt(), wkt_b in geom_wkt()) {
        let a = io::st_geom_from_text(&wkt_a, None).unwrap();
        let b = io::st_geom_from_text(&wkt_b, None).unwrap();
        let d = predicates::st_distance(&a, &b).unwrap().unwrap();
        prop_assert_eq!(predicates::st_dwithin(&a, &b, 0.0).unwrap(), d == 0.0);
    }

    #[cfg(feature = "transform")]
    #[test]
    fn transform_roundtrip_within_tolerance(
        lon in 138.6..140.9f64,
        lat in 34.6..37.4f64,
    ) {
        // 4326 → UTM 54N → 4326 must come back within ~1e-9 degrees.
        let src = io::st_geom_from_text(&format!("POINT({lon:?} {lat:?})"), Some(4326)).unwrap();
        let projected = kenro::functions::transform::st_transform(&src, 32654).unwrap();
        let back = kenro::functions::transform::st_transform(&projected, 4326).unwrap();
        let g = geom::decode_auto(&back).unwrap();
        let geo_types::Geometry::Point(p) = g.geometry else { unreachable!() };
        prop_assert!((p.x() - lon).abs() < 1e-9, "{} vs {lon}", p.x());
        prop_assert!((p.y() - lat).abs() < 1e-9, "{} vs {lat}", p.y());
    }

    #[cfg(feature = "h3")]
    #[test]
    fn h3_cells_are_valid_positive_i64(
        lon in -179.9..179.9f64,
        lat in -89.9..89.9f64,
        res in 0..=15i64,
    ) {
        let blob = io::st_geom_from_text(&format!("POINT({lon:?} {lat:?})"), Some(4326)).unwrap();
        let cell = kenro::functions::h3::h3_latlng_to_cell(&blob, res).unwrap();
        prop_assert!(cell > 0, "bit 63 must be clear: {cell}");
        let s = kenro::functions::h3::h3_cell_to_string(cell).unwrap();
        prop_assert_eq!(kenro::functions::h3::h3_string_to_cell(&s).unwrap(), cell);
    }

    // Overlay/buffer/MVT run through i_overlay's integer-grid mesh — the
    // one dependency whose robustness contract we cannot inspect. These
    // pin "any valid finite input returns Ok or Err, never panics" (on
    // wasm a panic aborts the whole instance).

    #[test]
    fn overlay_ops_never_panic(wkt_a in geom_wkt(), wkt_b in geom_wkt()) {
        let a = io::st_geom_from_text(&wkt_a, None).unwrap();
        let b = io::st_geom_from_text(&wkt_b, None).unwrap();
        let _ = overlay::st_intersection(&a, &b);
        let _ = overlay::st_difference(&a, &b);
        let _ = overlay::st_sym_difference(&a, &b);
        let _ = overlay::st_union(&a, &b);
    }

    #[test]
    fn make_valid_output_is_valid(
        pts in prop::collection::vec((-100.0..100.0f64, -100.0..100.0f64), 3..9),
    ) {
        use kenro::functions::accessors;
        // Random rings are self-intersecting more often than not — exactly
        // the input MakeValid exists for. Output must always validate.
        let mut ring: Vec<String> = pts.iter().map(|(x, y)| format!("{x:?} {y:?}")).collect();
        ring.push(ring[0].clone());
        let wkt = format!("POLYGON(({}))", ring.join(","));
        let g = io::st_geom_from_text(&wkt, None).unwrap();
        let out = overlay::st_make_valid(&g).unwrap();
        prop_assert!(accessors::st_is_valid(&out).unwrap(), "{wkt}");
        prop_assert!(overlay::st_make_valid(&out).unwrap() == out, "not idempotent: {wkt}");
    }

    #[test]
    fn areal_intersection_never_exceeds_operands(a in rect_wkt(), b in rect_wkt()) {
        use kenro::functions::accessors;
        let ga = io::st_geom_from_text(&a, None).unwrap();
        let gb = io::st_geom_from_text(&b, None).unwrap();
        let cap = accessors::st_area(&ga).unwrap().min(accessors::st_area(&gb).unwrap());
        let out = overlay::st_intersection(&ga, &gb).unwrap();
        let area = accessors::st_area(&out).unwrap();
        // i_overlay snaps coordinates onto its internal grid, so the result
        // boundary can exceed a razor-thin operand by ~1e-8 of the combined
        // extent per vertex; allow that error band scaled by the result
        // perimeter (with a wide safety factor), nothing more.
        let span = [&ga, &gb]
            .iter()
            .flat_map(|g| {
                [rtree::st_min_x(g), rtree::st_max_x(g), rtree::st_min_y(g), rtree::st_max_y(g)]
            })
            .map(|v| v.unwrap().unwrap().abs())
            .fold(1.0f64, f64::max);
        let allowance = 1e-7 * span * (accessors::st_perimeter(&out).unwrap() + 4.0);
        prop_assert!(area <= cap + allowance + 1e-9, "{area} > {cap} (+{allowance})");
    }

    #[test]
    fn buffer_never_panics(wkt in geom_wkt(), d in -50.0..50.0f64, quad in 1..16i32) {
        let g = io::st_geom_from_text(&wkt, None).unwrap();
        let _ = overlay::st_buffer(&g, d, None);
        let _ = overlay::st_buffer(&g, d, Some(&format!("quad_segs={quad}")));
    }

    #[test]
    fn union_aggregate_never_panics(
        wkts in prop::collection::vec(geom_wkt(), 0..8),
    ) {
        let mut acc = overlay::UnionAggregate::new();
        for wkt in &wkts {
            let g = io::st_geom_from_text(wkt, None).unwrap();
            let _ = acc.step(&g); // mixed dimension classes may Err — fine
        }
        let _ = acc.finish();
    }

    #[test]
    fn mvt_pipeline_never_panics(
        wkt in geom_wkt(),
        bounds_wkt in rect_wkt(),
        extent in 1..8192i32,
        buffer in 0..1024i32,
        clip in 0..2i32,
    ) {
        let g = io::st_geom_from_text(&wkt, None).unwrap();
        let bounds = io::st_geom_from_text(&bounds_wkt, None).unwrap();
        let result = mvt::st_as_mvt_geom(&g, &bounds, Some(extent), Some(buffer), Some(clip));
        // Whatever survives the transform must also survive encoding.
        if let Ok(Some(tile_geom)) = result {
            let mut acc = mvt::MvtAggregate::new();
            acc.step(&tile_geom, None, Some(extent), None).unwrap();
            acc.finish().unwrap();
        }
    }

    #[test]
    fn envelope_invariants_and_fast_path_agreement(wkt in geom_wkt()) {
        let canonical = io::st_geom_from_text(&wkt, None).unwrap();
        let stored = io::st_as_gpb(&canonical).unwrap();
        // min <= max always.
        let (minx, maxx) = (
            rtree::st_min_x(&canonical).unwrap().unwrap(),
            rtree::st_max_x(&canonical).unwrap().unwrap(),
        );
        prop_assert!(minx <= maxx);
        // Header-envelope fast path ≡ WKB-computed envelope.
        for (fast, slow) in [
            (rtree::st_min_x(&stored), rtree::st_min_x(&canonical)),
            (rtree::st_max_x(&stored), rtree::st_max_x(&canonical)),
            (rtree::st_min_y(&stored), rtree::st_min_y(&canonical)),
            (rtree::st_max_y(&stored), rtree::st_max_y(&canonical)),
        ] {
            prop_assert_eq!(fast.unwrap(), slow.unwrap());
        }
    }
}
