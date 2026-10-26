use route_planner::{
    athletics::{normalize, plan},
    itinerary::{polyline, Itinerary},
    load_osm, motion, Coord,
};

fn map(reverse: bool, oneway: bool, split: bool) -> String {
    let refs = if reverse {
        vec![1, 4, 3, 2, 1]
    } else {
        vec![1, 2, 3, 4, 1]
    };
    let part = |id, refs: &[u64]| {
        format!(
            "<way id=\"{id}\">{}<tag k=\"leisure\" v=\"track\"/>{}</way>",
            refs.iter()
                .map(|id| format!("<nd ref=\"{id}\"/>"))
                .collect::<String>(),
            if oneway {
                "<tag k=\"oneway\" v=\"yes\"/>"
            } else {
                ""
            }
        )
    };
    format!("<osm><node id=\"1\" lat=\"30\" lon=\"10\"/><node id=\"2\" lat=\"30\" lon=\"10.001\"/><node id=\"3\" lat=\"30.001\" lon=\"10.001\"/><node id=\"4\" lat=\"30.001\" lon=\"10\"/><node id=\"5\" lat=\"30\" lon=\"9.999\"/><way id=\"9\"><nd ref=\"5\"/><nd ref=\"1\"/><tag k=\"highway\" v=\"footway\"/></way>{}</osm>",if split {part(1,&refs[..3])+&part(2,&refs[2..])}else{part(1,&refs)})
}
#[test]
fn tagged_split_and_reversed_tracks_are_ccw() {
    for reverse in [false, true] {
        for split in [false, true] {
            let net = load_osm(map(reverse, false, split).as_bytes()).unwrap();
            assert_eq!(net.tracks.len(), 1);
            let t = &net.tracks[0];
            assert_eq!(t.source, "tagged");
            assert!(t.counterclockwise_allowed);
            assert!(t.points[1].lon > t.points[0].lon);
            assert_eq!(normalize(&t.points).unwrap(), t.points);
        }
    }
}
#[test]
fn clockwise_oneway_is_not_silently_reversed() {
    let net = load_osm(map(true, true, false).as_bytes()).unwrap();
    let t = &net.tracks[0];
    assert!(!t.counterclockwise_allowed);
    assert!(plan(&net, &[t.points[0]], t, 2000.).is_err());
}
#[test]
fn prefix_once_partial_lap_and_unreachable_entry() {
    let mut net = load_osm(map(false, false, false).as_bytes()).unwrap();
    let t = net.tracks[0].clone();
    let start = Coord::new(9.999, 30.);
    let trip = plan(&net, &[start], &t, 1100.).unwrap();
    assert!(trip.prefix.length_m > 90.);
    let expanded = trip.expand(1100.).unwrap();
    assert_eq!(expanded.length_m, 1100.);
    assert_eq!(expanded.points[0].lon, start.lon);
    assert_eq!(
        expanded
            .points
            .iter()
            .filter(|p| p.lon == start.lon)
            .count(),
        1
    );
    assert!(plan(&net, &[start], &t, 50.).is_err());
    net.graph.retain_edges(|g, e| g[e].way_id != 9);
    net.rebuild_index();
    assert!(plan(&net, &[start], &t, 1100.).is_err());
}
#[test]
fn invalid_rings_and_missing_osm_nodes_are_rejected() {
    assert!(normalize(&[Coord::new(10., 30.); 4]).is_err());
    assert!(normalize(&[
        Coord::new(10., 30.),
        Coord::new(10.001, 30.001),
        Coord::new(10., 30.001),
        Coord::new(10.001, 30.)
    ])
    .is_err());
    let xml = map(false, false, false).replace("<node id=\"2\" lat=\"30\" lon=\"10.001\"/>", "");
    let net = load_osm(xml.as_bytes()).unwrap();
    assert!(net.tracks.is_empty());
    assert_eq!(net.graph.edge_count(), 2);
}
#[test]
fn motion_integrals_acceleration_and_reproducibility() {
    let net = load_osm(map(false, false, false).as_bytes()).unwrap();
    let trip = Itinerary::looping(polyline(&net.tracks[0].points).unwrap());
    let route = trip.expand(2300.).unwrap();
    for seed in 0..20 {
        let a = motion::generate(&route, 900, seed).unwrap();
        let b = motion::generate(&route, 900, seed).unwrap();
        assert!((a.last().unwrap().distance - 2300.).abs() < 0.01);
        assert_eq!(a.last().unwrap().speed, 0.);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.distance, y.distance);
            assert_eq!(x.steps, y.steps);
        }
        for w in a.windows(2) {
            let dv = w[1].speed - w[0].speed;
            assert!(dv <= motion::ACCEL + 1e-8 && dv >= -motion::BRAKE - 1e-8);
            assert!((w[1].distance - w[0].distance - (w[1].speed + w[0].speed) * 0.5).abs() < 1e-8);
            assert!(w[1].steps >= w[0].steps);
        }
    }
    assert!(motion::generate(&route, 10, 7).is_err());
    assert!(motion::generate(&route, 0, 7).is_err());
}

#[test]
fn steady_running_retains_variation_without_second_to_second_jaggedness() {
    // A straight synthetic route isolates pace/cadence noise from corner limits.
    let route = polyline(&[Coord::new(10., 30.), Coord::new(10.02, 30.)]).unwrap();
    let rms =
        |values: &[f64]| (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt();
    for duration in [500, 800, 1200] {
        for seed in 0..20 {
            let samples = motion::generate(&route, duration, seed).unwrap();
            // Start/stop and geometric constraints have their own tests; these
            // bounds specifically protect the steady-running stochastic model.
            let middle = &samples[30..samples.len() - 30];
            let mean = middle.iter().map(|s| s.speed).sum::<f64>() / middle.len() as f64;
            let deviations: Vec<_> = middle.iter().map(|s| s.speed - mean).collect();
            let accel: Vec<_> = middle.windows(2).map(|w| w[1].speed - w[0].speed).collect();
            let jerk: Vec<_> = accel.windows(2).map(|w| w[1] - w[0]).collect();
            let cadence: Vec<_> = middle
                .windows(2)
                .map(|w| (w[1].steps - w[0].steps) * 60.)
                .collect();
            let cadence_change: Vec<_> = cadence.windows(2).map(|w| w[1] - w[0]).collect();
            assert!(
                (0.003..0.08).contains(&(rms(&deviations) / mean)),
                "variation lost: {duration}/{seed}"
            );
            assert!(
                rms(&accel) / mean < 0.004,
                "pace changes too abrupt: {duration}/{seed}"
            );
            assert!(
                rms(&jerk) / mean < 0.004,
                "pace too jagged: {duration}/{seed}"
            );
            assert!(
                rms(&cadence_change) < 0.4,
                "cadence too jagged: {duration}/{seed}"
            );
        }
    }
}

#[test]
fn bend_speed_is_limited_at_crossings_and_lap_seams() {
    let mut xy = vec![(0., -3.), (100., -3.)];
    for i in 1..=30 {
        let a = -std::f64::consts::FRAC_PI_2 + i as f64 / 30. * std::f64::consts::PI;
        xy.push((100. + 3. * a.cos(), 3. * a.sin()));
    }
    xy.push((0., 3.));
    for i in 1..30 {
        let a = std::f64::consts::FRAC_PI_2 + i as f64 / 30. * std::f64::consts::PI;
        xy.push((3. * a.cos(), 3. * a.sin()));
    }
    xy.push(xy[0]);
    let coords: Vec<_> = xy
        .into_iter()
        .map(|(x, y)| Coord::new(10. + x / 96405., 30. + y / 111132.))
        .collect();
    let route = Itinerary::looping(polyline(&coords).unwrap())
        .expand(2000.)
        .unwrap();
    for seed in 0..8 {
        let samples = motion::generate(&route, 600, seed).unwrap();
        for vertex in &route.points {
            let i = samples
                .partition_point(|s| s.distance < vertex.s)
                .clamp(1, samples.len() - 1);
            let (a, b) = (&samples[i - 1], &samples[i]);
            let v2 = a.speed * a.speed + 2. * (b.speed - a.speed) * (vertex.s - a.distance);
            assert!(
                v2 * vertex.curvature.abs() <= 2.5 + 0.1,
                "centripetal acceleration {} at {}",
                v2 * vertex.curvature.abs(),
                vertex.s
            );
        }
    }
}

#[test]
fn manual_track_needs_shared_entry_and_respects_boundary() {
    let net = load_osm(map(false, false, false).as_bytes()).unwrap();
    let mut candidate = net.tracks[0].clone();
    candidate.id = "manual".into();
    candidate.source = "manual".into();
    let start = Coord::new(9.999, 30.);
    assert!(plan(&net, &[start], &candidate, 1000.).is_ok());
    for c in &mut candidate.points {
        c.lat += 0.002;
    }
    assert!(plan(&net, &[start], &candidate, 1000.).is_err());
    let mut bounded = net.clone();
    bounded.boundaries = vec![vec![
        Coord::new(9.998, 29.998),
        Coord::new(10.0005, 29.998),
        Coord::new(10.0005, 30.002),
        Coord::new(9.998, 30.002),
    ]];
    assert!(plan(&bounded, &[start], &net.tracks[0], 1000.).is_err());
    let restricted = load_osm(map(true, true, false).as_bytes()).unwrap();
    let mut manual = restricted.tracks[0].clone();
    manual.source = "manual".into();
    manual.counterclockwise_allowed = true;
    assert!(plan(&restricted, &[start], &manual, 1000.).is_err());
}

#[test]
fn road_track_tag_is_only_unconfirmed_geometry() {
    let xml = map(false, false, false).replace(
        "<tag k=\"leisure\" v=\"track\"/>",
        "<tag k=\"highway\" v=\"track\"/>",
    );
    let net = load_osm(xml.as_bytes()).unwrap();
    assert_eq!(net.tracks.len(), 1);
    assert_eq!(net.tracks[0].source, "geometry");
}
