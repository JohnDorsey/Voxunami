//! PROTOTYPE (branch graphics-ao-from-volume-prototype): the corner shade of a quad worked out by the fragment shader from the voxels that block the sun
//! (`volume_shade_level` in `contact_on.wgsl`) must give the picture the corner levels the mesher works out give (`voxel_meshing::corner_occlusion`).

use super::contact_trace::occupancy_where;
use super::*;

/// The brightness of every pixel, as linear brightnesses.
fn brightnesses(image: &[u8]) -> Vec<f64> {
    (0..SIZE)
        .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
        .map(|(x, y)| linear_brightness(pixel(image, x, y)))
        .collect()
}

/// A floor of stone (up to y = 15) with one block on it at (16, 16, 16) and another one two cells away that makes a step with it, meshed with the sky all round, drawn
/// from above with the shade of `how`: the mesher's corner levels (`Vertex`), none (`Nothing`), or the shader's from the voxels (`Volume`, which is the mesher's quads
/// with the flag on every one, whatever their corner levels are: the shader does not read them).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum How {
    Vertex,
    Nothing,
    Volume,
}

fn picture(context: &GraphicsContext, name: &str, how: How, camera: &Camera) -> Vec<f64> {
    let mut renderer = new_renderer(context);
    let solid = |x: usize, y: usize, z: usize| {
        y < 16
            || [
                (16, 16, 16),
                (17, 16, 16),
                (17, 17, 16),
                (12, 16, 12),
                (14, 16, 12),
            ]
            .contains(&(x, y, z))
    };
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    for voxel in LocalVoxelIndex::all() {
        if solid(voxel.x().into(), voxel.y().into(), voxel.z().into()) {
            values[voxel.index()] = RED;
        }
    }
    let storage = ChannelStorage::from_values(&values);
    let mut opaque = vec![true; 256];
    opaque[0] = false;
    let mut mesh = mesh_brick(
        &BrickNeighbourhood::with_face_neighbours(BrickChannels::blocks_only(&storage), [None; 6]),
        &BlockTable::of_cubes(&opaque),
    )
    .pack();
    for quad in &mut mesh.quads {
        match how {
            How::Vertex => {}
            How::Nothing => quad.shade = 0,
            How::Volume => quad.shade = 1 << 31,
        }
    }
    renderer.upload_brick(ORIGIN_BRICK, &mesh);
    renderer.set_occupancy(ORIGIN_BRICK, &occupancy_where(solid));
    renderer.set_atmosphere(Atmosphere {
        sun_direction: [0.0, 1.0, 0.0],
        sun_light: 120,
        ambient_light: 120,
        shadows: true,
        ..Atmosphere::default()
    });
    let image = render(&mut renderer, camera);
    save_image(&format!("volume_shade_{how:?}"), name, &image);
    brightnesses(&image)
}

#[test]
fn the_shader_makes_the_shade_the_mesher_makes_of_the_voxels() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut camera = camera_at(ORIGIN_BRICK, [15.0, 26.0, 14.0]);
        camera.pitch = -Camera::MAXIMUM_PITCH;
        let by_mesher = picture(&context, &name, How::Vertex, &camera);
        let by_shader = picture(&context, &name, How::Volume, &camera);
        let none = picture(&context, &name, How::Nothing, &camera);
        let worst = by_mesher
            .iter()
            .zip(&by_shader)
            .fold(0.0f64, |worst, (a, b)| worst.max((a - b).abs()));
        let shaded_by = by_mesher
            .iter()
            .zip(&none)
            .fold(0.0f64, |most, (a, b)| most.max((a - b).abs()));
        assert!(
            shaded_by > 0.05,
            "{name}: the scene has shade to compare ({shaded_by})"
        );
        assert!(
            worst < 0.01,
            "{name}: the shader's shade differs from the mesher's by {worst} of a brightness the shade changes by {shaded_by}"
        );
    }
}
