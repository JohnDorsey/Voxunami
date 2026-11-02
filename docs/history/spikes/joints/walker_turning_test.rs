//! TEMPORARY: a character standing still on a flat floor of a ship that turns about a horizontal axis. In the ship's own axes the floor stays and the pull of gravity turns,
//! so this is what a player aboard a turning ship gives the controller.
mod common;

use common::*;
use physics::MoveInput;

#[test]
fn turning() {
    for (axis, rate) in [("diagonal", 1.0e-4f64), ("diagonal", 1.0e-3), ("diagonal", 1.0e-2), ("diagonal", 5.0e-2), ("diagonal", 0.2), ("x only", 1.0e-2)] {
        let seconds = (0.6 / rate).min(60.0);
        let steps = (seconds * 60.0) as usize;
        let mut scene = Scene::with_floor(0);
        let mut body = spawn(0.5, 2.7, 0.5);
        let mut eye_y = Vec::new();
        let mut eye_x = Vec::new();
        let mut eye_z = Vec::new();
        for index in 0..(steps + 240) {
            // 4 s of settling with the pull straight down, then the turn begins.
            let turned = if index < 240 { 0.0 } else { rate * (index - 240) as f64 / 60.0 };
            let g = 5.0;
            let gravity = if axis == "x only" { [0.0, (-g * turned.cos()) as f32, (g * turned.sin()) as f32] } else { let part = g * turned.sin() / 2.0f64.sqrt(); [part as f32, (-g * turned.cos()) as f32, part as f32] };
            body.update_terrain(&mut scene);
            body.keep_clear(&mut scene);
            body.step_with_gravity(MoveInput::default(), gravity);
            if index >= 240 {
                let eye = eye(&body);
                eye_x.push(eye[0]);
                eye_y.push(eye[1]);
                eye_z.push(eye[2]);
            }
        }
        // How far the eye is from a smooth path: the step it makes against the median of the steps around it.
        let steps_y: Vec<f64> = eye_y.windows(2).map(|pair| pair[1] - pair[0]).collect();
        let mut worst = 0.0f64;
        let mut hops = 0;
        for i in 5..steps_y.len() - 5 {
            let mut around: Vec<f64> = steps_y[i - 5..=i + 5].to_vec();
            around.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let jump = (steps_y[i] - around[5]).abs();
            worst = worst.max(jump);
            if jump > 5.0e-4 {
                hops += 1;
            }
        }
        println!(
            "TURN {axis} rate {rate:e} rad/s for {seconds:.0} s (final tilt {:.2} rad): sideways drift x {:+.2e} z {:+.2e}; eye height changed {:+.2e}; {} steps with a jerk over 0.5 mm in {} (largest {:.2e})",
            rate * seconds,
            eye_x.last().unwrap() - eye_x[0],
            eye_z.last().unwrap() - eye_z[0],
            eye_y.last().unwrap() - eye_y[0],
            hops,
            steps_y.len(),
            worst
        );
    }
}
