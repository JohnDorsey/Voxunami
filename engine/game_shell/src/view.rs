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
    renderer.set_atmosphere(atmosphere(view_radius_bricks, content.ambient_light()));
}

/// The sky and fog for a view of this radius, with faces no light reaches drawn at `ambient_light`.
pub fn atmosphere(view_radius_bricks: i64, ambient_light: u8) -> Atmosphere {
    let reach = (view_radius_bricks * 32) as f32;
    Atmosphere {
        fog_start: reach * 0.6,
        fog_end: reach - 16.0,
        ambient_light,
        ..Atmosphere::default()
    }
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

/// Draws the entities the game's plugins give an appearance, each as a box.
pub fn show_entity_boxes<C: Connection, G: GameDefinition>(
    renderer: &mut VoxelRenderer,
    session: &Session<C, G>,
) {
    renderer.set_entity_boxes(&session.entity_boxes());
}

/// The voxel a position is in, as its three coordinates in binary: `x 0b1011 y 0b110 z -0b101`. A
/// negative coordinate is its size in binary with a minus sign (not in two's complement).
pub fn binary_voxel_coordinates(position: WorldPosition) -> String {
    let brick = position.brick();
    let offset = position.offset();
    let coordinates = [
        brick.x() * 32 + offset[0].floor() as i64,
        brick.y() * 32 + offset[1].floor() as i64,
        brick.z() * 32 + offset[2].floor() as i64,
    ];
    let binary = |value: i64| {
        let sign = if value < 0 { "-" } else { "" };
        format!("{sign}0b{:b}", value.unsigned_abs())
    };
    format!(
        "x {} y {} z {}",
        binary(coordinates[0]),
        binary(coordinates[1]),
        binary(coordinates[2])
    )
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

    #[test]
    fn coordinates_are_the_voxel_the_position_is_in_in_binary() {
        let binary = |position: [f64; 3]| {
            binary_voxel_coordinates(camera_at(position, 0.0, 0.0, 12).position)
        };
        assert_eq!(binary([0.0, 0.0, 0.0]), "x 0b0 y 0b0 z 0b0");
        assert_eq!(binary([11.9, 6.0, 5.5]), "x 0b1011 y 0b110 z 0b101");
        // Across bricks: 32 + 5 = 100101, 110 = 0b1101110.
        assert_eq!(
            binary([37.2, 110.0, 64.0]),
            "x 0b100101 y 0b1101110 z 0b1000000"
        );
        // The voxel is the one below, so -0.5 is voxel -1 and -1.0 is too.
        assert_eq!(binary([-0.5, -1.0, -32.0]), "x -0b1 y -0b1 z -0b100000");
        assert_eq!(binary([-1.5, -2.0, -33.0]), "x -0b10 y -0b10 z -0b100001");
    }
}
