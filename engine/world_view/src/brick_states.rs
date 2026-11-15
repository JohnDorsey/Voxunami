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

use std::collections::HashMap;
use std::sync::Arc;

use voxel_grid::Brick;
use voxel_meshing::PackedBrickMesh;
use voxel_space::BrickCoordinate;

/// What is known of the picture of a brick.
enum Picture {
    /// There is no up-to-date picture, and none is being made.
    Missing,
    /// A job is making it.
    Meshing,
    /// It is made and up to date, and not on the screen: held back until the viewer asks for it.
    Waiting(PackedBrickMesh),
    /// It is made and up to date, and the renderer has been told (or the mesher does not hold pictures back and told it as soon as it was made).
    Current,
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
    /// The renderer holds a mesh of the brick.
    displayed: bool,
}

impl BrickState {
    fn new() -> Self {
        Self {
            data: None,
            generating: false,
            picture: Picture::Missing,
            shown: false,
            displayed: false,
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
        matches!(self.picture, Picture::Waiting(_) | Picture::Current)
    }

    /// A picture is waiting to be shown.
    pub(crate) fn is_waiting(&self) -> bool {
        matches!(self.picture, Picture::Waiting(_))
    }

    pub(crate) fn is_shown(&self) -> bool {
        self.shown
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
}

impl Tally {
    fn of(state: &BrickState) -> Self {
        Self {
            loaded: usize::from(state.data.is_some()),
            generating: usize::from(state.generating),
            meshing: usize::from(state.is_meshing()),
            meshed: usize::from(state.is_meshed()),
        }
    }

    /// Takes away what a record counted before a change and adds what it counts after.
    fn replace(&mut self, before: Self, after: Self) {
        self.loaded = self.loaded + after.loaded - before.loaded;
        self.generating = self.generating + after.generating - before.generating;
        self.meshing = self.meshing + after.meshing - before.meshing;
        self.meshed = self.meshed + after.meshed - before.meshed;
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
        let result = change(state);
        self.tally.replace(before, Tally::of(state));
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

    /// The picture that was made is up to date, and is told to the renderer (by the caller) at once.
    pub(crate) fn deliver(&mut self, coordinate: BrickCoordinate) {
        self.change(coordinate, |state| state.picture = Picture::Current);
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
            let Picture::Waiting(mesh) = std::mem::replace(&mut state.picture, Picture::Current)
            else {
                unreachable!("it was waiting");
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
            if matches!(state.picture, Picture::Current) {
                state.picture = Picture::Missing;
            }
            Some(std::mem::take(&mut state.displayed))
        })
        .flatten()
    }

    /// The picture of a brick, if there is one made, is out of date: it is dropped, and the brick is to be meshed again. A job that is making one is left, and
    /// its result is found out of date when it comes.
    pub(crate) fn drop_picture(&mut self, coordinate: BrickCoordinate) {
        self.change_if_known(coordinate, |state| {
            if state.is_meshed() {
                state.picture = Picture::Missing;
            }
        });
    }

    /// A brick goes out of range: its voxels are dropped, and so is any picture of it and any job making one. That it is shown and what the renderer holds
    /// of it stay, for whoever shows it to take down.
    pub(crate) fn unload(&mut self, coordinate: BrickCoordinate) {
        self.change_if_known(coordinate, |state| {
            state.data = None;
            state.picture = Picture::Missing;
        });
    }

    /// Every picture is out of date, made or being made (what they are drawn from changed).
    pub(crate) fn drop_all_pictures(&mut self) {
        for state in self.records.values_mut() {
            state.picture = Picture::Missing;
        }
        self.records.retain(|_, state| !state.is_idle());
        self.tally = Tally::default();
        for state in self.records.values() {
            self.tally.add(Tally::of(state));
        }
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
            if state.is_meshed() && state.data.is_none() {
                return Err(format!("{coordinate:?} has a picture and is not loaded"));
            }
            if held_back {
                if state.is_waiting() && state.shown {
                    return Err(format!("{coordinate:?} has a picture waiting and is shown"));
                }
                if matches!(state.picture, Picture::Current) && !state.shown {
                    return Err(format!(
                        "{coordinate:?} has a picture that was given out and is not shown, so it would never be meshed again"
                    ));
                }
                if state.displayed && !state.shown {
                    return Err(format!("{coordinate:?} is on the screen and not shown"));
                }
            }
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
