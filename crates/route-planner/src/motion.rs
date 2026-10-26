//! Deterministic one-second integration. No distance is assigned after integration.
use crate::{biomech::gait, Route};
use rand::{rngs::StdRng, SeedableRng};
use rand_distr::{Distribution, StandardNormal};

pub const VERSION: &str = "motion-v3";
pub const MAX_SPEED: f64 = 6.3;
pub const ACCEL: f64 = 0.65;
pub const BRAKE: f64 = 1.0;

#[derive(Clone, Debug)]
pub struct Sample {
    pub time: i64,
    pub distance: f64,
    pub speed: f64,
    pub steps: f64,
}

/// A slowly changing stochastic target followed by a separate response state.
/// Correlation alone leaves a white-noise innovation in every speed increment;
/// the response attenuates those innovations before they reach the runner.
struct SmoothNoise {
    target: f64,
    value: f64,
    memory: f64,
    response: f64,
    innovation: f64,
}

impl SmoothNoise {
    fn new(correlation_s: f64, response_s: f64, stddev: f64) -> Self {
        let memory = (-1. / correlation_s).exp();
        let response = (-1. / response_s).exp();
        // Stationary variance of the cascaded AR(1) processes. Compensate the
        // filter's attenuation so smoothing does not simply erase variation.
        let variance_ratio = (1. - response) * (1. + memory * response)
            / ((1. + response) * (1. - memory * response));
        Self {
            target: 0.,
            value: 0.,
            memory,
            response,
            innovation: stddev * ((1. - memory * memory) / variance_ratio).sqrt(),
        }
    }

    fn step(&mut self, rng: &mut StdRng) -> f64 {
        let z: f64 = StandardNormal.sample(rng);
        self.target = self.memory * self.target + self.innovation * z;
        self.value = self.response * self.value + (1. - self.response) * self.target;
        self.value
    }
}

pub fn generate(route: &Route, duration: i64, seed: u64) -> Result<Vec<Sample>, String> {
    if duration < 2
        || duration > 86400
        || route.points.len() < 2
        || !route.length_m.is_finite()
        || route.length_m <= 0.
        || route.length_m > 100_000.
        || route.points.iter().any(|p| {
            !p.s.is_finite() || !p.curvature.is_finite() || !p.lat.is_finite() || !p.lon.is_finite()
        })
        || route.points.first().map(|p| p.s != 0.).unwrap_or(true)
        || route.points.windows(2).any(|w| w[1].s <= w[0].s)
        || route
            .points
            .last()
            .map(|p| (p.s - route.length_m).abs() > 1e-6)
            .unwrap_or(true)
    {
        return Err("运动输入无效：需要有效路线及 2–86400 秒时长".into());
    }
    let mut rng = StdRng::seed_from_u64(seed ^ 0x9e3779b97f4a7c15);
    let mut noise = Vec::with_capacity(duration as usize);
    // Engineering defaults, not parameters fitted to measured human running.
    let mut pace_noise = SmoothNoise::new(24., 4., 0.02);
    for _ in 0..duration {
        noise.push(pace_noise.step(&mut rng));
    }
    // Squared-speed envelope on route vertices, including repeated lap seams.
    let mut caps: Vec<f64> = route
        .points
        .iter()
        .map(|p| {
            if p.curvature.abs() < 1e-9 {
                MAX_SPEED * MAX_SPEED
            } else {
                (2.5 / p.curvature.abs()).min(MAX_SPEED * MAX_SPEED)
            }
        })
        .collect();
    for i in (0..caps.len() - 1).rev() {
        caps[i] =
            caps[i].min(caps[i + 1] + 2. * BRAKE * (route.points[i + 1].s - route.points[i].s));
    }
    for i in 1..caps.len() {
        caps[i] =
            caps[i].min(caps[i - 1] + 2. * ACCEL * (route.points[i].s - route.points[i - 1].s));
    }
    let cap_at = |s: f64| {
        let i = route
            .points
            .partition_point(|p| p.s < s)
            .clamp(1, route.points.len() - 1);
        let a = &route.points[i - 1];
        let b = &route.points[i];
        let back = caps[i - 1] + 2. * ACCEL * (s - a.s).max(0.);
        let ahead = caps[i] + 2. * BRAKE * (b.s - s).max(0.);
        back.min(ahead).max(0.).sqrt().min(MAX_SPEED)
    };
    let simulate = |base: f64| {
        let mut out = vec![Sample {
            time: 0,
            distance: 0.,
            speed: 0.,
            steps: 0.,
        }];
        let (mut s, mut v) = (0., 0.);
        for t in 1..=duration {
            let target = (base * (1. + noise[t as usize - 1])).max(0.);
            let end_cap = (duration - t) as f64 * BRAKE;
            let desired = target.min(cap_at(s + v)).min(end_cap);
            let next = desired.clamp((v - BRAKE).max(0.), (v + ACCEL).min(MAX_SPEED));
            s += (v + next) * 0.5;
            v = next;
            out.push(Sample {
                time: t,
                distance: s,
                speed: v,
                steps: 0.,
            });
        }
        out
    };
    let (mut low, mut high) = (0., MAX_SPEED * 4.);
    if simulate(high).last().unwrap().distance < route.length_m - 0.001 {
        return Err("给定距离和时长无法满足加减速或弯道约束，请延长时长".into());
    }
    let mut result = simulate(0.);
    for _ in 0..64 {
        let mid = (low + high) * 0.5;
        result = simulate(mid);
        let distance = result.last().unwrap().distance;
        if (distance - route.length_m).abs() < 0.00001 {
            break;
        }
        if distance < route.length_m {
            low = mid;
        } else {
            high = mid;
        }
    }
    if (result.last().unwrap().distance - route.length_m).abs() > 0.01 {
        return Err("运动约束求解未收敛，请调整时长".into());
    }
    let mut cadence_rng = StdRng::seed_from_u64(seed ^ 0xc6a4a7935bd1e995);
    let mut cadence_noise = SmoothNoise::new(10., 3., 1.);
    let mut steps = 0.;
    for i in 1..result.len() {
        let v = (result[i - 1].speed + result[i].speed) * 0.5;
        let cad_noise = cadence_noise.step(&mut cadence_rng);
        let cadence = (gait(v, 0.).cadence + cad_noise).clamp(80., 220.) * (v / 1.5).min(1.);
        steps += cadence / 60.;
        result[i].steps = steps;
    }
    Ok(result)
}
