//! Property tests over the SQLite-free core: roundtrips, determinism, and
//! cross-function invariants. Strategies generate geometries that are valid
//! by construction (points, rects, star-convex polygons) rather than
//! fighting proptest into arbitrary valid polygons.

use proptest::prelude::*;

use kenro::functions::{io, predicates, rtree};
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
        srid in prop_oneof![Just(0), Just(4326), Just(6668), -1..100_000i32],
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
