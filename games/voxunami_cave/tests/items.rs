//! Items on the ground and in inventories, and the game modes, played through a server and a session.

mod common;

use common::{Player, PlayerExt, Rig, SEED, above_ground, ground_height};
use entities::{Inventory, ItemStack};
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use player_presence::{PLAYER_INVENTORY_SLOTS, PlayerClient, player_kind};
use plugin_host::{Argument, CommandSpec, Plugin, Registrar};
use voxel_space::VoxelCoordinate;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::request_block;
use voxunami_cave::items::{FALL_INTERVAL_TICKS, dropped_item_kind, voxel_of};
use voxunami_cave::plugins::cave_plugins;

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

/// One server tick and the player taking it in.
fn tick(rig: &mut Rig, player: &mut Player, count: usize) {
    for _ in 0..count {
        rig.step();
        player.advance().unwrap();
        let _ = player.update_view();
    }
}

/// A player hovering `height` above the ground at the column (16, 16), looking down, and the voxel of that
/// ground.
fn hovering(rig: &mut Rig, height: f32) -> (Player, VoxelCoordinate) {
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, height);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    let ground = ground_height(&player, 16, 16);
    (player, VoxelCoordinate::new(16, ground, 16))
}

/// What each item on the ground is and where, in order of identifier.
fn on_the_ground(rig: &Rig) -> Vec<(VoxelCoordinate, ItemStack)> {
    rig.server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == dropped_item_kind())
        .map(|entity| {
            (
                voxel_of(entity.pose.position),
                entity.components.get::<ItemStack>().unwrap().unwrap(),
            )
        })
        .collect()
}

/// The inventory of the first player there is.
fn inventory(rig: &Rig) -> Inventory {
    rig.server
        .world()
        .entities()
        .iter()
        .find(|entity| entity.kind == player_kind())
        .expect("a player has an entity")
        .components
        .get::<Inventory>()
        .unwrap()
        .unwrap()
}

fn reply_to(rig: &mut Rig, player: &mut Player, command: &str) -> Vec<String> {
    player.send_command(command);
    tick(rig, player, 2);
    player.take_command_replies()
}

/// The identifier of the block the player sees in a voxel.
fn block_at(rig: &Rig, player: &Player, voxel: VoxelCoordinate) -> NamespacedId {
    let raw = player.mesher().material_at(voxel).unwrap();
    rig.content.registries()[0]
        .1
        .identifier_of_raw(raw)
        .unwrap()
        .clone()
}

#[test]
fn worlds_are_creative_until_told_otherwise_and_the_mode_is_asked_and_set_with_a_command() {
    let mut rig = Rig::new();
    let (mut player, _) = hovering(&mut rig, 1.5);
    assert_eq!(
        reply_to(&mut rig, &mut player, "/gamemode"),
        ["this world is in creative mode"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/gamemode survival"),
        ["this world is now in survival mode"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/gamemode"),
        ["this world is in survival mode"]
    );
    for wrong in ["/gamemode adventure", "/gamemode survival now"] {
        assert_eq!(
            reply_to(&mut rig, &mut player, wrong),
            ["usage: /gamemode [creative|survival]"],
            "{wrong}"
        );
    }
    assert_eq!(
        reply_to(&mut rig, &mut player, "/gamemode creative"),
        ["this world is now in creative mode"]
    );
}

#[test]
fn in_creative_nothing_is_dropped_and_nothing_is_used_up() {
    let mut rig = Rig::new();
    let (mut player, ground) = hovering(&mut rig, 1.5);
    request_block(&mut player, ground, "base_blocks:air");
    tick(&mut rig, &mut player, 12);
    assert_eq!(on_the_ground(&rig), []);
    assert_eq!(inventory(&rig), Inventory::new(PLAYER_INVENTORY_SLOTS));
    // And a block can be placed with nothing in the inventory.
    request_block(&mut player, ground, "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    assert_eq!(block_at(&rig, &player, ground), id("base_blocks:stone"));
    assert_eq!(inventory(&rig), Inventory::new(PLAYER_INVENTORY_SLOTS));
}

#[test]
fn in_survival_a_block_broken_near_the_player_goes_straight_into_their_inventory() {
    let mut rig = Rig::new();
    let (mut player, ground) = hovering(&mut rig, 1.5);
    reply_to(&mut rig, &mut player, "/gamemode survival");
    let broken = block_at(&rig, &player, ground);
    request_block(&mut player, ground, "base_blocks:air");
    tick(&mut rig, &mut player, 3);
    assert_eq!(block_at(&rig, &player, ground), id("base_blocks:air"));
    assert_eq!(on_the_ground(&rig), [], "picked up, so not on the ground");
    let held = inventory(&rig);
    assert_eq!(held.slot(0), Some(&ItemStack::new(broken.clone(), 1)));
    assert_eq!(held.count_of(&broken), 1);
}

#[test]
fn in_survival_an_item_out_of_reach_of_the_player_lies_there_falls_and_joins_another_and_waits_for_them()
 {
    let mut rig = Rig::new();
    // Eight above the ground: out of pickup range, within reach of the player's hand.
    let (mut player, ground) = hovering(&mut rig, 8.0);
    let at = |height: i64| VoxelCoordinate::new(ground.x(), ground.y() + height, ground.z());
    // Two blocks of stone, one on the other, built in creative, and then broken from the top, in survival.
    request_block(&mut player, at(1), "base_blocks:stone");
    request_block(&mut player, at(2), "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    reply_to(&mut rig, &mut player, "/gamemode survival");
    request_block(&mut player, at(2), "base_blocks:air");
    tick(&mut rig, &mut player, 2);
    // The upper one is on the ground where it was broken, and nothing is in the inventory.
    let lying = on_the_ground(&rig);
    assert_eq!(
        lying,
        [(at(2), ItemStack::new(id("base_blocks:stone"), 1))],
        "the block that is broken leaves its item where it was"
    );
    assert_eq!(inventory(&rig), Inventory::new(PLAYER_INVENTORY_SLOTS));
    // The stone under it holds it up. Break that, and it falls into the voxel the lower one was in, where they
    // join. (How soon depends on which tick it is, so the test waits more than a whole turn of the fall.)
    request_block(&mut player, at(1), "base_blocks:air");
    tick(&mut rig, &mut player, FALL_INTERVAL_TICKS as usize * 3);
    let lying = on_the_ground(&rig);
    let [(rests, stack)] = &lying[..] else {
        panic!("joined into one stack: {lying:?}");
    };
    assert_eq!(*stack, ItemStack::new(id("base_blocks:stone"), 2));
    assert_eq!(
        *rests,
        at(1),
        "on the ground, which is where the lower stone was"
    );
    assert_eq!(inventory(&rig), Inventory::new(PLAYER_INVENTORY_SLOTS));

    // Different items in one voxel do not join: break the ground under the stone, and a different block's item
    // is in the voxel it falls into.
    request_block(&mut player, at(0), "base_blocks:air");
    tick(&mut rig, &mut player, FALL_INTERVAL_TICKS as usize * 3);
    let lying = on_the_ground(&rig);
    assert_eq!(lying.len(), 2, "{lying:?}");
    assert!(lying.iter().all(|(voxel, _)| *voxel == at(0)), "{lying:?}");
    assert_ne!(lying[0].1.item, lying[1].1.item);

    // The player comes down to them, and they come to the player: both together.
    player.camera.position = voxunami_cave::items::centre_of(at(2));
    player.teleport_to_camera();
    tick(&mut rig, &mut player, 6);
    assert_eq!(on_the_ground(&rig), []);
    assert_eq!(inventory(&rig).count_of(&id("base_blocks:stone")), 2);
}

#[test]
fn in_survival_placing_needs_the_item_and_uses_it_up() {
    let mut rig = Rig::new();
    let (mut player, ground) = hovering(&mut rig, 1.5);
    let stone = id("base_blocks:stone");
    let at = |height: i64, along: i64| {
        VoxelCoordinate::new(ground.x() + along, ground.y() + height, ground.z())
    };

    // Survival, nothing in the inventory: refused, with the reason, and nothing is placed.
    reply_to(&mut rig, &mut player, "/gamemode survival");
    request_block(&mut player, at(1, 0), "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    assert_eq!(player.take_command_replies(), ["you have no stone"]);
    assert_eq!(block_at(&rig, &player, at(1, 0)), id("base_blocks:air"));

    // Some stone, got by building it in creative and breaking it in survival.
    reply_to(&mut rig, &mut player, "/gamemode creative");
    request_block(&mut player, at(1, 0), "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    reply_to(&mut rig, &mut player, "/gamemode survival");
    request_block(&mut player, at(1, 0), "base_blocks:air");
    tick(&mut rig, &mut player, 3);
    assert_eq!(inventory(&rig).count_of(&stone), 1);

    // With one: placed, and used up; a second is refused.
    request_block(&mut player, at(1, 0), "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    assert_eq!(player.take_command_replies(), Vec::<String>::new());
    assert_eq!(block_at(&rig, &player, at(1, 0)), stone);
    assert_eq!(inventory(&rig).count_of(&stone), 0);
    request_block(&mut player, at(1, 1), "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    assert_eq!(player.take_command_replies(), ["you have no stone"]);
    assert_eq!(block_at(&rig, &player, at(1, 1)), id("base_blocks:air"));
}

/// A plugin for tests: `/fill N M` puts a full stack of something that is not stone in the first N slots of the
/// caller's inventory, so that there is no room, and M stone (if M is not 0) in the slot after them.
#[derive(Debug)]
struct Filler;

impl Plugin for Filler {
    fn id(&self) -> NamespacedId {
        id("test:filler")
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        vec![id("player_presence:players")]
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        registrar.command(
            CommandSpec::new("fill", "fills slots")
                .argument(Argument::number("slots"))
                .argument(Argument::number("stone")),
            move |context, client, arguments| {
                let count = arguments.number(0) as usize;
                let stone = arguments.number(1) as u32;
                let entity = context
                    .world()
                    .entities()
                    .iter()
                    .find(|entity| {
                        entity.kind == player_kind()
                            && entity.components.get::<PlayerClient>()
                                == Ok(Some(PlayerClient(client)))
                    })
                    .map(|entity| entity.id);
                let Some(entity) = entity else {
                    return;
                };
                let slots = (0..PLAYER_INVENTORY_SLOTS)
                    .map(|slot| {
                        if slot < count {
                            Some(ItemStack::new(id("base_blocks:wood"), 64))
                        } else if slot == count && stone > 0 {
                            Some(ItemStack::new(id("base_blocks:stone"), stone))
                        } else {
                            None
                        }
                    })
                    .collect();
                let _ = context.set_component(entity, &Inventory::from_slots(slots));
            },
        );
    }
}

#[test]
fn an_item_that_does_not_fit_stays_on_the_ground_and_a_partly_fitting_one_leaves_the_rest() {
    let content = CaveContent::new();
    let world = content.new_world(SEED);
    let mut plugins = cave_plugins(&content);
    plugins.push(Box::new(Filler));
    let mut rig = Rig::with_plugins(content, world, plugins);
    let (mut player, ground) = hovering(&mut rig, 1.5);
    reply_to(&mut rig, &mut player, "/gamemode survival");

    // Every slot full of wood: a stone broken lies where it was broken.
    reply_to(&mut rig, &mut player, "/fill 9 0");
    let broken = block_at(&rig, &player, ground);
    request_block(&mut player, ground, "base_blocks:air");
    tick(&mut rig, &mut player, 6);
    let lying = on_the_ground(&rig);
    assert_eq!(lying.len(), 1, "{lying:?}");
    assert_eq!(lying[0].1, ItemStack::new(broken.clone(), 1));
    assert_eq!(inventory(&rig).count_of(&broken), 0);
    assert_eq!(inventory(&rig).count_of(&id("base_blocks:wood")), 9 * 64);

    // A free slot: it is picked up.
    reply_to(&mut rig, &mut player, "/fill 8 0");
    tick(&mut rig, &mut player, 3);
    assert_eq!(on_the_ground(&rig), []);
    assert_eq!(inventory(&rig).count_of(&broken), 1);
}

/// The world a rig's server holds, saved and loaded again into a new rig.
fn reloaded(rig: Rig) -> Rig {
    let Rig {
        content, server, ..
    } = rig;
    let world = server.into_world();
    let bytes = world.save(&content.registries()).unwrap();
    let mut content = CaveContent::new();
    let parsed = world_storage::ParsedWorld::parse(&bytes).unwrap();
    let seed = parsed.world_seed();
    let (loaded, report) = parsed.load(content.load_context(seed)).unwrap();
    assert!(report.extra_channels.is_empty() && report.missing_content.is_empty());
    Rig::with_world(content, loaded)
}

#[test]
fn the_game_mode_and_the_items_on_the_ground_are_saved_with_the_world() {
    let mut rig = Rig::new();
    let (mut player, ground) = hovering(&mut rig, 8.0);
    let at = |height: i64| VoxelCoordinate::new(ground.x(), ground.y() + height, ground.z());
    request_block(&mut player, at(1), "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    reply_to(&mut rig, &mut player, "/gamemode survival");
    request_block(&mut player, at(1), "base_blocks:air");
    tick(&mut rig, &mut player, 2);
    let lying = on_the_ground(&rig);
    assert_eq!(lying.len(), 1);
    player.say_goodbye();
    rig.step();

    let mut again = reloaded(rig);
    assert_eq!(on_the_ground(&again), lying, "it is still there");
    let mut player = again.join([16.5, 110.0, 16.5], 0.0, 0.0);
    again.settle(&mut [&mut player]);
    player.take_command_replies();
    assert_eq!(
        reply_to(&mut again, &mut player, "/gamemode"),
        ["this world is in survival mode"]
    );
}

#[test]
fn a_world_that_is_in_creative_when_saved_comes_back_in_creative_having_saved_nothing_of_the_mode()
{
    let mut rig = Rig::new();
    let (mut player, _) = hovering(&mut rig, 1.5);
    // Survival and back again: the world is as if it had never been told.
    reply_to(&mut rig, &mut player, "/gamemode survival");
    reply_to(&mut rig, &mut player, "/gamemode creative");
    player.say_goodbye();
    rig.step();
    let Rig {
        content, server, ..
    } = rig;
    let world = server.into_world();
    assert_eq!(
        voxunami_cave::items::read_game_mode(world.opaque_sections()),
        None
    );
    let _ = content;
}

#[test]
fn the_screen_says_how_many_of_the_selected_block_a_player_has_in_survival_only() {
    let mut rig = Rig::new();
    let (mut player, ground) = hovering(&mut rig, 1.5);
    let above = VoxelCoordinate::new(ground.x(), ground.y() + 1, ground.z());
    assert_eq!(player.hud_text(), "placing stone (whole)");

    reply_to(&mut rig, &mut player, "/gamemode survival");
    assert_eq!(player.hud_text(), "placing stone (whole), 0 in hand");

    // Some stone: built in creative, broken in survival.
    reply_to(&mut rig, &mut player, "/gamemode creative");
    assert_eq!(player.hud_text(), "placing stone (whole)");
    request_block(&mut player, above, "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    reply_to(&mut rig, &mut player, "/gamemode survival");
    request_block(&mut player, above, "base_blocks:air");
    tick(&mut rig, &mut player, 3);
    assert_eq!(player.hud_text(), "placing stone (whole), 1 in hand");
}

#[test]
fn a_player_who_joins_is_told_the_game_mode_the_world_is_in() {
    let mut rig = Rig::new();
    let (mut first, _) = hovering(&mut rig, 1.5);
    reply_to(&mut rig, &mut first, "/gamemode survival");
    let mut second = rig.join([20.5, 110.0, 20.5], 0.0, 0.0);
    rig.settle(&mut [&mut second, &mut first]);
    assert_eq!(second.hud_text(), "placing stone (whole), 0 in hand");
    reply_to(&mut rig, &mut first, "/gamemode creative");
    rig.settle(&mut [&mut second, &mut first]);
    assert_eq!(
        second.hud_text(),
        "placing stone (whole)",
        "told when it changes"
    );
}

#[test]
fn an_item_on_the_ground_is_drawn_as_a_small_box_of_its_blocks_colour_standing_on_the_floor_of_its_voxel()
 {
    let mut rig = Rig::new();
    // Out of reach of pickup, so that the items stay where they are.
    let (mut player, ground) = hovering(&mut rig, 8.0);
    let stone_at = VoxelCoordinate::new(ground.x(), ground.y() + 1, ground.z());
    let dirt_at = VoxelCoordinate::new(ground.x() + 3, ground.y() + 1, ground.z());
    assert_eq!(player.entity_boxes(), [], "nothing lies there yet");
    request_block(&mut player, stone_at, "base_blocks:stone");
    request_block(&mut player, dirt_at, "base_blocks:dirt");
    tick(&mut rig, &mut player, 3);
    reply_to(&mut rig, &mut player, "/gamemode survival");
    request_block(&mut player, stone_at, "base_blocks:air");
    request_block(&mut player, dirt_at, "base_blocks:air");
    tick(&mut rig, &mut player, FALL_INTERVAL_TICKS as usize * 2);

    let number = |name: &str| {
        rig.content.registries()[0]
            .1
            .raw_id_of(&id(name))
            .expect("the game has the block")
    };
    let (stone, dirt) = (number("base_blocks:stone"), number("base_blocks:dirt"));
    assert_ne!(stone, dirt);
    let boxes = player.entity_boxes();
    assert_eq!(boxes.len(), 2, "{boxes:?}");
    // (In order of the entities: the stone was broken first.)
    assert_eq!([boxes[0].material, boxes[1].material], [stone, dirt]);
    for (drawn, voxel) in boxes.iter().zip([stone_at, dirt_at]) {
        assert_eq!(drawn.size, [4, 4, 4]);
        // In the middle of its voxel along the ground, and a quarter of a voxel above the floor of it.
        assert_eq!(voxunami_cave::items::voxel_of(drawn.position), voxel);
        let offset = drawn.position.offset();
        assert_eq!(
            [offset[0].fract(), offset[1].fract(), offset[2].fract()],
            [0.5, 0.25, 0.5]
        );
    }
}

#[test]
fn a_stack_that_fits_in_part_is_taken_in_part_and_the_rest_stays_on_the_ground() {
    let content = CaveContent::new();
    let world = content.new_world(SEED);
    let mut plugins = cave_plugins(&content);
    plugins.push(Box::new(Filler));
    let mut rig = Rig::with_plugins(content, world, plugins);
    let (mut player, ground) = hovering(&mut rig, 8.0);
    let at = |height: i64| VoxelCoordinate::new(ground.x(), ground.y() + height, ground.z());
    let stone = id("base_blocks:stone");
    // Two stones, built in creative and broken in survival, out of reach of picking up: they join on the ground.
    request_block(&mut player, at(1), "base_blocks:stone");
    request_block(&mut player, at(2), "base_blocks:stone");
    tick(&mut rig, &mut player, 3);
    reply_to(&mut rig, &mut player, "/gamemode survival");
    request_block(&mut player, at(2), "base_blocks:air");
    request_block(&mut player, at(1), "base_blocks:air");
    tick(&mut rig, &mut player, FALL_INTERVAL_TICKS as usize * 3);
    assert_eq!(
        on_the_ground(&rig),
        [(at(1), ItemStack::new(stone.clone(), 2))]
    );

    // Eight slots of wood, and 63 stone in the ninth: room for one more stone, and no more.
    reply_to(&mut rig, &mut player, "/fill 8 63");
    player.camera.position = voxunami_cave::items::centre_of(at(2));
    player.teleport_to_camera();
    tick(&mut rig, &mut player, 6);
    assert_eq!(inventory(&rig).count_of(&stone), 64, "one more fitted");
    assert_eq!(
        on_the_ground(&rig),
        [(at(1), ItemStack::new(stone.clone(), 1))],
        "and one is left, where it was"
    );
}
