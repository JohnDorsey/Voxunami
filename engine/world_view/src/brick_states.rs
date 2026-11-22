//! What the mesher knows of each brick, in one place.
//!
//! A brick has a record: whether its voxels are loaded or being generated, what is known of its picture (not made, being made, made and waiting to be shown,
//! or made and given out), and whether a viewer asked for it to be shown and has a mesh on the screen. These used to be seven sets and maps of coordinates
//! that the mesher kept in step by hand, and a method that changed one and forgot another left a brick that was never made again. Now a change is a method
//! here, which changes the one record, and what is derived from the records (how many jobs are in flight, how many bricks are loaded) is worked out from the
//! change itself, so it cannot drift.
//!
//! The facts are not one enum because they are not exclusive: a brick that is shown keeps being shown while it is made again after an edit, and a brick that is
//! shown stays shown after its voxels are unloaded (hiding it is the viewer's to do); its voxels may be loaded while a generator is still making them.
//!
//! A brick that is taken down may have its picture **held** (`docs/specs/viewpoints-spec.md` 9f): the renderer keeps the mesh, hidden, the record keeps the picture's
//! size and what the sun is told of the brick, and showing it again is a move that makes nothing. What is held is charged to a budget, and the oldest go first.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use voxel_grid::Brick;
use voxel_meshing::{BrickOccupancy, PackedBrickMesh};
use voxel_space::BrickCoordinate;

use crate::cube_states::Taken;

/// What a picture that is held costs the budget at least, whatever its mesh takes: a brick with nothing to see is held without a mesh (so that it is not meshed again to
/// find that out), and a record takes about this much (a coordinate, the state, an entry in the order of the held).
pub(crate) const MINIMUM_HELD_COST: usize = 256;

/// What is known of the picture of a brick.
enum Picture {
    /// There is no up-to-date picture, and none is being made.
    Missing,
    /// A job is making it.
    Meshing,
    /// It is made and up to date, and not on the screen: held back until the viewer asks for it.
    Waiting(PackedBrickMesh),
    /// It is made and up to date, and the renderer has been told (or the mesher does not hold pictures back and told it as soon as it was made). `mesh_bytes` is what
    /// the mesh takes.
    Current { mesh_bytes: usize },
    /// It is made and up to date, and **kept** though the brick is not shown: the renderer holds the mesh, hidden, if the state says it is `displayed`, and showing the
    /// brick again is a move. `order` says how long ago it was taken down (a number that is never used twice and counts up: no clock), which is the order it is let go in.
    Held { mesh_bytes: usize, order: u64 },
}

/// What is held: the pictures of bricks that are not shown and are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeldBricks {
    /// How many bricks have a picture held.
    pub bricks: usize,
    /// Of those, how many the renderer holds a mesh of, hidden (the rest have nothing to see).
    pub with_mesh: usize,
    /// What the budget is charged for them: their meshes' bytes and what the sun is told of them, a brick with nothing to see being charged a little.
    pub bytes: usize,
}

/// The record of one brick.
pub(crate) struct BrickState {
    /// The voxels, once they are loaded.
    data: Option<Arc<Brick>>,
    /// A job is generating the voxels. (They may be loaded meanwhile, from what is known of the brick, and then the job's result is not wanted.)
    generating: bool,
    picture: Picture,
    /// Asked to be shown and not hidden since: only when the pictures are held back.
    shown: bool,
    /// The renderer holds a mesh of the brick (drawn if the brick is shown, hidden if its picture is held).
    displayed: bool,
    /// What the sun is told of the brick (which voxels block it), kept with a picture for as long as there is one, when pictures are held: it is told again when a held
    /// brick is shown, since it was taken back when the brick went out of range.
    occupancy: Option<BrickOccupancy>,
}

impl BrickState {
    fn new() -> Self {
        Self {
            data: None,
            generating: false,
            picture: Picture::Missing,
            shown: false,
            displayed: false,
            occupancy: None,
        }
    }

    /// A record that says nothing is dropped.
    fn is_idle(&self) -> bool {
        self.data.is_none()
            && !self.generating
            && matches!(self.picture, Picture::Missing)
            && !self.shown
            && !self.displayed
    }

    pub(crate) fn data(&self) -> Option<&Arc<Brick>> {
        self.data.as_ref()
    }

    pub(crate) fn is_generating(&self) -> bool {
        self.generating
    }

    pub(crate) fn is_meshing(&self) -> bool {
        matches!(self.picture, Picture::Meshing)
    }

    /// A picture is made and up to date (whether it is waiting or has been given out).
    pub(crate) fn is_meshed(&self) -> bool {
        matches!(self.picture, Picture::Waiting(_) | Picture::Current { .. })
    }

    /// A picture is waiting to be shown.
    pub(crate) fn is_waiting(&self) -> bool {
        matches!(self.picture, Picture::Waiting(_))
    }

    pub(crate) fn is_shown(&self) -> bool {
        self.shown
    }

    /// The picture is kept though the brick is not shown.
    pub(crate) fn is_held(&self) -> bool {
        matches!(self.picture, Picture::Held { .. })
    }

    /// What the budget is charged for a picture that is held: the bytes of its mesh and of what the sun is told, and at least [`MINIMUM_HELD_COST`].
    fn held_cost(&self) -> usize {
        let mesh_bytes = match self.picture {
            Picture::Held { mesh_bytes, .. } | Picture::Current { mesh_bytes } => mesh_bytes,
            _ => 0,
        };
        (mesh_bytes + self.occupancy.as_ref().map_or(0, BrickOccupancy::byte_size))
            .max(MINIMUM_HELD_COST)
    }

    /// The number a held picture was given when it was taken down.
    fn held_order(&self) -> Option<u64> {
        match self.picture {
            Picture::Held { order, .. } => Some(order),
            _ => None,
        }
    }

    /// There is neither a picture nor a job making one: the brick is to be meshed if it can be.
    pub(crate) fn needs_meshing(&self) -> bool {
        matches!(self.picture, Picture::Missing)
    }
}

/// What is counted over all the records, kept up to date by the change of each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    loaded: usize,
    generating: usize,
    meshing: usize,
    meshed: usize,
    held: usize,
    held_with_mesh: usize,
    held_bytes: usize,
}

impl Tally {
    fn of(state: &BrickState) -> Self {
        Self {
            loaded: usize::from(state.data.is_some()),
            generating: usize::from(state.generating),
            meshing: usize::from(state.is_meshing()),
            meshed: usize::from(state.is_meshed()),
            held: usize::from(state.is_held()),
            held_with_mesh: usize::from(state.is_held() && state.displayed),
            held_bytes: if state.is_held() {
                state.held_cost()
            } else {
                0
            },
        }
    }

    /// Takes away what a record counted before a change and adds what it counts after.
    fn replace(&mut self, before: Self, after: Self) {
        self.loaded = self.loaded + after.loaded - before.loaded;
        self.generating = self.generating + after.generating - before.generating;
        self.meshing = self.meshing + after.meshing - before.meshing;
        self.meshed = self.meshed + after.meshed - before.meshed;
        self.held = self.held + after.held - before.held;
        self.held_with_mesh = self.held_with_mesh + after.held_with_mesh - before.held_with_mesh;
        self.held_bytes = self.held_bytes + after.held_bytes - before.held_bytes;
    }

    fn add(&mut self, other: Self) {
        self.replace(Self::default(), other);
    }
}

/// The records of the bricks that are known of, by coordinate. A brick that nothing is known of has none.
#[derive(Default)]
pub(crate) struct BrickStates {
    records: HashMap<BrickCoordinate, BrickState>,
    tally: Tally,
    /// How many bytes of pictures of bricks that are not shown are kept, at most. 0, the default, keeps none.
    budget: usize,
    /// The bricks whose pictures are held, by how long ago they were taken down: the first is the oldest. Kept by [`BrickStates::change`], from the records.
    held_order: BTreeMap<u64, BrickCoordinate>,
    /// The number the next picture that is held is given.
    next_order: u64,
}

impl BrickStates {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn get(&self, coordinate: BrickCoordinate) -> Option<&BrickState> {
        self.records.get(&coordinate)
    }

    /// The voxels of a brick, if they are loaded.
    pub(crate) fn data(&self, coordinate: BrickCoordinate) -> Option<&Arc<Brick>> {
        self.get(coordinate).and_then(BrickState::data)
    }

    pub(crate) fn is_shown(&self, coordinate: BrickCoordinate) -> bool {
        self.get(coordinate).is_some_and(BrickState::is_shown)
    }

    /// Whether a picture of the brick is waiting to be shown.
    pub(crate) fn is_waiting(&self, coordinate: BrickCoordinate) -> bool {
        self.get(coordinate).is_some_and(BrickState::is_waiting)
    }

    /// Whether the picture of the brick is kept though it is not shown.
    pub(crate) fn is_held(&self, coordinate: BrickCoordinate) -> bool {
        self.get(coordinate).is_some_and(BrickState::is_held)
    }

    /// Whether the brick can be shown without making anything: a picture is waiting, or is held.
    pub(crate) fn is_ready(&self, coordinate: BrickCoordinate) -> bool {
        self.is_waiting(coordinate) || self.is_held(coordinate)
    }

    /// Whether pictures are kept when their bricks are taken down (there is a budget).
    pub(crate) fn holds_pictures(&self) -> bool {
        self.budget > 0
    }

    /// Sets how many bytes of pictures of bricks that are not shown are kept ([`BrickStates::evict_over_budget`] lets go of the ones over it). 0 keeps none.
    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
    }

    /// What is held now.
    pub(crate) fn held_pictures(&self) -> HeldBricks {
        HeldBricks {
            bricks: self.tally.held,
            with_mesh: self.tally.held_with_mesh,
            bytes: self.tally.held_bytes,
        }
    }

    /// What the sun is told of the brick, if it is kept (only with a picture, and only when pictures are held).
    pub(crate) fn occupancy(&self, coordinate: BrickCoordinate) -> Option<&BrickOccupancy> {
        self.get(coordinate)
            .and_then(|state| state.occupancy.as_ref())
    }

    /// How many bricks have their voxels loaded.
    pub(crate) fn loaded_count(&self) -> usize {
        self.tally.loaded
    }

    /// How many bricks have an up-to-date picture, waiting or given out.
    pub(crate) fn meshed_count(&self) -> usize {
        self.tally.meshed
    }

    /// Jobs in flight: voxels being generated and pictures being made.
    pub(crate) fn jobs_in_flight(&self) -> usize {
        self.tally.generating + self.tally.meshing
    }

    /// The bricks with their voxels loaded, in no order.
    pub(crate) fn loaded_coordinates(&self) -> impl Iterator<Item = BrickCoordinate> + '_ {
        self.records
            .iter()
            .filter(|(_, state)| state.data.is_some())
            .map(|(coordinate, _)| *coordinate)
    }

    /// The bricks that were asked to be shown and not hidden since, in no order.
    pub(crate) fn shown_coordinates(&self) -> impl Iterator<Item = BrickCoordinate> + '_ {
        self.records
            .iter()
            .filter(|(_, state)| state.shown)
            .map(|(coordinate, _)| *coordinate)
    }

    /// Changes the record of a brick, making one if there is none and dropping it if it says nothing afterwards, and keeps the tally.
    fn change<R>(
        &mut self,
        coordinate: BrickCoordinate,
        change: impl FnOnce(&mut BrickState) -> R,
    ) -> R {
        let state = self
            .records
            .entry(coordinate)
            .or_insert_with(BrickState::new);
        let before = Tally::of(state);
        let order_before = state.held_order();
        let result = change(state);
        // What the sun is told lasts as long as a picture does.
        if matches!(state.picture, Picture::Missing | Picture::Meshing) {
            state.occupancy = None;
        }
        self.tally.replace(before, Tally::of(state));
        let order_after = state.held_order();
        if order_before != order_after {
            if let Some(order) = order_before {
                self.held_order.remove(&order);
            }
            if let Some(order) = order_after {
                self.held_order.insert(order, coordinate);
            }
        }
        if state.is_idle() {
            self.records.remove(&coordinate);
        }
        result
    }

    /// Like [`BrickStates::change`], for a change that does nothing to a brick that has no record, and so makes none.
    fn change_if_known<R>(
        &mut self,
        coordinate: BrickCoordinate,
        change: impl FnOnce(&mut BrickState) -> R,
    ) -> Option<R> {
        self.records
            .contains_key(&coordinate)
            .then(|| self.change(coordinate, change))
    }

    /// Takes the voxels of a brick that is known as it is (an edited one), if it has none yet.
    pub(crate) fn load(&mut self, coordinate: BrickCoordinate, data: Arc<Brick>) {
        self.change(coordinate, |state| {
            state.data.get_or_insert(data);
        });
    }

    /// A job is to generate the voxels of a brick.
    pub(crate) fn start_generating(&mut self, coordinate: BrickCoordinate) {
        self.change(coordinate, |state| state.generating = true);
    }

    /// The job that generated the voxels of a brick has finished: they are taken unless the brick has them already (as edited), and unless the job was
    /// given up (the brick was edited meanwhile, or the mesher reset).
    pub(crate) fn finish_generating(&mut self, coordinate: BrickCoordinate, data: Arc<Brick>) {
        self.change_if_known(coordinate, |state| {
            if state.generating {
                state.generating = false;
                state.data.get_or_insert(data);
            }
        });
    }

    /// The voxels of a brick were edited: they are taken again from what is known of them, and a job that was generating them is given up. (What is drawn
    /// of it and its neighbours is [`BrickStates::drop_picture`]'s.)
    pub(crate) fn forget_data(&mut self, coordinate: BrickCoordinate) {
        self.change_if_known(coordinate, |state| {
            state.data = None;
            state.generating = false;
        });
    }

    /// A job is to make the picture of a brick that has none.
    pub(crate) fn start_meshing(&mut self, coordinate: BrickCoordinate) {
        self.change(coordinate, |state| {
            debug_assert!(state.needs_meshing(), "a brick is meshed once at a time");
            state.picture = Picture::Meshing;
        });
    }

    /// The job that made the picture of a brick has finished. Returns whether it was wanted, which is whether the brick was still being meshed: if not (it
    /// went out of range, or what it is drawn from changed), the result is to be thrown away. The brick has no picture afterwards, until one is given
    /// with [`BrickStates::hold`] or [`BrickStates::deliver`].
    pub(crate) fn finish_meshing(&mut self, coordinate: BrickCoordinate) -> bool {
        self.change_if_known(coordinate, |state| {
            let meshing = state.is_meshing();
            if meshing {
                state.picture = Picture::Missing;
            }
            meshing
        })
        .unwrap_or(false)
    }

    /// The picture that was made is up to date and is kept until it is asked for.
    pub(crate) fn hold(&mut self, coordinate: BrickCoordinate, mesh: PackedBrickMesh) {
        self.change(coordinate, |state| state.picture = Picture::Waiting(mesh));
    }

    /// The picture that was made is up to date, and is told to the renderer (by the caller) at once. `mesh_bytes` is what its mesh takes.
    pub(crate) fn deliver(&mut self, coordinate: BrickCoordinate, mesh_bytes: usize) {
        self.change(coordinate, |state| {
            state.picture = Picture::Current { mesh_bytes };
        });
    }

    /// Keeps what the sun is told of a brick, with its picture, so that it can be told again when a held brick is shown. Nothing is kept unless pictures are held, and
    /// nothing for a brick that has no picture.
    pub(crate) fn keep_occupancy(
        &mut self,
        coordinate: BrickCoordinate,
        occupancy: BrickOccupancy,
    ) {
        if self.budget == 0 {
            return;
        }
        self.change_if_known(coordinate, |state| {
            if matches!(state.picture, Picture::Waiting(_) | Picture::Current { .. }) {
                state.occupancy = Some(occupancy);
            }
        });
    }

    /// Whether the renderer holds a mesh of the brick.
    pub(crate) fn is_displayed(&self, coordinate: BrickCoordinate) -> bool {
        self.get(coordinate).is_some_and(|state| state.displayed)
    }

    pub(crate) fn set_displayed(&mut self, coordinate: BrickCoordinate, displayed: bool) {
        self.change(coordinate, |state| state.displayed = displayed);
    }

    /// Gives out the waiting picture of a brick: the brick is shown, and its picture is the renderer's now. `None` if none is waiting.
    pub(crate) fn hand_out(&mut self, coordinate: BrickCoordinate) -> Option<PackedBrickMesh> {
        self.change_if_known(coordinate, |state| {
            if !state.is_waiting() {
                return None;
            }
            let Picture::Waiting(mesh) = std::mem::replace(&mut state.picture, Picture::Missing)
            else {
                unreachable!("it was waiting");
            };
            state.picture = Picture::Current {
                mesh_bytes: mesh.byte_size(),
            };
            state.shown = true;
            Some(mesh)
        })
        .flatten()
    }

    /// Takes a shown brick down. Its picture was the renderer's, and is gone with it: the brick has none until it is made again, which it is if it is wanted
    /// again. Returns whether the renderer held a mesh of it (and now does not), or `None` if the brick was not shown.
    pub(crate) fn take_down(&mut self, coordinate: BrickCoordinate) -> Option<bool> {
        self.change_if_known(coordinate, |state| {
            if !state.shown {
                return None;
            }
            state.shown = false;
            if matches!(state.picture, Picture::Current { .. }) {
                state.picture = Picture::Missing;
            }
            Some(std::mem::take(&mut state.displayed))
        })
        .flatten()
    }

    /// Takes a shown brick down and keeps its picture if it can: there is a budget and the picture is up to date (it is not being made again after an edit). The
    /// renderer is then to **hide** the mesh it holds ([`Taken::Hidden`], or nothing if there is none) and not to take it down, and the brick is shown again by
    /// [`BrickStates::reveal`]. If not, as [`BrickStates::take_down`] (and [`Taken::Removed`] where that says the renderer held a mesh). `None` if the brick was not shown.
    pub(crate) fn hold_down(&mut self, coordinate: BrickCoordinate) -> Option<Taken> {
        let holds = self.budget > 0
            && self.get(coordinate).is_some_and(|state| {
                state.shown && matches!(state.picture, Picture::Current { .. })
            });
        if !holds {
            return self.take_down(coordinate).map(|displayed| {
                if displayed {
                    Taken::Removed
                } else {
                    Taken::Nothing
                }
            });
        }
        let order = self.next_order;
        self.next_order += 1;
        self.change_if_known(coordinate, |state| {
            let Picture::Current { mesh_bytes } = state.picture else {
                unreachable!("it was checked");
            };
            state.shown = false;
            state.picture = Picture::Held { mesh_bytes, order };
            if state.displayed {
                Taken::Hidden
            } else {
                Taken::Nothing
            }
        })
    }

    /// Shows a brick whose picture is held, with it: returns whether the renderer holds a mesh of it, which it is to show (it is hidden). `None` if the picture of the
    /// brick is not held.
    pub(crate) fn reveal(&mut self, coordinate: BrickCoordinate) -> Option<bool> {
        self.change_if_known(coordinate, |state| {
            let Picture::Held { mesh_bytes, .. } = state.picture else {
                return None;
            };
            state.shown = true;
            state.picture = Picture::Current { mesh_bytes };
            Some(state.displayed)
        })
        .flatten()
    }

    /// Lets go of held pictures, the oldest first, until what is held is within the budget, and returns the bricks whose hidden meshes the renderer is to take down. A
    /// brick that `keep` says is kept (one that is wanted: it is about to be shown) is not let go of, so what is held may pass the budget by those.
    pub(crate) fn evict_over_budget(
        &mut self,
        keep: impl Fn(BrickCoordinate) -> bool,
    ) -> Vec<BrickCoordinate> {
        let mut excess = self.tally.held_bytes.saturating_sub(self.budget);
        if excess == 0 {
            return Vec::new();
        }
        let mut victims = Vec::new();
        for &coordinate in self.held_order.values() {
            if excess == 0 {
                break;
            }
            if keep(coordinate) {
                continue;
            }
            let cost = self
                .records
                .get(&coordinate)
                .expect("a brick in the order of the held has a record")
                .held_cost();
            excess = excess.saturating_sub(cost);
            victims.push(coordinate);
        }
        let mut to_take_down = Vec::new();
        for coordinate in victims {
            let displayed = self.change(coordinate, |state| {
                state.picture = Picture::Missing;
                std::mem::take(&mut state.displayed)
            });
            if displayed {
                to_take_down.push(coordinate);
            }
        }
        to_take_down
    }

    /// The picture of a brick, if there is one made, is out of date: it is dropped, and the brick is to be meshed again. A job that is making one is left, and
    /// its result is found out of date when it comes. Returns whether the picture was held and the renderer holds a hidden mesh of it, which it is to take down (a
    /// brick that is shown keeps its mesh until the new one is made).
    pub(crate) fn drop_picture(&mut self, coordinate: BrickCoordinate) -> bool {
        self.change_if_known(coordinate, |state| {
            if state.is_held() {
                state.picture = Picture::Missing;
                return std::mem::take(&mut state.displayed);
            }
            if state.is_meshed() {
                state.picture = Picture::Missing;
            }
            false
        })
        .unwrap_or(false)
    }

    /// A brick goes out of range: its voxels are dropped, and so is any picture of it that is not held, and any job making one. That it is shown and what the renderer
    /// holds of it stay, for whoever shows it to take down. A picture that is held is kept: it is made from what the voxels were, and is shown without them; and so is the
    /// picture of a brick that is shown, if pictures are held, for it is the one that is held when the brick is taken down.
    pub(crate) fn unload(&mut self, coordinate: BrickCoordinate) {
        let holds = self.budget > 0;
        self.change_if_known(coordinate, |state| {
            state.data = None;
            // (A brick that is shown goes out of range before the viewer takes it down, and its picture is what is held then, if pictures are held.)
            let kept = state.is_held() || holds && matches!(state.picture, Picture::Current { .. });
            if !kept {
                state.picture = Picture::Missing;
            }
        });
    }

    /// Every picture is out of date, made or being made (what they are drawn from changed). Returns the bricks whose pictures were held and whose hidden meshes the
    /// renderer is to take down, in order.
    pub(crate) fn drop_all_pictures(&mut self) -> Vec<BrickCoordinate> {
        let mut to_take_down: Vec<BrickCoordinate> = self
            .records
            .iter()
            .filter(|(_, state)| state.is_held() && state.displayed)
            .map(|(coordinate, _)| *coordinate)
            .collect();
        to_take_down.sort();
        for state in self.records.values_mut() {
            if state.is_held() {
                state.displayed = false;
            }
            state.picture = Picture::Missing;
            state.occupancy = None;
        }
        self.records.retain(|_, state| !state.is_idle());
        self.tally = Tally::default();
        self.held_order.clear();
        for state in self.records.values() {
            self.tally.add(Tally::of(state));
        }
        to_take_down
    }

    /// Forgets every brick, and says which had a mesh on the renderer, in order.
    pub(crate) fn clear(&mut self) -> Vec<BrickCoordinate> {
        let mut displayed: Vec<BrickCoordinate> = self
            .records
            .iter()
            .filter(|(_, state)| state.displayed)
            .map(|(coordinate, _)| *coordinate)
            .collect();
        displayed.sort();
        self.records.clear();
        self.tally = Tally::default();
        self.held_order.clear();
        displayed
    }

    /// Checks what the records and the changes cannot make impossible, and says what is wrong if it is not so (the first thing found). `held_back` is whether
    /// the mesher keeps its pictures until they are asked for.
    pub(crate) fn check_consistency(&self, held_back: bool) -> Result<(), String> {
        let mut counted = Tally::default();
        for (coordinate, state) in &self.records {
            counted.add(Tally::of(state));
            if state.is_idle() {
                return Err(format!("{coordinate:?} has a record that says nothing"));
            }
            // (A picture that was given out may have no voxels: a brick whose picture was held is shown before its voxels are loaded again.)
            if state.is_waiting() && state.data.is_none() {
                return Err(format!("{coordinate:?} has a picture and is not loaded"));
            }
            if state.is_held() && state.shown {
                return Err(format!("{coordinate:?} has a picture held and is shown"));
            }
            if let Some(order) = state.held_order()
                && self.held_order.get(&order) != Some(coordinate)
            {
                return Err(format!(
                    "{coordinate:?} is held, and the order of the held pictures has not got it where it says"
                ));
            }
            if held_back {
                if state.is_waiting() && state.shown {
                    return Err(format!("{coordinate:?} has a picture waiting and is shown"));
                }
                if matches!(state.picture, Picture::Current { .. }) && !state.shown {
                    return Err(format!(
                        "{coordinate:?} has a picture that was given out and is not shown, so it would never be meshed again"
                    ));
                }
                if state.displayed && !state.shown && !state.is_held() {
                    return Err(format!("{coordinate:?} is on the screen and not shown"));
                }
            }
        }
        if self.held_order.len() != counted.held {
            return Err(format!(
                "{} bricks are in the order of the held pictures and {} are held",
                self.held_order.len(),
                counted.held
            ));
        }
        if counted != self.tally {
            return Err(format!(
                "the records count {counted:?} and the tally says {:?}",
                self.tally
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
