//! What the third round of the ghost study (`docs/history/loop-probe.md`, section 18) measures with: a seeded random source, the distance between two runs (by what a player could
//! see: places, turns, speeds of the cars), and the summary of a column of numbers.

use crate::contraption::*;
use rapier3d::prelude::{Rotation, Vector};

use crate::contraption::Shape;

/// SplitMix64: a small seeded generator, the same numbers on every machine.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }

    /// Evenly spread in the logarithm (a mass or a speed over several orders).
    pub fn log_range(&mut self, low: f64, high: f64) -> f64 {
        (low.ln() + (high.ln() - low.ln()) * self.unit()).exp()
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next_u64() % items.len() as u64) as usize]
    }
}

/// How far a run is from another, in what a player sees: the greatest over the cars asked about and the ticks asked about.
#[derive(Clone, Copy, Debug, Default)]
pub struct Deviation {
    /// Metres.
    pub position: f64,
    /// Metres a second.
    pub velocity: f64,
    /// Radians.
    pub angle: f64,
    /// The second at which the position was farthest off.
    pub at: f64,
}

impl Deviation {
    /// Whether a player could see it, by the thresholds of the study: 10 mm of place, 5 mrad of turn, 50 mm/s of speed.
    pub fn visible(&self) -> bool {
        self.position > 0.010 || self.angle > 0.005 || self.velocity > 0.050
    }
}

/// The states of a run at the end of tick `k`, whatever number of steps a tick was cut into.
pub fn at_tick<'a>(trace: &'a Trace, tick_length: f64, k: usize) -> &'a Vec<CarState> {
    trace.at(k as f64 * tick_length)
}

/// The distance of `trace` from `reference`, at the end of every tick from `from` to `to` seconds, over the `cars`.
pub fn deviation(scene: &Scene, trace: &Trace, reference: &Trace, tick_length: f64, cars: &[usize], from: f64, to: f64) -> Deviation {
    let mut worst = Deviation::default();
    let first = (from / tick_length).round() as usize;
    let last = (to / tick_length).round() as usize;
    for k in first..=last {
        let (a, b) = (at_tick(trace, tick_length, k), at_tick(reference, tick_length, k));
        for &i in cars {
            let place = (a[i].pose.translation - b[i].pose.translation).length();
            let speed = (a[i].linvel - b[i].linvel).length();
            // A ball's turn is not seen (nothing is drawn on it) and is the least settled number of a solver (the friction at a glancing blow).
            let turn = if matches!(scene.cars[i].shape, Shape::Ball(_)) { 0.0 } else { (a[i].pose.rotation * b[i].pose.rotation.inverse()).to_axis_angle().1.abs() };
            if place > worst.position {
                worst.position = place;
                worst.at = k as f64 * tick_length;
            }
            worst.velocity = worst.velocity.max(speed);
            worst.angle = worst.angle.max(turn.min(std::f64::consts::TAU - turn));
        }
    }
    worst
}

/// The greatest overlap (metres) between cars of different groups, and with the ground, in a run (every frame).
pub fn deepest_overlap(scene: &Scene, trace: &Trace) -> f64 {
    let mut deepest = 0.0f64;
    for frame in &trace.frames {
        for a in 0..scene.cars.len() {
            deepest = deepest.max(overlap_ground(scene, frame, a));
            for b in a + 1..scene.cars.len() {
                if scene.cars[a].group != scene.cars[b].group {
                    deepest = deepest.max(overlap(scene, frame, a, b));
                }
            }
        }
    }
    deepest
}

/// The `p`-th percentile (0 to 100) of the values, by the nearest rank.
pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    let rank = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}
