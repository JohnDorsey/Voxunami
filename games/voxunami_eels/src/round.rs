//! The rules of a match of eels, on an arena it is given, with no idea of servers or clients.
//!
//! Up to [`MAXIMUM_EELS`] eels race through the arena. Each swims one cell along its heading every
//! [`STEP_TICKS`] ticks and leaves its body behind as a wall of its colour. An eel that touches a
//! wall, anyone's body, or another eel's new cell dies, and its body stays. The last eel swimming
//! wins the round and a point; a lone player simply plays until they die. A new round starts a few
//! seconds after each one ends, with the arena cleared.
//!
//! A step works from the arena as it stood: every eel decides where it is going, then every eel
//! that is still alive moves, so the order eels are looked at in changes nothing. Two eels that
//! want the same cell both die. An eel is not hurt by the body it left in its previous step: that
//! is what lets it turn sideways into the cell its own wing was in. And its wings fold in on a step
//! that turns, so the eel is only its head cell then: a ribbon swung round would sweep back across
//! the body it has just laid, and an eel could never turn.

use std::collections::{BTreeMap, VecDeque};

use voxel_space::FaceDirection;

use crate::arena::{ARENA_EDGE, Cell, is_inside};
use crate::eel::{Orientation, Steer};

/// The most eels in a match, one for each colour.
pub const MAXIMUM_EELS: usize = 8;

/// The ticks a second the game's server runs at.
pub const TICKS_PER_SECOND: u32 = 30;

/// The ticks between one step of the eels and the next: 10 cells a second.
pub const STEP_TICKS: u32 = 3;

/// How long before a round starts, and how long a finished round is shown, in ticks.
pub const COUNTDOWN_TICKS: u32 = 3 * TICKS_PER_SECOND;
pub const OVER_TICKS: u32 = 3 * TICKS_PER_SECOND;

/// The steers an eel remembers: a player who taps faster than the eel steps does not lose turns,
/// but cannot stack up a long queue either.
const STEER_QUEUE: usize = 3;

/// What a cell of the arena is painted as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    Clear,
    /// The body an eel leaves behind, in the colour of the eel with this slot.
    Trail(u8),
    /// The part of an eel that swims, in its colour.
    Head(u8),
}

/// The arena as the rules see it.
pub trait Arena {
    /// Whether a cell is solid: a wall, or the body of an eel. Cells outside the arena are.
    fn is_occupied(&mut self, cell: Cell) -> bool;

    fn paint(&mut self, cell: Cell, paint: Paint);

    /// Clears every body from the inside of the arena.
    fn clear_inside(&mut self);
}

/// Where a match is in its rounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nobody is playing.
    Idle,
    /// The eels are in place, and set off when the ticks run out.
    Countdown {
        ticks_left: u32,
    },
    Running,
    /// A round just ended; `winner` is the slot of the eel that won it, if any did.
    Over {
        ticks_left: u32,
        winner: Option<u8>,
    },
}

/// Where each slot's eel starts and which way it heads. The first four meet in the middle if
/// nobody turns, the others do the same a level above and below.
pub fn start_of(slot: u8) -> (Cell, FaceDirection) {
    const MIDDLE: i128 = ARENA_EDGE / 2;
    const NEAR: i128 = 8;
    const FAR: i128 = ARENA_EDGE - 1 - NEAR;
    const LOW: i128 = ARENA_EDGE / 5;
    const HIGH: i128 = ARENA_EDGE - 1 - LOW;
    match slot % MAXIMUM_EELS as u8 {
        0 => ([MIDDLE, MIDDLE, NEAR], FaceDirection::PositiveZ),
        1 => ([FAR, MIDDLE, MIDDLE], FaceDirection::NegativeX),
        2 => ([MIDDLE, MIDDLE, FAR], FaceDirection::NegativeZ),
        3 => ([NEAR, MIDDLE, MIDDLE], FaceDirection::PositiveX),
        4 => ([MIDDLE, LOW, NEAR], FaceDirection::PositiveZ),
        5 => ([FAR, LOW, MIDDLE], FaceDirection::NegativeX),
        6 => ([MIDDLE, HIGH, FAR], FaceDirection::NegativeZ),
        _ => ([NEAR, HIGH, MIDDLE], FaceDirection::PositiveX),
    }
}

/// One eel in a round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Eel {
    slot: u8,
    head: Cell,
    orientation: Orientation,
    alive: bool,
    pending: VecDeque<Steer>,
    /// The cells it occupies now, which are painted as its head: its whole ribbon, or only the head
    /// cell on a step that turned (see [`Match`]).
    ribbon: Vec<Cell>,
}

impl Eel {
    pub fn slot(&self) -> u8 {
        self.slot
    }

    /// The cell at the middle of the ribbon.
    pub fn head(&self) -> Cell {
        self.head
    }

    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    pub fn ribbon(&self) -> &[Cell] {
        &self.ribbon
    }
}

/// Where a match stands, for telling the players.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Standing {
    pub slot: u8,
    /// Whether the player's eel is swimming.
    pub alive: bool,
    /// Rounds won.
    pub wins: u32,
}

/// The rounds of eels, played one after another for as long as players are there.
#[derive(Debug)]
pub struct Match {
    phase: Phase,
    present: [bool; MAXIMUM_EELS],
    eels: [Option<Eel>; MAXIMUM_EELS],
    wins: [u32; MAXIMUM_EELS],
    /// Ticks since the eels last stepped.
    ticks_into_step: u32,
}

impl Default for Match {
    fn default() -> Self {
        Self::new()
    }
}

impl Match {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            present: [false; MAXIMUM_EELS],
            eels: std::array::from_fn(|_| None),
            wins: [0; MAXIMUM_EELS],
            ticks_into_step: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_present(&self, slot: u8) -> bool {
        self.present
            .get(usize::from(slot))
            .copied()
            .unwrap_or(false)
    }

    /// The eel of a slot, in this round: it is there from the start of the round to the start of
    /// the next, dead or alive, if the player was there when the round began.
    pub fn eel(&self, slot: u8) -> Option<&Eel> {
        self.eels.get(usize::from(slot))?.as_ref()
    }

    pub fn wins(&self, slot: u8) -> u32 {
        self.wins.get(usize::from(slot)).copied().unwrap_or(0)
    }

    pub fn alive_count(&self) -> usize {
        self.eels.iter().flatten().filter(|eel| eel.alive).count()
    }

    fn participants(&self) -> usize {
        self.eels.iter().flatten().count()
    }

    /// The standing of every player who is here.
    pub fn standings(&self) -> Vec<Standing> {
        (0..MAXIMUM_EELS as u8)
            .filter(|&slot| self.is_present(slot))
            .map(|slot| Standing {
                slot,
                alive: self.eel(slot).is_some_and(Eel::is_alive),
                wins: self.wins(slot),
            })
            .collect()
    }

    /// How far a live eel is between its last step and its next, from 0 to just under 1, for
    /// showing it moving smoothly.
    pub fn fraction_of_step(&self) -> f32 {
        if self.phase == Phase::Running {
            self.ticks_into_step as f32 / STEP_TICKS as f32
        } else {
            0.0
        }
    }

    /// A player takes a slot. They swim from the next round.
    pub fn join(&mut self, slot: u8) {
        if let Some(present) = self.present.get_mut(usize::from(slot)) {
            *present = true;
        }
    }

    /// A player leaves. Their eel, if it is swimming, dies where it is.
    pub fn leave(&mut self, slot: u8, arena: &mut impl Arena) {
        if let Some(present) = self.present.get_mut(usize::from(slot)) {
            *present = false;
        }
        self.wins[usize::from(slot) % MAXIMUM_EELS] = 0;
        let before_the_start = matches!(self.phase, Phase::Countdown { .. });
        if let Some(entry) = self.eels.get_mut(usize::from(slot)) {
            match entry {
                // Before the round starts the eel has not been anywhere: it is just taken away.
                Some(eel) if before_the_start => {
                    for &cell in &eel.ribbon {
                        arena.paint(cell, Paint::Clear);
                    }
                    *entry = None;
                }
                Some(eel) => Self::kill(eel, arena),
                None => {}
            }
        }
    }

    /// Asks a slot's eel to steer at its next step, if it is swimming (or about to).
    pub fn steer(&mut self, slot: u8, steer: Steer) {
        if !matches!(self.phase, Phase::Countdown { .. } | Phase::Running) {
            return;
        }
        if let Some(Some(eel)) = self.eels.get_mut(usize::from(slot))
            && eel.alive
            && eel.pending.len() < STEER_QUEUE
        {
            eel.pending.push_back(steer);
        }
    }

    /// Advances the match by one tick.
    pub fn tick(&mut self, arena: &mut impl Arena) {
        match self.phase {
            Phase::Idle => {
                if self.present.iter().any(|&present| present) {
                    self.start_round(arena);
                }
            }
            Phase::Countdown { ticks_left } => {
                if !self.present.iter().any(|&present| present) {
                    self.phase = Phase::Idle;
                } else if ticks_left == 0 {
                    self.phase = Phase::Running;
                    self.ticks_into_step = 0;
                } else {
                    // Players who came in during the countdown are in the round.
                    self.spawn_present(arena);
                    self.phase = Phase::Countdown {
                        ticks_left: ticks_left - 1,
                    };
                }
            }
            Phase::Running => {
                self.ticks_into_step += 1;
                if self.ticks_into_step == STEP_TICKS {
                    self.ticks_into_step = 0;
                    self.step(arena);
                }
                self.finish_if_over();
            }
            Phase::Over { ticks_left, winner } => {
                if ticks_left > 0 {
                    self.phase = Phase::Over {
                        ticks_left: ticks_left - 1,
                        winner,
                    };
                } else if self.present.iter().any(|&present| present) {
                    self.start_round(arena);
                } else {
                    self.phase = Phase::Idle;
                }
            }
        }
    }

    fn start_round(&mut self, arena: &mut impl Arena) {
        arena.clear_inside();
        self.eels = std::array::from_fn(|_| None);
        self.spawn_present(arena);
        self.phase = Phase::Countdown {
            ticks_left: COUNTDOWN_TICKS,
        };
    }

    /// Puts an eel at the start of every present slot that has none.
    fn spawn_present(&mut self, arena: &mut impl Arena) {
        for slot in 0..MAXIMUM_EELS as u8 {
            if !self.is_present(slot) || self.eels[usize::from(slot)].is_some() {
                continue;
            }
            let (head, heading) = start_of(slot);
            let orientation = Orientation::new(heading);
            let ribbon = orientation.ribbon(head).to_vec();
            for &cell in &ribbon {
                arena.paint(cell, Paint::Head(slot));
            }
            self.eels[usize::from(slot)] = Some(Eel {
                slot,
                head,
                orientation,
                alive: true,
                pending: VecDeque::new(),
                ribbon,
            });
        }
    }

    fn kill(eel: &mut Eel, arena: &mut impl Arena) {
        if eel.alive {
            eel.alive = false;
            // What was swimming is a wall now, like the rest of its body.
            for &cell in &eel.ribbon {
                arena.paint(cell, Paint::Trail(eel.slot));
            }
        }
    }

    /// One step of every swimming eel.
    fn step(&mut self, arena: &mut impl Arena) {
        // Every eel decides where it goes.
        struct Plan {
            slot: usize,
            orientation: Orientation,
            head: Cell,
            ribbon: Vec<Cell>,
        }
        let mut plans = Vec::new();
        for (slot, eel) in self.eels.iter_mut().enumerate() {
            let Some(eel) = eel.as_mut().filter(|eel| eel.alive) else {
                continue;
            };
            let orientation = match eel.pending.pop_front() {
                Some(steer) => eel.orientation.steered(steer),
                None => eel.orientation,
            };
            let head = orientation.ahead(eel.head);
            // The wings fold in on a step that turns: swinging a ribbon round would sweep it back
            // across the body the eel has just laid.
            let ribbon = if orientation.heading() == eel.orientation.heading() {
                orientation.ribbon(head).to_vec()
            } else {
                vec![head]
            };
            plans.push(Plan {
                slot,
                orientation,
                head,
                ribbon,
            });
        }
        // Which cells more than one eel is going into.
        let mut claims: BTreeMap<Cell, usize> = BTreeMap::new();
        for plan in &plans {
            for &cell in &plan.ribbon {
                *claims.entry(cell).or_default() += 1;
            }
        }
        // Who dies, judged from the arena as it stands, before anything moves.
        let mut dies = Vec::with_capacity(plans.len());
        for plan in &plans {
            let own = &self.eels[plan.slot]
                .as_ref()
                .expect("a plan is for an eel")
                .ribbon;
            let hit = plan.ribbon.iter().any(|&cell| {
                !is_inside(cell)
                    || claims[&cell] > 1
                    || (!own.contains(&cell) && arena.is_occupied(cell))
            });
            dies.push(hit);
        }
        for (plan, dies) in plans.into_iter().zip(dies) {
            let eel = self.eels[plan.slot].as_mut().expect("a plan is for an eel");
            if dies {
                Self::kill(eel, arena);
                continue;
            }
            // The head that was is body now, and the eel is where it was going.
            for &cell in &eel.ribbon {
                arena.paint(cell, Paint::Trail(eel.slot));
            }
            for &cell in &plan.ribbon {
                arena.paint(cell, Paint::Head(eel.slot));
            }
            eel.orientation = plan.orientation;
            eel.head = plan.head;
            eel.ribbon = plan.ribbon;
        }
    }

    /// Ends the round if it is decided: with two or more eels, when one is left; with one, when it
    /// is dead.
    fn finish_if_over(&mut self) {
        let alive = self.alive_count();
        let decided = if self.participants() >= 2 {
            alive <= 1
        } else {
            alive == 0
        };
        if !decided {
            return;
        }
        let winner = (self.participants() >= 2 && alive == 1)
            .then(|| {
                self.eels
                    .iter()
                    .flatten()
                    .find(|eel| eel.alive)
                    .map(|eel| eel.slot)
            })
            .flatten();
        if let Some(slot) = winner {
            self.wins[usize::from(slot)] += 1;
        }
        self.phase = Phase::Over {
            ticks_left: OVER_TICKS,
            winner,
        };
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use proptest::prelude::*;

    use super::*;

    /// An arena that remembers what was painted, with a wall round it.
    #[derive(Debug, Default)]
    struct TestArena {
        painted: HashMap<Cell, Paint>,
        clearings: usize,
    }

    impl Arena for TestArena {
        fn is_occupied(&mut self, cell: Cell) -> bool {
            !is_inside(cell) || self.painted.contains_key(&cell)
        }

        fn paint(&mut self, cell: Cell, paint: Paint) {
            match paint {
                Paint::Clear => self.painted.remove(&cell),
                other => self.painted.insert(cell, other),
            };
        }

        fn clear_inside(&mut self) {
            self.painted.clear();
            self.clearings += 1;
        }
    }

    /// Ticks until the round runs.
    fn start(game: &mut Match, arena: &mut TestArena) {
        for _ in 0..=COUNTDOWN_TICKS + 1 {
            game.tick(arena);
        }
        assert_eq!(game.phase(), Phase::Running);
    }

    fn steps(game: &mut Match, arena: &mut TestArena, count: u32) {
        for _ in 0..count * STEP_TICKS {
            game.tick(arena);
        }
    }

    #[test]
    fn nothing_happens_until_someone_is_there_then_a_round_counts_down_and_starts() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        for _ in 0..50 {
            game.tick(&mut arena);
        }
        assert_eq!(game.phase(), Phase::Idle);
        assert!(arena.painted.is_empty());

        game.join(0);
        game.tick(&mut arena);
        assert_eq!(
            game.phase(),
            Phase::Countdown {
                ticks_left: COUNTDOWN_TICKS
            }
        );
        // The eel is in place, painted as a head, and it does not move while the countdown runs.
        let (head, _) = start_of(0);
        let eel = game.eel(0).unwrap().clone();
        assert_eq!(eel.head(), head);
        for &cell in eel.ribbon() {
            assert_eq!(arena.painted[&cell], Paint::Head(0));
        }
        for _ in 0..COUNTDOWN_TICKS {
            game.tick(&mut arena);
        }
        assert_eq!(game.eel(0).unwrap(), &eel);
        game.tick(&mut arena);
        assert_eq!(game.phase(), Phase::Running);
    }

    #[test]
    fn the_share_of_a_step_done_grows_with_the_ticks_while_running_and_is_nothing_otherwise() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        assert_eq!(game.fraction_of_step(), 0.0);
        game.join(0);
        game.tick(&mut arena);
        assert_eq!(game.fraction_of_step(), 0.0, "counting down");
        start(&mut game, &mut arena);
        // Running: the share goes up a third a tick and starts again at each step.
        let third = 1.0 / STEP_TICKS as f32;
        let mut share = game.fraction_of_step();
        for _ in 0..2 * STEP_TICKS {
            game.tick(&mut arena);
            let next = game.fraction_of_step();
            assert!((0.0..1.0).contains(&next));
            let expected = if share + third < 0.999 {
                share + third
            } else {
                0.0
            };
            assert!((next - expected).abs() < 1e-6, "{share} then {next}");
            share = next;
        }
    }

    #[test]
    fn an_eel_swims_a_cell_every_few_ticks_and_leaves_a_body_behind() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        start(&mut game, &mut arena);
        let (head, heading) = start_of(0);
        let first_ribbon = game.eel(0).unwrap().ribbon().to_vec();
        for tick in 1..STEP_TICKS {
            game.tick(&mut arena);
            assert_eq!(game.eel(0).unwrap().head(), head, "tick {tick}");
        }
        game.tick(&mut arena);
        let eel = game.eel(0).unwrap();
        let [dx, dy, dz] = heading.offset();
        assert_eq!(eel.head(), [head[0] + dx, head[1] + dy, head[2] + dz]);
        for cell in first_ribbon {
            assert_eq!(arena.painted[&cell], Paint::Trail(0));
        }
        for &cell in eel.ribbon() {
            assert_eq!(arena.painted[&cell], Paint::Head(0));
        }
        steps(&mut game, &mut arena, 4);
        assert_eq!(game.eel(0).unwrap().head(), [head[0], head[1], head[2] + 5]);
        // 2 ribbons of 3 painted per step, all different: the body is a solid sheet.
        assert_eq!(arena.painted.len(), 3 * 6);
    }

    #[test]
    fn a_lone_eel_that_reaches_the_wall_dies_leaving_its_body_and_the_round_ends_with_no_winner() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        start(&mut game, &mut arena);
        let (head, _) = start_of(0);
        let cells_to_the_wall = ARENA_EDGE - 1 - head[2];
        steps(&mut game, &mut arena, cells_to_the_wall as u32);
        assert!(game.eel(0).unwrap().is_alive());
        assert_eq!(game.eel(0).unwrap().head()[2], ARENA_EDGE - 1);
        steps(&mut game, &mut arena, 1);
        assert!(!game.eel(0).unwrap().is_alive());
        assert_eq!(
            game.phase(),
            Phase::Over {
                ticks_left: OVER_TICKS,
                winner: None
            }
        );
        assert_eq!(game.wins(0), 0);
        // Its body is still there, all of it, as a wall.
        assert_eq!(arena.painted.len(), 3 * (cells_to_the_wall as usize + 1));
        assert!(
            arena
                .painted
                .values()
                .all(|paint| *paint == Paint::Trail(0))
        );
    }

    #[test]
    fn eels_that_meet_head_on_both_die() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        game.join(2); // facing each other along the same line
        start(&mut game, &mut arena);
        // They are 47 cells apart and close 2 a step: they meet after 24 steps.
        steps(&mut game, &mut arena, 23);
        assert_eq!(game.alive_count(), 2);
        steps(&mut game, &mut arena, 1);
        assert_eq!(game.alive_count(), 0);
        assert!(matches!(game.phase(), Phase::Over { winner: None, .. }));
        assert_eq!((game.wins(0), game.wins(2)), (0, 0));
    }

    #[test]
    fn the_last_eel_swimming_wins_a_point_and_the_next_round_starts_clean() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        game.join(1);
        start(&mut game, &mut arena);
        // Eel 0 turns toward the near wall and reaches it; eel 1 swims on.
        game.steer(0, Steer::Left);
        game.steer(0, Steer::Left);
        steps(&mut game, &mut arena, 12);
        assert!(!game.eel(0).unwrap().is_alive());
        assert_eq!(game.wins(1), 1);
        assert!(matches!(
            game.phase(),
            Phase::Over {
                winner: Some(1),
                ..
            }
        ));
        // The winner is still where it was when the round ended, alive.
        assert!(game.eel(1).unwrap().is_alive());

        // Some seconds later a new round: the arena was cleared and both eels are back.
        let clearings = arena.clearings;
        for _ in 0..=OVER_TICKS {
            game.tick(&mut arena);
        }
        assert_eq!(arena.clearings, clearings + 1);
        assert!(matches!(game.phase(), Phase::Countdown { .. }));
        for slot in [0, 1] {
            let eel = game.eel(slot).unwrap();
            assert!(eel.is_alive());
            assert_eq!(eel.head(), start_of(slot).0);
        }
        assert_eq!(arena.painted.len(), 6, "only the two new eels are painted");
        assert_eq!(game.wins(1), 1, "the score stays");
    }

    #[test]
    fn an_eel_may_turn_into_its_own_wing_with_its_wings_folded_but_not_back_into_its_older_body() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        start(&mut game, &mut arena);
        steps(&mut game, &mut arena, 6);
        let head = game.eel(0).unwrap().head();
        assert_eq!(game.eel(0).unwrap().ribbon().len(), 3);
        // Heading +z, wide along x. Turning left heads along +x, into the cell its own wing was
        // in a step ago: allowed, and the eel is only its head cell for that step.
        game.steer(0, Steer::Left);
        steps(&mut game, &mut arena, 1);
        let eel = game.eel(0).unwrap();
        assert!(eel.is_alive());
        assert_eq!(eel.ribbon(), [[head[0] + 1, head[1], head[2]]]);
        // Straight on, the ribbon is whole again.
        steps(&mut game, &mut arena, 1);
        assert_eq!(game.eel(0).unwrap().ribbon().len(), 3);
        assert!(game.eel(0).unwrap().is_alive());

        // A second eel that turns twice in a row heads back along the body it laid: it dies.
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        start(&mut game, &mut arena);
        steps(&mut game, &mut arena, 6);
        game.steer(0, Steer::Left);
        game.steer(0, Steer::Left);
        steps(&mut game, &mut arena, 1);
        assert!(game.eel(0).unwrap().is_alive());
        steps(&mut game, &mut arena, 1);
        assert!(!game.eel(0).unwrap().is_alive());
    }

    #[test]
    fn rolling_changes_the_wall_an_eel_leaves() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        start(&mut game, &mut arena);
        game.steer(0, Steer::Roll);
        steps(&mut game, &mut arena, 1);
        let eel = game.eel(0).unwrap();
        let ribbon = eel.ribbon();
        // Wide along y now: the three cells differ in y only.
        assert!(
            ribbon
                .iter()
                .all(|cell| cell[0] == ribbon[0][0] && cell[2] == ribbon[0][2])
        );
        assert_eq!(ribbon[2][1] - ribbon[0][1], 2);
        assert!(eel.orientation().is_on_its_edge());
        // And it does not hurt: the eel swims on.
        steps(&mut game, &mut arena, 3);
        assert!(game.eel(0).unwrap().is_alive());
    }

    #[test]
    fn an_eel_steers_once_a_step_in_the_order_asked_and_remembers_only_a_few() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        start(&mut game, &mut arena);
        // Three steers are remembered, in order; the rest of the ten are lost.
        for steer in [Steer::Left, Steer::Up, Steer::Roll] {
            game.steer(0, steer);
        }
        for _ in 0..7 {
            game.steer(0, Steer::Right);
        }
        let heading = |game: &Match| game.eel(0).unwrap().orientation().heading();
        steps(&mut game, &mut arena, 1);
        assert_eq!(heading(&game), FaceDirection::PositiveX);
        steps(&mut game, &mut arena, 1);
        assert_eq!(heading(&game), FaceDirection::PositiveY);
        assert!(!game.eel(0).unwrap().orientation().is_on_its_edge());
        steps(&mut game, &mut arena, 1);
        assert!(game.eel(0).unwrap().orientation().is_on_its_edge());
        // That was all three that were remembered: no more steering.
        steps(&mut game, &mut arena, 2);
        assert_eq!(heading(&game), FaceDirection::PositiveY);
        assert!(game.eel(0).unwrap().is_alive());
    }

    #[test]
    fn a_player_who_leaves_mid_round_dies_where_they_are_and_one_who_joins_waits_for_the_next() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        game.join(1);
        start(&mut game, &mut arena);
        steps(&mut game, &mut arena, 3);
        game.join(2);
        assert!(game.eel(2).is_none(), "no eel until the next round");
        game.leave(0, &mut arena);
        assert!(!game.eel(0).unwrap().is_alive());
        assert!(!game.is_present(0));
        for &cell in game.eel(0).unwrap().ribbon() {
            assert_eq!(arena.painted[&cell], Paint::Trail(0));
        }
        game.tick(&mut arena);
        // The one who is left wins; the newcomer was not part of it.
        assert_eq!(game.wins(1), 1);
        for _ in 0..=OVER_TICKS {
            game.tick(&mut arena);
        }
        assert!(game.eel(2).is_some());
        assert!(game.eel(0).is_none());
        assert_eq!(
            game.standings().iter().map(|s| s.slot).collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn players_who_come_or_go_during_the_countdown_are_in_the_round_or_never_were() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(0);
        game.tick(&mut arena);
        game.join(1);
        game.join(5);
        game.tick(&mut arena);
        // The newcomers have eels at their starts, painted, from the next tick.
        for slot in [1, 5] {
            let eel = game.eel(slot).unwrap();
            assert_eq!(eel.head(), start_of(slot).0);
            for &cell in eel.ribbon() {
                assert_eq!(arena.painted[&cell], Paint::Head(slot));
            }
        }
        // One who leaves before the start leaves nothing behind.
        let leaver = game.eel(5).unwrap().ribbon().to_vec();
        game.leave(5, &mut arena);
        assert!(game.eel(5).is_none());
        assert!(leaver.iter().all(|cell| !arena.painted.contains_key(cell)));
        assert_eq!(arena.painted.len(), 6);
        start(&mut game, &mut arena);
        assert_eq!(game.alive_count(), 2);
    }

    #[test]
    fn when_everyone_leaves_the_match_goes_idle() {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        game.join(3);
        start(&mut game, &mut arena);
        game.leave(3, &mut arena);
        game.tick(&mut arena);
        assert!(matches!(game.phase(), Phase::Over { .. }));
        for _ in 0..=OVER_TICKS + 1 {
            game.tick(&mut arena);
        }
        assert_eq!(game.phase(), Phase::Idle);
    }

    #[test]
    fn every_slot_starts_inside_the_arena_apart_from_the_others_and_not_at_a_wall() {
        let mut ribbons = Vec::new();
        for slot in 0..MAXIMUM_EELS as u8 {
            let (head, heading) = start_of(slot);
            let ribbon = Orientation::new(heading).ribbon(head);
            for cell in ribbon {
                assert!(is_inside(cell));
                assert!(!ribbons.contains(&cell), "slot {slot}");
                ribbons.push(cell);
            }
            // At least a few steps of room straight ahead.
            let mut ahead = head;
            for _ in 0..5 {
                ahead = Orientation::new(heading).ahead(ahead);
                assert!(
                    Orientation::new(heading)
                        .ribbon(ahead)
                        .into_iter()
                        .all(is_inside)
                );
            }
        }
    }

    /// Plays a whole match from steers: at each step every eel is asked for `steers[slot][step]`.
    fn play(slots: usize, steers: &[Vec<u8>]) -> (Match, TestArena) {
        let (mut game, mut arena) = (Match::new(), TestArena::default());
        for slot in 0..slots as u8 {
            game.join(slot);
        }
        start(&mut game, &mut arena);
        let steps = steers.iter().map(Vec::len).max().unwrap_or(0);
        for step in 0..steps {
            for (slot, queue) in steers.iter().enumerate() {
                if let Some(steer) = queue.get(step).and_then(|&i| Steer::from_index(i % 7)) {
                    game.steer(slot as u8, steer);
                }
            }
            steps_one(&mut game, &mut arena);
            check_invariants(&game, &arena);
        }
        (game, arena)
    }

    fn steps_one(game: &mut Match, arena: &mut TestArena) {
        for _ in 0..STEP_TICKS {
            game.tick(arena);
        }
    }

    /// What must hold whatever the players do.
    fn check_invariants(game: &Match, arena: &TestArena) {
        let mut seen = HashMap::new();
        for slot in 0..MAXIMUM_EELS as u8 {
            let Some(eel) = game.eel(slot) else { continue };
            if eel.is_alive() {
                for &cell in eel.ribbon() {
                    assert!(is_inside(cell), "{cell:?}");
                    assert_eq!(arena.painted.get(&cell), Some(&Paint::Head(slot)));
                    assert!(
                        seen.insert(cell, slot).is_none(),
                        "{cell:?} is in two live eels"
                    );
                }
            }
        }
        // Everything painted is in the arena, and nothing is a head but a live eel's.
        for (cell, paint) in &arena.painted {
            assert!(is_inside(*cell), "{cell:?}");
            if let Paint::Head(slot) = paint {
                assert_eq!(seen.get(cell), Some(slot), "{cell:?} is a stray head");
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn whatever_the_players_do_live_eels_never_overlap_and_bodies_stay_in_the_arena(
            slots in 1usize..=8,
            steers in prop::collection::vec(prop::collection::vec(0u8..7, 0..120), 8),
        ) {
            let (first_game, first_arena) = play(slots, &steers);
            // And the same steers always give the same match, cell for cell.
            let (second_game, second_arena) = play(slots, &steers);
            prop_assert_eq!(first_game.standings(), second_game.standings());
            prop_assert_eq!(first_arena.painted, second_arena.painted);
        }
    }
}
