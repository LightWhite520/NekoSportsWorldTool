//! Shared motion-to-protocol adapter. All counters come from one integrated timeline.
use super::{
    geom::{fmt_gain_time, Rng},
    model::{GenPoint, Track},
};
use route_planner::{noise::GpsJitter, Coord, Route};

pub fn build(
    route: &Route,
    duration: i64,
    seed: u64,
    start_ms: i64,
    fences: &[Vec<Coord>],
) -> Result<Track, String> {
    if start_ms
        .checked_add(duration.saturating_mul(1000))
        .is_none()
    {
        return Err("开始时间超出范围".into());
    }
    let samples = route_planner::motion::generate(route, duration, seed)?;
    let mut observation = Rng::new(seed ^ 0xa0761d6478bd642f);
    let mut elevation = Rng::new(seed ^ 0xe7037ed1a0b428db);
    let mut jitter = GpsJitter::new(0.92, 0.15, 0.08);
    let mut altitude = 82.;
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
        let observed = if fences.is_empty() {
            Coord::new(
                c.lon + dx / (111320. * c.lat.to_radians().cos()),
                c.lat + dy / 111132.,
            )
        } else {
            c
        };
        // This is a synthetic altitude process, not measured terrain.
        altitude += 0.01 * (82. - altitude) + elevation.gauss(0., 0.015);
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
    if !fences.is_empty()
        && (!locs
            .iter()
            .all(|p| route_planner::boundary::contains(fences, Coord::new(p.gLng, p.gLat)))
            || !locs.windows(2).all(|w| {
                route_planner::boundary::segment_inside(
                    fences,
                    Coord::new(w[0].gLng, w[0].gLat),
                    Coord::new(w[1].gLng, w[1].gLat),
                )
            }))
    {
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
