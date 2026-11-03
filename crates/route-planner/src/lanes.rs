//! Adjacent-lane geometry is planned before timing; GPS observation noise is separate.
use crate::{
    itinerary::{at, polyline, Itinerary},
    Coord, Route,
};
use rand::{rngs::StdRng, Rng, SeedableRng};

pub const LANE_WIDTH_M: f64 = 1.22;

struct Change {
    start: f64,
    length: f64,
    from: f64,
    to: f64,
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (10.0 + t * (-15.0 + 6.0 * t))
}

fn schedule(offsets: &[f64], distance: f64, seed: u64) -> Vec<Change> {
    let mut rng = StdRng::seed_from_u64(seed ^ 0x243f_6a88_85a3_08d3);
    let mut lane = rng.gen_range(0..offsets.len());
    // Merge gently from the entry/reference line; do not move the connector.
    let mut changes = vec![Change {
        start: 0.0,
        length: 40.0,
        from: 0.0,
        to: offsets[lane],
    }];
    let mut s = 40.0 + rng.gen_range(180.0..300.0);
    while offsets.len() > 1 && s < distance {
        let next = if lane == 0 {
            1
        } else if lane + 1 == offsets.len() {
            lane - 1
        } else if rng.gen_bool(0.5) {
            lane + 1
        } else {
            lane - 1
        };
        let length = rng.gen_range(35.0..65.0);
        changes.push(Change {
            start: s,
            length,
            from: offsets[lane],
            to: offsets[next],
        });
        lane = next;
        s += length + rng.gen_range(220.0..420.0);
    }
    changes
}

fn offset_at(changes: &[Change], s: f64) -> f64 {
    let i = changes.partition_point(|c| c.start <= s).saturating_sub(1);
    let c = &changes[i];
    c.from + (c.to - c.from) * smooth((s - c.start) / c.length)
}

fn inside(fences: &[Vec<Coord>], points: &[Coord]) -> bool {
    fences.is_empty()
        || points
            .windows(2)
            .all(|w| crate::boundary::segment_inside(fences, w[0], w[1]))
}

pub struct Plan {
    pub route: Route,
    pub prefix_m: f64,
    pub laps: Vec<f64>,
}

pub fn expand(
    trip: &Itinerary,
    distance: f64,
    seed: u64,
    fences: &[Vec<Coord>],
) -> Result<Route, String> {
    Ok(plan(trip, distance, seed, fences)?.route)
}

/// Relative lanes around a reference line, never an assumed numbered first lane.
/// An unsuitable/non-simple circuit keeps its original geometry. A blocked lane
/// is removed as a whole, not squeezed into a fractional lane or snapped to a fence.
pub fn plan(
    trip: &Itinerary,
    distance: f64,
    seed: u64,
    fences: &[Vec<Coord>],
) -> Result<Plan, String> {
    let baseline_route = trip.expand(distance)?;
    let mut baseline_laps = Vec::new();
    let mut remaining = distance - trip.prefix.length_m;
    while remaining > 1e-6 {
        let lap = remaining.min(trip.circuit.length_m);
        baseline_laps.push(lap);
        remaining -= lap;
    }
    let baseline = Plan {
        route: baseline_route,
        prefix_m: trip.prefix.length_m,
        laps: baseline_laps,
    };
    let ring: Vec<_> = trip
        .circuit
        .points
        .iter()
        .map(|p| Coord::new(p.lon, p.lat))
        .collect();
    if ring.len() < 4 || ring.first() != ring.last() || crate::athletics::normalize(&ring).is_err()
    {
        return Ok(baseline);
    }
    let n = ring.len() - 1;
    let mx = 111320.0 * ring[0].lat.to_radians().cos();
    let xy = |a: Coord, b: Coord| [(b.lon - a.lon) * mx, (b.lat - a.lat) * 111132.0];
    let mut normals = Vec::with_capacity(n + 1);
    for i in 0..n {
        let a = xy(ring[(i + n - 1) % n], ring[i]);
        let b = xy(ring[i], ring[(i + 1) % n]);
        let (la, lb) = (a[0].hypot(a[1]), b[0].hypot(b[1]));
        if la < 0.01 || lb < 0.01 {
            return Ok(baseline);
        }
        let (a, b) = ([-a[1] / la, a[0] / la], [-b[1] / lb, b[0] / lb]);
        let denominator = 1.0 + a[0] * b[0] + a[1] * b[1];
        if denominator < 0.5 {
            return Ok(baseline);
        }
        normals.push([(a[0] + b[0]) / denominator, (a[1] + b[1]) / denominator]);
    }
    normals.push(normals[0]);
    let shifted = |s: f64, offset: f64| {
        let s = s.rem_euclid(trip.circuit.length_m);
        let i = trip.circuit.points.partition_point(|p| p.s < s).clamp(1, n);
        let (a, b) = (&trip.circuit.points[i - 1], &trip.circuit.points[i]);
        let t = ((s - a.s) / (b.s - a.s)).clamp(0.0, 1.0);
        let normal = [
            normals[i - 1][0] * (1.0 - t) + normals[i][0] * t,
            normals[i - 1][1] * (1.0 - t) + normals[i][1] * t,
        ];
        Coord::new(
            a.lon + (b.lon - a.lon) * t + normal[0] * offset / mx,
            a.lat + (b.lat - a.lat) * t + normal[1] * offset / 111132.0,
        )
    };
    let mut offsets: Vec<_> = [-LANE_WIDTH_M, 0.0, LANE_WIDTH_M]
        .into_iter()
        .filter(|&offset| {
            let loop_points: Vec<_> = trip
                .circuit
                .points
                .iter()
                .map(|p| shifted(p.s, offset))
                .collect();
            inside(fences, &loop_points) && crate::athletics::normalize(&loop_points).is_ok()
        })
        .collect();
    // Removing a blocked middle lane must never create a two-lane jump.
    if offsets.len() == 2 && offsets[1] - offsets[0] > LANE_WIDTH_M * 1.01 {
        offsets = vec![0.0];
    }
    if offsets.is_empty() || (offsets.len() == 1 && offsets[0] == 0.0) {
        return Ok(baseline);
    }
    let max_progress = (distance - trip.prefix.length_m) * 2.0 + trip.circuit.length_m;
    let changes = schedule(&offsets, max_progress, seed);
    let mut coords: Vec<_> = trip
        .prefix
        .points
        .iter()
        .map(|p| Coord::new(p.lon, p.lat))
        .collect();
    let mut length = trip.prefix.length_m;
    let mut lap = 0.0;
    let mut lap_starts = Vec::new();
    'laps: while lap < max_progress {
        for pair in trip.circuit.points.windows(2) {
            let count = (pair[1].s - pair[0].s).ceil() as usize;
            for j in 0..count {
                let s = pair[0].s + (pair[1].s - pair[0].s) * j as f64 / count as f64;
                let c = shifted(s, offset_at(&changes, lap + s));
                let mut separation = f64::INFINITY;
                if let Some(&previous) = coords.last() {
                    let d = xy(previous, c);
                    separation = d[0].hypot(d[1]);
                    length += separation;
                }
                if separation >= 1e-6 {
                    coords.push(c);
                }
                if pair[0].s == 0.0 && j == 0 {
                    lap_starts.push(coords.len() - 1);
                }
                if length >= distance + 3.0 {
                    break 'laps;
                }
                if coords.len() > 300_000 {
                    return Err("换道路线采样规模过大".into());
                }
            }
        }
        lap += trip.circuit.length_m;
    }
    let mut route = polyline(&coords)?;
    if route.length_m < distance {
        return Err("换道路线长度不足".into());
    }
    // Cut at the requested *actual* arc length, not at the old lap parameter.
    let end = at(&route, distance);
    let prefix_m = route.points[lap_starts[0]].s;
    let mut boundaries: Vec<_> = lap_starts
        .iter()
        .map(|&i| route.points[i].s)
        .filter(|s| *s < distance)
        .collect();
    boundaries.push(distance);
    let laps = boundaries.windows(2).map(|p| p[1] - p[0]).collect();
    let end_curvature = route.points[route
        .points
        .partition_point(|p| p.s < distance)
        .min(route.points.len() - 1)]
    .curvature;
    route.points.retain(|p| p.s < distance);
    route.points.push(crate::RoutePoint {
        lat: end.lat,
        lon: end.lon,
        s: distance,
        curvature: end_curvature,
    });
    route.length_m = distance;
    let points: Vec<_> = route
        .points
        .iter()
        .map(|p| Coord::new(p.lon, p.lat))
        .collect();
    if !inside(fences, &points) {
        return Ok(baseline);
    }
    Ok(Plan {
        route,
        prefix_m,
        laps,
    })
}
