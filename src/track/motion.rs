//! Shared motion-to-protocol adapter. All counters come from one integrated timeline.
use super::{
    geom::{fmt_gain_time, Rng},
    model::{GenPoint, Track},
};
use route_planner::{noise::GpsJitter, Coord, Route};

pub fn build_regular(
    route: &Route,
    duration: i64,
    seed: u64,
    start_ms: i64,
    fences: &[Vec<Coord>],
) -> Result<Track, String> {
    build_with_variation(route, duration, seed, start_ms, fences, true, 1.0, 1.0)
}

pub fn build_with_variation(
    route: &Route,
    duration: i64,
    seed: u64,
    start_ms: i64,
    fences: &[Vec<Coord>],
    regular: bool,
    variation: f64,
    drift_variation: f64,
) -> Result<Track, String> {
    validate_drift_variation(drift_variation)?;
    if start_ms
        .checked_add(duration.saturating_mul(1000))
        .is_none()
    {
        return Err("开始时间超出范围".into());
    }
    let samples = route_planner::motion::generate_with_variation(route, duration, seed, variation)?;
    let mut observation = Rng::new(seed ^ 0xa0761d6478bd642f);
    let mut elevation = super::altitude::SyntheticAltitude::new(seed);
    let mut jitter = GpsJitter::new(0.92, 0.15, 0.08);
    let mut track_drift = super::track_drift::TrackDrift::new(seed);
    let mut locs = Vec::with_capacity(samples.len() + 1);
    for (i, sample) in samples.iter().enumerate() {
        let c = route_planner::itinerary::at(route, sample.distance);
        let (dx, dy) = jitter.step(
            1.,
            [
                observation.gauss(0., 1.),
                observation.gauss(0., 1.),
                observation.gauss(0., 1.),
                observation.gauss(0., 1.),
            ],
        );
        let (dx, dy) = if regular {
            if i == 0 {
                (0., 0.)
            } else {
                track_drift.step()
            }
        } else {
            (dx, dy)
        };
        let observed = Coord::new(
            c.lon + dx * drift_variation / (111320. * c.lat.to_radians().cos()),
            c.lat + dy * drift_variation / 111132.,
        );
        // This is a synthetic altitude process, not measured terrain.
        let altitude = elevation.step();
        let prior = if i > 0 { &samples[i - 1] } else { sample };
        let distance = sample.distance - prior.distance;
        let next = route_planner::itinerary::at(route, (sample.distance + 1.).min(route.length_m));
        let bearing = ((next.lon - c.lon) * c.lat.to_radians().cos())
            .atan2(next.lat - c.lat)
            .to_degrees()
            .rem_euclid(360.);
        let steps = sample.steps.round() as i64;
        locs.push(GenPoint {
            id: locs.len() as i64 + 1,
            flag: start_ms,
            lat: -1.,
            lng: -1.,
            gLat: observed.lat,
            gLng: observed.lon,
            speed: sample.speed * 3.6,
            avgSpeed: distance,
            radius: 2.,
            accuracy: 2.,
            ptype: if i + 1 == samples.len() { 6 } else { 0 },
            locType: 1,
            hasAltitude: true,
            totalTime: sample.time,
            totalDis: sample.distance,
            validDis: sample.distance,
            validTime: sample.time,
            steps,
            // Reserved wire field; stride is derived from distance and steps.
            stepDistance: 0.,
            gainTime: fmt_gain_time(start_ms + sample.time * 1000),
            gainTimeMs: start_ms + sample.time * 1000,
            queueNum: 0,
            coorType: "gcj02".into(),
            bdA: altitude,
            bdD: bearing,
            bdS: sample.speed,
            bdG: 1,
            count: 0,
            dtr: 0.,
            state: 1,
            locationId: String::new(),
        });
        if i == 0 {
            let mut sentinel = locs[0].clone();
            sentinel.id = 2;
            sentinel.ptype = 5;
            locs.push(sentinel);
        }
    }
    let inside = |locs: &[GenPoint]| {
        fences.is_empty()
            || (locs
                .iter()
                .all(|p| route_planner::boundary::contains(fences, Coord::new(p.gLng, p.gLat)))
                && locs.windows(2).all(|w| {
                    route_planner::boundary::segment_inside(
                        fences,
                        Coord::new(w[0].gLng, w[0].gLat),
                        Coord::new(w[1].gLng, w[1].gLat),
                    )
                }))
    };
    if !inside(&locs) {
        // Reduce the entire continuous offset field together. Do not clamp or
        // snap individual points at a boundary, which would introduce jumps.
        let offsets: Vec<_> = locs
            .iter()
            .map(|p| {
                let base = route_planner::itinerary::at(route, p.totalDis);
                (base, p.gLat - base.lat, p.gLng - base.lon)
            })
            .collect();
        for scale in [0.5, 0.25, 0.125, 0.0625, 0.0] {
            for (point, (base, lat, lon)) in locs.iter_mut().zip(&offsets) {
                point.gLat = base.lat + lat * scale;
                point.gLng = base.lon + lon * scale;
            }
            if inside(&locs) {
                break;
            }
        }
    }
    if !inside(&locs) {
        return Err("观测点连线越出围栏，请调整路线；未移动或吸附点位".into());
    }
    let mut track = Track {
        totalTime: 0,
        totalDistance: 0.,
        validDistance: 0.,
        validTime: 0,
        startTime: start_ms,
        startLatitude: locs[0].gLat,
        startLongitude: locs[0].gLng,
        locations: locs,
        totalSteps: 0,
        speedPerTenSec: vec![],
        stepsPerTenSec: vec![],
        segments: vec![],
    };
    super::stats::rebuild(&mut track);
    track.validate_consistency()?;
    Ok(track)
}

pub fn validate_drift_variation(value: f64) -> Result<(), String> {
    if !value.is_finite() || !(0.0..=5.0).contains(&value) {
        return Err("路线漂移强度需要 0–5 倍的有限数值".into());
    }
    Ok(())
}

#[cfg(test)]
mod drift_tests {
    use super::*;

    fn route() -> Route {
        route_planner::itinerary::polyline(&[Coord::new(10.0, 30.0), Coord::new(10.01, 30.0)])
            .unwrap()
    }

    #[test]
    fn drift_changes_only_coordinates_and_remains_bounded() {
        let route = route();
        for regular in [false, true] {
            let baseline =
                build_with_variation(&route, 480, 7, 123000, &[], regular, 1.0, 0.0).unwrap();
            let changed =
                build_with_variation(&route, 480, 7, 123000, &[], regular, 1.0, 5.0).unwrap();
            assert!(baseline
                .locations
                .iter()
                .zip(&changed.locations)
                .any(|(a, b)| a.gLat != b.gLat || a.gLng != b.gLng));
            let mut normalized = changed.clone();
            normalized.startLatitude = baseline.startLatitude;
            normalized.startLongitude = baseline.startLongitude;
            for ((point, original), observed) in normalized
                .locations
                .iter_mut()
                .zip(&baseline.locations)
                .zip(&changed.locations)
            {
                if regular {
                    let dx = (observed.gLng - original.gLng)
                        * 111320.0
                        * original.gLat.to_radians().cos();
                    let dy = (observed.gLat - original.gLat) * 111132.0;
                    assert!(dx.hypot(dy) <= 1.75);
                }
                point.gLat = original.gLat;
                point.gLng = original.gLng;
            }
            assert_eq!(
                serde_json::to_value(normalized).unwrap(),
                serde_json::to_value(baseline).unwrap()
            );
        }
        for bad in [-1.0, f64::NAN, f64::INFINITY, 5.1] {
            assert!(build_with_variation(&route, 480, 7, 0, &[], true, 1.0, bad).is_err());
        }
    }

    #[test]
    fn drift_never_leaves_a_fence_including_routes_on_its_boundary() {
        let route = route();
        for bottom in [29.999, 30.0] {
            let fence = vec![
                Coord::new(9.999, bottom),
                Coord::new(10.011, bottom),
                Coord::new(10.011, 30.001),
                Coord::new(9.999, 30.001),
            ];
            let fences = vec![fence];
            let track = build_with_variation(&route, 480, 7, 0, &fences, true, 1.0, 5.0).unwrap();
            assert!(track.locations.windows(2).all(
                |pair| route_planner::boundary::segment_inside(
                    &fences,
                    Coord::new(pair[0].gLng, pair[0].gLat),
                    Coord::new(pair[1].gLng, pair[1].gLat),
                )
            ));
        }
    }
}
