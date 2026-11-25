//! What the picture is made from (`docs/specs/viewpoints-spec.md`, 6.1, 6.5 and phase 5): the player's eyes, a **map** that goes round a focus, or a camera that was
//! detached and flies.
//!
//! The session's `camera` is the player's: where their body is and which way they look, what the server is told. [`Session::drawn_camera`] is the camera the picture is
//! made from: the same in [`ViewMode::Body`], and in the other two a camera of the view's own, which the player's keys and the server never move. Whichever is not
//! drawn, the player's eyes or the map, is **kept ready**: its place is held warm in the terrain (`Session::keep_ready` does that for a game that asks), the player's
//! while the map is drawn, and the map's for a counted while after it was left ([`MAP_WARM_TICKS`]), so that going from one to the other is a move of pictures.

use std::cell::RefCell;
use std::collections::BTreeMap;

use glam::DVec3;
use renderer::Camera;
use transport::Connection;
use voxel_space::{BrickBox, GridId, Orientation, WorldPosition};

use super::Session;
use crate::definition::GameDefinition;
use crate::view_rig::{Focus, FocusFrame, Orbit, next_focus};

/// How many ticks of the server's the map is kept ready for after the player went back to their own eyes: 30 seconds at 20 ticks a second (`viewpoints-spec.md`, 6.1: the
/// first design has the map tracked for this long, and then let go of; the lazy window after it is not built). Counted in ticks, never timed.
pub const MAP_WARM_TICKS: u64 = 600;

/// How far the map's camera, or a camera that flies, may go from where the terrain's cubes were chosen before they are chosen again, as a share of the distance to the
/// nearest thing drawn: a tenth (`LodView::set_reselect_tolerance`; a camera that goes round the map crossed a brick every frame, and choosing the cubes again took 13 ms of
/// the frame, `docs/specs/viewpoints-spec.md`, 9n). The player's own eyes have none: they choose them again in every brick, as they always did.
pub const MAP_RESELECT_TOLERANCE: f64 = 0.25;

/// About how far a player's parts are from their eyes, in voxels, for the size of the focus that is the body.
const BODY_RADIUS: f64 = 2.0;

/// How many ticks the box of bricks a grid has is trusted for before it is worked out again (a ship that is being built grows).
const BOX_TRUSTED_TICKS: u64 = 40;

/// What a game's keys ask of the picture's camera (`ClientContext::view`): the keys are the game's, and what they do is the engine's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewCommand {
    /// The map, if the player's eyes or a camera that flies are drawn; the player's eyes, if the map is.
    ToggleMap,
    /// A camera that flies off, if the player's eyes or the map are drawn; the player's eyes, if it is.
    ToggleDetached,
    /// The map's next focus (a negative step is the one before). Nothing, unless the map is drawn.
    CycleFocus(i32),
}

/// What the picture is made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// The player's eyes.
    Body,
    /// The map: a camera that goes round a focus ([`Session::orbit`]).
    Map,
    /// A camera that was let go of and flies where the player points it.
    Fly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum State {
    #[default]
    Body,
    Orbit(Orbit),
    Fly(Camera),
}

/// What the session keeps of how its picture is made.
#[derive(Debug, Default)]
pub(super) struct ViewRig {
    state: State,
    /// The orbit the map had when it was left, and the tick it was left at (a detached camera that comes back leaves it as it was).
    left: Option<(Orbit, u64)>,
    /// The boxes of bricks that grids have, with the tick each was worked out at: a ship's is worked out from all its bricks, which is not done for every picture.
    boxes: RefCell<BTreeMap<GridId, (u64, Option<BrickBox>)>>,
}

impl<C: Connection, G: GameDefinition> Session<C, G> {
    /// Does what a key of the game asked of the camera ([`ViewCommand`]).
    pub fn apply_view_command(&mut self, command: ViewCommand) {
        match command {
            ViewCommand::ToggleMap => match self.view_mode() {
                ViewMode::Map => self.show_body(),
                ViewMode::Body | ViewMode::Fly => self.show_map(),
            },
            ViewCommand::ToggleDetached => match self.view_mode() {
                ViewMode::Fly => self.show_body(),
                ViewMode::Body | ViewMode::Map => self.detach_camera(),
            },
            ViewCommand::CycleFocus(step) => self.cycle_focus(step),
        }
    }

    /// What the picture is made from.
    pub fn view_mode(&self) -> ViewMode {
        match self.rig.state {
            State::Body => ViewMode::Body,
            State::Orbit(_) => ViewMode::Map,
            State::Fly(_) => ViewMode::Fly,
        }
    }

    /// The map's orbit, while the map is drawn.
    pub fn orbit(&self) -> Option<Orbit> {
        match self.rig.state {
            State::Orbit(orbit) => Some(orbit),
            _ => None,
        }
    }

    /// What the map goes round (the player's body while no map is drawn).
    pub fn focus(&self) -> Focus {
        self.orbit().map_or(Focus::Body, |orbit| orbit.focus)
    }

    /// The camera the picture is made from: the player's own, the map's, or the one that flies.
    pub fn drawn_camera(&self) -> Camera {
        match self.rig.state {
            State::Body => self.camera,
            State::Fly(camera) => camera,
            State::Orbit(orbit) if self.map_is_orthographic() => {
                orbit.orthographic_camera(&self.focus_frame(orbit.focus), &self.camera)
            }
            State::Orbit(orbit) => orbit.camera(&self.focus_frame(orbit.focus), &self.camera),
        }
    }

    /// Whether the map that is drawn is orthographic: it is drawn, and the game says the level's map is ([`GameDefinition::orthographic_map`]).
    pub fn map_is_orthographic(&self) -> bool {
        matches!(self.rig.state, State::Orbit(_))
            && self
                .client
                .world()
                .world()
                .level_id(self.level)
                .is_some_and(G::orthographic_map)
    }

    /// Where the cubes of the drawn picture are chosen from: the camera's place, and for an orthographic map the focus (the eye of such a camera is only a way back from what it
    /// looks at, and the bricks that are drawn as bricks are those at the middle of the picture).
    pub(crate) fn drawn_viewer(&self) -> WorldPosition {
        match self.rig.state {
            State::Orbit(orbit) if self.map_is_orthographic() => {
                self.focus_frame(orbit.focus).centre
            }
            _ => self.drawn_camera().position,
        }
    }

    /// Draws the map: a camera that goes round the focus it had when it was last left (round the player's body, if it has none, or the ship it was round is gone).
    pub fn show_map(&mut self) {
        if matches!(self.rig.state, State::Orbit(_)) {
            return;
        }
        let orbit = match self.rig.left.take() {
            Some((orbit, _)) if self.focus_exists(orbit.focus) => orbit,
            _ => Orbit::new(Focus::Body, &self.focus_frame(Focus::Body)),
        };
        self.rig.state = State::Orbit(orbit);
    }

    /// Draws what the player's eyes see. A map that was drawn is kept ready for [`MAP_WARM_TICKS`].
    pub fn show_body(&mut self) {
        if let State::Orbit(orbit) = self.rig.state {
            self.rig.left = Some((orbit, self.client.world().tick().get()));
        }
        self.rig.state = State::Body;
    }

    /// Lets go of the camera: it flies from where it is, and what the player does with their body does not move it. Whatever was drawn is where it starts.
    pub fn detach_camera(&mut self) {
        let camera = self.drawn_camera();
        if let State::Orbit(orbit) = self.rig.state {
            self.rig.left = Some((orbit, self.client.world().tick().get()));
        }
        self.rig.state = State::Fly(camera);
    }

    /// Turns the map's camera round its focus by angles in radians (the azimuth is round the focus's up, the elevation above it).
    pub fn orbit_by(&mut self, azimuth: f64, elevation: f64) {
        if let State::Orbit(orbit) = &mut self.rig.state {
            *orbit = orbit.rotated_by(azimuth, elevation);
        }
    }

    /// Brings the map's camera nearer or sends it farther from its focus by a factor (2 is twice as far).
    pub fn orbit_zoom(&mut self, factor: f64) {
        if let State::Orbit(orbit) = self.rig.state {
            let frame = self.focus_frame(orbit.focus);
            self.rig.state = State::Orbit(orbit.zoomed_by(factor, &frame));
        }
    }

    /// Locks the map on the next thing that can be (or, with a negative step, the one before): the player's own body, then the ships of the level in the order of their
    /// numbers. The camera keeps its angles, and starts as far from the new focus as it was from the old one in its sizes.
    pub fn cycle_focus(&mut self, step: i32) {
        let State::Orbit(orbit) = self.rig.state else {
            return;
        };
        let candidates = self.focus_candidates();
        let focus = next_focus(orbit.focus, &candidates, step);
        if focus == orbit.focus {
            return;
        }
        let (old, new) = (self.focus_frame(orbit.focus), self.focus_frame(focus));
        // (The distance is as many radii of the new focus as it was of the old: a ship and a planet are not seen from as far.)
        let in_radii = orbit.distance / old.radius.max(1.0);
        let kept = Orbit {
            focus,
            distance: (in_radii * new.radius.max(1.0))
                .max(Orbit::nearest_distance(&new))
                .min(Orbit::MAXIMUM_DISTANCE),
            ..orbit
        };
        self.rig.state = State::Orbit(kept);
    }

    /// What the map can be locked on, in order: the player's body, and the ships of the level the player is in that have a place (the bricks of a ship, or the extent of a
    /// planet).
    pub fn focus_candidates(&self) -> Vec<Focus> {
        let tick = self.client.world().tick().get();
        let mut focuses = vec![Focus::Body];
        let ships = self.client.world().grids();
        focuses.extend(
            ships
                .iter()
                .filter(|(_, ship)| ship.level == self.level)
                .filter(|(id, _)| self.grid_box(*id, tick).is_some())
                .map(|(id, _)| Focus::Grid(id)),
        );
        focuses
    }

    /// Turns the camera that flies, by angles in radians.
    pub fn fly_look_by(&mut self, yaw: f64, pitch: f64) {
        if let State::Fly(camera) = &mut self.rig.state {
            camera.look_by(yaw, pitch);
        }
    }

    /// Moves the camera that flies, by a movement in its own axes (x right, y up, z backwards), in voxels.
    pub fn fly_move_relative(&mut self, movement: DVec3) {
        if let State::Fly(camera) = &mut self.rig.state {
            camera.move_relative(movement);
        }
    }

    /// Moves the camera that flies, by a movement in the world's axes, in voxels.
    pub fn fly_move_in_world(&mut self, movement: DVec3) {
        if let State::Fly(camera) = &mut self.rig.state {
            camera.move_in_world(movement);
        }
    }

    /// The places that are kept ready now, by grid: the ones a game asked for with [`Session::keep_ready`], and what the view keeps ready by itself. While the map or a
    /// detached camera is drawn, that is the player's own place, in the root grid of the level and in every planet; for [`MAP_WARM_TICKS`] after the map is left, the map's.
    pub(crate) fn warm_places_now(&self) -> BTreeMap<GridId, WorldPosition> {
        let mut places = match self.rig.state {
            State::Orbit(_) | State::Fly(_) => self.places_in_grids(self.camera.position),
            State::Body => match self.rig.left {
                Some((orbit, left_at))
                    if self.client.world().tick().get().saturating_sub(left_at)
                        <= MAP_WARM_TICKS =>
                {
                    let camera = orbit.camera(&self.focus_frame(orbit.focus), &self.camera);
                    self.places_in_grids(camera.position)
                }
                _ => BTreeMap::new(),
            },
        };
        places.extend(self.warm_places.iter().map(|(grid, place)| (*grid, *place)));
        places
    }

    /// The places the drawn camera is in, by grid, when it is not the player's own (the server is asked for the edits round them as round the player).
    pub(super) fn drawn_places_besides_the_body(&self) -> BTreeMap<GridId, WorldPosition> {
        match self.rig.state {
            State::Body => BTreeMap::new(),
            _ => self.places_in_grids(self.drawn_viewer()),
        }
    }

    /// The bricks, by grid, that the nodes drawn from the drawn camera were chosen for when it is not the player's own: where the views last chose them, which is not the
    /// camera's own brick while the camera is within [`MAP_RESELECT_TOLERANCE`] of it. **This is what the server is asked the nodes round**, so that both ends choose from
    /// the same place (`docs/specs/viewpoints-spec.md`, test 5).
    pub(super) fn drawn_nodes_were_chosen_for(
        &self,
    ) -> BTreeMap<GridId, voxel_space::BrickCoordinate> {
        let mut chosen = BTreeMap::new();
        if matches!(self.rig.state, State::Body) {
            return chosen;
        }
        for (grid, place) in self.drawn_places_besides_the_body() {
            chosen.insert(
                grid,
                self.nodes_chosen_for(grid).unwrap_or_else(|| place.brick()),
            );
        }
        chosen
    }

    /// The brick, in the coordinates of the grid (the root grid of the level, or a planet), that the nodes drawn of it were last chosen for: the one the camera was in then.
    /// `None` for a grid whose terrain is not drawn in nodes, and before the first.
    pub(crate) fn nodes_chosen_for(&self, grid: GridId) -> Option<voxel_space::BrickCoordinate> {
        match &self.terrain {
            super::TerrainView::Coarse(coarse) if grid == GridId::root(self.level) => {
                coarse.view.wanted_centre()
            }
            _ => self
                .planets
                .get(&grid)
                .and_then(|planet| planet.selected_centre()),
        }
    }

    /// A position in the world as a place in the root grid of the level and in each planet (a grid with terrain of its own), in each grid's own coordinates.
    fn places_in_grids(&self, position: WorldPosition) -> BTreeMap<GridId, WorldPosition> {
        let mut places = BTreeMap::from([(GridId::root(self.level), position)]);
        for (id, ship) in self.client.world().grids().iter() {
            if ship.terrain.is_some()
                && ship.level == self.level
                && let Some(local) = ship.pose.world_to_local(position)
            {
                places.insert(id, local);
            }
        }
        places
    }

    fn focus_exists(&self, focus: Focus) -> bool {
        match focus {
            Focus::Body => true,
            Focus::Grid(id) => self
                .grid_box(id, self.client.world().tick().get())
                .is_some(),
        }
    }

    /// Where a focus is. A ship that has no place (it is gone, or has no bricks) is the player's own body.
    fn focus_frame(&self, focus: Focus) -> FocusFrame {
        let body = FocusFrame {
            centre: self.camera.position,
            orientation: Orientation::from_components(
                self.camera.frame_rotation.to_array().map(f64::from),
            )
            .unwrap_or(Orientation::IDENTITY),
            radius: BODY_RADIUS,
        };
        let Focus::Grid(id) = focus else {
            return body;
        };
        let Some(ship) = self.client.world().grids().get(id) else {
            return body;
        };
        let Some(bounds) = self.grid_box(id, self.client.world().tick().get()) else {
            return body;
        };
        // The middle of the box of bricks, and half the length of its diagonal, in the ship's own axes.
        let (min, max) = (bounds.min(), bounds.max());
        let edge = voxel_space::BRICK_EDGE_LENGTH as f64;
        let size = [
            (max.x() - min.x() + 1) as f64 * edge,
            (max.y() - min.y() + 1) as f64 * edge,
            (max.z() - min.z() + 1) as f64 * edge,
        ];
        let middle = WorldPosition::new(min, [0.0; 3]).translated(size.map(|length| length / 2.0));
        let Some(centre) = ship.pose.local_to_world(middle) else {
            return body;
        };
        FocusFrame {
            centre,
            orientation: ship.pose.orientation,
            radius: libm::sqrt(size.iter().map(|length| length * length).sum::<f64>()) / 2.0,
        }
    }

    /// The box of bricks a grid has, in its own coordinates: the extent of a planet or of a ship that is far, and for a ship the bricks it holds (worked out again after
    /// [`BOX_TRUSTED_TICKS`]).
    fn grid_box(&self, id: GridId, tick: u64) -> Option<BrickBox> {
        let ship = self.client.world().grids().get(id)?;
        if let Some(terrain) = ship.terrain {
            return Some(terrain.extent);
        }
        if let Some(far) = ship.far {
            return Some(far);
        }
        let mut boxes = self.rig.boxes.borrow_mut();
        if let Some((at, bounds)) = boxes.get(&id)
            && tick.saturating_sub(*at) <= BOX_TRUSTED_TICKS
        {
            return *bounds;
        }
        let bounds = ship.voxels.bricks().map(|(brick, _)| brick).fold(
            None,
            |bounds: Option<(voxel_space::BrickCoordinate, voxel_space::BrickCoordinate)>,
             brick| match bounds {
                None => Some((brick, brick)),
                Some((low, high)) => Some((
                    voxel_space::BrickCoordinate::new(
                        low.x().min(brick.x()),
                        low.y().min(brick.y()),
                        low.z().min(brick.z()),
                    ),
                    voxel_space::BrickCoordinate::new(
                        high.x().max(brick.x()),
                        high.y().max(brick.y()),
                        high.z().max(brick.z()),
                    ),
                )),
            },
        );
        let bounds = bounds.and_then(|(low, high)| BrickBox::new(low, high));
        boxes.insert(id, (tick, bounds));
        bounds
    }
}
