//! Small, slowly varying observation offset for the opt-in athletics mode.
use super::geom::Rng;

pub const MAX_DRIFT_M: f64 = 0.35;

pub struct TrackDrift {
    rng: Rng,
    target: [f64; 2],
    offset: [f64; 2],
}

impl TrackDrift {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed ^ 0x6a09_e667_f3bc_c909),
            target: [0.; 2],
            offset: [0.; 2],
        }
    }

    /// One second per step. Reserve two centimetres for wire coordinate rounding.
    pub fn step(&mut self) -> (f64, f64) {
        for axis in 0..2 {
            self.target[axis] = 0.98 * self.target[axis] + self.rng.gauss(0., 0.025);
        }
        let radius = self.target[0].hypot(self.target[1]);
        let scale = ((MAX_DRIFT_M - 0.02) / radius.max(1e-12)).min(1.);
        for axis in 0..2 {
            self.target[axis] *= scale;
            self.offset[axis] += 0.08 * (self.target[axis] - self.offset[axis]);
        }
        (self.offset[0], self.offset[1])
    }
}
