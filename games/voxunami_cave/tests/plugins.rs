//! Cave's plugins together: that they hear of each other's work through events and not by calling
//! one another, which is what lets a plugin the game does not know about take part.

mod common;

use common::{Rig, SEED};
use namespaced_id::NamespacedId;
use plugin_host::{HandlerSpec, Phase, Plugin, Registrar, SystemSpec, Verdict};
use server::GridView;
use voxel_space::{GridId, VoxelCoordinate};
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::request_block;
use voxunami_cave::plugins::{BlockEditRequested, cave_plugins};

const GROUND: i128 = 102;

/// A rig whose server runs Cave's plugins and these others.
fn rig_with(others: Vec<Box<dyn Plugin>>) -> Rig {
    let content = CaveContent::new();
    let world = content.new_overworld_world(SEED);
    let mut plugins = cave_plugins(&content);
    plugins.extend(others);
    Rig::with_plugins(content, world, plugins)
}

struct Scripted {
    id: NamespacedId,
    depends_on: Vec<NamespacedId>,
    register: Box<dyn Fn(&mut Registrar<'_>) + Send + Sync>,
}

impl Plugin for Scripted {
    fn id(&self) -> NamespacedId {
        self.id.clone()
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        self.depends_on.clone()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        (self.register)(registrar);
    }
}

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

fn at(x: i128, y: i128, z: i128) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

fn ticks(rig: &mut Rig, count: usize) {
    for _ in 0..count {
        rig.step();
    }
}

#[test]
fn a_plugin_the_game_does_not_know_can_refuse_an_edit_and_the_player_is_told_why() {
    let lamp_block = CaveContent::new().blocks.lamp;
    let mut rig = rig_with(vec![Box::new(Scripted {
        id: id("test:protection"),
        depends_on: vec![id("voxunami_cave:block_editing")],
        register: Box::new(move |registrar| {
            registrar.on_ask::<BlockEditRequested>(
                HandlerSpec::new("test:no_lamps"),
                move |event, _| {
                    if event.block == lamp_block {
                        Verdict::Refuse("lamps are not allowed here".to_owned())
                    } else {
                        Verdict::Allow
                    }
                },
            );
        }),
    })]);
    let mut player = rig.join([16.5, GROUND as f64 + 4.0, 16.5], 0.0, 0.0);
    ticks(&mut rig, 3);
    player.send_command(&format!("/tp 16.5 {} 16.5", GROUND as f64 + 4.0));
    for _ in 0..3 {
        rig.step();
        player.advance().unwrap();
    }
    // (What the setting up was answered with is not what the test asks about.)
    player.take_command_replies();
    let block_channel = rig.content.block_channel();
    let spot = at(16, GROUND + 1, 16);
    request_block(&mut player, spot, "base_blocks:lamp");
    request_block(&mut player, at(17, GROUND + 1, 16), "base_blocks:stone");
    for _ in 0..5 {
        rig.step();
        player.advance().unwrap();
    }
    let world = rig.server.world();
    assert_eq!(
        world.main().overrides.get(block_channel, spot),
        0,
        "the lamp was refused"
    );
    assert_eq!(
        world
            .main()
            .overrides
            .get(block_channel, at(17, GROUND + 1, 16)),
        rig.content.blocks.stone,
        "the stone was not"
    );
    assert_eq!(
        player.take_command_replies(),
        ["lamps are not allowed here"]
    );
}

#[test]
fn a_fire_that_any_plugin_lights_is_noticed_by_fire_and_light_though_neither_was_called() {
    // A plugin that sets a block on fire in the air, in the same phase as the fire rule, before it.
    let fire_block = CaveContent::new().blocks.fire;
    let block_channel = CaveContent::new().block_channel();
    let spot = at(30, GROUND + 6, 30);
    let mut rig = rig_with(vec![Box::new(Scripted {
        id: id("test:arsonist"),
        depends_on: Vec::new(),
        register: Box::new(move |registrar| {
            let mut done = false;
            registrar.system(
                SystemSpec::new("test:arson", Phase::Rules)
                    .before("voxunami_cave:burn")
                    .writes(&[block_channel]),
                move |context| {
                    if !done {
                        done = true;
                        let mut grid = context.grid(GridId::MAIN).unwrap();
                        grid.set(block_channel, spot, fire_block).unwrap();
                    }
                },
            );
        }),
    })]);
    // A player near enough for fires to burn.
    let mut player = rig.join([30.5, GROUND as f64 + 8.0, 30.5], 0.0, 0.0);
    ticks(&mut rig, 3);
    player.send_command(&format!("/tp 30.5 {} 30.5", GROUND as f64 + 8.0));
    ticks(&mut rig, 3);
    let light_channel = rig.content.light_channel();
    let overrides = |rig: &Rig| rig.server.world().main().overrides.clone();
    let now = overrides(&rig);
    assert_eq!(
        now.get(block_channel, spot),
        fire_block,
        "the fire is there"
    );
    // The light heard of it: a flame gives its light.
    assert!(
        now.get(light_channel, spot) >= u32::from(base_blocks::FIRE_EMISSION) - 2,
        "light {}",
        now.get(light_channel, spot)
    );
    // And the fire rule looked at it, for only that rule makes a fire go out.
    ticks(&mut rig, 600);
    let later = overrides(&rig);
    assert_eq!(later.get(block_channel, spot), rig.content.blocks.air);
    assert_eq!(
        later.get(light_channel, spot),
        0,
        "and the light went out with it"
    );
}
