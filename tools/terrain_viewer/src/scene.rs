//! What the viewer shows: terrain, its colours, and how streamed meshes reach the renderer.

use std::sync::Arc;

use renderer::{Atmosphere, Camera, VoxelRenderer};
use terrain::{RollingHillsSettings, TerrainGenerator, TerrainMaterials, rolling_hills};
use voxel_grid::{ChannelDefinition, ChannelIndex, GridLayout};
use voxel_space::{BrickCoordinate, WorldPosition};

use crate::options::Options;
use crate::streaming::{MeshUpdate, StreamingSettings, WorldStreamer};

const MATERIALS: TerrainMaterials = TerrainMaterials {
    air: 0,
    stone: 1,
    dirt: 2,
    grass: 3,
};

/// sRGB colours for the material values above, in value order.
const PALETTE: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [0.50, 0.50, 0.53],
    [0.45, 0.30, 0.18],
    [0.30, 0.62, 0.20],
];

pub fn new_streamer(options: &Options) -> WorldStreamer {
    let layout = Arc::new(
        GridLayout::new(vec![ChannelDefinition {
            identifier: "viewer:material"
                .parse()
                .expect("constant identifier is valid"),
            default_value: MATERIALS.air,
        }])
        .expect("a single channel is a valid layout"),
    );
    let material: ChannelIndex = layout
        .channel_index(
            &"viewer:material"
                .parse()
                .expect("constant identifier is valid"),
        )
        .expect("the channel was just defined");
    let generator: Arc<dyn TerrainGenerator> = Arc::new(rolling_hills(
        RollingHillsSettings::for_one_metre_voxels(options.seed),
        material,
        MATERIALS,
    ));
    let worker_count = std::thread::available_parallelism()
        .map_or(2, |count| count.get().saturating_sub(1).max(2));
    WorldStreamer::new(
        StreamingSettings {
            view_radius_bricks: options.view_radius,
            worker_count,
        },
        layout,
        material,
        MATERIALS.air,
        generator,
    )
}

/// Palette and fog suited to the view distance: the world fades into the sky just before it ends.
pub fn configure_renderer(renderer: &mut VoxelRenderer, options: &Options) {
    renderer.set_palette(&PALETTE);
    let reach = (options.view_radius * 32) as f32;
    renderer.set_atmosphere(Atmosphere {
        fog_start: reach * 0.6,
        fog_end: reach - 16.0,
        ..Atmosphere::default()
    });
}

pub fn apply_updates(renderer: &mut VoxelRenderer, updates: Vec<MeshUpdate>) {
    for update in updates {
        match update {
            MeshUpdate::Upload { coordinate, quads } => renderer.upload_brick(coordinate, &quads),
            MeshUpdate::Remove(coordinate) => renderer.remove_brick(coordinate),
        }
    }
}

/// The starting camera from the command line (position in voxels, angles in degrees).
pub fn initial_camera(options: &Options) -> Camera {
    let [x, y, z] = options.camera;
    let voxel = |value: f64| value.floor() as i64;
    let brick = BrickCoordinate::new(
        voxel(x).div_euclid(32),
        voxel(y).div_euclid(32),
        voxel(z).div_euclid(32),
    );
    let offset = [
        (x - (brick.x() * 32) as f64) as f32,
        (y - (brick.y() * 32) as f64) as f32,
        (z - (brick.z() * 32) as f64) as f32,
    ];
    let mut camera = Camera::new(WorldPosition::new(brick, offset));
    camera.yaw = options.yaw_degrees.to_radians();
    camera.pitch = options.pitch_degrees.to_radians();
    camera.far_plane = (options.view_radius * 32 * 2) as f32;
    camera
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starting_camera_lands_in_the_right_brick_and_offset() {
        let options = Options {
            camera: [-1.0, 110.5, 70.0],
            yaw_degrees: 90.0,
            pitch_degrees: -30.0,
            ..Options::default()
        };
        let camera = initial_camera(&options);
        assert_eq!(camera.position.brick(), BrickCoordinate::new(-1, 3, 2));
        assert_eq!(camera.position.offset(), [31.0, 14.5, 6.0]);
        assert!((camera.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((camera.pitch + std::f32::consts::FRAC_PI_6).abs() < 1e-6);
    }
}
