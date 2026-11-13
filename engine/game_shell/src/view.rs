//! Glue between a session and the renderer.

use block_models::BlockTable;
use data_packs::Tile;
use renderer::{Atmosphere, Avatar, Camera, OverlayRect, PictureSize, ShaderPack, VoxelRenderer};
use transport::Connection;
use voxel_space::WorldPosition;
use world_view::{MeshUpdate, OccupancyChange};

use crate::definition::GameDefinition;
use crate::options::ViewOptions;
use crate::package_data::PackageData;
use crate::session::Session;

/// Palette (with the pictures of the blocks that have them) and fog suited to the view distance: the world fades into the sky just before it ends.
pub fn configure_renderer(
    renderer: &mut VoxelRenderer,
    block_table: &BlockTable,
    package: &PackageData,
    view_radius_bricks: i128,
) {
    renderer.set_palette_with_pictures(
        &block_table.palette(),
        &block_table.palette_pictures(),
        &picture_sizes(&package.tile_classes),
    );
    renderer.set_atmosphere(atmosphere(view_radius_bricks, package.ambient_light));
    // (The shapes the voxels that block the sun in part refer to: `docs/specs/contact-trace-spec.md`, section 13.)
    renderer.set_sun_shapes(&block_table.shape_cells());
}

/// Reads the shader packs the player has (the game data folder's, and the folder `--shader-packs` names), tells the session of them and of the one `--shader-pack` chose, and
/// returns them for [`apply_shader_pack`]. A pack that is no good is a warning on the error stream and the others are kept; a folder asked for that cannot be read, and a pack
/// asked for that is not there, are errors.
pub fn prepare_shader_packs<C: Connection, G: GameDefinition>(
    view: &ViewOptions,
    session: &mut Session<C, G>,
) -> Result<Vec<ShaderPack>, String> {
    let shelf = crate::shader_packs::shelf_of(
        crate::shader_packs::default_directory().as_deref(),
        view.shader_packs.as_deref(),
    )?;
    for problem in &shelf.problems {
        eprintln!("warning: shader pack: {problem}");
    }
    let names = shelf.names();
    let chosen = match &view.shader_pack {
        Some(text) => Some(
            crate::shader_packs::pick(&names, text)
                .map(|index| names[index].id.clone())
                .map_err(|error| format!("--shader-pack: {error}"))?,
        ),
        None => None,
    };
    session.offer_shader_packs(names, chosen);
    Ok(shelf.packs)
}

/// Gives the renderer the shader pack the player's settings choose (or the game's own): at once if it is the one the renderer has. An error says the graphics card refused it,
/// and the renderer draws as it did; the caller takes the pack off the settings (`Session::reject_shader_pack`).
pub fn apply_shader_pack(
    renderer: &mut VoxelRenderer,
    packs: &[ShaderPack],
    settings: &crate::client_commands::ClientSettings,
) -> Result<(), String> {
    let wanted = settings
        .shader_pack
        .as_ref()
        .and_then(|id| packs.iter().find(|pack| pack.id() == id));
    renderer.set_shader_pack(wanted).map_err(|error| {
        format!(
            "the shader pack {} is refused:\n{error}",
            wanted.map_or("", ShaderPack::name)
        )
    })
}

/// The pictures of a game's blocks as the renderer takes them: a size at a time, smallest first, which is the order the table's picture words count the sizes in.
pub fn picture_sizes(tile_classes: &[Vec<Tile>]) -> Vec<PictureSize<'_>> {
    tile_classes
        .iter()
        .map(|tiles| PictureSize {
            size: tiles
                .first()
                .expect("a size of picture has pictures (a package is read with at least one)")
                .size(),
            tiles: tiles.iter().map(|tile| tile.rgba()).collect(),
            specular: if tiles.iter().any(|tile| tile.specular().is_some()) {
                tiles.iter().map(|tile| tile.specular()).collect()
            } else {
                Vec::new()
            },
            normal: if tiles.iter().any(|tile| tile.normal().is_some()) {
                tiles.iter().map(|tile| tile.normal()).collect()
            } else {
                Vec::new()
            },
        })
        .collect()
}

/// The sky and fog for a view of this radius, with faces no light reaches drawn at `ambient_light`.
pub fn atmosphere(view_radius_bricks: i128, ambient_light: u8) -> Atmosphere {
    let reach = (view_radius_bricks * 32) as f32;
    Atmosphere {
        fog_start: reach * 0.6,
        fog_end: reach - 16.0,
        ambient_light,
        ..Atmosphere::default()
    }
}

/// Lights the next pictures: the sky and fog, the ambient light the player has set, and, if the game has a sun in the level the player is in, the sun, the sky's light and
/// what is in the sun's way. Done in every frame: the sun moves with the world's clock and the colour of the sky with the camera.
pub fn light_the_scene<C: Connection, G: GameDefinition>(
    renderer: &mut VoxelRenderer,
    session: &Session<C, G>,
) {
    let (lit, occluders) = lit_atmosphere(session);
    renderer.set_sun_occluders(&occluders);
    renderer.set_atmosphere(lit);
    renderer.set_picture_distance(session.settings().texture_distance as f32);
    renderer.set_pixelated_shadows(session.settings().pixelated_shadows);
}

/// What [`light_the_scene`] gives the renderer: the atmosphere, and what can be in the sun's way.
pub fn lit_atmosphere<C: Connection, G: GameDefinition>(
    session: &Session<C, G>,
) -> (Atmosphere, Vec<renderer::SunOccluder>) {
    let mut lit = atmosphere(
        session.fog_radius_bricks(),
        session.settings().ambient_light,
    );
    let Some(sun) = session.sun_view() else {
        return (lit, Vec::new());
    };
    lit.sky_color = sun.sky_color;
    lit.sun_direction = sun.direction;
    // (The lamps light a wall as they did at every time of the day: the sides of faces are shaded from one place.)
    lit.side_direction = Some(Atmosphere::default().sun_direction);
    lit.sun_light = sun.sun_light;
    lit.sky_light = sun.sky_light;
    // (The sun is what the shadows are of: a world with none has no shadows to ask for.)
    lit.shadows = session.settings().shadows;
    lit.sky_light_in_shadow = sun.sky_light_in_shadow;
    lit.sky_light_under_cover = sun.sky_light_under_cover;
    lit.night_light = sun.night_light;
    lit.heavens = sun.heavens;
    (lit, sun.occluders)
}

/// Puts the screen over the next pictures: lays it out for a picture of `size` pixels (at the whole-number scale that suits its
/// height), paints it as rectangles and gives them to the renderer. `focus` is the button that has the keyboard focus. Returns the
/// layout, for finding what a click at a pixel is on.
pub fn show_screen(
    renderer: &mut VoxelRenderer,
    screen: &crate::screen::Screen,
    size: (u32, u32),
    focus: Option<u32>,
) -> ui::Layout {
    let laid = ui::layout(
        &screen.uis,
        (size.0 as i32, size.1 as i32),
        ui::scale_for_height(size.1),
        focus,
    );
    let rectangles: Vec<OverlayRect> = ui::paint(&laid)
        .into_iter()
        .map(|painted| OverlayRect {
            x: painted.area.x,
            y: painted.area.y,
            width: painted.area.width,
            height: painted.area.height,
            color: painted.color,
        })
        .collect();
    renderer.set_overlay(&rectangles);
    laid
}

pub fn apply_updates(renderer: &mut VoxelRenderer, updates: Vec<MeshUpdate>) {
    for update in updates {
        match update {
            MeshUpdate::Upload { coordinate, mesh } => renderer.upload_brick(coordinate, &mesh),
            MeshUpdate::Remove(coordinate) => renderer.remove_brick(coordinate),
            MeshUpdate::UploadGridBrick {
                grid,
                coordinate,
                mesh,
            } => renderer.upload_grid_brick(grid, coordinate, &mesh),
            MeshUpdate::RemoveGridBrick { grid, coordinate } => {
                renderer.remove_grid_brick(grid, coordinate);
            }
            MeshUpdate::GridPose { grid, pose } => renderer.set_grid_pose(grid, pose),
            MeshUpdate::GridIsGround(grid) => renderer.set_grid_is_ground(grid, true),
            MeshUpdate::RemoveGrid(grid) => renderer.remove_grid(grid),
            MeshUpdate::UploadCoarse { cube, mesh } => renderer.upload_coarse_node(cube, &mesh),
            MeshUpdate::RemoveCoarse(cube) => renderer.remove_coarse_node(cube),
            MeshUpdate::UploadGridCoarse { grid, cube, mesh } => {
                renderer.upload_grid_coarse_node(grid, cube, &mesh);
            }
            MeshUpdate::RemoveGridCoarse { grid, cube } => {
                renderer.remove_grid_coarse_node(grid, cube);
            }
        }
    }
}

/// Tells the renderer which voxels block the sun, for the walk of the sun's ray through them (`docs/specs/contact-trace-spec.md`): what the session's meshers have said of
/// their bricks since the last call, for the terrain of the level, for the planets and for the ships. Done after [`apply_updates`], so that a brick's occupancy arrives with its mesh
/// or later, and before a picture is drawn.
pub fn apply_occupancy<C: Connection, G: GameDefinition>(
    renderer: &mut VoxelRenderer,
    session: &mut Session<C, G>,
) {
    let terrain = session.terrain_grid();
    for (grid, coordinate, change) in session.take_occupancy_updates() {
        if grid == terrain {
            match change {
                OccupancyChange::Set(occupancy) => renderer.set_occupancy(coordinate, &occupancy),
                OccupancyChange::Clear => renderer.clear_occupancy(coordinate),
            }
        } else {
            // A planet or a ship: each grid has a window of its own, and a face is walked in the windows of the others too (`docs/specs/contact-trace-spec.md`, section 14).
            match change {
                OccupancyChange::Set(occupancy) => {
                    renderer.set_grid_occupancy(grid, coordinate, &occupancy);
                }
                OccupancyChange::Clear => renderer.clear_grid_occupancy(grid, coordinate),
            }
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

/// The voxel a position is in, as its three coordinates in hexadecimal: `x 0x1f y 0x6e z -0x5`. A negative coordinate is its size in hexadecimal with a minus sign (not in
/// two's complement). The letter and its number are joined by a no-break space ([`ui::NO_BREAK_SPACE`]), so that where the screen wraps the line it breaks between the
/// coordinates and not inside one.
pub fn hexadecimal_voxel_coordinates(position: WorldPosition) -> String {
    let brick = position.brick();
    let offset = position.offset();
    let coordinates = [
        brick.x() * 32 + offset[0].floor() as i128,
        brick.y() * 32 + offset[1].floor() as i128,
        brick.z() * 32 + offset[2].floor() as i128,
    ];
    let hexadecimal = |value: i128| {
        let sign = if value < 0 { "-" } else { "" };
        format!("{sign}0x{:x}", value.unsigned_abs())
    };
    ["x", "y", "z"]
        .into_iter()
        .zip(coordinates)
        .map(|(axis, value)| format!("{axis}{}{}", ui::NO_BREAK_SPACE, hexadecimal(value)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A camera at a position given in voxels, with angles in degrees.
pub fn camera_at(position: [f64; 3], yaw_degrees: f64, pitch_degrees: f64) -> Camera {
    let mut camera = Camera::new(WorldPosition::from_voxels(position));
    camera.yaw = yaw_degrees.to_radians();
    camera.pitch = pitch_degrees.to_radians();
    camera
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    #[test]
    fn the_pictures_of_a_package_go_to_the_renderer_a_size_at_a_time_smallest_first_unchanged() {
        let tile = |size: u32, byte: u8| {
            Tile::from_rgba(size, vec![byte; size as usize * size as usize * 4])
        };
        let classes = vec![
            vec![tile(8, 1), tile(8, 2)],
            vec![tile(16, 3)],
            vec![tile(64, 4)],
        ];
        let sizes = picture_sizes(&classes);
        assert_eq!(
            sizes
                .iter()
                .map(|size| (size.size, size.tiles.len()))
                .collect::<Vec<_>>(),
            [(8, 2), (16, 1), (64, 1)]
        );
        assert_eq!(sizes[0].tiles[1], classes[0][1].rgba());
        assert_eq!(sizes[2].tiles[0], classes[2][0].rgba());
        assert!(picture_sizes(&[]).is_empty());
        assert!(
            sizes.iter().all(|size| size.specular.is_empty()),
            "no companions, none handed on"
        );
    }

    #[test]
    fn the_specular_pictures_of_a_package_go_with_their_pictures_and_a_picture_without_one_has_none()
     {
        let plain = |size: u32, byte: u8| {
            Tile::from_rgba(size, vec![byte; size as usize * size as usize * 4])
        };
        let shining = plain(8, 2).with_specular(vec![7; 8 * 8 * 4]);
        let classes = vec![vec![plain(8, 1), shining], vec![plain(16, 3)]];
        let sizes = picture_sizes(&classes);
        assert_eq!(
            sizes[0].specular.len(),
            2,
            "one for each picture of the size"
        );
        assert_eq!(sizes[0].specular[0], None);
        assert_eq!(sizes[0].specular[1], Some(&[7u8; 8 * 8 * 4][..]));
        assert!(
            sizes[1].specular.is_empty(),
            "a size none of whose pictures has one hands on none"
        );
    }

    #[test]
    fn the_normal_pictures_of_a_package_go_with_their_pictures_beside_the_specular_ones() {
        let plain = |size: u32, byte: u8| {
            Tile::from_rgba(size, vec![byte; size as usize * size as usize * 4])
        };
        let bumpy = plain(8, 2).with_normal(vec![9; 8 * 8 * 4]);
        let both = plain(16, 3)
            .with_specular(vec![4; 16 * 16 * 4])
            .with_normal(vec![5; 16 * 16 * 4]);
        let classes = vec![vec![plain(8, 1), bumpy], vec![both]];
        let sizes = picture_sizes(&classes);
        assert_eq!(sizes[0].normal.len(), 2, "one for each picture of the size");
        assert_eq!(sizes[0].normal[0], None);
        assert_eq!(sizes[0].normal[1], Some(&[9u8; 8 * 8 * 4][..]));
        assert!(
            sizes[0].specular.is_empty(),
            "a size with normal pictures only has no specular ones"
        );
        assert_eq!(sizes[1].normal[0], Some(&[5u8; 16 * 16 * 4][..]));
        assert_eq!(sizes[1].specular[0], Some(&[4u8; 16 * 16 * 4][..]));
    }

    #[test]
    fn the_camera_lands_in_the_right_brick_and_offset() {
        let camera = camera_at([-1.0, 110.5, 70.0], 90.0, -30.0);
        assert_eq!(camera.position.brick(), BrickCoordinate::new(-1, 3, 2));
        assert_eq!(camera.position.offset(), [31.0, 14.5, 6.0]);
        assert!((camera.yaw - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((camera.pitch + std::f64::consts::FRAC_PI_6).abs() < 1e-12);
    }

    #[test]
    fn coordinates_are_the_voxel_the_position_is_in_in_hexadecimal() {
        // (Shown with plain spaces: the one between a letter and its number does not let a line break there.)
        let hexadecimal = |position: [f64; 3]| {
            hexadecimal_voxel_coordinates(camera_at(position, 0.0, 0.0).position)
                .replace(ui::NO_BREAK_SPACE, " ")
        };
        assert_eq!(hexadecimal([0.0, 0.0, 0.0]), "x 0x0 y 0x0 z 0x0");
        assert_eq!(hexadecimal([11.9, 6.0, 5.5]), "x 0xb y 0x6 z 0x5");
        // Across bricks: 32 + 5 = 0x25, 110 = 0x6e, 64 = 0x40.
        assert_eq!(hexadecimal([37.2, 110.0, 64.0]), "x 0x25 y 0x6e z 0x40");
        // The voxel is the one below, so -0.5 is voxel -1 and -1.0 is too.
        assert_eq!(hexadecimal([-0.5, -1.0, -32.0]), "x -0x1 y -0x1 z -0x20");
        assert_eq!(hexadecimal([-1.5, -2.0, -33.0]), "x -0x2 y -0x2 z -0x21");
    }

    #[test]
    fn coordinates_a_hundred_and_twenty_bits_wide_are_whole_and_break_only_between_coordinates() {
        // 2^120 - 1, the edge of the world, and its negative: thirty digits each.
        let edge: i128 = (1i128 << 120) - 1;
        let brick = BrickCoordinate::new(edge.div_euclid(32), -edge.div_euclid(32), 0);
        let position = WorldPosition::new(brick, [31.0, 0.0, 0.0]);
        let shown = hexadecimal_voxel_coordinates(position);
        let plain = shown.replace(ui::NO_BREAK_SPACE, " ");
        assert_eq!(plain, format!("x 0x{:x} y -0x{:x} z 0x0", edge, edge - 31));
        // Wrapped to a line of 40 letters it breaks at the spaces between coordinates: every line starts with a letter and its number, and none ends with a letter.
        let lines = ui::wrap_text(&shown, 40);
        assert!(lines.len() >= 2, "{lines:?}");
        for line in &lines {
            assert!(
                line.starts_with("x 0x") || line.starts_with("y -0x") || line.starts_with("z 0x"),
                "{line:?}"
            );
            assert!(
                !line.ends_with(" x") && !line.ends_with(" y") && !line.ends_with(" z"),
                "{line:?}"
            );
        }
    }
}
