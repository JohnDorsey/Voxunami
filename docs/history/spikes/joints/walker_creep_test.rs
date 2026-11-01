//! TEMPORARY: how far a character standing on a flat floor creeps when gravity is tilted from the floor's vertical, with the tilt fixed or changing.
mod common;

use common::*;
use physics::MoveInput;

#[test]
fn creep() {
    // (tilt at the start, how much it changes each step)
    for (tilt, change) in [(5.0e-5f64, 1.0e-12f64), (5.0e-5, 5.0e-12), (5.0e-5, 2.0e-11), (5.0e-5, 1.0e-10), (5.0e-5, 1.0e-8), (5.0e-5, 1.0e-6)] {
        let mut scene = Scene::with_floor(0);
        let mut body = spawn(0.5, 2.7, 0.5);
        let mut index = 0u32;
        let mut step = |body: &mut physics::CharacterBody, count: usize| {
            for _ in 0..count {
                let tilt_now = (tilt + change * f64::from(index)) as f32;
                index += 1;
                let gravity = [-8.0 * tilt_now, -8.0, -8.0 * tilt_now];
                body.update_terrain(&mut scene);
                body.keep_clear(&mut scene);
                body.step_with_gravity(MoveInput::default(), gravity);
            }
        };
        step(&mut body, 120);
        let start = eye(&body);
        let mut last_y = start[1];
        let mut hops = Vec::new();
        for second in 1..=120 {
            step(&mut body, 60);
            let now = eye(&body);
            if (now[1] - last_y).abs() > 1.0e-4 {
                hops.push((second, now[1] - last_y));
            }
            last_y = now[1];
        }
        let end = eye(&body);
        println!("CREEP tilt {tilt:e} change {change:e}: in 120 s x {:+.3e}, y {:+.3e}; {} jumps in height of over 0.1 mm between seconds; the first ones (second, change): {:?}; the largest {:.2e}",
            end[0] - start[0], end[1] - start[1], hops.len(), &hops[..hops.len().min(8)], hops.iter().map(|h| h.1.abs()).fold(0.0, f64::max));
    }
}
