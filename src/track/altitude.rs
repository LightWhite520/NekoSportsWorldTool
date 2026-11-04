//! 轨迹海拔覆盖。
//!
//! 海拔是轨迹点的一部分，而不是提交时临时拼出来的字段。统一在生成器
//! 完成后覆盖 `bdA`，这样提交体的 totalAscent、OBS 的 run_data 和每圈
//! elevationGain 都会读取同一份数据。

#![allow(non_snake_case)]

use super::geom::{round_to, Rng};
use super::model::Track;

/// Synthetic observation drift, not terrain. Two response stages suppress
/// second-to-second innovations before a manual range can amplify them.
pub(super) struct SyntheticAltitude {
    rng: Rng,
    target: f64,
    response: f64,
    value: f64,
    alpha: f64,
}

impl SyntheticAltitude {
    pub(super) fn new(seed: u64) -> Self {
        let mut profile = Self {
            rng: Rng::new(seed ^ 0xe7037ed1a0b428db),
            target: 82.0,
            response: 82.0,
            value: 82.0,
            alpha: 1.0 - (-1.0_f64 / 20.0).exp(),
        };
        // Initialize both filters before the run; avoid a flat startup transient
        // being stretched to the full manual range on short records.
        for _ in 0..300 {
            profile.step();
        }
        profile
    }

    /// Advance one second, matching the shared motion timeline.
    pub(super) fn step(&mut self) -> f64 {
        self.target += 0.01 * (82.0 - self.target) + self.rng.gauss(0.0, 0.015);
        self.response += self.alpha * (self.target - self.response);
        self.value += self.alpha * (self.response - self.value);
        self.value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AltitudeRange {
    pub min_m: f64,
    pub max_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AltitudeSpec {
    Single(f64),
    Range(AltitudeRange),
}

pub fn parse_spec(text: &str) -> Result<Option<AltitudeSpec>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if let Some((left, right)) = text.split_once('-') {
        let min_m = left
            .trim()
            .parse::<f64>()
            .map_err(|_| "手动海拔区间格式应为 min-max，例如 11.6-22.8".to_string())?;
        let max_m = right
            .trim()
            .parse::<f64>()
            .map_err(|_| "手动海拔区间格式应为 min-max，例如 11.6-22.8".to_string())?;
        return Ok(Some(AltitudeSpec::Range(validate_range(min_m, max_m)?)));
    }
    let altitude_m = text
        .parse::<f64>()
        .map_err(|_| "手动海拔应为数字或 min-max 区间，例如 17.2 或 11.6-22.8".to_string())?;
    validate_altitude(altitude_m)?;
    Ok(Some(AltitudeSpec::Single(altitude_m)))
}

fn validate_altitude(altitude_m: f64) -> Result<(), String> {
    if !altitude_m.is_finite() || !(-500.0..=9000.0).contains(&altitude_m) {
        return Err("手动海拔必须是 -500 到 9000 米之间的数字".into());
    }
    Ok(())
}

fn validate_range(min_m: f64, max_m: f64) -> Result<AltitudeRange, String> {
    validate_altitude(min_m)?;
    validate_altitude(max_m)?;
    if min_m > max_m {
        return Err("手动海拔区间的最小值不能大于最大值".into());
    }
    Ok(AltitudeRange { min_m, max_m })
}

/// 将轨迹所有点的百度海拔覆盖为用户输入的绝对海拔（米）。
///
/// 返回 `Err` 而不是静默接受 NaN/无穷值，避免生成不可序列化或被服务端
/// 拒绝的提交。海拔范围采用常见地表范围，既能覆盖地下场地也能覆盖高原。
pub fn override_bd_a(track: &mut Track, altitude_m: f64) -> Result<(), String> {
    validate_altitude(altitude_m)?;
    for point in &mut track.locations {
        point.bdA = round_to(altitude_m, 2);
        point.hasAltitude = true;
    }
    Ok(())
}

/// 将生成器的连续海拔曲线映射到用户指定的绝对海拔区间。
/// 区间是上下界；不足 0.2m 的源变化不强行拉满，避免放大近乎平坦的噪声。
pub fn override_bd_a_range(track: &mut Track, range: AltitudeRange) -> Result<(), String> {
    let range = validate_range(range.min_m, range.max_m)?;
    let (mut current_min, mut current_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for point in &track.locations {
        current_min = current_min.min(point.bdA);
        current_max = current_max.max(point.bdA);
    }
    let span = current_max - current_min;
    let source_midpoint = (current_min + current_max) / 2.0;
    let target_span = range.max_m - range.min_m;
    let target_midpoint = (range.min_m + range.max_m) / 2.0;
    for point in &mut track.locations {
        let mapped = target_midpoint + (point.bdA - source_midpoint) / span.max(0.2) * target_span;
        point.bdA = round_to(mapped.clamp(range.min_m, range.max_m), 2);
        point.hasAltitude = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::submit::total_ascent;
    use crate::track::generator::build;

    fn points() -> Vec<(f64, f64)> {
        vec![(38.901678, 121.540241), (38.902564, 121.541233)]
    }

    #[test]
    fn override_replaces_every_bd_a_and_clears_ascent() {
        let mut track =
            build(1200.0, 600, 7, (38.9, 121.54), 1_700_000_000_000, &points()).unwrap();
        override_bd_a(&mut track, 36.75).unwrap();
        assert!(track.locations.iter().all(|p| p.bdA == 36.75));
        assert_eq!(total_ascent(&track.locations), 0.0);
    }

    #[test]
    fn rejects_non_finite_or_out_of_range_values() {
        let mut track =
            build(1000.0, 500, 1, (38.9, 121.54), 1_700_000_000_000, &points()).unwrap();
        assert!(override_bd_a(&mut track, f64::NAN).is_err());
        assert!(override_bd_a(&mut track, 9001.0).is_err());
    }

    #[test]
    fn parses_single_value_and_range() {
        assert_eq!(
            parse_spec("17.2").unwrap(),
            Some(AltitudeSpec::Single(17.2))
        );
        assert_eq!(
            parse_spec("11.6-22.8").unwrap(),
            Some(AltitudeSpec::Range(AltitudeRange {
                min_m: 11.6,
                max_m: 22.8
            }))
        );
        assert!(parse_spec("22.8-11.6").is_err());
    }

    #[test]
    fn range_does_not_expand_nearly_flat_noise() {
        let mut track = build(1200.0, 600, 7, (38.9, 121.54), 0, &points()).unwrap();
        for (i, point) in track.locations.iter_mut().enumerate() {
            point.bdA = 82.0 + if i % 2 == 0 { 1e-8 } else { -1e-8 };
        }
        override_bd_a_range(
            &mut track,
            AltitudeRange {
                min_m: 11.6,
                max_m: 22.8,
            },
        )
        .unwrap();
        assert!(track.locations.iter().all(|p| p.bdA == 17.2));
        assert_eq!(total_ascent(&track.locations), 0.0);

        override_bd_a_range(
            &mut track,
            AltitudeRange {
                min_m: 18.5,
                max_m: 18.5,
            },
        )
        .unwrap();
        assert!(track.locations.iter().all(|p| p.bdA == 18.5));
    }

    #[test]
    fn generated_altitude_is_smooth_after_range_mapping_across_seeds() {
        for seed in 0..32 {
            let mut track = build(1200.0, 600, seed, (38.9, 121.54), 0, &points()).unwrap();
            assert!(track
                .locations
                .windows(2)
                .all(|p| (p[1].bdA - p[0].bdA).abs() < 0.02));
            override_bd_a_range(
                &mut track,
                AltitudeRange {
                    min_m: 11.6,
                    max_m: 22.8,
                },
            )
            .unwrap();
            assert_eq!(track.locations[0].bdA, track.locations[1].bdA);
            let changes: Vec<_> = track
                .locations
                .windows(2)
                .filter(|p| p[1].totalTime > p[0].totalTime)
                .map(|p| p[1].bdA - p[0].bdA)
                .collect();
            let max_step = changes.iter().copied().map(f64::abs).fold(0.0, f64::max);
            assert!(max_step < 0.25, "seed={seed}, max step={max_step}");
            assert!(
                changes.windows(2).all(|d| (d[1] - d[0]).abs() < 0.05),
                "seed={seed}"
            );
            assert!(track
                .locations
                .iter()
                .all(|p| (11.6..=22.8).contains(&p.bdA)));
        }
    }

    #[test]
    fn short_and_constant_profiles_do_not_fill_the_entire_range() {
        let original = build(1200.0, 600, 7, (38.9, 121.54), 0, &points()).unwrap();
        let mut short = original.clone();
        short.locations.truncate(4); // t=0 sentinel and the first two seconds.
        override_bd_a_range(
            &mut short,
            AltitudeRange {
                min_m: 11.6,
                max_m: 22.8,
            },
        )
        .unwrap();
        assert!(short
            .locations
            .iter()
            .all(|p| (16.95..=17.45).contains(&p.bdA)));

        let mut constant = original;
        for p in &mut constant.locations {
            p.bdA = 82.0;
        }
        override_bd_a_range(
            &mut constant,
            AltitudeRange {
                min_m: 11.6,
                max_m: 22.8,
            },
        )
        .unwrap();
        assert!(constant.locations.iter().all(|p| p.bdA == 17.2));
        constant.locations.clear();
        override_bd_a_range(
            &mut constant,
            AltitudeRange {
                min_m: 11.6,
                max_m: 22.8,
            },
        )
        .unwrap();
    }

    #[test]
    fn range_mapping_stays_inside_requested_bounds() {
        let mut track =
            build(1200.0, 600, 7, (38.9, 121.54), 1_700_000_000_000, &points()).unwrap();
        let before = serde_json::to_value(&track).unwrap();
        override_bd_a_range(
            &mut track,
            AltitudeRange {
                min_m: 11.6,
                max_m: 22.8,
            },
        )
        .unwrap();
        assert!(track
            .locations
            .iter()
            .all(|p| (11.6..=22.8).contains(&p.bdA)));
        let mut after = serde_json::to_value(&track).unwrap();
        for (point, original) in after["locations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(before["locations"].as_array().unwrap())
        {
            point["bdA"] = original["bdA"].clone();
            point["hasAltitude"] = original["hasAltitude"].clone();
        }
        assert_eq!(
            after, before,
            "altitude mapping must preserve other track data"
        );
        // Optional offline evidence: numeric timeline only, without coordinates.
        if let Some(path) = std::env::var_os("NEKO_ALTITUDE_COMPARISON_FILE") {
            let output = serde_json::json!({
                "distance_m": track.totalDistance,
                "duration_s": track.totalTime,
                "range_m": [11.6, 22.8],
                "seed": 7,
                "ascent_m": total_ascent(&track.locations),
                "samples": track.locations.iter().map(|p| serde_json::json!({
                    "time_s": p.totalTime, "altitude_m": p.bdA
                })).collect::<Vec<_>>()
            });
            std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
        }
    }
}
