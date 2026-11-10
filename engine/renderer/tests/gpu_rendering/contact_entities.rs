//! The sun's exact short ray against the boxes of entities (`docs/specs/contact-trace-spec.md`, section 16): an avatar or an item on the ground is no voxel of any grid, so the walk
//! through the lattice cannot find it, and the shadow map leaves the same lit strip at its foot as at a wall's.
//!
//! The nearest few entities are given to the shader as boxes, and a face tests the sun's ray against them analytically. These tests are of the shader's answer for a place on a face
//! (`ContactDebug`) against `world_view::sun_blocked_by_boxes` together with the face's own grid's walk, on random boxes and avatars put in the sun's way, in the terrain and in a grid
//! that is turned; and of the picture: the floor is dark from the very foot of an avatar or an item that stands on it (and is not when the walk leaves entities out, the control),
//! and not darkened on the side of the sun.

use world_view::{SunBox, sun_blocked_by_boxes};

use super::contact_grids::{OtherBlocks, exact_answer_with_the_other, probe_in_the_world};
use super::contact_trace::{
    Mount, Probe, Random, SUNS, Soup, floor_row_beside, occupancy_where, random_probe,
    set_sun_for_the_walk, shader_answer,
};
use super::*;

/// An entity as the tests put it in the world.
#[derive(Clone, Copy, Debug)]
enum Entity {
    /// An avatar whose eyes are here.
    Avatar([f64; 3]),
    /// A box with this middle and these sizes, in eighths of a voxel (made even and put from 2 to 8 as the renderer does).
    Item([f64; 3], [u8; 3]),
}

impl Entity {
    /// The box the reference tests the sun's ray against, worked out here from what an avatar and a box are drawn as.
    fn sun_box(self) -> SunBox {
        match self {
            // Two voxels, the eyes in the middle of the upper one.
            Entity::Avatar(eyes) => {
                SunBox::around([eyes[0], eyes[1] - 0.5, eyes[2]], [0.5, 1.0, 0.5])
            }
            Entity::Item(middle, size) => SunBox::around(
                middle,
                size.map(|size| f64::from(size.clamp(1, 8).div_ceil(2) * 2) / 16.0),
            ),
        }
    }
}

fn set_entities(renderer: &mut VoxelRenderer, entities: &[Entity]) {
    let avatars: Vec<Avatar> = entities
        .iter()
        .filter_map(|entity| match entity {
            Entity::Avatar(eyes) => Some(Avatar {
                position: place_in_voxels(*eyes),
                color: 1,
            }),
            Entity::Item(..) => None,
        })
        .collect();
    let items: Vec<EntityBox> = entities
        .iter()
        .filter_map(|entity| match entity {
            Entity::Item(middle, size) => Some(EntityBox {
                position: place_in_voxels(*middle),
                size: *size,
                material: GREEN,
            }),
            Entity::Avatar(_) => None,
        })
        .collect();
    renderer.set_avatars(&avatars);
    renderer.set_entity_boxes(&items);
}

fn normalised(vector: [f64; 3]) -> [f64; 3] {
    let length = vector.iter().map(|part| part * part).sum::<f64>().sqrt();
    vector.map(|part| part / length)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A random entity at `middle` (for an avatar the middle of its box, so that its eyes are a half over).
fn entity_at(random: &mut Random, middle: [f64; 3]) -> Entity {
    if random.below(2) == 0 {
        Entity::Avatar([middle[0], middle[1] + 0.5, middle[2]])
    } else {
        Entity::Item(middle, std::array::from_fn(|_| 1 + random.below(8) as u8))
    }
}

/// Entities for a probe: one put on the sun's line a random distance from the place (behind it, in the way within the walk's reach, or a good deal past it, and a little to each
/// side), and up to two put anywhere about it.
fn entities_about(
    random: &mut Random,
    place: [f64; 3],
    sun_world: [f64; 3],
    longest: f64,
) -> Vec<Entity> {
    let across = normalised(cross(
        sun_world,
        if sun_world[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        },
    ));
    let other = cross(sun_world, across);
    // (From behind the place, under the face, where the entity is no shade at all, to a good deal past the reach.)
    let along = -0.5 * longest + longest * 1.7 * random.unit();
    let (first, second) = (1.6 * random.unit() - 0.8, 1.6 * random.unit() - 0.8);
    let middle: [f64; 3] = std::array::from_fn(|axis| {
        place[axis] + sun_world[axis] * along + across[axis] * first + other[axis] * second
    });
    let mut entities = vec![entity_at(random, middle)];
    for _ in 0..random.below(3) {
        let anywhere: [f64; 3] =
            std::array::from_fn(|axis| place[axis] + 12.0 * random.unit() - 6.0);
        entities.push(entity_at(random, anywhere));
    }
    entities
}

/// Whether the camera's line to the probe's place is clear of the entities (grown by a third of a voxel): one in the way would hide the face, and the answer is read from its pixel.
fn camera_sees_the_probe(mount: Mount, probe: &Probe, entities: &[Entity]) -> bool {
    let (eye, target) = (mount.to_world(probe.eye), mount.to_world(probe.target));
    let line: [f64; 3] = std::array::from_fn(|axis| target[axis] - eye[axis]);
    let length = line.iter().map(|part| part * part).sum::<f64>().sqrt();
    let grown: Vec<SunBox> = entities
        .iter()
        .map(|entity| {
            let mut found = entity.sun_box();
            for axis in 0..3 {
                found.low[axis] -= 0.34;
                found.high[axis] += 0.34;
            }
            found
        })
        .collect();
    !sun_blocked_by_boxes(eye, line, length + 0.5, &grown)
}

/// Tries random probes at the faces of a soup (mounted as `mount`, in a renderer that is told of it), each with entities about it, for the suns and cell counts given: each probe's
/// answer is the reference's (the face's own grid's walk, or the sun's ray meeting a box within 1.75 voxels of its length for each cell), or the test fails. Returns how many were
/// blocked, how many were clear, and how many the entities decided (blocked by them, and not by the grid).
fn entities_against_the_reference(
    context: &GraphicsContext,
    mount: Mount,
    cells: &[u32],
    suns: &[[f32; 3]],
    probes: usize,
    seed: u64,
) -> (usize, usize, usize) {
    let name = adapter_name(context);
    let mut random = Random(seed);
    let soup = Soup::new(&mut random, 0.22);
    let mut renderer = mount.renderer_with(context, &soup);
    let (mut blocked, mut clear, mut by_entities) = (0, 0, 0);
    for &sun in suns {
        set_sun_for_the_walk(&mut renderer, sun);
        let sun_world = Vec3::from_array(sun).normalize().to_array().map(f64::from);
        let toward_sun = mount.to_local(sun_world);
        for &cell_count in cells {
            renderer.set_contact_debug(ContactDebug::Answer { cells: cell_count });
            let longest = 1.75 * f64::from(cell_count);
            let (mut found, mut attempts) = (0, 0);
            while found < probes {
                attempts += 1;
                assert!(attempts < probes * 800, "{name}: too few probes are robust");
                let probe = random_probe(&mut random, &soup, toward_sun);
                let (place, _) = probe_in_the_world(mount, &probe);
                let entities = entities_about(&mut random, place, sun_world, longest);
                if !camera_sees_the_probe(mount, &probe, &entities) {
                    continue;
                }
                let boxes: Vec<SunBox> = entities.iter().map(|entity| entity.sun_box()).collect();
                let by_the_boxes: OtherBlocks<'_> =
                    &|place, _, sun| sun_blocked_by_boxes(place, sun, longest, &boxes);
                let Some((exact, decided_by_entities)) = exact_answer_with_the_other(
                    (&soup, mount),
                    by_the_boxes,
                    &probe,
                    sun_world,
                    cell_count,
                ) else {
                    continue;
                };
                set_entities(&mut renderer, &entities);
                let shader = shader_answer(&mut renderer, &probe, mount);
                assert_eq!(
                    shader,
                    exact,
                    "{name}: sun {sun:?}, {cell_count} cells, front cell {:?}, place {:?}, entities {entities:?}: the shader says {}, the reference {}{}",
                    probe.front,
                    probe.fraction,
                    if shader { "blocked" } else { "clear" },
                    if exact { "blocked" } else { "clear" },
                    if decided_by_entities {
                        " (by the entities)"
                    } else {
                        ""
                    },
                );
                found += 1;
                if exact {
                    blocked += 1;
                } else {
                    clear += 1;
                }
                if decided_by_entities {
                    by_entities += 1;
                }
            }
        }
    }
    renderer.set_contact_debug(ContactDebug::Off);
    (blocked, clear, by_entities)
}

#[test]
fn the_shader_finds_the_entities_in_the_suns_way_that_the_reference_finds_in_the_terrain() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (blocked, clear, by_entities) = entities_against_the_reference(
            &context,
            Mount::Terrain,
            &[2, 6],
            &SUNS[..9],
            10,
            0xE171,
        );
        assert!(
            blocked >= 40 && clear >= 40 && by_entities >= 25,
            "{name}: {blocked} blocked, {clear} clear, {by_entities} by the entities: the test cannot see the entities"
        );
    }
}

#[test]
fn the_shader_finds_the_entities_in_the_suns_way_that_the_reference_finds_at_a_ship_that_is_turned()
{
    for context in contexts() {
        let name = adapter_name(&context);
        let half = std::f64::consts::FRAC_1_SQRT_2;
        for (turn, orientation) in [
            ("half turn about y", half_turn_about_y()),
            (
                "a turn about a slanted axis",
                Orientation::from_components([0.3, 0.4, 0.0, (1.0f64 - 0.09 - 0.16).sqrt()])
                    .unwrap(),
            ),
            (
                "a quarter turn about y",
                Orientation::from_components([0.0, half, 0.0, half]).unwrap(),
            ),
        ] {
            let (blocked, clear, by_entities) = entities_against_the_reference(
                &context,
                Mount::Grid(orientation),
                &[2, 6],
                &SUNS[..9],
                9,
                0x5418,
            );
            assert!(
                blocked >= 25 && clear >= 25 && by_entities >= 15,
                "{name}, {turn}: {blocked} blocked, {clear} clear, {by_entities} by the entities"
            );
        }
    }
}

/// The floor of the node (one cell thick, its top at y = 1) with nothing on it, in the terrain and in the volume.
fn bare_floor(context: &GraphicsContext) -> VoxelRenderer {
    let mut renderer = new_renderer(context);
    let blocks: Vec<([u8; 3], u32, u32)> = (0..32u8)
        .flat_map(|z| (0..32u8).map(move |x| ([x, 0, z], GREEN, 0)))
        .collect();
    place_blocks(&mut renderer, ORIGIN_BRICK, &blocks, Some(0));
    renderer.set_occupancy(ORIGIN_BRICK, &occupancy_where(|_, y, _| y == 0));
    renderer
}

/// The entities the picture tests stand on the floor with their west faces in the plane x = 14, and how high each is over the floor: an avatar (two voxels), a cube of an item
/// (one), and a slab of an item (a quarter of a voxel).
fn standing_entities() -> [(&'static str, Entity, f64); 3] {
    [
        ("an avatar", Entity::Avatar([14.5, 2.5, 16.0]), 2.0),
        (
            "an item a voxel across",
            Entity::Item([14.5, 1.5, 16.0], [8, 8, 8]),
            1.0,
        ),
        (
            "a flat item",
            Entity::Item([14.5, 1.125, 16.0], [8, 2, 8]),
            0.25,
        ),
    ]
}

const ELEVATIONS: [f64; 5] = [5.0, 15.0, 30.0, 53.0, 75.0];

/// How many pixels of the floor west of the entity's foot are lit that its shadow covers, and how many there were, over the heights of the camera and the suns: the shadow reaches
/// `height / tan(elevation)` from the foot.
fn holes_at_the_foot(renderer: &mut VoxelRenderer, height_of_entity: f64) -> (usize, usize) {
    let (mut holes, mut counted) = (0, 0);
    for camera_height in [20, 60] {
        for elevation in ELEVATIONS {
            let true_end = height_of_entity / super::contact_trace::elevation_tangent(elevation);
            let (lit, found) = floor_row_beside(
                renderer,
                elevation,
                camera_height,
                (14.0, -1.0),
                16.0,
                0.004,
                (true_end - 0.05).min(1.5),
            );
            holes += lit;
            counted += found;
        }
    }
    (holes, counted)
}

#[test]
fn the_floor_is_dark_from_the_very_foot_of_an_entity_that_stands_on_it_and_not_when_the_walk_leaves_entities_out()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        for (what, entity, height) in standing_entities() {
            let mut renderer = bare_floor(&context);
            set_entities(&mut renderer, &[entity]);
            let (holes, counted) = holes_at_the_foot(&mut renderer, height);
            assert!(counted >= 30, "{name}, {what}: {counted} pixels counted");
            assert_eq!(
                holes, 0,
                "{name}, {what}: {holes} of {counted} pixels at its foot are lit"
            );
            // The control: the same, with the walk leaving entities out, has the strip the shadow map leaves.
            renderer.set_entities_in_the_sun_walk(false);
            let (lit_without, _) = holes_at_the_foot(&mut renderer, height);
            assert!(
                lit_without >= 8,
                "{name}, {what}: without the entities in the walk only {lit_without} pixels are lit, the test could not see a gap"
            );
        }
    }
}

#[test]
fn the_floor_on_the_side_of_the_sun_is_as_bright_with_an_entity_standing_on_it_as_without_the_walk()
{
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = bare_floor(&context);
        set_entities(&mut renderer, &[standing_entities()[0].1]);
        // East of the avatar's east face (x = 15) the floor looks at the sun and nothing is between: every pixel is lit.
        for camera_height in [20, 60] {
            for elevation in ELEVATIONS {
                let (lit, counted) = floor_row_beside(
                    &mut renderer,
                    elevation,
                    camera_height,
                    (15.0, 1.0),
                    16.0,
                    0.004,
                    1.5,
                );
                assert!(counted >= 30, "{name}: {counted} pixels counted");
                assert_eq!(
                    lit, counted,
                    "{name}, sun {elevation}, camera {camera_height} up: only {lit} of {counted} pixels on the sun's side of the avatar are lit"
                );
            }
        }
    }
}

#[test]
fn the_walk_has_the_code_for_entities_only_while_some_are_near_and_asked_for() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = bare_floor(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.0, 12.0, 24.0]);
        renderer.set_atmosphere(shadow_atmosphere(
            0,
            super::contact_trace::sun_at(30.0),
            true,
        ));
        let walked = |renderer: &mut VoxelRenderer| {
            let statistics = renderer.render_to_rgba(&camera, SIZE, SIZE).1;
            (statistics.traced_windows, statistics.walked_entities)
        };
        assert_eq!(walked(&mut renderer), (1, false), "{name}: no entities");
        set_entities(&mut renderer, &[standing_entities()[0].1]);
        assert_eq!(walked(&mut renderer), (1, true), "{name}: an avatar");
        renderer.set_entities_in_the_sun_walk(false);
        assert_eq!(walked(&mut renderer), (1, false), "{name}: left out");
        renderer.set_entities_in_the_sun_walk(true);
        assert_eq!(walked(&mut renderer), (1, true), "{name}: asked for again");
        set_entities(&mut renderer, &[]);
        assert_eq!(walked(&mut renderer), (1, false), "{name}: gone");
    }
}
