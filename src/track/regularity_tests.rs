use super::generate_road::{
    align_network, build_road, build_track, plan_road_view, plan_road_view_for_mode, RouteMode,
};
use super::geom::wgs84_to_bd09;

#[path = "../../tests/common/stadium.rs"]
mod fixture;

#[test]
fn offline_comparison() {
    let (net, points) = fixture::stadium();
    let road = build_road(2110.0, 900, 7, 0, &points, &points, &net).unwrap();
    let track = build_track(2110.0, 900, 7, 0, &points, &points, &net).unwrap();
    road.validate_consistency().unwrap();
    track.validate_consistency().unwrap();
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
    for seed in 0..20 {
        let track = build_track(2110., 900, seed, 123000, &points, &points, &aligned).unwrap();
        track.validate_consistency().unwrap();
        assert_eq!(track.locations.len(), 902);
        assert_eq!(track.totalTime, 900);
        assert!((track.totalDistance - 2110.).abs() < 0.001);
        assert_eq!(track.locations[1].ptype, 5);
        let mut previous_offset = (0., 0.);
        for (i, p) in track.locations.iter().enumerate() {
            let expected =
                super::route_view::position(&baseline.route, &baseline.arcs, p.totalDis).unwrap();
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
    let track = build_track(2110., 900, 7, 123000, &points, &points, &net).unwrap();
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
