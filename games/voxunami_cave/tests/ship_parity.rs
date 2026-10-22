//! What a rule does in the terrain it does aboard a ship: the promise of ships is that any block behaviour a game or a mod adds works on board
//! with no effort from its author. Each scenario is written once over a `Site` and run in both places; a rule that is written for one grid and
//! not for the other is a test that fails here.

mod common;

use common::parity::{BOTH, Place, Site};
use common::{FLOOR, Scene, flat_floor, tick_and_follow};
use protocol::Message;
use voxunami_cave::content::CaveContent;
use voxunami_cave::messages::SetBlock;

/// Where the sites are: three above the floor.
fn origin() -> [i64; 3] {
    [4, FLOOR as i64 + 3, 4]
}

/// A plank of wood, 12 by 12 and one thick, with nine fires on it. (One fire sometimes burns out without catching anything: it ages one tick in four
/// and catches a neighbour by a chance, so about one in six dies alone, and which one does depends on the tick it is first looked at, which a loaded
/// machine can move. Nine are sure to start a blaze, and a test that is about whether fires burn must not hang on the luck of one.)
fn burning_plank(scene: &mut Scene, place: Place) -> Site {
    let content = CaveContent::new();
    let site = Site::new(scene, place, origin());
    let mut blocks: Vec<([i64; 3], u32)> = Vec::new();
    for x in 0..12 {
        for z in 0..12 {
            blocks.push(([x, 0, z], content.blocks.wood));
        }
    }
    for x in [2, 6, 10] {
        for z in [2, 6, 10] {
            blocks.push(([x, 1, z], content.blocks.fire));
        }
    }
    site.set(scene, content.block_channel(), blocks);
    site
}

fn plank_and_fire_left(scene: &mut Scene, site: &Site) -> (usize, usize) {
    let content = CaveContent::new();
    let wood = site.count(
        scene,
        content.block_channel(),
        content.blocks.wood,
        [0, 0, 0],
        [12, 1, 12],
    );
    let fire = site.count(
        scene,
        content.block_channel(),
        content.blocks.fire,
        [0, 0, 0],
        [12, 3, 12],
    );
    (wood, fire)
}

#[test]
fn a_fire_burns_a_plank_away_in_the_terrain_and_aboard_a_ship() {
    for place in BOTH {
        let mut scene = Scene::new();
        flat_floor(&mut scene);
        let mut player = scene
            .rig
            .join([10.0, f64::from(FLOOR) + 6.0, 10.0], 0.0, 0.0);
        scene.rig.settle(&mut [&mut player]);
        let site = burning_plank(&mut scene, place);
        tick_and_follow(&mut scene, &mut player, 400);
        let (wood, fire) = plank_and_fire_left(&mut scene, &site);
        assert_eq!(fire, 0, "{place:?}: the fire went out");
        assert!(
            wood <= 14,
            "{place:?}: {wood} of 144 blocks of wood are left"
        );
    }
}

#[test]
fn a_fire_waits_while_no_player_is_near_and_burns_when_one_comes_in_the_terrain_and_aboard_a_ship()
{
    for place in BOTH {
        let mut scene = Scene::new();
        flat_floor(&mut scene);
        // A player far from the fire (more than its range of 64 voxels).
        let mut player = scene
            .rig
            .join([300.0, f64::from(FLOOR) + 6.0, 300.0], 0.0, 0.0);
        scene.rig.settle(&mut [&mut player]);
        let site = burning_plank(&mut scene, place);
        tick_and_follow(&mut scene, &mut player, 150);
        assert_eq!(
            plank_and_fire_left(&mut scene, &site),
            (144, 9),
            "{place:?}: nothing burns with nobody near"
        );
        // The player comes to it: it burns.
        player.send_command(&format!("/tp 10 {} 10", FLOOR as i64 + 6));
        tick_and_follow(&mut scene, &mut player, 400);
        let (wood, fire) = plank_and_fire_left(&mut scene, &site);
        assert_eq!(fire, 0, "{place:?}");
        assert!(wood <= 14, "{place:?}: {wood} left");
    }
}

/// A lamp in the middle of air above a plank of stone, at a place that is whole bricks from the origin (the light works in cells of bricks, so
/// the same offsets must fall in the same cells in both places): the light it makes at some offsets.
fn light_of_a_lamp(place: Place, lamp_stays: bool, ticks_after: usize) -> Vec<u32> {
    let content = CaveContent::new();
    let mut scene = Scene::new();
    let mut player = scene.rig.join([40.0, 232.0, 40.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    let site = Site::new(&mut scene, place, [32, 224, 32]);
    let mut blocks: Vec<([i64; 3], u32)> = Vec::new();
    for x in 0..12 {
        for z in 0..12 {
            blocks.push(([x, 0, z], content.blocks.stone));
        }
    }
    blocks.push(([6, 2, 6], content.blocks.lamp));
    site.set(&mut scene, content.block_channel(), blocks);
    tick_and_follow(&mut scene, &mut player, 30);
    if !lamp_stays {
        site.set(
            &mut scene,
            content.block_channel(),
            vec![([6, 2, 6], content.blocks.air)],
        );
        tick_and_follow(&mut scene, &mut player, ticks_after);
    }
    let offsets = [
        [6, 3, 6],
        [7, 2, 6],
        [6, 2, 9],
        [2, 2, 6],
        [6, 6, 6],
        [10, 4, 10],
        [6, 1, 6],
        [0, 1, 0],
    ];
    site.read(&mut scene, content.light_channel(), &offsets)
}

#[test]
fn a_lamp_lights_the_air_around_it_the_same_in_the_terrain_and_aboard_a_ship() {
    let terrain = light_of_a_lamp(Place::Terrain, true, 0);
    let ship = light_of_a_lamp(Place::Ship, true, 0);
    assert!(
        terrain[0] > 0 && terrain[1] > 0,
        "the lamp lights something: {terrain:?}"
    );
    assert_eq!(ship, terrain);
}

#[test]
fn taking_a_lamp_away_fades_the_light_the_same_in_the_terrain_and_aboard_a_ship() {
    // (The light fades over many ticks rather than going at once: that is how the light of Cave works, in the terrain as well.)
    let lit = light_of_a_lamp(Place::Terrain, true, 0);
    for ticks in [30, 150] {
        let terrain = light_of_a_lamp(Place::Terrain, false, ticks);
        let ship = light_of_a_lamp(Place::Ship, false, ticks);
        assert!(
            terrain.iter().zip(&lit).all(|(now, before)| now < before),
            "{ticks}: {terrain:?} against {lit:?}"
        );
        assert_eq!(ship, terrain, "{ticks} ticks after");
    }
}

#[test]
fn a_player_can_change_a_block_within_reach_and_not_beyond_it_in_the_terrain_and_aboard_a_ship() {
    for place in BOTH {
        let content = CaveContent::new();
        let mut scene = Scene::new();
        flat_floor(&mut scene);
        let mut player = scene
            .rig
            .join([10.0, f64::from(FLOOR) + 6.0, 10.0], 0.0, 0.0);
        scene.rig.settle(&mut [&mut player]);
        let site = Site::new(&mut scene, place, origin());
        site.set(
            &mut scene,
            content.block_channel(),
            vec![
                ([6, 0, 6], content.blocks.stone),
                ([6, 0, 11], content.blocks.stone),
            ],
        );
        // The player is at (10, +6, 10), so the block at offset (6, 0, 6) is 6 away and the one at (6, 0, 11) is 6 as well; the reach is 10.
        // Put them within and beyond it by moving the player: first near, then 40 voxels along x.
        let ask = |scene: &mut Scene, player: &mut common::Player, offset: [i64; 3]| {
            let message = SetBlock {
                grid: site.grid,
                at: site.voxel(offset),
                block: "base_blocks:air".parse().unwrap(),
                state: 0,
            };
            player.send_game_message(SetBlock::kind(), message.encode());
            tick_and_follow(scene, player, 3);
        };
        ask(&mut scene, &mut player, [6, 0, 6]);
        assert_eq!(
            site.read(&mut scene, content.block_channel(), &[[6, 0, 6]]),
            [content.blocks.air],
            "{place:?}: a block within reach is broken"
        );
        player.send_command(&format!("/tp 60 {} 10", FLOOR as i64 + 6));
        tick_and_follow(&mut scene, &mut player, 5);
        ask(&mut scene, &mut player, [6, 0, 11]);
        assert_eq!(
            site.read(&mut scene, content.block_channel(), &[[6, 0, 11]]),
            [content.blocks.stone],
            "{place:?}: a block beyond reach is not"
        );
    }
}
