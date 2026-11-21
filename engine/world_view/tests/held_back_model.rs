//! The mesher that holds its pictures back, against a model of what the renderer is told: whatever is done to it and in whatever order (moving, showing, hiding,
//! editing, changing what is drawn from, resetting, with work in flight or not), the sets that say what state a brick is in agree, and once it has nothing more to
//! do every brick that is wanted is ready or shown, and what is on the screen for a wanted brick is what a mesher that was never disturbed makes of it.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use coarse_cells::FineRegion;
use proptest::prelude::*;
use voxel_space::BrickCoordinate;
use world_storage::test_support::brick;
use world_view::{MeshUpdate, ViewChannels, WorldMesher};

mod common;

use common::{LitBrick, Scene, scene};

const VIEW_RADIUS: u32 = 2;

#[derive(Clone, Debug)]
enum Operation {
    /// One update, with whatever is in flight left in flight.
    Update,
    /// Updates until nothing is pending.
    Settle,
    Move {
        axis: usize,
        bricks: i128,
    },
    /// Shows one of the bricks that are ready, if any.
    Show {
        which: usize,
    },
    /// Shows every brick that is ready.
    ShowAll,
    /// Hides one of the bricks that were shown, if any.
    Hide {
        which: usize,
    },
    Edit {
        x: i128,
        y: i128,
        z: i128,
        stone: bool,
    },
    Channels {
        lit: bool,
    },
    Reset,
}

/// A voxel coordinate along a horizontal axis: anywhere, or on the face of a brick, where an edit changes the picture of its neighbour as well.
fn horizontal() -> impl Strategy<Value = i128> {
    prop_oneof![
        1 => -70i128..70,
        2 => (-2i128..=2, any::<bool>()).prop_map(|(brick, far_face)| brick * 32 + if far_face { 31 } else { 0 }),
    ]
}

fn operations() -> impl Strategy<Value = Vec<Operation>> {
    prop::collection::vec(
        prop_oneof![
            4 => Just(Operation::Update),
            3 => Just(Operation::Settle),
            3 => (0usize..3, -2i128..=2).prop_map(|(axis, bricks)| Operation::Move { axis, bricks }),
            3 => (0usize..1000).prop_map(|which| Operation::Show { which }),
            2 => Just(Operation::ShowAll),
            3 => (0usize..1000).prop_map(|which| Operation::Hide { which }),
            4 => (horizontal(), 0i128..12, horizontal(), any::<bool>())
                .prop_map(|(x, y, z, stone)| Operation::Edit { x, y, z, stone }),
            1 => any::<bool>().prop_map(|lit| Operation::Channels { lit }),
            1 => Just(Operation::Reset),
        ],
        1..40,
    )
}

/// What the renderer holds, as the updates tell it.
#[derive(Default)]
struct Renderer {
    meshes: HashMap<BrickCoordinate, world_view::MeshUpdate>,
}

impl Renderer {
    fn apply(&mut self, updates: Vec<MeshUpdate>) {
        for update in updates {
            match update {
                MeshUpdate::Upload { coordinate, .. } => {
                    self.meshes.insert(coordinate, update);
                }
                MeshUpdate::Remove(coordinate) => {
                    assert!(
                        self.meshes.remove(&coordinate).is_some(),
                        "told to take down {coordinate:?}, which the renderer does not hold"
                    );
                }
                other => panic!("a mesher of bricks said {other:?}"),
            }
        }
    }
}

struct Trial {
    scene: Scene,
    mesher: WorldMesher,
    centre: BrickCoordinate,
    plain: ViewChannels,
    lit: ViewChannels,
    lit_now: bool,
    renderer: Renderer,
    shown: BTreeSet<BrickCoordinate>,
}

fn wanted(centre: BrickCoordinate) -> Vec<BrickCoordinate> {
    let region = FineRegion::new(centre, VIEW_RADIUS);
    let reach = i128::from(VIEW_RADIUS) + 3;
    let mut wanted = Vec::new();
    for x in -reach..=reach {
        for y in -reach..=reach {
            for z in -reach..=reach {
                let at = BrickCoordinate::new(centre.x() + x, centre.y() + y, centre.z() + z);
                if region.wants_brick(at) {
                    wanted.push(at);
                }
            }
        }
    }
    wanted
}

impl Trial {
    fn new() -> Self {
        let mut scene = scene();
        scene.assign_around(brick(0, 0, 0), 14);
        let plain = ViewChannels::new(scene.material, None, None);
        let hook = Arc::new(LitBrick {
            lit: brick(0, 0, 0),
            brightness: 7,
        });
        let lit = scene.channels_lit_by(&hook);
        let mesher = scene
            .mesher_seeing(i128::from(VIEW_RADIUS), plain.clone())
            .with_held_back_display();
        Self {
            scene,
            mesher,
            centre: brick(0, 0, 0),
            plain,
            lit,
            lit_now: false,
            renderer: Renderer::default(),
            shown: BTreeSet::new(),
        }
    }

    fn check(&self, after: &Operation) {
        if let Err(problem) = self.mesher.check_consistency() {
            panic!("after {after:?}: {problem}");
        }
    }

    fn run(&mut self, operation: &Operation) {
        match *operation {
            Operation::Update => {
                let updates = self.mesher.update(self.scene.world.main(), self.centre);
                self.renderer.apply(updates);
            }
            Operation::Settle => {
                let updates = self.mesher.settle(self.scene.world.main(), self.centre);
                self.renderer.apply(updates);
            }
            Operation::Move { axis, bricks } => {
                let mut at = [self.centre.x(), self.centre.y(), self.centre.z()];
                at[axis] = (at[axis] + bricks).clamp(-4, 4);
                self.centre = brick(at[0], at[1], at[2]);
                let updates = self.mesher.update(self.scene.world.main(), self.centre);
                self.renderer.apply(updates);
            }
            Operation::Show { which } => {
                let ready: Vec<BrickCoordinate> = wanted(self.centre)
                    .into_iter()
                    .filter(|at| self.mesher.is_ready(*at))
                    .collect();
                if ready.is_empty() {
                    return;
                }
                let at = ready[which % ready.len()];
                let update = self.mesher.show(at);
                self.renderer.apply(update.into_iter().collect());
                self.shown.insert(at);
            }
            Operation::ShowAll => {
                for at in wanted(self.centre) {
                    if self.mesher.is_ready(at) {
                        let update = self.mesher.show(at);
                        self.renderer.apply(update.into_iter().collect());
                        self.shown.insert(at);
                    }
                }
            }
            Operation::Hide { which } => {
                if self.shown.is_empty() {
                    return;
                }
                let at = *self.shown.iter().nth(which % self.shown.len()).unwrap();
                let update = self.mesher.hide(at);
                self.renderer.apply(update.into_iter().collect());
                self.shown.remove(&at);
            }
            Operation::Edit { x, y, z, stone } => {
                let block = if stone {
                    "base_blocks:stone"
                } else {
                    "base_blocks:air"
                };
                let edited = self.scene.edit([x, y, z], block);
                self.mesher.brick_changed(edited);
            }
            Operation::Channels { lit } => {
                self.lit_now = lit;
                let channels = if lit { &self.lit } else { &self.plain };
                self.mesher.set_channels(channels.clone());
            }
            Operation::Reset => {
                let updates = self.mesher.reset();
                self.renderer.apply(updates);
                self.shown.clear();
            }
        }
    }

    /// With nothing more to do, what is wanted is ready or shown, and what is on the screen for it is what an undisturbed mesher makes.
    fn check_at_rest(&mut self) {
        let updates = self.mesher.settle(self.scene.world.main(), self.centre);
        self.renderer.apply(updates);
        self.check(&Operation::Settle);

        let mut undisturbed = self
            .scene
            .mesher_seeing(
                i128::from(VIEW_RADIUS),
                if self.lit_now {
                    self.lit.clone()
                } else {
                    self.plain.clone()
                },
            )
            .with_held_back_display();
        undisturbed.settle(self.scene.world.main(), self.centre);

        for at in wanted(self.centre) {
            assert!(
                self.mesher.is_ready(at) || self.mesher.is_shown(at),
                "{at:?} is wanted, and is neither ready nor shown, with nothing more to do"
            );
            if !self.mesher.is_shown(at) {
                continue;
            }
            let expected = match undisturbed.show(at) {
                Some(update) => Some(update),
                None => {
                    assert!(
                        undisturbed.is_shown(at),
                        "{at:?} is not ready when undisturbed"
                    );
                    None
                }
            };
            assert_eq!(
                self.renderer.meshes.get(&at),
                expected.as_ref(),
                "what is on the screen for {at:?} is not what is made of it now"
            );
        }
        for at in self.renderer.meshes.keys() {
            assert!(
                self.mesher.is_shown(*at),
                "{at:?} is on the screen and is not shown"
            );
        }
    }
}

proptest! {
    // A few seconds by default; `PROPTEST_CASES=300` searches further.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|cases| cases.parse().ok()).unwrap_or(10),
        ..ProptestConfig::default()
    })]

    #[test]
    fn whatever_is_done_to_a_held_back_mesher_its_states_agree_and_it_ends_up_drawing_what_is_wanted(
        operations in operations(),
    ) {
        let mut trial = Trial::new();
        // Something to work on from the start.
        trial.run(&Operation::Settle);
        for operation in &operations {
            trial.run(operation);
            trial.check(operation);
        }
        trial.check_at_rest();
    }
}
