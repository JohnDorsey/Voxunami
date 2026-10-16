//! Glue between a session and the renderer.

use renderer::{Atmosphere, Avatar, Camera, VoxelRenderer};
use transport::Connection;
use voxel_space::{BrickCoordinate, WorldPosition};
use world_view::MeshUpdate;

use crate::definition::{GameContent, GameDefinition};
use crate::session::Session;

/// Palette and fog suited to the view distance: the world fades into the sky just before it ends.
pub fn configure_renderer(
    renderer: &mut VoxelRenderer,
    content: &impl GameContent,
    view_radius_bricks: i64,
) {
    renderer.set_palette(&content.block_table().palette());
    let reach = (view_radius_bricks * 32) as f32;
    renderer.set_atmosphere(Atmosphere {
        fog_start: reach * 0.6,
        fog_end: reach - 16.0,
        ambient_light: content.ambient_light(),
        ..Atmosphere::default()
    });
}

pub fn apply_updates(renderer: &mut VoxelRenderer, updates: Vec<MeshUpdate>) {
    for update in updates {
        match update {
            MeshUpdate::Upload { coordinate, mesh } => renderer.upload_brick(coordinate, &mesh),
            MeshUpdate::Remove(coordinate) => renderer.remove_brick(coordinate),
        }
    }
}

/// Draws the other players of a session: each as an avatar where their eyes are, in a colour of their own.
pub fn show_other_players<C: Connection, G: GameDefinition>(
    renderer: &mut VoxelRenderer,
    session: &Session<C, G>,
) {
    let avatars: Vec<Avatar> = session
        .other_players()
        .filter(|_| G::shows_other_players())
        .map(|(client, pose)| Avatar {
            position: pose.position,
            color: client.get(),
        })
        .collect();
    renderer.set_avatars(&avatars);
}

/// A camera at a position given in voxels, with angles in degrees.
pub fn camera_at(
    position: [f64; 3],
    yaw_degrees: f32,
    pitch_degrees: f32,
    view_radius_bricks: i64,
) -> Camera {
    let brick = BrickCoordinate::new(
        (position[0].floor() as i64).div_euclid(32),
        (position[1].floor() as i64).div_euclid(32),
        (position[2].floor() as i64).div_euclid(32),
    );
    let offset = [
        (position[0] - (brick.x() * 32) as f64) as f32,
        (position[1] - (brick.y() * 32) as f64) as f32,
        (position[2] - (brick.z() * 32) as f64) as f32,
    ];
    let mut camera = Camera::new(WorldPosition::new(brick, offset));
    camera.yaw = yaw_degrees.to_radians();
    camera.pitch = pitch_degrees.to_radians();
    camera.far_plane = (view_radius_bricks * 32 * 2) as f32;
    camera
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_lands_in_the_right_brick_and_offset() {
        let camera = camera_at([-1.0, 110.5, 70.0], 90.0, -30.0, 12);
        assert_eq!(camera.position.brick(), BrickCoordinate::new(-1, 3, 2));
        assert_eq!(camera.position.offset(), [31.0, 14.5, 6.0]);
        assert!((camera.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((camera.pitch + std::f32::consts::FRAC_PI_6).abs() < 1e-6);
        assert_eq!(camera.far_plane, 768.0);
    }
}
