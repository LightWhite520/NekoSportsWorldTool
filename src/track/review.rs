//! Read-only presentation of the final track that will be submitted.
use super::{geom::round_to, model::Track};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Point {
    pub time_s: f64,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RunReview {
    pub start_ms: i64,
    pub duration_s: i64,
    pub distance_m: f64,
    pub steps: i64,
    pub ascent_m: f64,
    pub avg_pace_s: f64,
    pub avg_cadence: f64,
    pub avg_stride_cm: Option<f64>,
    pub route: Vec<(f64, f64)>,
    pub pace: Vec<Point>,
    pub cadence: Vec<Point>,
    pub stride: Vec<Point>,
    pub altitude: Vec<Point>,
}

impl RunReview {
    pub fn from_track(track: &Track) -> Result<Self, String> {
        track.validate_consistency()?;
        let mut cadence = Vec::new();
        let mut stride = Vec::new();
        let mut elapsed = 0;
        for (distance, steps) in track.speedPerTenSec.iter().zip(&track.stepsPerTenSec) {
            elapsed += distance.time;
            cadence.push(Point {
                time_s: elapsed as f64,
                value: (distance.time > 0).then(|| steps.value * 60.0 / distance.time as f64),
            });
            stride.push(Point {
                time_s: elapsed as f64,
                value: (steps.value > 0.0).then(|| distance.value * 100.0 / steps.value),
            });
        }
        Ok(Self {
            start_ms: track.startTime,
            duration_s: track.totalTime,
            distance_m: track.totalDistance,
            steps: track.totalSteps,
            ascent_m: crate::api::submit::total_ascent(&track.locations),
            avg_pace_s: track.totalTime as f64 * 1000.0 / track.totalDistance,
            avg_cadence: track.totalSteps as f64 * 60.0 / track.totalTime as f64,
            avg_stride_cm: (track.totalSteps > 0)
                .then(|| track.totalDistance * 100.0 / track.totalSteps as f64),
            route: track.locations.iter().map(|p| (p.gLat, p.gLng)).collect(),
            // Match the encoded OBS pace, including its precision and stopped samples.
            pace: track
                .locations
                .iter()
                .map(|p| Point {
                    time_s: p.totalTime as f64,
                    value: (p.speed > 0.0).then(|| round_to(60.0 / p.speed, 4) * 60.0),
                })
                .collect(),
            cadence,
            stride,
            altitude: track
                .locations
                .iter()
                .map(|p| Point {
                    time_s: p.totalTime as f64,
                    value: Some(round_to(p.bdA, 2)),
                })
                .collect(),
        })
    }
}

pub fn format_pace(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "—".into();
    }
    let seconds = seconds.round() as i64;
    format!("{}′{:02}″", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_uses_final_track_and_windows_including_partial_tail() {
        let mut track = crate::track::generator::build(
            1200.0,
            603,
            7,
            (30.0, 10.0),
            123000,
            &[(30.0, 10.0), (30.001, 10.001)],
        )
        .unwrap();
        crate::track::altitude::override_bd_a_range(
            &mut track,
            crate::track::altitude::AltitudeRange {
                min_m: 11.6,
                max_m: 22.8,
            },
        )
        .unwrap();
        let before = serde_json::to_string(&track).unwrap();
        let review = RunReview::from_track(&track).unwrap();
        assert_eq!(serde_json::to_string(&track).unwrap(), before);
        assert_eq!(review.start_ms, track.startTime);
        assert_eq!(
            review.route.last(),
            Some(&(
                track.locations.last().unwrap().gLat,
                track.locations.last().unwrap().gLng
            ))
        );
        assert!(review.pace[0].value.is_some());
        assert_eq!(review.pace.last().unwrap().value, None);
        assert_eq!(format_pace(402.0), "6′42″");
        assert_eq!(review.cadence.last().unwrap().time_s, 603.0);
        assert_eq!(
            review.cadence.last().unwrap().value,
            Some(track.stepsPerTenSec.last().unwrap().value * 20.0)
        );
        for ((cadence, stride), (distance, steps)) in review
            .cadence
            .iter()
            .zip(&review.stride)
            .zip(track.speedPerTenSec.iter().zip(&track.stepsPerTenSec))
        {
            if let Some(stride) = stride.value {
                let meters = cadence.value.unwrap() / 60.0 * stride / 100.0 * distance.time as f64;
                assert!((meters - distance.value).abs() < 1e-8);
            } else {
                assert_eq!(steps.value, 0.0);
            }
        }
        assert_eq!(
            review.ascent_m,
            crate::api::submit::total_ascent(&track.locations)
        );
        assert!(review
            .altitude
            .iter()
            .all(|p| (11.6..=22.8).contains(&p.value.unwrap())));
    }
}
