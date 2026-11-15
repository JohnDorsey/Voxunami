//! The contact ghost: what the contacts between worlds do, worked out apart from the worlds (step 4c-5-2 of `docs/decisions/joints-and-cars.md`; the study is sections 16 to 18 of
//! `docs/history/loop-probe.md`).
//!
//! A mechanism (a group of ships coupled by their joints) has a world of its own, the truth of what it does, and what is near it has worlds of its own too (the frame everything
//! else is in, other mechanisms' worlds): in none of them is anything of the others, so **what a mechanism does cannot change by a bit because something is near**. When bodies of
//! two worlds could touch, a ghost is made of them. It holds a copy of each of those bodies, and of the couplings of the mechanism, in *two* worlds that are stepped alike: in one
//! the bodies of different sides meet each other, and in the other they pass through each other. What the contacts did to a body is what the first has that the second has not, and
//! it is added to what the body did in its own world. When no two bodies of different sides touched in the tick there is nothing to add (the corrections are `None`: not a
//! difference of rounding, nothing), so a world that was not touched is what it was, to the bit.
//!
//! The form is the study's "ghost, difference" with the ground in the worlds of the bodies and not in the ghost (the groups' own worlds have it), and with the three rules that it
//! found for it to be right: the ghost steps as the mechanism's world does (the caller gives the number of steps), the mechanism's world is as converged as the ghost (the same
//! settings and passes: the ghost is made with the settings of the world it follows), and the second world takes the first's pieces as they were.
//!
//! The ghost does not know the worlds it copies from: the caller tells it where each body is at the start of a tick ([`ContactGhost::refresh`]), steps it
//! ([`ContactGhost::step`]), steps the real worlds, and gives each body the [`Correction`] the ghost has for it.

use std::collections::BTreeMap;

use voxel_space::BrickCoordinate;

use crate::bodies::{
    BodyError, BodyId, BodySettings, BodyState, BodyWorld, Correction, CouplingError, CouplingId,
};

/// Which of the worlds a body truly is in: bodies of one side are in the same world, and meet each other there as they do; the ghost works out what bodies of different
/// sides do to each other. At most [`ContactGhost::MOST_SIDES`] of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Side(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GhostError {
    #[error(transparent)]
    Body(#[from] BodyError),
    #[error(transparent)]
    Coupling(#[from] CouplingError),
    #[error("a ghost has at most {0} sides, and {1:?} is more")]
    TooManySides(u32, Side),
    #[error("the ghost has no body {0:?}")]
    NotInTheGhost(BodyId),
}

#[derive(Debug)]
/// A body of the ghost: which side it is on, and which state of the world it was copied from it holds.
struct Member {
    side: Side,
    version: u64,
}

/// A coupling of the ghost, between two of its bodies.
#[derive(Debug)]
struct GhostCoupling {
    id: CouplingId,
    first: BodyId,
    second: BodyId,
}

#[derive(Debug)]
pub struct ContactGhost {
    /// The bodies as they would be if their sides met: the world that decides how a tick is stepped.
    with: BodyWorld,
    /// The same bodies, passing through those of other sides, stepped as the first was.
    without: BodyWorld,
    members: BTreeMap<BodyId, Member>,
    couplings: Vec<GhostCoupling>,
    next_coupling: u64,
    /// Whether two bodies of different sides touched in the last step.
    touched: bool,
    /// Whether a tick in which nothing could touch is left unstepped.
    gated: bool,
    /// How many ticks the ghost was stepped in.
    steps_made: u64,
}

impl ContactGhost {
    /// How many sides a ghost has at most.
    pub const MOST_SIDES: u32 = 32;

    /// A ghost with nothing in it, for bodies in a frame whose origin is `origin`, solved with `settings` (those of the world of the mechanism that it follows).
    pub fn new(origin: BrickCoordinate, settings: BodySettings) -> Self {
        Self {
            with: BodyWorld::new(origin, settings),
            without: BodyWorld::new(origin, settings),
            members: BTreeMap::new(),
            couplings: Vec::new(),
            next_coupling: 0,
            touched: false,
            gated: true,
            steps_made: 0,
        }
    }

    /// Puts a copy of a body of `world` in the ghost, on `side`, in the place `state` says (in the ghost's own frame). The copy is of what the body is now: its colliders (shared
    /// with it, not copied voxel by voxel), mass, kind and solver dials. It never sleeps.
    pub fn add_body_from(
        &mut self,
        side: Side,
        world: &BodyWorld,
        id: BodyId,
        state: BodyState,
    ) -> Result<(), GhostError> {
        if side.0 >= Self::MOST_SIDES {
            return Err(GhostError::TooManySides(Self::MOST_SIDES, side));
        }
        let version = world.version_of(id).ok_or(BodyError::UnknownBody(id))?;
        world.copy_body_into(id, &mut self.with, state)?;
        world.copy_body_into(id, &mut self.without, state)?;
        self.with.set_side(id, side.0, false)?;
        self.without.set_side(id, side.0, true)?;
        self.members.insert(id, Member { side, version });
        Ok(())
    }

    /// Takes a body out of the ghost, and the couplings it was in.
    pub fn remove_body(&mut self, id: BodyId) -> Result<(), GhostError> {
        if self.members.remove(&id).is_none() {
            return Err(GhostError::NotInTheGhost(id));
        }
        self.couplings
            .retain(|coupling| coupling.first != id && coupling.second != id);
        self.with.remove_body(id)?;
        self.without.remove_body(id)?;
        Ok(())
    }

    /// Whether a body is in the ghost.
    pub fn contains(&self, id: BodyId) -> bool {
        self.members.contains_key(&id)
    }

    /// Copies a coupling of `world` into the ghost, between the bodies of it that are in the ghost.
    pub fn add_coupling_from(
        &mut self,
        world: &BodyWorld,
        coupling: CouplingId,
    ) -> Result<(), GhostError> {
        let (first, second, pivots, options) = world
            .coupling_parts(coupling)
            .ok_or(CouplingError::UnknownCoupling(coupling))?;
        for body in [first, second] {
            if !self.members.contains_key(&body) {
                return Err(GhostError::NotInTheGhost(body));
            }
        }
        let id = CouplingId(self.next_coupling);
        self.next_coupling += 1;
        self.with.couple(id, first, second, &pivots, options)?;
        self.without.couple(id, first, second, &pivots, options)?;
        self.couplings.push(GhostCoupling { id, first, second });
        Ok(())
    }

    /// Lets go of every coupling that holds these two bodies together.
    pub fn remove_couplings_between(
        &mut self,
        first: BodyId,
        second: BodyId,
    ) -> Result<(), GhostError> {
        let mut kept = Vec::new();
        for coupling in std::mem::take(&mut self.couplings) {
            let between = (coupling.first == first && coupling.second == second)
                || (coupling.first == second && coupling.second == first);
            if between {
                self.with.decouple(coupling.id)?;
                self.without.decouple(coupling.id)?;
            } else {
                kept.push(coupling);
            }
        }
        self.couplings = kept;
        Ok(())
    }

    /// Tells the ghost where a body is now (`state`, in the ghost's frame), with what is applied to it for the tick, from the world it truly is in: and if something about the
    /// body changed that the copy must follow (an edit of its bricks, its mass, its kind) the copy is made again.
    pub fn refresh(
        &mut self,
        id: BodyId,
        world: &BodyWorld,
        state: BodyState,
    ) -> Result<(), GhostError> {
        let member = self
            .members
            .get_mut(&id)
            .ok_or(GhostError::NotInTheGhost(id))?;
        let version = world.version_of(id).ok_or(BodyError::UnknownBody(id))?;
        if version != member.version {
            member.version = version;
            let side = member.side;
            world.recopy_body_into(id, &mut self.with)?;
            world.recopy_body_into(id, &mut self.without)?;
            self.with.set_side(id, side.0, false)?;
            self.without.set_side(id, side.0, true)?;
        }
        let (force, torque) = world.pending_forces(id).ok_or(BodyError::UnknownBody(id))?;
        for copy in [&mut self.with, &mut self.without] {
            copy.set_state(id, state)?;
            copy.set_pending_forces(id, force, torque)?;
        }
        Ok(())
    }

    /// Steps both worlds of the ghost one tick, cut into `steps` steps: the world in which the sides meet decides how (where a step is cut at a touch, which pieces are hits), and
    /// the other takes the same pieces. `steps` is what the world that the mechanism is in steps by (`BodyWorld::steps_this_tick`). **A tick in which no two bodies of different
    /// sides could touch is not stepped at all** (see [`ContactGhost::set_gated`]): its result is that no sides touched, and the copies that would have been stepped are put
    /// again where the worlds have them at the start of the next tick. Returns whether the ghost was stepped.
    pub fn step(&mut self, steps: u32) -> bool {
        self.touched = false;
        if self.gated && !self.with.could_touch_across_sides() {
            return false;
        }
        let plan = self.with.step_recording(steps);
        self.without.step_replaying(&plan);
        self.touched = plan.crossed();
        self.steps_made += 1;
        true
    }

    /// Whether the ghost is stepped only in a tick in which bodies of different sides could touch (the default), or in every tick. Not gated, a ghost of a mechanism that sleeps in
    /// its own world costs twice what the mechanism does awake, all the time (`docs/history/loop-probe.md`, section 24); it is for tests and for measuring.
    pub fn set_gated(&mut self, gated: bool) {
        self.gated = gated;
    }

    /// How many ticks the ghost has been stepped in.
    pub fn steps_made(&self) -> u64 {
        self.steps_made
    }

    /// Whether bodies of different sides touched in the last step.
    pub fn touched(&self) -> bool {
        self.touched
    }

    /// What the contacts did to a body in the last step, to be added to what it did in its own world: `None` if no two bodies of different sides touched, or if they did and this
    /// body was not moved by it, to the bit.
    pub fn correction(&self, id: BodyId) -> Option<Correction> {
        if !self.touched || !self.members.contains_key(&id) {
            return None;
        }
        self.with.correction_against(&self.without, id)
    }
}
