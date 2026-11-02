//! Offline diagnostic command: intentionally independent of config and account loaders.
use super::{get, parse_flags};
use route_planner::{
    itinerary::{polyline, Itinerary},
    Coord,
};

pub fn run(args: &[&str]) -> i32 {
    match check(args) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
fn check(args: &[&str]) -> Result<serde_json::Value, String> {
    let flags = parse_flags(args);
    let number = |name: &str, default: f64| -> Result<f64, String> {
        match get(&flags, name) {
            Some(s) => s.parse().map_err(|_| format!("--{name} 必须为数字")),
            None => Ok(default),
        }
    };
    let distance = number("distance", 2000.)?;
    let seconds = number("duration", 800.)?;
    if !seconds.is_finite() || seconds.fract() != 0. || !(2. ..=86400.).contains(&seconds) {
        return Err("--duration 需要 2–86400 的整数秒".into());
    }
    let seed = get(&flags, "seed")
        .unwrap_or("7")
        .parse::<u64>()
        .map_err(|_| "--seed 需要无符号整数")?;
    let (itinerary, candidate_count) = if let Some(path) = get(&flags, "map") {
        let net = crate::track::generate_road::load_network_path(path)?;
        let Some(id) = get(&flags, "candidate") else {
            return Ok(
                serde_json::json!({"candidates":net.tracks.iter().map(|c|serde_json::json!({"id":c.id,"source":c.source,"counterclockwise_allowed":c.counterclockwise_allowed,"length_m":polyline(&c.points).ok().map(|r|r.length_m)})).collect::<Vec<_>>() }),
            );
        };
        let candidate = net
            .tracks
            .iter()
            .find(|c| c.id == id)
            .ok_or("没有指定的跑道候选")?;
        (
            route_planner::athletics::plan(&net, &[candidate.points[0]], candidate, distance)?,
            net.tracks.len(),
        )
    } else {
        let coords: Vec<_> = (0..=120)
            .map(|i| {
                let t = i as f64 / 120. * std::f64::consts::TAU;
                Coord::new(
                    10. + 60. * t.cos() / (111320. * 30f64.to_radians().cos()),
                    30. + 60. * t.sin() / 111132.,
                )
            })
            .collect();
        (Itinerary::looping(polyline(&coords)?), 0)
    };
    let route = itinerary.expand(distance)?;
    let samples = route_planner::motion::generate(&route, seconds as i64, seed)?;
    let mut max_accel = 0f64;
    let mut max_brake = 0f64;
    let mut max_jerk = 0f64;
    for w in samples.windows(2) {
        let a = w[1].speed - w[0].speed;
        max_accel = max_accel.max(a);
        max_brake = max_brake.max(-a);
    }
    for w in samples.windows(3) {
        max_jerk = max_jerk.max((w[2].speed - 2. * w[1].speed + w[0].speed).abs());
    }
    let last = samples.last().unwrap();
    let mut report = serde_json::json!({"model":route_planner::motion::VERSION,"seed":seed,"duration_s":last.time,"distance_m":last.distance,"distance_error_m":last.distance-distance,
        "steps":last.steps.round(),"prefix_m":itinerary.prefix.length_m,"circuit_m":itinerary.circuit.length_m,"candidates":candidate_count,"max_acceleration_m_s2":max_accel,"max_braking_m_s2":max_brake,"max_jerk_m_s3":max_jerk,"samples":samples.len()});
    // Optional numeric timeline for offline comparisons; never includes coordinates.
    if get(&flags, "timeline").is_some() {
        report["timeline"] =
            serde_json::json!(samples.iter().map(|s| serde_json::json!({
            "time_s": s.time, "distance_m": s.distance, "speed_m_s": s.speed, "steps": s.steps
        })).collect::<Vec<_>>());
    }
    Ok(report)
}
