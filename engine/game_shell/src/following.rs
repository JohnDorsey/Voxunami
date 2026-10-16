//! Showing the player where the server says they are, smoothly.
//!
//! A server that moves players tells the client where its player is a few times a second (once a
//! tick, and only when that changed). Shown as it comes, the view would jump that often. A
//! [`PoseFollower`] works out how fast the player is going from the last two positions it was told,
//! and carries the view on that way until the next one comes, so that steady movement looks steady.
//! When a position does not come for longer than a tick is expected to take, the player has stopped
//! (the server only says what changed), and the view is put at the last position it was given.

use std::time::{Duration, Instant};

use voxel_space::WorldPosition;

/// How long without news of a position before the player is taken to be standing still: a little more
/// than a tick at the game's rate.
const NO_NEWS_MEANS_STILL: Duration = Duration::from_millis(90);

/// The longest the view is carried on from the last position: whatever the speed, never far ahead of
/// what the server last said.
const LONGEST_CARRY_ON: Duration = Duration::from_millis(120);

/// How a view follows a position that is told to it now and then.
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseFollower {
    /// The last position told, and when it was first seen.
    last: Option<(WorldPosition, Instant)>,
    /// The speed worked out from the last two positions, in voxels a second.
    velocity: [f32; 3],
}

impl PoseFollower {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets everything: the next position told is shown as it is (the player was moved).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Where the view should be at `now`, given that the server last said the player is at `told`.
    pub fn view_position(&mut self, told: WorldPosition, now: Instant) -> WorldPosition {
        match self.last {
            Some((last, _)) if last == told => {}
            Some((last, since)) => {
                let seconds = now.duration_since(since).as_secs_f32().max(0.005);
                let moved = told.relative_to(last);
                // A jump a long way in one go (a teleport) is not a speed.
                let far = moved.iter().any(|component| component.abs() > 8.0);
                self.velocity = if far || seconds > 0.5 {
                    [0.0; 3]
                } else {
                    moved.map(|component| component / seconds)
                };
                self.last = Some((told, now));
            }
            None => {
                self.velocity = [0.0; 3];
                self.last = Some((told, now));
            }
        }
        let (_, since) = self.last.expect("a position was just recorded");
        let waited = now.duration_since(since);
        if waited > NO_NEWS_MEANS_STILL {
            self.velocity = [0.0; 3];
            return told;
        }
        let seconds = waited.min(LONGEST_CARRY_ON).as_secs_f32();
        told.translated(self.velocity.map(|speed| speed * seconds))
    }
}

/// How fast a view turns toward the angles the server gives it, as a share of what is left to turn
/// each second (a turn is mostly done in about a tenth of a second).
const TURN_RATE: f32 = 14.0;

/// An angle (radians) moved toward `target` by the way of the shorter turn, by an amount that
/// depends on `seconds`. Never goes past the target, and a target far round the other way is reached
/// by the short way: from just under a half turn to just over it is a small step. Uses only
/// arithmetic, so it is the same on every machine.
pub fn turned_toward(current: f32, target: f32, seconds: f32) -> f32 {
    let whole = std::f32::consts::TAU;
    let difference =
        (target - current + std::f32::consts::PI).rem_euclid(whole) - std::f32::consts::PI;
    current + difference * (seconds * TURN_RATE).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    fn at(x: f32) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::new(0, 0, 0), [x, 10.0, 5.0])
    }

    fn x_of(position: WorldPosition) -> f32 {
        let brick = position.brick();
        brick.x() as f32 * 32.0 + position.offset()[0]
    }

    const TICK: Duration = Duration::from_millis(50);

    #[test]
    fn a_view_turns_toward_its_angle_the_short_way_and_never_past_it() {
        use std::f32::consts::{FRAC_PI_2, PI};
        // Part of the way after a short time, all of it after a long one.
        let part = turned_toward(0.0, FRAC_PI_2, 0.016);
        assert!(0.0 < part && part < FRAC_PI_2 * 0.5, "{part}");
        assert!((turned_toward(0.0, FRAC_PI_2, 1.0) - FRAC_PI_2).abs() < 1e-5);
        assert_eq!(turned_toward(1.0, 1.0, 0.016), 1.0);
        // From 170 degrees to -170 degrees is 20 degrees by the short way, and it goes up past 180.
        let (from, to) = (170f32.to_radians(), (-170f32).to_radians());
        let step = turned_toward(from, to, 1.0);
        assert!((step - (to + 2.0 * PI)).abs() < 1e-4, "{step}");
        // No time, no turn.
        assert_eq!(turned_toward(0.3, 2.0, 0.0), 0.3);
    }

    #[test]
    fn the_first_position_is_shown_as_it_is() {
        let mut follower = PoseFollower::new();
        let now = Instant::now();
        assert_eq!(follower.view_position(at(3.0), now), at(3.0));
    }

    #[test]
    fn steady_movement_is_carried_on_between_positions_so_it_looks_smooth() {
        let mut follower = PoseFollower::new();
        let start = Instant::now();
        // Moving at 5 voxels a second, told every 50 ms.
        let mut shown = Vec::new();
        for tick in 0..6u32 {
            let told = at(10.0 + 0.25 * tick as f32);
            let arrived = start + TICK * tick;
            // Three frames between each pair of positions told.
            for frame in 0..3u32 {
                let now = arrived + Duration::from_millis(u64::from(frame) * 16);
                shown.push((x_of(follower.view_position(told, now)), now));
            }
        }
        // After the first two positions the speed is known, and the view is ahead of the last
        // position told by speed times the time since: it keeps up with a player moving at 5.
        for (x, now) in shown.iter().skip(6) {
            let seconds = now.duration_since(start).as_secs_f32();
            let truth = 10.0 + 5.0 * seconds;
            assert!((x - truth).abs() < 0.1, "{x} against {truth}");
        }
    }

    #[test]
    fn a_player_who_stops_is_shown_where_the_server_last_said_and_not_carried_past_it() {
        let mut follower = PoseFollower::new();
        let start = Instant::now();
        follower.view_position(at(10.0), start);
        follower.view_position(at(10.25), start + TICK);
        // No news for three ticks: standing still.
        let shown = follower.view_position(at(10.25), start + TICK * 4);
        assert_eq!(shown, at(10.25));
        // And it does not start again by itself.
        let later = follower.view_position(at(10.25), start + TICK * 5);
        assert_eq!(later, at(10.25));
    }

    #[test]
    fn the_view_is_never_carried_far_past_the_last_position_told() {
        let mut follower = PoseFollower::new();
        let start = Instant::now();
        follower.view_position(at(0.0), start);
        // 20 voxels a second.
        follower.view_position(at(1.0), start + TICK);
        let shown = x_of(follower.view_position(at(1.0), start + TICK + Duration::from_millis(80)));
        // At most the longest carrying on, at the speed worked out.
        assert!(shown - 1.0 <= 20.0 * 0.121, "{shown}");
        assert!(shown >= 1.0);
    }

    #[test]
    fn a_teleport_is_shown_at_once_and_does_not_make_the_view_rush_ahead() {
        let mut follower = PoseFollower::new();
        let start = Instant::now();
        follower.view_position(at(0.0), start);
        let shown = follower.view_position(at(500.0), start + TICK);
        assert_eq!(shown, at(500.0));
        let next = follower.view_position(at(500.0), start + TICK * 2);
        assert_eq!(next, at(500.0));
        // Resetting forgets the speed too.
        follower.view_position(at(500.2), start + TICK * 3);
        follower.reset();
        assert_eq!(follower.view_position(at(7.0), start + TICK * 4), at(7.0));
    }
}
