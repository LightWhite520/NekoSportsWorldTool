//! Cadence variability precedes step integration. These are bounded engineering
//! defaults, not parameters estimated from a person's screenshot.
use super::SmoothNoise;
use rand::{rngs::StdRng, Rng, SeedableRng};

pub(super) struct Cadence {
    rng: StdRng,
    event_rng: StdRng,
    rhythm: SmoothNoise,
    adjustment: SmoothNoise,
    variation: f64,
    event_start: i64,
    event_duration: i64,
    stride_offset: f64,
    previous_cadence: Option<f64>,
}

impl Cadence {
    pub(super) fn new(seed: u64, variation: f64) -> Self {
        let mut event_rng = StdRng::seed_from_u64(seed ^ 0x8ebc6af09c88c6e3);
        let mut model = Self {
            rng: StdRng::seed_from_u64(seed ^ 0xc6a4a7935bd1e995),
            event_start: event_rng.gen_range(70..=150),
            event_rng,
            rhythm: SmoothNoise::new(40.0, 6.0, 3.5 * variation),
            adjustment: SmoothNoise::new(4.0, 1.0, 2.0 * variation),
            variation,
            event_duration: 0,
            stride_offset: 0.0,
            previous_cadence: None,
        };
        for _ in 0..180 {
            model.rhythm.step(&mut model.rng);
            model.adjustment.step(&mut model.rng);
        }
        model
    }

    fn stride_scale(&mut self, time: i64) -> f64 {
        if time < self.event_start {
            return 1.0;
        }
        if self.event_duration == 0 {
            self.event_duration = self.event_rng.gen_range(16..=28);
            let sign = if self.event_rng.gen_bool(0.5) {
                1.0
            } else {
                -1.0
            };
            // Stronger settings continue to scale ordinary variability; rare
            // stride excursions remain bounded at the requested 20–30% scale.
            self.stride_offset =
                sign * self.event_rng.gen_range(0.20..=0.30) * self.variation.min(1.0);
        }
        let elapsed = time - self.event_start;
        if elapsed >= self.event_duration {
            self.event_start = time + self.event_rng.gen_range(140..=260);
            self.event_duration = 0;
            return 1.0;
        }
        let phase = elapsed as f64 / self.event_duration as f64;
        let envelope = (std::f64::consts::PI * phase).sin().powi(2);
        1.0 + self.stride_offset * envelope
    }

    pub(super) fn step(&mut self, time: i64, speed: f64) -> f64 {
        let noise = self.rhythm.step(&mut self.rng) + self.adjustment.step(&mut self.rng);
        let cadence = (crate::biomech::gait(speed, 0.0).cadence + noise).clamp(80.0, 220.0);
        let cadence = (cadence / self.stride_scale(time)).clamp(80.0, 220.0);
        let cadence = self
            .previous_cadence
            .map(|previous| cadence.clamp(previous - 8.0, previous + 8.0))
            .unwrap_or(cadence);
        self.previous_cadence = Some(cadence);
        // Slow/stop steps shorten as speed falls. No independent stride values
        // are serialized: distance and integrated steps determine stride.
        cadence * (speed / 1.5).max(0.0).sqrt().min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rare_stride_excursions_are_bounded_smooth_and_disabled_at_zero() {
        let mut zero = Cadence::new(7, 0.0);
        let mut active = Cadence::new(7, 1.0);
        let mut peak: f64 = 0.0;
        let mut previous: f64 = 1.0;
        let mut altered = 0;
        for time in 0..1200 {
            assert_eq!(zero.stride_scale(time), 1.0);
            let scale = active.stride_scale(time);
            assert!((0.7..=1.3).contains(&scale));
            assert!((scale - previous).abs() < 0.07);
            peak = peak.max((scale - 1.0).abs());
            altered += usize::from((scale - 1.0).abs() > 0.01);
            previous = scale;
        }
        assert!(peak >= 0.20);
        assert!((20..300).contains(&altered));
    }
}
