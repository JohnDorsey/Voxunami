//! What a replaced package makes the server's bodies do again (`hot-reload-plan.md`, step 5): a ship is made again only if it holds a block that behaves differently now;
//! a reload that only repaints makes nothing again; what rests on the ground still does; and what is made again is what a server started with the new package makes.

mod common;

use common::docking::{make_going, stones};
use common::{
    FLOOR, Scene, changed_package, flat_floor, original_package, pose_at, ship_id, ship_voxels,
};
use namespaced_id::NamespacedId;
use server::GridView;
use voxel_space::{GravitySettings, GridId, LevelIndex, VoxelCoordinate};
use voxunami_cave::content::CaveContent;

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

/// Makes a ship of 3 by 3 by 3 of one block, with the origin at `pose`, in a tick of its own.
fn make_ship_of(scene: &mut Scene, at: [f64; 3], held: bool, block: fn(&CaveContent) -> u32) {
    let content = CaveContent::new();
    let (block_number, channel) = (block(&content), content.block_channel());
    scene.run(move |world| {
        let grid = world
            .create_grid(LevelIndex::MAIN, pose_at(at[0], at[1], at[2]))
            .unwrap();
        world.set_grid_held(grid, held).unwrap();
        let mut view = world.grid(grid).unwrap();
        for x in 0..3 {
            for y in 0..3 {
                for z in 0..3 {
                    view.set(channel, VoxelCoordinate::new(x, y, z), block_number)
                        .unwrap();
                }
            }
        }
    });
}

fn stone(content: &CaveContent) -> u32 {
    content.blocks.stone
}

fn dirt(content: &CaveContent) -> u32 {
    content.blocks.dirt
}

/// A floor, and four ships resting on it: two of stone (numbers 0 and 2) and two of dirt (1 and 3).
fn ships_at_rest() -> (Scene, impl Fn() -> grid_bodies::BodiesReport) {
    let (mut scene, report) = Scene::reporting();
    flat_floor(&mut scene);
    for (number, (block, x)) in [
        (stone as fn(&CaveContent) -> u32, 10.0),
        (dirt, 20.0),
        (stone, 30.0),
        (dirt, 40.0),
    ]
    .into_iter()
    .enumerate()
    {
        let _ = number;
        make_ship_of(&mut scene, [x, FLOOR, 30.0], false, block);
    }
    scene.ticks(80);
    (scene, report)
}

fn mass_of(scene: &Scene, number: u64) -> f64 {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .unwrap()
        .mass
}

fn height_of(scene: &Scene, number: u64) -> f64 {
    ship_voxels(scene, number)[1]
}

/// Dirt twice as dense as it was (the density is in kilograms a voxel, 1000 by default).
fn dirt_is_twice_as_dense() -> game_shell::PackageData {
    changed_package(|name, definition| {
        if name == "base_blocks:dirt" {
            definition.set_property_value(id("engine:density"), None, 2000);
        }
    })
}

#[test]
fn a_reload_that_only_repaints_makes_nothing_again_and_moves_nothing() {
    let (mut scene, report) = ships_at_rest();
    let before = (
        report(),
        (0..4).map(|n| mass_of(&scene, n)).collect::<Vec<_>>(),
    );
    let heights: Vec<f64> = (0..4).map(|n| height_of(&scene, n)).collect();
    assert!(before.0.ship_bricks_made >= 4, "the ships were made once");

    scene
        .package
        .replace(changed_package(|_, definition| {
            definition.color = [1.0, 0.0, 0.0]
        }))
        .unwrap();
    scene.ticks(20);
    assert_eq!(scene.package.version(), 1);
    let after = (
        report(),
        (0..4).map(|n| mass_of(&scene, n)).collect::<Vec<_>>(),
    );
    assert_eq!(
        after.0.ship_bricks_made, before.0.ship_bricks_made,
        "no ship was made again"
    );
    assert_eq!(
        after.0.terrain_bricks_made, before.0.terrain_bricks_made,
        "nor the ground"
    );
    assert_eq!(after.1, before.1);
    for number in 0..4 {
        assert_eq!(height_of(&scene, number), heights[number as usize]);
    }
}

#[test]
fn a_change_of_dirt_makes_the_ships_that_hold_dirt_again_and_no_other_and_the_ground_holds_them_up()
{
    let (mut scene, report) = ships_at_rest();
    let before = report();
    let masses: Vec<f64> = (0..4).map(|n| mass_of(&scene, n)).collect();
    let heights: Vec<f64> = (0..4).map(|n| height_of(&scene, n)).collect();
    assert_eq!(masses[0], masses[2]);
    assert_eq!(masses[1], masses[3]);

    scene.package.replace(dirt_is_twice_as_dense()).unwrap();
    scene.ticks(80);
    let after = report();
    assert_eq!(
        after.ship_bricks_made - before.ship_bricks_made,
        2,
        "the one brick of each of the two ships of dirt"
    );
    assert_eq!(mass_of(&scene, 0), masses[0], "stone weighs what it did");
    assert_eq!(mass_of(&scene, 2), masses[2]);
    for number in [1, 3] {
        assert!(
            (mass_of(&scene, number) - 2.0 * masses[number as usize]).abs()
                < 1e-9 * masses[number as usize],
            "dirt weighs twice what it did"
        );
    }
    // (The colliders of the ground are made again, a few a tick, and what rests on it goes on resting.)
    for number in 0..4 {
        assert!(
            (height_of(&scene, number) - heights[number as usize]).abs() < 0.25,
            "ship {number} stays on the floor: {} then {}",
            heights[number as usize],
            height_of(&scene, number)
        );
    }
}

#[test]
fn a_ship_that_holds_nothing_that_changed_falls_through_ground_that_became_passable() {
    // A ship of dirt on a floor of stone: the ship is not made again (it holds no stone), so what lets it fall is that the colliders of the ground are.
    let (mut scene, report) = Scene::reporting();
    flat_floor(&mut scene);
    make_ship_of(&mut scene, [30.0, FLOOR, 30.0], false, dirt);
    scene.ticks(60);
    let resting = height_of(&scene, 0);
    assert!(
        (FLOOR - 0.5..FLOOR + 0.5).contains(&resting),
        "rests on the floor: {resting}"
    );
    let made = report().ship_bricks_made;

    scene
        .package
        .replace(changed_package(|name, definition| {
            if name == "base_blocks:stone" {
                definition.set_property_value(id("base_blocks:passable"), None, 1);
            }
        }))
        .unwrap();
    scene.ticks(120);
    assert_eq!(
        report().ship_bricks_made,
        made,
        "the ship was not made again"
    );
    let fallen = height_of(&scene, 0);
    assert!(
        fallen < resting - 5.0,
        "fell through the floor: {resting} then {fallen}"
    );
}

#[test]
fn what_is_made_again_is_what_a_server_that_started_with_the_new_package_makes() {
    let (mut scene, _) = ships_at_rest();
    scene.package.replace(dirt_is_twice_as_dense()).unwrap();
    scene.ticks(5);

    // The same ships in a server whose package was this from the start.
    let mut fresh = Scene::new();
    fresh.package.replace(dirt_is_twice_as_dense()).unwrap();
    flat_floor(&mut fresh);
    for (block, x) in [
        (stone as fn(&CaveContent) -> u32, 10.0),
        (dirt, 20.0),
        (stone, 30.0),
        (dirt, 40.0),
    ] {
        make_ship_of(&mut fresh, [x, FLOOR, 30.0], false, block);
    }
    fresh.ticks(5);
    for number in 0..4 {
        let (remade, started) = (
            scene
                .rig
                .server
                .world()
                .grids()
                .get(ship_id(number))
                .unwrap(),
            fresh
                .rig
                .server
                .world()
                .grids()
                .get(ship_id(number))
                .unwrap(),
        );
        assert_eq!(remade.mass, started.mass, "mass of ship {number}");
        assert_eq!(
            remade.centre_of_mass, started.centre_of_mass,
            "centre of mass of ship {number}"
        );
    }
}

/// Where a stone ship that was made going at 3 voxels a second in weightless space is after 80 ticks, with a ship of dirt beside it and the density of dirt changed at
/// tick 20 (or not changed at all).
fn where_a_going_ship_of_stone_is(dirt_changes: bool) -> [f64; 3] {
    let mut scene = Scene::new();
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0; 3],
            ..GravitySettings::default()
        });
    });
    make_going(
        &mut scene,
        pose_at(30.0, FLOOR + 30.0, 30.0),
        stones(0, 4),
        [3.0, 0.0, 1.0],
    );
    make_ship_of(&mut scene, [30.0, FLOOR + 60.0, 30.0], true, dirt);
    for tick in 0..80 {
        if dirt_changes && tick == 20 {
            scene.package.replace(dirt_is_twice_as_dense()).unwrap();
        }
        scene.ticks(1);
    }
    ship_voxels(&scene, 0)
}

#[test]
fn a_ship_that_holds_nothing_that_changed_goes_on_exactly_as_it_would_have() {
    let undisturbed = where_a_going_ship_of_stone_is(false);
    let with_dirt_changed = where_a_going_ship_of_stone_is(true);
    assert!(undisturbed[0] > 30.0 + 3.0, "it went: {undisturbed:?}");
    // (Not made again, so not a bit of it differs; a ship that is made again from the world's place for it is within 1e-6 of a voxel.)
    assert_eq!(undisturbed, with_dirt_changed);
}

#[test]
fn a_chain_of_cars_coupled_by_joints_that_hold_what_changed_is_made_again_and_stays_coupled_and_goes_on_round()
 {
    use common::chains::{a_chain, a_chain_in_orbit, centre_of, mass_of, minus};
    use common::{circular_speed, length};

    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    scene.ticks(20);
    assert_eq!(report().couplings, 2, "coupled");
    let (weight, (middle, going)) = (mass_of(&scene, &ships), centre_of(&scene, &ships));

    // Stone twice as dense: every car holds stone, and the cars are coupled, so the whole of the bodies is made again.
    scene
        .package
        .replace(changed_package(|name, definition| {
            if name == "base_blocks:stone" {
                definition.set_property_value(id("engine:density"), None, 2000);
            }
        }))
        .unwrap();
    scene.ticks(40);
    let heavier = mass_of(&scene, &ships);
    assert!(
        heavier > 1.5 * weight && heavier < 2.0 * weight + 1.0,
        "it weighs what its stone weighs now: {weight} then {heavier} (the joints are not stone)"
    );
    assert_eq!(report().couplings, 2, "still coupled");
    // It went on round: the speed is what it was (what a car weighs is not what pulls it), and in the forty ticks (two seconds) since it has gone as far as its speed says,
    // to within a twentieth.
    let (now, speed_now) = centre_of(&scene, &ships);
    assert!(
        (length(speed_now) - length(going)).abs() < 1.0,
        "{going:?} then {speed_now:?}"
    );
    let gone = length(minus(now, middle));
    assert!(
        (gone - 2.0 * speed).abs() < 0.05 * 2.0 * speed,
        "went {gone} voxels in 2 seconds at about {speed} a second"
    );
}

#[test]
fn a_reload_that_only_repaints_does_not_light_the_world_again_and_one_that_changes_what_a_block_does_does()
 {
    // (Lighting the world again clears every light and has every lamp look again: it is what a change to what a block gives off or lets through needs, and a visit to
    // every voxel of every ship. A light written where no lamp gives it is a mark that is there until something clears it.)
    let mut scene = Scene::new();
    let content = CaveContent::new();
    let (channel, spot) = (content.light_channel(), VoxelCoordinate::new(5, 150, 5));
    scene.run(move |world| {
        world
            .grid(GridId::root(LevelIndex::MAIN))
            .unwrap()
            .set(channel, spot, 7)
            .unwrap();
    });
    scene.ticks(3);
    let mark = |scene: &Scene| scene.rig.server.world().main().overrides.get(channel, spot);
    assert_eq!(mark(&scene), 7, "the mark is there");

    scene
        .package
        .replace(changed_package(|_, definition| {
            definition.color = [1.0, 0.0, 0.0]
        }))
        .unwrap();
    scene.ticks(3);
    assert_eq!(mark(&scene), 7, "repainted: the light was not made again");

    scene
        .package
        .replace(changed_package(|name, definition| {
            if name == "base_blocks:lamp" {
                definition.set_property_value(id("base_blocks:emission"), None, 10);
            }
        }))
        .unwrap();
    scene.ticks(3);
    assert_eq!(mark(&scene), 0, "a lamp that gives other light: it was");
}

#[test]
fn cars_whose_joint_block_stopped_being_a_joint_are_no_longer_coupled() {
    use common::chains::{a_chain, a_chain_in_orbit};
    use common::circular_speed;

    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    scene.ticks(20);
    assert_eq!(report().couplings, 2, "coupled");

    // The blocks that made the joints are blocks that make none.
    scene
        .package
        .replace(changed_package(|name, definition| {
            if name == "voxunami_cave:dev_ball_joint" {
                definition.remove_property_value(&id("engine:joint_face"), None);
            }
        }))
        .unwrap();
    scene.ticks(40);
    assert_eq!(report().couplings, 0, "no joint, no coupling");
    // And the cars are still in the world, free of one another.
    for ship in &ships {
        assert!(scene.rig.server.world().grids().get(*ship).is_some());
    }
}

#[test]
fn a_package_put_back_as_it_was_after_a_change_makes_the_ships_that_hold_what_changed_again_once_more()
 {
    let (mut scene, report) = ships_at_rest();
    scene.package.replace(dirt_is_twice_as_dense()).unwrap();
    scene.ticks(10);
    let between = report().ship_bricks_made;
    scene.package.replace(original_package()).unwrap();
    scene.ticks(10);
    assert_eq!(report().ship_bricks_made - between, 2);
    assert_eq!(mass_of(&scene, 1), mass_of(&scene, 3));
    assert_eq!(mass_of(&scene, 0), mass_of(&scene, 2));
    let _ = GridId::root(LevelIndex::MAIN);
}
