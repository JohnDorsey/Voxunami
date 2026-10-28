//! Draws voxel bricks with wgpu.
//!
//! The renderer is built to run on integrated GPUs: meshes are packed 8-byte quads expanded in the
//! vertex shader, bricks outside the view are culled before any GPU work, and distance fog hides
//! the edge of the loaded world so a short view distance looks intentional.
//!
//! Besides bricks it draws avatars: a small figure for each other player, in one of a fixed set of colours.
//!
//! Placement is camera-relative. Each brick's offset from the camera is computed from exact
//! integer brick coordinates, so images are identical wherever in the world the scene is.

mod camera;
mod crosshair;
mod graphics_context;
mod overlay;
mod sun;
mod voxel_renderer;

pub use camera::{Camera, Frustum};
pub use graphics_context::{
    AdapterPreference, GraphicsContext, GraphicsError, describe, new_instance,
};
pub use overlay::OverlayRect;
pub use sun::{
    MAXIMUM_SUN_BALLS, MOST_OF_A_BALL, SUN_DISC_SOFTNESS, SunBall, SunOccluder, Sunlight,
    TURNED_NO_MORE_FROM, TURNED_UP_TO, TWILIGHT_SOFTNESS, balls_and_ground, daylight_at,
    sun_visibility, sunlight,
};
pub use voxel_renderer::{
    AVATAR_COLOR_COUNT, Atmosphere, Avatar, EntityBox, MAXIMUM_GAME_MATERIALS, RenderStatistics,
    VoxelRenderer,
};
