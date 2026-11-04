use super::generate_road::{
    align_network, build_road, build_regular_road, plan_road_view, plan_road_view_for_mode, RouteMode,
};
use super::geom::wgs84_to_bd09;

#[path = "../../tests/common/stadium.rs"]
mod fixture;

#[test]
fn shared_variation_reaches_every_route_mode_without_changing_altitude_or_totals() {
    let (mut net, points) = fixture::stadium();
    let points: Vec<_> = points.into_iter().map(|(a, b)| wgs84_to_bd09(a, b)).collect();
    let mut outline: Vec<_> = net.graph.node_weights().map(|n| {
        let (lat, lon) = wgs84_to_bd09(n.coord.lat, n.coord.lon);
        route_planner::Coord::new(lon, lat)
    }).collect();
    outline.push(outline[0]);
    let candidate = route_planner::athletics::TrackCandidate {
        id: "synthetic-variation".into(), source: "manual".into(),
        counterclockwise_allowed: true, points: outline,
    };
    align_network(&mut net);
    for mode in 0..4 {
        let build = |variation| match mode {
            0 => super::generator::build_with_variation(2110., 900, 7, points[0], 123000, &points, variation, 1.0),
            1 | 2 => super::generate_road::build_road_with_variation(
                2110., 900, 7, 123000, &points, &points, &net, mode == 2, variation, 1.0,
            ),
            _ => super::generate_road::build_track_with_variation(
                &net, &points, 2110., 900, 7, 123000, &candidate, variation, 1.0,
            ),
        };
        let baseline = build(1.0).unwrap();
        for variation in [0.0, 2.0, 5.0] {
            let track = build(variation).unwrap();
            track.validate_consistency().unwrap();
            assert!((track.totalDistance - 2110.).abs() < 0.01);
            assert_eq!(track.totalTime, 900);
            assert_eq!(track.startTime, baseline.startTime);
            assert!(track.locations.iter().zip(&baseline.locations).any(|(a, b)| a.speed != b.speed));
            for (point, original) in track.locations.iter().zip(&baseline.locations) {
                assert_eq!(point.bdA, original.bdA);
            }
            assert!(points.iter().all(|&p| super::wire::passes_point(&track, p)));
            let review = super::review::RunReview::from_track(&track).unwrap();
            assert_eq!(review.steps, track.totalSteps);
            assert_eq!(review.route.len(), track.locations.len());
        }
    }
}

#[test]
fn offline_comparison() {
    let (net, points) = fixture::stadium();
    let road = build_road(2110.0, 900, 7, 0, &points, &points, &net).unwrap();
    let track = build_regular_road(2110.0, 900, 7, 0, &points, &points, &net).unwrap();
    road.validate_consistency().unwrap();
    track.validate_consistency().unwrap();
    assert!((road.totalDistance - track.totalDistance).abs() < 0.01);
    assert_eq!(road.totalTime, track.totalTime);
    assert_eq!(road.locations.len(), track.locations.len());
    for (a, b) in road.locations.iter().zip(&track.locations) {
        assert_eq!((a.totalTime, a.bdA), (b.totalTime, b.bdA));
    }
    if let Some(directory) = std::env::var_os("NEKO_TRACK_COMPARISON_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("road.json"),
            serde_json::to_vec(&road).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("track.json"),
            serde_json::to_vec(&track).unwrap(),
        )
        .unwrap();
    }
}

fn offset(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (
        (a.1 - b.1) * 111320. * b.0.to_radians().cos(),
        (a.0 - b.0) * 111132.,
    )
}

#[test]
fn track_samples_follow_the_route_with_bounded_continuous_drift() {
    let (net, points) = fixture::stadium();
    let points: Vec<_> = points
        .into_iter()
        .map(|(a, b)| wgs84_to_bd09(a, b))
        .collect();
    let baseline = plan_road_view(&net, &points, 2110., 900, 7, &[]).unwrap();
    let mut aligned = net;
    align_network(&mut aligned);
    let circuit = route_planner::Route {
        points: baseline.route.iter().zip(&baseline.arcs).map(|(&(lat,lon), &s)| route_planner::RoutePoint {
            lat, lon, s, curvature: 0.0,
        }).collect(),
        length_m: baseline.length_m,
    };
    for seed in 0..20 {
        let journey = super::generate_road::regular_journey(&circuit,2110.,seed,&aligned).unwrap();
        let track = build_regular_road(2110., 900, seed, 123000, &points, &points, &aligned).unwrap();
        track.validate_consistency().unwrap();
        assert_eq!(track.locations.len(), 902);
        assert_eq!(track.totalTime, 900);
        assert!((track.totalDistance - 2110.).abs() < 0.001);
        assert_eq!(track.locations[1].ptype, 5);
        let mut previous_offset = (0., 0.);
        for (i, p) in track.locations.iter().enumerate() {
            let c = route_planner::itinerary::at(&journey.route,p.totalDis);
            let expected = (c.lat,c.lon);
            let delta = offset((p.gLat, p.gLng), expected);
            assert!(
                delta.0.hypot(delta.1) <= 0.35,
                "seed={seed}, sample={i}, offset={delta:?}"
            );
            assert!((delta.0 - previous_offset.0).hypot(delta.1 - previous_offset.1) < 0.08);
            previous_offset = delta;
            assert_eq!(p.totalTime, i.saturating_sub(1) as i64);
            assert_ne!(p.ptype, -1, "no fabricated failed fixes in track mode");
            if i > 1 {
                assert!(p.totalDis > track.locations[i - 1].totalDis);
                let step = offset(
                    (p.gLat, p.gLng),
                    (track.locations[i - 1].gLat, track.locations[i - 1].gLng),
                );
                assert!(step.0.hypot(step.1) < 6.5, "unexpected jump");
            }
        }
    }
}

#[test]
fn track_preview_equals_generated_and_encoded_coordinates() {
    use std::io::Read;
    let (net, points) = fixture::stadium();
    let points: Vec<_> = points
        .into_iter()
        .map(|(a, b)| wgs84_to_bd09(a, b))
        .collect();
    let preview =
        plan_road_view_for_mode(&net, &points, 2110., 900, 7, &[], RouteMode::Track).unwrap();
    let mut net = net;
    align_network(&mut net);
    let track = build_regular_road(2110., 900, 7, 123000, &points, &points, &net).unwrap();
    assert_eq!(
        preview.route,
        track
            .locations
            .iter()
            .map(|p| (p.gLat, p.gLng))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        preview.arcs,
        track
            .locations
            .iter()
            .map(|p| p.totalDis)
            .collect::<Vec<_>>()
    );
    let object = super::wire::build_obs_object(&track, 1, "offline", 1, "{}");
    let bytes = crate::crypto::envelope::b64_decode(object["run_data"].as_str().unwrap()).unwrap();
    let mut plain = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut plain)
        .unwrap();
    let wrapper: serde_json::Value = serde_json::from_str(&plain).unwrap();
    let encoded: Vec<serde_json::Value> =
        serde_json::from_str(wrapper["allLocJson"].as_str().unwrap()).unwrap();
    assert_eq!(encoded.len(), preview.route.len());
    for ((lat, lon), point) in preview.route.iter().zip(&encoded) {
        let (lat, lon) = super::wire::bd09_to_gcj02(*lat, *lon);
        assert!((point["gLat"].as_f64().unwrap() - lat).abs() < 5.1e-8);
        assert!((point["gLng"].as_f64().unwrap() - lon).abs() < 5.1e-8);
    }
    let end = super::route_view::trajectory_position(
        &preview.route,
        &preview.arcs,
        preview.length_m - 0.001,
    )
    .unwrap();
    let last = *preview.route.last().unwrap();
    let delta = offset(end, last);
    assert!(delta.0.hypot(delta.1) < 0.01);
    assert_eq!(
        super::route_view::trajectory_position(&preview.route, &preview.arcs, preview.length_m),
        Some(last)
    );
    assert_eq!(
        super::route_view::trajectory_position(
            &preview.route,
            &preview.arcs,
            preview.length_m + 10.
        ),
        Some(last)
    );
    assert_ne!(
        preview.route.first(),
        preview.route.last(),
        "fixture must end on a partial lap"
    );
}

#[test]
fn track_mode_and_ordinary_modes_roundtrip_independently() {
    for mode in [RouteMode::Legacy, RouteMode::Road, RouteMode::Track] {
        assert_eq!(RouteMode::from_str(mode.as_str()), mode);
    }
    assert_eq!(RouteMode::default(), RouteMode::Legacy);
    assert!(!RouteMode::Legacy.uses_network());
    assert!(RouteMode::Road.uses_network());
    assert!(RouteMode::Track.uses_network());
}

#[test]
fn confirmed_counterclockwise_track_uses_bounded_drift_and_exact_preview() {
    use super::generate_road::{build_track, plan_track_view, track_itinerary};
    let (net, points) = fixture::stadium();
    let points: Vec<_> = points.into_iter().map(|(a, b)| wgs84_to_bd09(a, b)).collect();
    let mut outline: Vec<_> = net.graph.node_weights().map(|n| n.coord).collect();
    outline.push(outline[0]);
    let candidate = route_planner::athletics::TrackCandidate {
        id: "synthetic-stadium".into(), source: "manual".into(), counterclockwise_allowed: true,
        points: outline.into_iter().map(|c| {
            let (lat, lon) = wgs84_to_bd09(c.lat, c.lon);
            route_planner::Coord::new(lon, lat)
        }).collect(),
    };
    let mut aligned = net.clone();
    align_network(&mut aligned);
    let itinerary = track_itinerary(&aligned, &points, 2110., &candidate).unwrap();
    for seed in 0..8 {
        let route = route_planner::lanes::expand(&itinerary,2110.,seed,&[]).unwrap();
        let preview = plan_track_view(&net, &points, 2110., 900, seed, &[], &candidate).unwrap();
        let track = build_track(&aligned, &points, 2110., 900, seed, 123000, &candidate).unwrap();
        assert!(preview.track_mode);
        assert_eq!(preview.route, track.locations.iter().map(|p| (p.gLat, p.gLng)).collect::<Vec<_>>());
        for p in &track.locations {
            let expected = route_planner::itinerary::at(&route, p.totalDis);
            let delta = offset((p.gLat, p.gLng), (expected.lat, expected.lon));
            assert!(delta.0.hypot(delta.1) <= 0.35);
        }
        assert_eq!(super::route_view::position_with_end(&preview.route, &preview.arcs, preview.length_m, true), preview.route.last().copied());
    }
}
