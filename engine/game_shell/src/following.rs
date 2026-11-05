//! Showing the player where the server says they are, smoothly.
//!
//! A server that moves players tells the client where its player is a few times a second (once a
//! tick, and only when that changed). Shown as it comes, the view would jump that often. A
//! [`PoseFollower`] works out how fast the player is going from the last two positions it was told,
//! and carries the view on that way until the next one comes, so that steady movement looks steady.
//! When a position does not come for longer than a tick is expected to take, the player has stopped
//! (the server only says what changed), and the view is put at the last position it was given.
//!
//! The same is done for everything else that is seen to move and is told a tick at a time: the way a
//! ship faces ([`TurnFollower`]) and a whole ship ([`GridFollower`]). A thing that is carried on must
//! be carried on in the same way as what stands on it or looks at it, or the distance between them
//! shakes by a tick of travel: a camera carried on at a ship's speed, looking at a ship shown where it
//! was last told, shakes by the speed times 50 ms.

use std::time::{Duration, Instant};

use voxel_space::{GridPose, Orientation, WorldPosition};

/// How many ticks without news of a position, after the one that told it, before the player is taken to be standing still: a little more than a tick at the game's rate. It is
/// measured in the time of the ticks that have come (the newest tick seen, against the one that told the position), not in the time on the wall: a server that is late has told
/// nobody anything, and a thing that is not told of is not thereby still. (Judged by the wall, one tick that came 60 milliseconds late put a thing that went 3,000 voxels a second
/// back 270 voxels, and then threw it forward again.)
const NO_NEWS_MEANS_STILL: Duration = Duration::from_millis(90);

/// The longest the view is carried on from the last position: whatever the speed, never far ahead of
/// what the server last said. A tick that is later than this is a stop, in the picture, until it comes.
const LONGEST_CARRY_ON: Duration = Duration::from_millis(250);

/// How much the speed of something may change between two positions told, however fast it goes, before that is taken for a jump (a teleport, a
/// ship put somewhere) and not for travel: in voxels a second. For a turn it is [`TURN_ALLOWANCE`].
const SPEED_ALLOWANCE: f64 = 100.0;

/// How much of the faster of two speeds they may differ by besides, since the time between two positions is the time between two frames and not between two
/// ticks: a tick that is seen a frame late makes the speed worked out a third too high, and the next a third too low.
const SPEED_SHARE: f64 = 0.75;

/// The same allowance for a turn, in radians a second.
const TURN_ALLOWANCE: f64 = 2.0;

fn length(vector: [f64; 3]) -> f64 {
    libm::sqrt(vector.iter().map(|component| component * component).sum())
}

/// Whether a speed can follow another: no more different than the allowance and a share of the faster.
fn continues(before: [f64; 3], after: [f64; 3], allowance: f64) -> bool {
    let change = length([0, 1, 2].map(|axis| after[axis] - before[axis]));
    change <= allowance + SPEED_SHARE * length(before).max(length(after))
}

/// A speed worked out from the positions told, and whether to believe it. A jump a long way in one go (a teleport) is not a speed, and a fast ship that is
/// really going that fast is: the speed of the last two positions is believed if it is near the speed before, or near the speed worked out the time before
/// that (two jumps of the same size in a row are travel).
#[derive(Clone, Copy, Debug, Default)]
struct Believed {
    /// The speed the view is carried on at.
    speed: [f64; 3],
    /// The speed worked out last, believed or not.
    worked_out: [f64; 3],
}

impl Believed {
    fn learn(&mut self, worked_out: [f64; 3], allowance: f64) {
        let steady = continues(self.speed, worked_out, allowance)
            || continues(self.worked_out, worked_out, allowance);
        self.speed = if steady { worked_out } else { [0.0; 3] };
        self.worked_out = worked_out;
    }

    fn rest(&mut self) {
        *self = Self::default();
    }
}

/// How a view follows a position that is told to it now and then.
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseFollower {
    /// The last position told, and when it was first seen.
    last: Option<(WorldPosition, Instant)>,
    /// The speed worked out from the last two positions, in voxels a second.
    velocity: Believed,
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
        self.view_position_seen(told, now, now)
    }

    /// The same, when the time at which a position told for the first time is taken to have been told is `seen` and not `now` (see [`TickClock`]).
    pub fn view_position_seen(
        &mut self,
        told: WorldPosition,
        seen: Instant,
        now: Instant,
    ) -> WorldPosition {
        match self.last {
            Some((last, _)) if last == told => {}
            Some((last, since)) => {
                let seconds = seen.duration_since(since).as_secs_f64().max(0.005);
                let moved = told.relative_to(last);
                let worked_out = if seconds > 0.5 {
                    [0.0; 3]
                } else {
                    moved.map(|component| component / seconds)
                };
                self.velocity.learn(worked_out, SPEED_ALLOWANCE);
                self.last = Some((told, seen));
            }
            None => {
                self.velocity.rest();
                self.last = Some((told, seen));
            }
        }
        let (_, since) = self.last.expect("a position was just recorded");
        if seen.saturating_duration_since(since) > NO_NEWS_MEANS_STILL {
            self.velocity.rest();
            return told;
        }
        let seconds = now
            .saturating_duration_since(since)
            .min(LONGEST_CARRY_ON)
            .as_secs_f64();
        told.translated(self.velocity.speed.map(|speed| speed * seconds))
    }
}

/// How a view follows the way something faces, which is told to it now and then, as [`PoseFollower`] does a position: it is turned on at the rate the last two
/// turns were told at.
#[derive(Clone, Copy, Debug, Default)]
pub struct TurnFollower {
    /// The last turn told, and when it was first seen.
    last: Option<(Orientation, Instant)>,
    /// The rate worked out from the last two turns: the axis it turns about, scaled by radians a second.
    rate: Believed,
}

impl TurnFollower {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the view should face at `now`, given that the server last said the thing faces `told`.
    pub fn view_orientation(&mut self, told: Orientation, now: Instant) -> Orientation {
        self.view_orientation_seen(told, now, now)
    }

    /// The same, when the time at which a turn told for the first time is taken to have been told is `seen` and not `now` (see [`TickClock`]).
    pub fn view_orientation_seen(
        &mut self,
        told: Orientation,
        seen: Instant,
        now: Instant,
    ) -> Orientation {
        match self.last {
            Some((last, _)) if last == told => {}
            Some((last, since)) => {
                let seconds = seen.duration_since(since).as_secs_f64().max(0.005);
                let worked_out = if seconds > 0.5 {
                    [0.0; 3]
                } else {
                    turn_between(last, told).map(|component| component / seconds)
                };
                self.rate.learn(worked_out, TURN_ALLOWANCE);
                self.last = Some((told, seen));
            }
            None => {
                self.rate.rest();
                self.last = Some((told, seen));
            }
        }
        let (_, since) = self.last.expect("a turn was just recorded");
        if seen.saturating_duration_since(since) > NO_NEWS_MEANS_STILL {
            self.rate.rest();
            return told;
        }
        let seconds = now
            .saturating_duration_since(since)
            .min(LONGEST_CARRY_ON)
            .as_secs_f64();
        turned_by(told, self.rate.speed.map(|rate| rate * seconds))
    }
}

/// The turn that takes `from` to `to` (the short way), as the axis it is about scaled by the angle, in radians.
fn turn_between(from: Orientation, to: Orientation) -> [f64; 3] {
    let [mut x, mut y, mut z, mut w] = to.compose(from.inverse()).components();
    if w < 0.0 {
        [x, y, z, w] = [-x, -y, -z, -w];
    }
    let sine = length([x, y, z]);
    if sine < 1e-12 {
        return [0.0; 3];
    }
    let scale = 2.0 * libm::atan2(sine, w) / sine;
    [x * scale, y * scale, z * scale]
}

/// `orientation` turned further by a turn given as an axis scaled by the angle, in the world's axes.
fn turned_by(orientation: Orientation, turn: [f64; 3]) -> Orientation {
    let angle = length(turn);
    if angle < 1e-15 {
        return orientation;
    }
    let half = angle / 2.0;
    let scale = libm::sin(half) / angle;
    Orientation::from_components([
        turn[0] * scale,
        turn[1] * scale,
        turn[2] * scale,
        libm::cos(half),
    ])
    .map_or(orientation, |step| step.compose(orientation))
}

/// How a view follows a ship (or a planet), whose place and facing are told to it now and then.
#[derive(Clone, Copy, Debug, Default)]
pub struct GridFollower {
    position: PoseFollower,
    turn: TurnFollower,
}

impl GridFollower {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the ship should be shown at `now`, given that the server last said it stands at `told`.
    pub fn view_pose(&mut self, told: GridPose, now: Instant) -> GridPose {
        self.view_pose_seen(told, now, now)
    }

    /// The same, when a pose told for the first time is taken to have been told at `seen` (see [`TickClock`]).
    pub fn view_pose_seen(&mut self, told: GridPose, seen: Instant, now: Instant) -> GridPose {
        GridPose::new(
            self.position.view_position_seen(told.position, seen, now),
            self.turn.view_orientation_seen(told.orientation, seen, now),
        )
    }
}

/// How much the earliest a tick could have come is let drift later in a second of the picture's time, in seconds: the clock of the picture and the server's do not keep quite the
/// same time, and the network is slower for a while now and then. (A rate, not an amount for each time it is asked: the camera, the ships and the other players each ask in a
/// frame, and what they are told must be one time or a ship that goes with the camera at a thousand voxels a second shows it a few centimetres off.)
const CLOCK_DRIFT_ALLOWED: f64 = 0.005;

/// When the ticks of the server come, worked out from when the frames saw them. A tick is seen at the first frame after it came, which is up to a frame late and
/// not by the same time each tick; a speed worked out from those times shakes, and the view with it. Ticks come at a steady rate, so the time of one is the time of
/// the first plus its number times the length of a tick; the earliest any tick was seen after that is the nearest to when it came.
#[derive(Clone, Copy, Debug)]
pub struct TickClock {
    start: Instant,
    seconds_per_tick: f64,
    /// When tick 0 would have come, in seconds after `start`, as near as the ticks seen say.
    origin: Option<f64>,
    /// The newest tick asked about; a lower number is another count of ticks (another world), and the clock starts again.
    newest: u64,
    /// When the clock was last asked, in seconds after `start`.
    asked_at: f64,
}

impl TickClock {
    pub fn new(ticks_per_second: u32, start: Instant) -> Self {
        Self {
            start,
            seconds_per_tick: 1.0 / f64::from(ticks_per_second.max(1)),
            origin: None,
            newest: 0,
            asked_at: 0.0,
        }
    }

    /// When tick `number`, which is the newest the frame at `now` has seen, is taken to have come: never after `now`.
    pub fn seen(&mut self, number: u64, now: Instant) -> Instant {
        if number < self.newest {
            self.origin = None;
        }
        self.newest = number;
        let ticks = number as f64 * self.seconds_per_tick;
        let elapsed = now.saturating_duration_since(self.start).as_secs_f64();
        let candidate = elapsed - ticks;
        let origin = match self.origin {
            Some(origin) => {
                (origin + CLOCK_DRIFT_ALLOWED * (elapsed - self.asked_at).max(0.0)).min(candidate)
            }
            None => candidate,
        };
        self.origin = Some(origin);
        self.asked_at = elapsed;
        self.start + Duration::from_secs_f64((origin + ticks).max(0.0))
    }
}

/// How fast a view turns toward the angles the server gives it, as a share of what is left to turn
/// each second (a turn is mostly done in about a tenth of a second).
const TURN_RATE: f64 = 14.0;

/// An angle (radians) moved toward `target` by the way of the shorter turn, by an amount that
/// depends on `seconds`. Never goes past the target, and a target far round the other way is reached
/// by the short way: from just under a half turn to just over it is a small step. Uses only
/// arithmetic, so it is the same on every machine.
pub fn turned_toward(current: f64, target: f64, seconds: f64) -> f64 {
    let whole = std::f64::consts::TAU;
    let difference =
        (target - current + std::f64::consts::PI).rem_euclid(whole) - std::f64::consts::PI;
    current + difference * (seconds * TURN_RATE).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    fn at(x: f64) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::new(0, 0, 0), [x, 10.0, 5.0])
    }

    fn x_of(position: WorldPosition) -> f64 {
        let brick = position.brick();
        brick.x() as f64 * 32.0 + position.offset()[0]
    }

    const TICK: Duration = Duration::from_millis(50);

    #[test]
    fn a_view_turns_toward_its_angle_the_short_way_and_never_past_it() {
        use std::f64::consts::{FRAC_PI_2, PI};
        // Part of the way after a short time, all of it after a long one.
        let part = turned_toward(0.0, FRAC_PI_2, 0.016);
        assert!(0.0 < part && part < FRAC_PI_2 * 0.5, "{part}");
        assert!((turned_toward(0.0, FRAC_PI_2, 1.0) - FRAC_PI_2).abs() < 1e-5);
        assert_eq!(turned_toward(1.0, 1.0, 0.016), 1.0);
        // From 170 degrees to -170 degrees is 20 degrees by the short way, and it goes up past 180.
        let (from, to) = (170f64.to_radians(), (-170f64).to_radians());
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
            let told = at(10.0 + 0.25 * tick as f64);
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
            let seconds = now.duration_since(start).as_secs_f64();
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

    /// Where something that goes at `speed` voxels a second along x is at `seconds`, as a position.
    fn along(speed: f64, seconds: f64) -> WorldPosition {
        WorldPosition::from_voxels([speed * seconds, 10.0, 5.0])
    }

    /// The frames of a 60 Hz picture and the ticks of a 20 Hz server, as they meet: a tick is seen at the first frame at or after it was sent (the
    /// frames are not quite on the ticks: the server's clock is 0.7 milliseconds ahead). Calls `each` with the time of the frame and the time the tick it
    /// shows was sent at.
    fn frames_and_ticks(seconds: f64, mut each: impl FnMut(f64, f64)) {
        let frame = 1.0 / 60.0;
        let mut now = 0.0;
        while now < seconds {
            let sent = ((now - 0.0007) * 20.0).floor() / 20.0 + 0.0007;
            each(now, sent.max(0.0));
            now += frame;
        }
    }

    fn instant_at(start: Instant, seconds: f64) -> Instant {
        start + Duration::from_secs_f64(seconds)
    }

    #[test]
    fn a_camera_and_the_ship_it_looks_at_keep_their_distance_when_both_are_carried_on_and_not_when_only_the_camera_is()
     {
        // A ship at 3,000 voxels a second, and a camera 5 voxels from it that goes with it (a player aboard).
        let speed = 3000.0;
        let start = Instant::now();
        let mut camera = PoseFollower::new();
        let mut ship = GridFollower::new();
        let mut clock = TickClock::new(20, start);
        let (mut carried_worst, mut stepped_worst) = (0.0f64, 0.0f64);
        frames_and_ticks(2.0, |now, sent| {
            let told_ship = GridPose::new(along(speed, sent), Orientation::IDENTITY);
            let told_camera = along(speed, sent).translated([5.0, 0.0, 0.0]);
            let at = instant_at(start, now);
            let seen = clock.seen((sent * 20.0).round() as u64, at);
            let camera_shown = x_of(camera.view_position_seen(told_camera, seen, at));
            let ship_shown = x_of(ship.view_pose_seen(told_ship, seen, at).position);
            let ship_stepped = x_of(told_ship.position);
            if now > 0.5 {
                carried_worst = carried_worst.max((camera_shown - ship_shown - 5.0).abs());
                stepped_worst = stepped_worst.max((camera_shown - ship_stepped - 5.0).abs());
            }
        });
        // (Measured with the ship shown where it was last told: the distance shook by up to a tick of travel, 150 voxels here.)
        assert!(stepped_worst > 50.0, "{stepped_worst}");
        assert!(carried_worst < 1e-6, "{carried_worst}");
    }

    /// The largest and smallest step between one frame and the next, as a share of the step a thing that moves steadily takes, once it is under way.
    fn steps_of(mut shown_at: impl FnMut(f64, f64) -> f64, nominal_step: f64) -> (f64, f64) {
        let mut before = None;
        let (mut largest, mut smallest) = (0.0f64, f64::MAX);
        frames_and_ticks(2.0, |now, sent| {
            let shown = shown_at(now, sent);
            if let Some(before) = before.filter(|_| now > 0.5) {
                let share = (shown - before) / nominal_step;
                largest = largest.max(share);
                smallest = smallest.min(share);
            }
            before = Some(shown);
        });
        (largest, smallest)
    }

    #[test]
    fn a_thing_faster_than_eight_voxels_a_tick_moves_smoothly_and_not_a_tick_at_a_time() {
        let speed = 7000.0;
        let nominal = speed / 60.0;
        let start = Instant::now();
        let mut follower = PoseFollower::new();
        let mut clock = TickClock::new(20, start);
        let (largest, smallest) = steps_of(
            |now, sent| {
                let at = instant_at(start, now);
                let seen = clock.seen((sent * 20.0).round() as u64, at);
                x_of(follower.view_position_seen(along(speed, sent), seen, at))
            },
            nominal,
        );
        // Carried on, no frame stands still or leaps, and it never goes back (the ticks are seen a frame early or late; the clock puts them on the beat).
        assert!(smallest > 0.8 && largest < 1.2, "{smallest} {largest}");
        // As told it stands still for two frames and leaps three frames at once: that is what it was before.
        let (largest, smallest) = steps_of(|_, sent| x_of(along(speed, sent)), nominal);
        assert!(smallest < 0.01 && largest > 2.5, "{smallest} {largest}");
    }

    #[test]
    fn the_tick_clock_never_puts_a_tick_after_the_frame_that_saw_it_and_starts_again_when_the_numbers_do()
     {
        let start = Instant::now();
        let mut clock = TickClock::new(20, start);
        frames_and_ticks(1.0, |now, sent| {
            let at = instant_at(start, now);
            let seen = clock.seen((sent * 20.0).round() as u64, at);
            assert!(seen <= at);
            // (The beat is found once a tick has been seen a frame earlier than the first: it is never later than the frame that saw it, and not more than a frame earlier.)
            assert!(at.duration_since(seen) < Duration::from_millis(60));
        });
        // Another world, whose count of ticks begins again: the clock is right at once and not after a long time.
        let at = instant_at(start, 2.0);
        let seen = clock.seen(3, at);
        assert!(at.saturating_duration_since(seen) < Duration::from_micros(1));
    }

    /// A small deterministic noise in [0, 1), so that a test of timing that is not on a beat is the same every time.
    fn noise(state: &mut u64) -> f64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*state >> 33) as f64 / f64::from(1u32 << 31)
    }

    /// The frames of a picture at `frame_rate` (each up to a millisecond off its beat) and the ticks of a 20 Hz server (each up to `latency` seconds later than its beat, the
    /// way a thread that sleeps and a computer that is busy deliver them), as they meet: a frame sees the newest tick that has come. Calls `each` with the time of the frame and
    /// the number of that tick.
    fn frames_and_late_ticks(
        seconds: f64,
        frame_rate: f64,
        latency: f64,
        seed: u64,
        each: impl FnMut(f64, u64),
    ) {
        frames_and_ticks_with_a_hiccup(seconds, frame_rate, latency, seed, None, each);
    }

    /// The same, with one tick (the number and the time, in seconds, it is late by besides) that comes late, as a tick does when the server is busy or the machine is.
    fn frames_and_ticks_with_a_hiccup(
        seconds: f64,
        frame_rate: f64,
        latency: f64,
        seed: u64,
        hiccup: Option<(u64, f64)>,
        mut each: impl FnMut(f64, u64),
    ) {
        let mut state = seed;
        let mut arrivals: Vec<f64> = Vec::new();
        let mut tick = 0u64;
        let mut now = 0.0;
        while now < seconds {
            while {
                let late = hiccup
                    .filter(|(number, _)| *number == tick)
                    .map_or(0.0, |(_, extra)| extra);
                let arrival = tick as f64 * 0.05 + 0.0007 + latency * noise(&mut state) + late;
                // (A tick does not come before the one before it.)
                let arrival = arrival.max(arrivals.last().copied().unwrap_or(0.0));
                if arrival <= now {
                    arrivals.push(arrival);
                    tick += 1;
                    true
                } else {
                    false
                }
            } {}
            if let Some(newest) = arrivals.len().checked_sub(1) {
                each(now, newest as u64);
            }
            now += (1.0 + 0.001 * (noise(&mut state) - 0.5) * frame_rate) / frame_rate;
        }
    }

    #[test]
    fn the_tick_clock_gives_everyone_who_asks_in_the_same_frame_the_same_time() {
        let start = Instant::now();
        let mut clock = TickClock::new(20, start);
        frames_and_late_ticks(2.0, 144.0, 0.003, 7, |now, tick| {
            let at = instant_at(start, now);
            let first = clock.seen(tick, at);
            assert_eq!(clock.seen(tick, at), first, "at {now}, tick {tick}");
            assert_eq!(clock.seen(tick, at), first, "at {now}, tick {tick}");
        });
    }

    #[test]
    fn a_camera_and_its_ship_keep_their_distance_when_the_window_asks_the_clock_for_each_of_them_in_turn()
     {
        // As the window does in a frame: the camera, then the ships, then the other players each ask when the tick they show was told.
        for speed in [700.0, 3000.0, 7000.0] {
            for (frame_rate, latency) in [(60.0, 0.0), (144.0, 0.003), (144.0, 0.012)] {
                let start = Instant::now();
                let mut camera = PoseFollower::new();
                let mut ship = GridFollower::new();
                let mut clock = TickClock::new(20, start);
                let mut worst = 0.0f64;
                frames_and_late_ticks(3.0, frame_rate, latency, 11, |now, tick| {
                    let told = along(speed, tick as f64 * 0.05);
                    let told_ship = GridPose::new(told, Orientation::IDENTITY);
                    let told_camera = told.translated([5.0, 0.0, 0.0]);
                    let at = instant_at(start, now);
                    let seen = clock.seen(tick, at);
                    let camera_shown = x_of(camera.view_position_seen(told_camera, seen, at));
                    let seen = clock.seen(tick, at);
                    let ship_shown = x_of(ship.view_pose_seen(told_ship, seen, at).position);
                    let _others = clock.seen(tick, at);
                    if now > 0.5 {
                        worst = worst.max((camera_shown - ship_shown - 5.0).abs());
                    }
                });
                // (Measured before the clock gave the same time to all who ask in a frame: up to the speed times 50 microseconds, a few centimetres at 700 and 35 at 7,000.)
                assert!(
                    worst < 1e-6,
                    "at {speed} voxels a second, {frame_rate} frames a second, ticks up to {latency} s late: the camera is {worst} voxels off the distance"
                );
            }
        }
    }

    #[test]
    fn a_tick_that_comes_late_does_not_put_a_thing_that_goes_fast_back_where_it_was_nor_make_it_stumble()
     {
        // A thing at 3,000 voxels a second, shown in frames of a 144 Hz picture; the 30th tick comes 100 milliseconds late (the server was busy: the next ones are on the beat).
        for lateness in [0.03, 0.06, 0.1] {
            let speed = 3000.0;
            let nominal = speed / 144.0;
            let start = Instant::now();
            let mut follower = PoseFollower::new();
            let mut clock = TickClock::new(20, start);
            let mut before = None;
            let (mut largest, mut smallest) = (0.0f64, f64::MAX);
            frames_and_ticks_with_a_hiccup(
                3.0,
                144.0,
                0.0,
                5,
                Some((30, lateness)),
                |now, tick| {
                    let at = instant_at(start, now);
                    let seen = clock.seen(tick, at);
                    let shown = x_of(follower.view_position_seen(
                        along(speed, tick as f64 * 0.05),
                        seen,
                        at,
                    ));
                    if let Some(before) = before.filter(|_| now > 0.5) {
                        let share = (shown - before) / nominal;
                        largest = largest.max(share);
                        smallest = smallest.min(share);
                    }
                    before = Some(shown);
                },
            );
            assert!(
                smallest > 0.7 && largest < 1.3,
                "a tick {lateness} s late: a step is from {smallest} to {largest} of the steady one"
            );
        }
    }

    #[test]
    fn a_fast_thing_put_somewhere_far_is_shown_there_at_once_and_does_not_rush_ahead() {
        let speed = 7000.0;
        let start = Instant::now();
        let mut follower = PoseFollower::new();
        let mut now = 0.0;
        for tick in 0..10 {
            follower.view_position(along(speed, f64::from(tick) * 0.05), instant_at(start, now));
            now += 0.05;
        }
        // Put 100,000 voxels away, standing still.
        let elsewhere = along(speed, 0.45).translated([100_000.0, 0.0, 0.0]);
        assert_eq!(
            follower.view_position(elsewhere, instant_at(start, now)),
            elsewhere
        );
        now += 0.05;
        assert_eq!(
            follower.view_position(elsewhere, instant_at(start, now)),
            elsewhere
        );
    }

    fn turned_about_y(angle: f64) -> Orientation {
        Orientation::from_components([0.0, libm::sin(angle / 2.0), 0.0, libm::cos(angle / 2.0)])
            .unwrap()
    }

    #[test]
    fn a_turning_ship_is_carried_on_at_its_rate_and_one_that_stops_turning_is_shown_as_told() {
        let rate = 1.0;
        let start = Instant::now();
        let mut follower = TurnFollower::new();
        let mut clock = TickClock::new(20, start);
        let angle_of =
            |orientation: Orientation| length(turn_between(Orientation::IDENTITY, orientation));
        let (largest, smallest) = steps_of(
            |now, sent| {
                let at = instant_at(start, now);
                let seen = clock.seen((sent * 20.0).round() as u64, at);
                angle_of(follower.view_orientation_seen(turned_about_y(rate * sent), seen, at))
            },
            rate / 60.0,
        );
        assert!(smallest > 0.8 && largest < 1.2, "{smallest} {largest}");
        // Stopped: told the same turn for longer than a tick is expected to take, it is shown as told.
        let last = turned_about_y(rate * 2.0);
        let later = instant_at(start, 2.5);
        assert_eq!(follower.view_orientation(last, later), last);
    }

    #[test]
    fn a_ship_turned_half_way_round_in_one_tick_is_a_jump_and_not_a_spin() {
        let start = Instant::now();
        let mut follower = TurnFollower::new();
        follower.view_orientation(turned_about_y(0.0), start);
        follower.view_orientation(turned_about_y(0.05), start + TICK);
        let flipped = turned_about_y(3.0);
        assert_eq!(
            follower.view_orientation(flipped, start + TICK * 2),
            flipped
        );
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
