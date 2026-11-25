//! The camera of a picture that is not the player's eyes (`docs/specs/viewpoints-spec.md`, 6.1, 6.5 and phase 5): an **orbit** round a **focus**, which is the map and
//! a third-person view alike.
//!
//! A focus is a thing with a place: the player's own body, or a grid (a ship, a planet, a moon). An [`Orbit`] is where the camera is round it: a distance and two angles,
//! **in the focus's own axes**, so that a camera locked on a planet keeps its place over the planet while the planet moves and turns on its rail, and the cut of the
//! planet does not change. All of it is cosmetic and client-side: nothing here is read by the simulation, and it needs no clock.

use glam::Quat;
use renderer::{Camera, Projection};
use voxel_space::{GridId, GridPose, Orientation, WorldPosition};

/// What an orbit camera is locked on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Focus {
    /// The player's own body: where their eyes are, in the room they are in.
    Body,
    /// A grid with a pose: a ship, a planet, a moon.
    Grid(GridId),
}

/// Where a focus is, as an orbit needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusFrame {
    /// The middle of the focus, in the world.
    pub centre: WorldPosition,
    /// The turn of the focus's own axes.
    pub orientation: Orientation,
    /// About how far its parts are from its middle, in voxels (0 for a point): the camera stays outside it, and starts at a few times it.
    pub radius: f64,
}

/// A camera that goes round a focus: `distance` voxels from its middle, in the direction the two angles say **in the focus's own axes**, looking at the middle with the
/// focus's up above it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub focus: Focus,
    /// Radians round the focus's up axis, from straight back along its z axis, turning toward its x axis. In `[-pi, pi)`.
    pub azimuth: f64,
    /// Radians above the focus's x-z plane, from `-MAXIMUM_ELEVATION` to `MAXIMUM_ELEVATION`.
    pub elevation: f64,
    /// Voxels from the middle of the focus, from [`Orbit::nearest_distance`] to [`Orbit::MAXIMUM_DISTANCE`].
    pub distance: f64,
}

impl Orbit {
    /// The highest the camera goes: just short of straight over the focus, so that the view never flips (as [`Camera::MAXIMUM_PITCH`]).
    pub const MAXIMUM_ELEVATION: f64 = Camera::MAXIMUM_PITCH;

    /// The farthest the camera goes, in voxels: 2^50, a million million, which is far past the nodes of the largest worlds (the range of a position is far greater, and
    /// the cubes that are drawn from there are a few of the coarsest).
    pub const MAXIMUM_DISTANCE: f64 = 1_125_899_906_842_624.0;

    /// How many radii of the focus the camera starts from.
    pub const STARTING_DISTANCE_IN_RADII: f64 = 4.0;

    /// How many radii of the focus is the nearest the camera comes: outside it.
    const NEAREST_IN_RADII: f64 = 1.5;

    /// The nearest the camera comes to anything, whatever its size, in voxels.
    const NEAREST_OF_ALL: f64 = 4.0;

    /// The nearest the camera comes to a focus of this size.
    pub fn nearest_distance(frame: &FocusFrame) -> f64 {
        (frame.radius * Self::NEAREST_IN_RADII).max(Self::NEAREST_OF_ALL)
    }

    /// An orbit round a focus, a little above it, from where all of it is in view.
    pub fn new(focus: Focus, frame: &FocusFrame) -> Self {
        Self {
            focus,
            azimuth: 0.0,
            elevation: 0.5,
            distance: (frame.radius * Self::STARTING_DISTANCE_IN_RADII)
                .max(Self::nearest_distance(frame))
                .min(Self::MAXIMUM_DISTANCE),
        }
    }

    /// The same orbit turned: the azimuth carried round into `[-pi, pi)`, the elevation held short of straight up and down.
    pub fn rotated_by(self, azimuth: f64, elevation: f64) -> Self {
        let turned = self.azimuth + azimuth;
        let wrapped = if turned.is_finite() {
            turned
                - std::f64::consts::TAU
                    * ((turned + std::f64::consts::PI) / std::f64::consts::TAU).floor()
        } else {
            self.azimuth
        };
        let raised = self.elevation + elevation;
        Self {
            azimuth: wrapped,
            elevation: if raised.is_finite() {
                raised.clamp(-Self::MAXIMUM_ELEVATION, Self::MAXIMUM_ELEVATION)
            } else {
                self.elevation
            },
            ..self
        }
    }

    /// The same orbit nearer or farther by a factor (2 is twice as far), between the nearest the focus allows and the farthest. A factor that is not over nothing, or not a
    /// number, changes nothing.
    pub fn zoomed_by(self, factor: f64, frame: &FocusFrame) -> Self {
        if !(factor.is_finite() && factor > 0.0) {
            return self;
        }
        Self {
            distance: (self.distance * factor)
                .max(Self::nearest_distance(frame))
                .min(Self::MAXIMUM_DISTANCE),
            ..self
        }
    }

    /// The smallest distance the eye of an orthographic map is from the middle of its focus, in voxels: its box reaches ahead from the eye, and behind the middle of the picture.
    const ORTHOGRAPHIC_EYE_DISTANCE: f64 = 128.0;

    /// How many voxels the picture of an orthographic map is high: what a perspective camera at this distance sees at the focus, so that zooming goes as it does there.
    pub fn orthographic_height(&self, template: &Camera) -> f64 {
        2.0 * libm::tan(f64::from(template.vertical_field_of_view) * 0.5) * self.distance
    }

    /// The camera of an orthographic map: the orbit's angles, the picture as high as [`Orbit::orthographic_height`] says, and the eye as far back as that (never nearer than
    /// `ORTHOGRAPHIC_EYE_DISTANCE`): in this view nothing changes with the distance of the eye, so what is zoomed is the box and not where the eye is, and what is
    /// near the focus is in the box whatever the zoom.
    pub fn orthographic_camera(&self, frame: &FocusFrame, template: &Camera) -> Camera {
        let height = self.orthographic_height(template);
        let eye = Self {
            distance: height.max(Self::ORTHOGRAPHIC_EYE_DISTANCE),
            ..*self
        };
        let mut camera = eye.camera(frame, template);
        // (Ahead from the eye: to the focus, and as far past it as the picture is high, and some more for the hills over and under it.)
        camera.projection = Projection::Orthographic {
            height: height as f32,
            depth: (eye.distance + height + 512.0) as f32,
        };
        camera
    }

    /// The camera: `template` (what it shows: the field of view, the nearest plane) placed on the orbit round `frame` and turned to look at its middle, in the focus's frame.
    pub fn camera(&self, frame: &FocusFrame, template: &Camera) -> Camera {
        let (sin_azimuth, cos_azimuth) = (libm::sin(self.azimuth), libm::cos(self.azimuth));
        let (sin_elevation, cos_elevation) = (libm::sin(self.elevation), libm::cos(self.elevation));
        // Back from the middle, in the focus's own axes.
        let own = [
            self.distance * sin_azimuth * cos_elevation,
            self.distance * sin_elevation,
            self.distance * cos_azimuth * cos_elevation,
        ];
        let offset = GridPose::new(frame.centre, frame.orientation).direction_to_world(own);
        let mut camera = *template;
        camera.position = frame.centre.translated(offset);
        // (A camera that looks along -z with these angles in the focus's frame looks back at the middle.)
        camera.yaw = self.azimuth;
        camera.pitch = -self.elevation;
        camera.frame_rotation = Quat::from_array(frame.orientation.components_single());
        camera
    }
}

/// The focus after `current`, `step` places on in the list `focuses` (in order, going round the end; a step back is negative). A focus that is not in the list (the ship it
/// was is out of range) starts the round, as the body, and so does a list with nothing in it.
pub fn next_focus(current: Focus, focuses: &[Focus], step: i32) -> Focus {
    let Some(position) = focuses.iter().position(|focus| *focus == current) else {
        return Focus::Body;
    };
    let count = focuses.len() as i64;
    let next = (position as i64 + i64::from(step)).rem_euclid(count);
    focuses[next as usize]
}

#[cfg(test)]
mod tests;
