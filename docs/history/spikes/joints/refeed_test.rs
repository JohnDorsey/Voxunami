//! TEMPORARY: what reading a body's state back into the f32 types of the interface and giving it to the solver again (as a ship moved to another frame, or loaded from a file, is) costs.
use physics::{BodyId, BodyKind, BodySettings, BodyState, BodyWorld};
use voxel_space::{BrickCoordinate, Orientation, WorldPosition};

fn run(ticks: u32, refeed_every: Option<u32>, velocity: [f32; 3], spin: [f32; 3]) -> BodyState {
    let mut world = BodyWorld::new(
        BrickCoordinate::ORIGIN,
        BodySettings {
            gravity: [0.0; 3],
            ..BodySettings::default()
        },
    );
    let state = BodyState {
        position: WorldPosition::new(BrickCoordinate::new(3, 5, -2), [7.3, 11.9, 20.1]),
        orientation: Orientation::from_yaw_pitch(0.7, 0.2),
        linear_velocity: velocity,
        angular_velocity: spin,
    };
    world.add_body(BodyId(1), state, BodyKind::Dynamic).unwrap();
    for tick in 1..=ticks {
        world.step();
        if let Some(every) = refeed_every {
            if tick % every == 0 {
                let read = world.state(BodyId(1)).unwrap();
                world.set_state(BodyId(1), read).unwrap();
            }
        }
    }
    world.state(BodyId(1)).unwrap()
}

fn apart(a: &BodyState, b: &BodyState) -> f64 {
    a.position
        .displacement_to(b.position)
        .in_voxels()
        .iter()
        .map(|part| part * part)
        .sum::<f64>()
        .sqrt()
}

#[test]
fn refeed() {
    let velocity = [786.3, -12.7, 301.9];
    let spin = [0.0, 0.0, 0.0];
    let hour = 72_000u32;
    let straight = run(hour, None, velocity, spin);
    println!("REFEED an hour at about 840 m/s, no spin: it went {:.1} m", apart(&run(0, None, velocity, spin), &straight));
    for every in [1u32, 20, 400, 7_200] {
        let fed = run(hour, Some(every), velocity, spin);
        println!("REFEED read back and given again every {every:>5} ticks ({} times): {:.3e} m from the uninterrupted one", hour / every, apart(&straight, &fed));
    }
    // A slow one with a spin, as a ship in a gentle turn.
    let (velocity, spin) = ([0.37, 0.0, -0.11], [0.013, 0.021, -0.007]);
    let straight = run(hour, None, velocity, spin);
    for every in [20u32, 400] {
        let fed = run(hour, Some(every), velocity, spin);
        println!("REFEED slow and turning, every {every:>4} ticks: {:.3e} m from the uninterrupted one", apart(&straight, &fed));
    }
}
