//! An eel's body and how it is steered: which way it heads, which way its view faces, and which
//! way its flat body is turned.
//!
//! An eel is a flat ribbon, three cells wide and one thick, that swims along an axis one cell at a
//! time. Which way the ribbon's width lies is the eel's *roll*: lying flat (wide horizontally) or
//! on its edge (wide vertically). Rolling is a quarter turn about the way it heads; it decides what
//! gaps the eel fits through, and what wall it leaves behind it.
//!
//! The player's view keeps the world's up, so steering is relative to the view and the roll does
//! not turn it. An eel heading up or down remembers which horizontal way it faced before, so that
//! turning left or right from there is relative to that.

use std::f32::consts::{FRAC_PI_2, PI};

use voxel_space::FaceDirection;

use crate::arena::Cell;

/// What a player can ask of an eel. Each takes effect at the eel's next step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steer {
    /// Turn to the left of the view, level.
    Left,
    Right,
    /// Turn upward: from level to heading up, from heading down to level.
    Up,
    Down,
    /// A quarter turn about the way it heads: from lying flat to on its edge and back.
    Roll,
}

impl Steer {
    pub const ALL: [Self; 5] = [Self::Left, Self::Right, Self::Up, Self::Down, Self::Roll];

    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }
}

fn is_vertical(direction: FaceDirection) -> bool {
    direction.axis() == 1
}

/// The horizontal direction a quarter turn to the left of `direction` (seen from above) faces.
fn left_of(direction: FaceDirection) -> FaceDirection {
    match direction {
        FaceDirection::NegativeZ => FaceDirection::NegativeX,
        FaceDirection::NegativeX => FaceDirection::PositiveZ,
        FaceDirection::PositiveZ => FaceDirection::PositiveX,
        FaceDirection::PositiveX => FaceDirection::NegativeZ,
        vertical => vertical,
    }
}

fn right_of(direction: FaceDirection) -> FaceDirection {
    left_of(left_of(left_of(direction)))
}

/// Which way an eel heads, which horizontal way its view faces, and how its body lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Orientation {
    heading: FaceDirection,
    /// The horizontal way the view faces: the heading, unless heading up or down.
    facing: FaceDirection,
    /// Whether the ribbon is wide vertically (on its edge) when heading horizontally.
    on_its_edge: bool,
}

impl Orientation {
    /// Heading `heading`, lying flat. An eel that starts heading up or down faces -z.
    pub fn new(heading: FaceDirection) -> Self {
        Self {
            heading,
            facing: if is_vertical(heading) {
                FaceDirection::NegativeZ
            } else {
                heading
            },
            on_its_edge: false,
        }
    }

    pub fn heading(self) -> FaceDirection {
        self.heading
    }

    pub fn facing(self) -> FaceDirection {
        self.facing
    }

    pub fn is_on_its_edge(self) -> bool {
        self.on_its_edge
    }

    /// The orientation after a steer.
    pub fn steered(self, steer: Steer) -> Self {
        let turn_from = if is_vertical(self.heading) {
            self.facing
        } else {
            self.heading
        };
        match steer {
            Steer::Left | Steer::Right => {
                let turned = if steer == Steer::Left {
                    left_of(turn_from)
                } else {
                    right_of(turn_from)
                };
                Self {
                    heading: turned,
                    facing: turned,
                    ..self
                }
            }
            Steer::Up => self.pitched(FaceDirection::PositiveY),
            Steer::Down => self.pitched(FaceDirection::NegativeY),
            Steer::Roll => Self {
                on_its_edge: !self.on_its_edge,
                ..self
            },
        }
    }

    /// Turning toward `vertical` (up or down): level becomes vertical, the other vertical becomes
    /// level, and already heading that way changes nothing.
    fn pitched(self, vertical: FaceDirection) -> Self {
        if self.heading == vertical {
            self
        } else if is_vertical(self.heading) {
            Self {
                heading: self.facing,
                ..self
            }
        } else {
            Self {
                heading: vertical,
                ..self
            }
        }
    }

    /// The axis (0 for x, 1 for y, 2 for z) along which the ribbon is wide.
    pub fn width_axis(self) -> usize {
        if is_vertical(self.heading) {
            if self.on_its_edge { 2 } else { 0 }
        } else if self.on_its_edge {
            1
        } else if self.heading.axis() == 0 {
            2
        } else {
            0
        }
    }

    /// The cells of the ribbon whose middle is `head`.
    pub fn ribbon(self, head: Cell) -> [Cell; 3] {
        let axis = self.width_axis();
        [-1, 0, 1].map(|step| {
            let mut cell = head;
            cell[axis] += step;
            cell
        })
    }

    /// The cell a head moves to.
    pub fn ahead(self, head: Cell) -> Cell {
        let offset = self.heading.offset();
        [
            head[0] + offset[0],
            head[1] + offset[1],
            head[2] + offset[2],
        ]
    }

    /// The camera's yaw (radians): 0 looks toward -z and a positive angle turns left.
    pub fn view_yaw(self) -> f32 {
        match self.facing {
            FaceDirection::NegativeZ => 0.0,
            FaceDirection::NegativeX => FRAC_PI_2,
            FaceDirection::PositiveZ => PI,
            _ => -FRAC_PI_2,
        }
    }

    /// The camera's pitch: a little down when heading level, along the heading when it is vertical
    /// (short of straight, which the camera cannot be).
    pub fn view_pitch(self) -> f32 {
        const STRAIGHT: f32 = FRAC_PI_2 - 0.05;
        const LEVEL: f32 = -0.35;
        match self.heading {
            FaceDirection::PositiveY => STRAIGHT,
            FaceDirection::NegativeY => -STRAIGHT,
            _ => LEVEL,
        }
    }

    /// Where the camera is, in voxels from the middle of the head: behind it, and when heading level
    /// above it.
    pub fn view_offset(self) -> [f32; 3] {
        const BEHIND: f32 = 6.0;
        const ABOVE: f32 = 2.5;
        let heading = self.heading.offset();
        let mut offset = heading.map(|step| -(step as f32) * BEHIND);
        if !is_vertical(self.heading) {
            offset[1] += ABOVE;
        }
        offset
    }
}

#[cfg(test)]
mod tests {
    use FaceDirection::*;

    use super::*;

    fn heading(heading: FaceDirection) -> Orientation {
        Orientation::new(heading)
    }

    #[test]
    fn left_and_right_turn_the_way_the_view_turns_and_four_of_them_come_round() {
        // Looking along -z, left is -x (as with the camera's positive yaw).
        assert_eq!(heading(NegativeZ).steered(Steer::Left).heading(), NegativeX);
        assert_eq!(
            heading(NegativeZ).steered(Steer::Right).heading(),
            PositiveX
        );
        for start in [NegativeX, PositiveX, NegativeZ, PositiveZ] {
            let mut turned = heading(start);
            for _ in 0..4 {
                turned = turned.steered(Steer::Left);
            }
            assert_eq!(turned, heading(start));
            assert_eq!(
                heading(start).steered(Steer::Left).steered(Steer::Right),
                heading(start)
            );
        }
    }

    #[test]
    fn up_and_down_climb_level_off_and_remember_which_way_the_view_faced() {
        let level = heading(PositiveX);
        let climbing = level.steered(Steer::Up);
        assert_eq!(
            (climbing.heading(), climbing.facing()),
            (PositiveY, PositiveX)
        );
        // Already climbing: nothing changes; down levels off, facing the same way as before.
        assert_eq!(climbing.steered(Steer::Up), climbing);
        assert_eq!(climbing.steered(Steer::Down), level);
        let diving = level.steered(Steer::Down);
        assert_eq!(diving.heading(), NegativeY);
        assert_eq!(diving.steered(Steer::Up), level);
        // Turning from a climb is relative to the way the view faced.
        let turned = climbing.steered(Steer::Left);
        assert_eq!((turned.heading(), turned.facing()), (NegativeZ, NegativeZ));
        assert_eq!(climbing.steered(Steer::Right).heading(), PositiveZ);
    }

    #[test]
    fn rolling_turns_the_ribbon_a_quarter_and_two_rolls_undo_it() {
        let flat = heading(NegativeZ);
        assert!(!flat.is_on_its_edge());
        assert_eq!(
            flat.width_axis(),
            0,
            "flat while heading along z: wide along x"
        );
        let edge = flat.steered(Steer::Roll);
        assert!(edge.is_on_its_edge());
        assert_eq!(edge.width_axis(), 1, "on its edge: wide along y");
        assert_eq!(edge.steered(Steer::Roll), flat);
        // The heading and facing are untouched.
        assert_eq!((edge.heading(), edge.facing()), (NegativeZ, NegativeZ));
        // Along x the flat ribbon is wide along z; climbing, it is wide along x or z.
        assert_eq!(heading(PositiveX).width_axis(), 2);
        assert_eq!(heading(PositiveY).width_axis(), 0);
        assert_eq!(heading(PositiveY).steered(Steer::Roll).width_axis(), 2);
    }

    #[test]
    fn the_ribbon_is_three_cells_across_the_heading_and_never_along_it() {
        for start in FaceDirection::ALL {
            for rolled in [false, true] {
                let orientation = if rolled {
                    heading(start).steered(Steer::Roll)
                } else {
                    heading(start)
                };
                let ribbon = orientation.ribbon([10, 20, 30]);
                assert_eq!(ribbon[1], [10, 20, 30]);
                let across = [
                    ribbon[2][0] - ribbon[0][0],
                    ribbon[2][1] - ribbon[0][1],
                    ribbon[2][2] - ribbon[0][2],
                ];
                assert_eq!(across.iter().map(|c| c.abs()).sum::<i128>(), 2, "{start:?}");
                let along = start.offset();
                assert_eq!(
                    (0..3).map(|axis| across[axis] * along[axis]).sum::<i128>(),
                    0,
                    "{start:?} {rolled}"
                );
            }
        }
    }

    #[test]
    fn the_view_looks_the_way_the_eel_heads_from_behind_it() {
        assert_eq!(heading(NegativeZ).view_yaw(), 0.0);
        assert!((heading(NegativeX).view_yaw() - FRAC_PI_2).abs() < 1e-6);
        assert!((heading(PositiveZ).view_yaw() - PI).abs() < 1e-6);
        assert!((heading(PositiveX).view_yaw() + FRAC_PI_2).abs() < 1e-6);
        assert_eq!(heading(NegativeZ).view_offset(), [0.0, 2.5, 6.0]);
        assert_eq!(heading(PositiveY).view_offset(), [0.0, -6.0, 0.0]);
        assert!(heading(PositiveY).view_pitch() > 1.4);
        assert!(heading(NegativeY).view_pitch() < -1.4);
        assert!(heading(PositiveX).view_pitch() < 0.0);
        // Rolling does not turn the view.
        let rolled = heading(PositiveX).steered(Steer::Roll);
        assert_eq!(rolled.view_yaw(), heading(PositiveX).view_yaw());
        assert_eq!(rolled.view_offset(), heading(PositiveX).view_offset());
    }

    #[test]
    fn steers_survive_their_index() {
        for steer in Steer::ALL {
            assert_eq!(Steer::from_index(steer.index()), Some(steer));
        }
        assert_eq!(Steer::from_index(5), None);
    }
}
