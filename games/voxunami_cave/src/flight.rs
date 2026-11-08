//! Flight from a command seat: what the keys of a seated pilot ask of a ship, the throttle, the torque the seat gives, the push of an engine, and the lines of the readout. The rules need no
//! server; the plugin that applies them to ships is in [`crate::plugins`]. (A voxel is a metre, so lengths are in metres here.)

use voxel_space::{LatticeRotation, VoxelCoordinate};
use world_storage::rail::{Orbit, RailError};

/// What one dev rocket engine pushes with at full throttle, in kilograms times voxels a second a second (newtons): a free push that uses no fuel. A ship of ten blocks (10,000 kg) with one engine
/// speeds up at 20 voxels a second a second.
pub const ENGINE_THRUST: f64 = 200_000.0;

/// How fast the seat can turn a ship, in radians a second a second, whatever the ship weighs: a large torque, worked out from the ship's inertia.
pub const SEAT_ANGULAR_ACCELERATION: f64 = 1.0;

/// The turn, in radians a second about one axis, at which the seat stops adding to it while a key is held.
pub const MOST_TURN_RATE: f64 = 1.0;

/// How much of the whole throttle the keys add or take away in a second.
pub const THROTTLE_PER_SECOND: f64 = 0.5;

/// A turn about every axis under this, in radians a second, with no key held, is none: the seat stops the ship exactly (so that it can go on a rail).
pub const STILL_RATE: f64 = 1.0e-6;

/// What the throttle keys ask of the throttle this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThrottleChange {
    Down,
    #[default]
    Hold,
    Up,
}

/// The keys of a seated pilot, as the ship takes them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlightKeys {
    /// 1 (W) pitches the nose down, -1 (S) up.
    pub pitch_down: i8,
    /// 1 (D) yaws to the right, -1 (A) to the left.
    pub yaw_right: i8,
    /// 1 (E) rolls to the right, -1 (Q) to the left.
    pub roll_right: i8,
    /// Shift raises the throttle, control lowers it.
    pub throttle: ThrottleChange,
}

impl FlightKeys {
    /// The keys from what a player's client says it holds: forward and strafe (W S and D A), the vertical key (space is 1, shift is -1), the sprint key (control) and the roll keys (E and Q).
    pub fn of_movement(forward: i8, strafe: i8, vertical: i8, sprint: bool, roll: i8) -> Self {
        let raise = vertical < 0;
        Self {
            pitch_down: forward,
            yaw_right: strafe,
            roll_right: roll,
            throttle: match (raise, sprint) {
                (true, false) => ThrottleChange::Up,
                (false, true) => ThrottleChange::Down,
                _ => ThrottleChange::Hold,
            },
        }
    }
}

/// The throttle, from 0 (off) to 1 (full), after `seconds` of the keys asking `change`.
pub fn throttle_after(throttle: f64, change: ThrottleChange, seconds: f64) -> f64 {
    let step = THROTTLE_PER_SECOND * seconds;
    match change {
        ThrottleChange::Up => (throttle + step).min(1.0),
        ThrottleChange::Down => (throttle - step).max(0.0),
        ThrottleChange::Hold => throttle,
    }
    .clamp(0.0, 1.0)
}

/// Which way a sitter faces, which way is up for them, and their right hand, in the axes of the ship the seat is in: unit vectors that are exact (a seat is turned by a turn of the lattice).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeatFrame {
    pub forward: [f64; 3],
    pub up: [f64; 3],
    pub right: [f64; 3],
}

impl SeatFrame {
    /// The frame of a seat standing in the orientation (the number of a turn of the lattice) its voxel has, or none for a number that is no turn. As it is made a sitter faces west
    /// (-x) with up up, so their right hand is north (-z).
    pub fn of_orientation(orientation: u32) -> Option<Self> {
        let rotation = LatticeRotation::from_index(u8::try_from(orientation).ok()?)?;
        let turned = |vector: [i128; 3]| rotation.apply(vector).map(|component| component as f64);
        Some(Self {
            forward: turned([-1, 0, 0]),
            up: turned([0, 1, 0]),
            right: turned([0, 0, -1]),
        })
    }
}

/// An engine of a ship: where it is, in the ship's own coordinates, and the way it pushes the ship (a unit vector, opposite its exhaust).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Engine {
    pub at: [f64; 3],
    pub push: [f64; 3],
}

/// The engine that stands in a voxel in an orientation, or none for a number that is no turn. As it is made the exhaust is up (+y), so it pushes down.
pub fn engine_at(voxel: VoxelCoordinate, orientation: u32) -> Option<Engine> {
    let rotation = LatticeRotation::from_index(u8::try_from(orientation).ok()?)?;
    let exhaust = rotation.apply([0, 1, 0]);
    Some(Engine {
        at: [voxel.x(), voxel.y(), voxel.z()].map(|coordinate| coordinate as f64 + 0.5),
        push: exhaust.map(|component| -(component as f64)),
    })
}

/// What the seat does to a ship's turn this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turning {
    /// The torque, in the ship's axes.
    pub torque: [f64; 3],
    /// The ship turns so little, with no key held, that it is to be made still exactly.
    pub still: bool,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The torque (in the ship's axes) that the keys ask, and that brakes the turns they do not ask, for a ship that turns at `angular_velocity` (in its axes) and has the inertias `inertia`
/// about the seat's right hand, up and forward (kilograms times voxels squared, in that order), in a tick of `seconds`. A key gives the seat's full torque about its axis (W about minus the right hand, S the right hand, D about minus
/// up, A up, E forward, Q minus forward) until the ship turns at [`MOST_TURN_RATE`] that way; an axis with no key is braked to a stop, by the full torque or, if that would overshoot, by what
/// stops it in the tick.
pub fn turning(
    frame: &SeatFrame,
    keys: &FlightKeys,
    angular_velocity: [f64; 3],
    inertia: [f64; 3],
    seconds: f64,
) -> Turning {
    // The three axes, with the sign of the turn each key asks for (the right hand turns the nose up).
    let axes = [
        (frame.right, -f64::from(keys.pitch_down)),
        (frame.up, -f64::from(keys.yaw_right)),
        (frame.forward, f64::from(keys.roll_right)),
    ];
    let mut torque = [0.0; 3];
    let mut any_key = false;
    let mut fastest: f64 = 0.0;
    for ((axis, ask), inertia) in axes.into_iter().zip(inertia) {
        let full = inertia * SEAT_ANGULAR_ACCELERATION;
        let rate = dot(angular_velocity, axis);
        fastest = fastest.max(rate.abs());
        let along = if ask != 0.0 {
            any_key = true;
            // Held: full torque that way, until it turns that fast.
            if ask * rate >= MOST_TURN_RATE {
                0.0
            } else {
                ask * full
            }
        } else {
            // Not held: whatever stops it, at most the full torque.
            -(inertia * rate / seconds).clamp(-full, full)
        };
        for index in 0..3 {
            torque[index] += along * axis[index];
        }
    }
    if !any_key && fastest < STILL_RATE {
        return Turning {
            torque: [0.0; 3],
            still: true,
        };
    }
    Turning {
        torque,
        still: false,
    }
}

/// What the orbit a ship is on looks like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OrbitSummary {
    /// No orbit: nothing pulls, or the ship goes straight at the middle of what pulls it.
    None,
    /// A closed orbit, with the nearest and farthest distances from the middle of the planet, in metres, and the seconds of one time round.
    Bound {
        periapsis: f64,
        apoapsis: f64,
        period: f64,
    },
    /// The ship is going too fast to be held: it escapes, past the nearest distance it has or had.
    Escape { periapsis: f64 },
}

impl OrbitSummary {
    /// What the orbit a ship's state is on looks like, or none if the state is on none.
    pub fn of(orbit: &Result<Orbit, RailError>) -> Self {
        match orbit {
            Err(_) => Self::None,
            Ok(orbit) if orbit.is_a_hyperbola() => Self::Escape {
                periapsis: orbit.nearest_distance(),
            },
            Ok(orbit) => Self::Bound {
                periapsis: orbit.nearest_distance(),
                apoapsis: orbit.farthest_distance(),
                period: orbit.period().abs(),
            },
        }
    }
}

/// How far a ship is from the middle of the planet that pulls it, how fast it goes against it, and how fast it goes away from it (negative toward it), from its place and its
/// velocity against the planet.
pub fn distance_speed_and_speed_away(place: [f64; 3], velocity: [f64; 3]) -> (f64, f64, f64) {
    let length = |vector: [f64; 3]| dot(vector, vector).sqrt();
    let distance = length(place);
    let away = if distance > 0.0 {
        dot(place, velocity) / distance
    } else {
        0.0
    };
    (distance, length(velocity), away)
}

/// Everything the readout of a seated pilot says.
#[derive(Clone, Debug, PartialEq)]
pub struct Readout {
    /// The throttle, 0 to 1.
    pub throttle: f64,
    pub engines: usize,
    /// How fast the engines at this throttle speed the ship up, in metres a second a second.
    pub acceleration: f64,
    /// How fast the ship turns, in radians a second.
    pub turn_rate: f64,
    /// The number of the planet that pulls the ship most, if one does.
    pub body: Option<u64>,
    /// How far the ship is from the middle of that planet, how fast it goes against it, and how fast it goes away from it (negative toward it), in metres and metres a second.
    pub distance: f64,
    pub speed: f64,
    pub speed_away: f64,
    pub orbit: OrbitSummary,
}

/// A number to the nearest whole one, with a comma between thousands.
fn grouped(value: f64) -> String {
    let rounded = value.round();
    let digits = format!("{:.0}", rounded.abs());
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(digit);
    }
    if rounded < 0.0 {
        text.insert(0, '-');
    }
    text
}

impl Readout {
    /// The lines to show a pilot, one for each thing.
    pub fn lines(&self) -> Vec<String> {
        let engines = match self.engines {
            0 => "no engines".to_owned(),
            1 => "1 engine".to_owned(),
            count => format!("{count} engines"),
        };
        let throttle = (self.throttle * 100.0).round();
        let mut lines = vec![if self.engines == 0 {
            format!("throttle {throttle:.0} %, {engines}")
        } else {
            format!(
                "throttle {throttle:.0} %, {engines}, {:.1} m/s2",
                self.acceleration
            )
        }];
        lines.push(format!("turning {:.2} rad/s", self.turn_rate));
        let Some(body) = self.body else {
            lines.push("no planet pulls here".to_owned());
            return lines;
        };
        lines.push(format!(
            "planet {body}: {} m from its middle, {} m/s, {:.1} m/s away",
            grouped(self.distance),
            grouped(self.speed),
            self.speed_away
        ));
        lines.push(match self.orbit {
            OrbitSummary::None => "orbit: none".to_owned(),
            OrbitSummary::Bound {
                periapsis,
                apoapsis,
                period,
            } => format!(
                "orbit: periapsis {} m, apoapsis {} m, period {} s",
                grouped(periapsis),
                grouped(apoapsis),
                grouped(period)
            ),
            OrbitSummary::Escape { periapsis } => {
                format!("orbit: escape, periapsis {} m", grouped(periapsis))
            }
        });
        lines
    }
}
