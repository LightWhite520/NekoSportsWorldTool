use route_planner::{
    itinerary::{at, polyline, Itinerary},
    lanes, Coord,
};

fn point(x: f64, y: f64) -> Coord {
    Coord::new(
        10.0 + x / (111320.0 * 30_f64.to_radians().cos()),
        30.0 + y / 111132.0,
    )
}
fn xy(c: Coord) -> (f64, f64) {
    (
        (c.lon - 10.0) * 111320.0 * 30_f64.to_radians().cos(),
        (c.lat - 30.0) * 111132.0,
    )
}
fn trip() -> Itinerary {
    let mut ring: Vec<_> = (0..120)
        .map(|i| {
            let a = i as f64 / 120.0 * std::f64::consts::TAU;
            point(60.0 * a.cos(), 60.0 * a.sin())
        })
        .collect();
    ring.push(ring[0]);
    Itinerary::looping(polyline(&ring).unwrap())
}

#[test]
fn adjacent_changes_follow_lane_width_and_actual_geometry_distance() {
    let trip = trip();
    let lane_width = 1.22;
    {
        for seed in 0..8 {
            let result = lanes::plan(&trip, 2100.0, seed, &[]).unwrap();
            let again = lanes::expand(&trip, 2100.0, seed, &[]).unwrap();
            assert_eq!(
                result
                    .route
                    .points
                    .iter()
                    .map(|p| (p.lon, p.lat, p.s))
                    .collect::<Vec<_>>(),
                again
                    .points
                    .iter()
                    .map(|p| (p.lon, p.lat, p.s))
                    .collect::<Vec<_>>()
            );
            let coords: Vec<_> = result
                .route
                .points
                .iter()
                .map(|p| Coord::new(p.lon, p.lat))
                .collect();
            let physical_length = polyline(&coords).unwrap().length_m;
            assert!((physical_length - 2100.0).abs() < 0.02);
            assert!((result.laps.iter().sum::<f64>() + result.prefix_m - 2100.0).abs() < 1e-6);
            assert!(result.laps[..result.laps.len() - 1]
                .windows(2)
                .any(|w| (w[1] - w[0]).abs() > 0.1));
            let radial: Vec<_> = coords
                .iter()
                .map(|&c| {
                    let (x, y) = xy(c);
                    x.hypot(y) - 60.0
                })
                .collect();
            assert!(radial.iter().all(|r| r.abs() < lane_width + 0.04));
            assert!(radial.iter().any(|r| r.abs() > lane_width - 0.06));
            assert!(
                radial.windows(2).all(|w| (w[1] - w[0]).abs() < 0.12),
                "no sideways jumps"
            );
            for w in coords.windows(2) {
                let (a, b) = (xy(w[0]), xy(w[1]));
                assert!((b.0 - a.0).hypot(b.1 - a.1) < 1.1);
                assert!(a.0 * b.1 - a.1 * b.0 > -1e-6, "CCW direction must remain");
            }
            let samples = route_planner::motion::generate(&result.route, 840, seed).unwrap();
            assert!((samples.last().unwrap().distance - physical_length).abs() < 0.02);
            for w in samples.windows(2) {
                let acceleration = w[1].speed - w[0].speed;
                assert!((-1.000001..=0.650001).contains(&acceleration));
            }
        }
    }
}

#[test]
fn connector_stays_fixed_and_unsuitable_circuits_keep_their_geometry() {
    let mut trip = trip();
    trip.prefix = polyline(&[point(100.0, 0.0), point(60.0, 0.0)]).unwrap();
    let result = lanes::plan(&trip, 1300.0, 7, &[]).unwrap();
    for s in [0.0, 10.0, 25.0, 39.0] {
        let (a, b) = (at(&result.route, s), at(&trip.prefix, s));
        assert!((a.lon - b.lon).abs() < 1e-8 && (a.lat - b.lat).abs() < 1e-8);
    }
    assert!((result.prefix_m - trip.prefix.length_m).abs() < 0.01);
    let open = Itinerary::looping(polyline(&[point(0.0, 0.0), point(100.0, 0.0)]).unwrap());
    let unchanged = lanes::expand(&open, 50.0, 7, &[]).unwrap();
    let baseline = open.expand(50.0).unwrap();
    assert_eq!(
        unchanged
            .points
            .iter()
            .map(|p| (p.lon, p.lat))
            .collect::<Vec<_>>(),
        baseline
            .points
            .iter()
            .map(|p| (p.lon, p.lat))
            .collect::<Vec<_>>()
    );
}

#[test]
fn lane_changes_respect_whole_segment_fences() {
    let trip = trip();
    // The reference ring lies inside this fence; the outer adjacent lane does not.
    let fences = vec![trip
        .circuit
        .points
        .iter()
        .map(|p| Coord::new(p.lon, p.lat))
        .collect()];
    for seed in 0..5 {
        let route = lanes::expand(&trip, 1500.0, seed, &fences).unwrap();
        assert!(route
            .points
            .windows(2)
            .all(|w| route_planner::boundary::segment_inside(
                &fences,
                Coord::new(w[0].lon, w[0].lat),
                Coord::new(w[1].lon, w[1].lat)
            )));
    }
}
