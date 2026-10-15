//! The invariants this crate exists for, checked against a small pretend game whose plugins can be
//! switched on and off between a save and a load.

use std::sync::Arc;

use proptest::prelude::*;
use voxel_grid::{Brick, ChannelDefinition};
use voxel_space::VoxelCoordinate;
use world_storage::container::{read_container, write_container};
use world_storage::test_support::Plugin::*;
use world_storage::test_support::{
    ALL_PLUGINS, Description, Game, Plugin, block_kind, brick, comparable, id, sample_world,
    spell_kind,
};
use world_storage::{
    BrickSource, ContainerError, LoadContext, LoadError, ParsedWorld, Section, TranslationError,
};

fn round_trip(bytes: &[u8], plugins: &[Plugin]) -> Vec<u8> {
    let mut game = Game::new(plugins);
    let (world, _) = game.load(bytes).unwrap();
    game.save(&world)
}

fn described(bytes: &[u8], plugins: &[Plugin]) -> Description {
    let mut game = Game::new(plugins);
    let (world, _) = game.load(bytes).unwrap();
    game.describe(&world)
}

#[test]
fn a_world_loaded_with_the_same_plugins_is_identical_and_saves_to_the_same_bytes() {
    let game = Game::new(&ALL_PLUGINS);
    let original = sample_world(&game);
    let bytes = game.save(&original);

    let mut again = Game::new(&ALL_PLUGINS);
    let (loaded, report) = again.load(&bytes).unwrap();

    assert_eq!(report.missing_content, []);
    assert_eq!(report.extra_channels, []);
    assert_eq!(report.unknown_registry_kinds, []);
    assert_eq!(report.unavailable_default_generator, None);
    assert_eq!(
        report.opaque_sections,
        [id("magic:spellbook"), id("unheard_of:data")]
    );
    assert_eq!(again.describe(&loaded), game.describe(&original));
    assert_eq!(again.save(&loaded), bytes);
}

#[test]
fn loading_without_plugins_loses_nothing_and_saving_again_brings_everything_back() {
    let game = Game::new(&ALL_PLUGINS);
    let original = game.describe(&sample_world(&game));
    let bytes = game.save(&sample_world(&game));

    // Only the base blocks remain: no newcomer blocks, fancy blocks, magic channels, or fancy terrain.
    let mut reduced = Game::new(&[BaseBlocks]);
    let (partial, report) = reduced.load(&bytes).unwrap();
    assert_eq!(
        report.missing_namespaces().into_iter().collect::<Vec<_>>(),
        ["fancy_blocks", "fancy_terrain", "newcomer"]
    );
    assert_eq!(report.extra_channels, [id("magic:mana"), id("magic:spell")]);
    assert_eq!(report.unknown_registry_kinds, [spell_kind()]);
    assert_eq!(
        report.unavailable_default_generator,
        Some(id("fancy_terrain:cliffs_v1"))
    );
    assert!(report.changed_channel_defaults.is_empty());

    // The missing content is present as placeholders, not silently turned into something else.
    let marble = reduced
        .blocks
        .runtime_id(&id("fancy_blocks:marble"))
        .unwrap();
    assert!(reduced.blocks.entry(marble).unwrap().is_missing());
    assert!(
        matches!(partial.effective_brick(brick(100, 0, 0)), BrickSource::MissingGenerator(name) if name == id("fancy_terrain:cliffs_v1"))
    );
    assert!(matches!(
        partial.effective_brick(brick(0, 0, 0)),
        BrickSource::Edited(_)
    ));

    let resaved = reduced.save(&partial);
    assert_eq!(
        comparable(described(&resaved, &ALL_PLUGINS)),
        comparable(original)
    );
}

#[test]
fn every_subset_of_plugins_and_every_order_preserves_the_world() {
    let game = Game::new(&ALL_PLUGINS);
    let original = comparable(game.describe(&sample_world(&game)));
    let bytes = game.save(&sample_world(&game));
    let subsets: [&[Plugin]; 6] = [
        &[BaseBlocks, FancyBlocks],
        &[BaseBlocks, Magic],
        &[Newcomer, BaseBlocks],
        &[BaseBlocks, FancyBlocks, FancyTerrain, Magic],
        &[BaseBlocks, Newcomer, FancyBlocks, Magic, FancyTerrain],
        &[BaseBlocks],
    ];
    for plugins in subsets {
        let through = round_trip(&bytes, plugins);
        assert_eq!(
            comparable(described(&through, &ALL_PLUGINS)),
            original,
            "{plugins:?}"
        );
    }
}

#[test]
fn enabling_a_plugin_that_shifts_runtime_ids_does_not_change_meaning() {
    // Saved without the newcomer plugin, loaded with it: every block's runtime id moves by two.
    let without = Game::new(&[BaseBlocks, FancyBlocks]);
    let bytes = without.save(&sample_world(&without));
    let mut with = Game::new(&[Newcomer, BaseBlocks, FancyBlocks]);
    let (loaded, report) = with.load(&bytes).unwrap();
    assert_eq!(report.missing_content, []);
    assert_eq!(
        with.raw_block("base_blocks:stone"),
        without.raw_block("base_blocks:stone") + 2
    );
    assert_eq!(
        comparable(with.describe(&loaded)),
        comparable(without.describe(&sample_world(&without)))
    );
}

#[test]
fn sections_of_unknown_plugins_survive_byte_for_byte_and_in_order() {
    let game = Game::new(&ALL_PLUGINS);
    let bytes = game.save(&sample_world(&game));
    let through = round_trip(&bytes, &[BaseBlocks]);
    let opaque_of = |bytes: &[u8]| -> Vec<Section> {
        read_container(bytes)
            .unwrap()
            .into_iter()
            .filter(|section| section.identifier.namespace() != "engine")
            .collect()
    };
    assert_eq!(opaque_of(&through), opaque_of(&bytes));
    assert_eq!(opaque_of(&through).len(), 2);
}

#[test]
fn engine_sections_from_a_newer_format_are_refused_but_plugin_sections_never_are() {
    let game = Game::new(&ALL_PLUGINS);
    let bytes = game.save(&sample_world(&game));
    let mut sections = read_container(&bytes).unwrap();
    for section in &mut sections {
        if section.identifier == id("magic:spellbook") {
            section.version = 99; // a plugin's own versioning is the plugin's business
        }
    }
    assert!(
        Game::new(&ALL_PLUGINS)
            .load(&write_container(&sections))
            .is_ok()
    );

    for section in &mut sections {
        if section.identifier == id("engine:channels") {
            section.version = 2;
        }
    }
    let error = Game::new(&ALL_PLUGINS)
        .load(&write_container(&sections))
        .unwrap_err();
    assert!(
        matches!(
            error,
            LoadError::UnsupportedEngineSection { version: 2, .. }
        ),
        "{error}"
    );
}

#[test]
fn damaged_and_truncated_files_are_errors_never_silently_wrong() {
    let game = Game::new(&ALL_PLUGINS);
    let bytes = game.save(&sample_world(&game));
    for position in (0..bytes.len()).step_by(97) {
        let mut damaged = bytes.clone();
        damaged[position] ^= 0x04;
        assert!(
            Game::new(&ALL_PLUGINS).load(&damaged).is_err(),
            "damage at {position} loaded"
        );
    }
    for length in (0..bytes.len()).step_by(211) {
        assert!(
            Game::new(&ALL_PLUGINS).load(&bytes[..length]).is_err(),
            "truncation to {length} loaded"
        );
    }
    let mut payload_damage = bytes.clone();
    let spot = payload_damage
        .windows(7)
        .position(|window| window == b"mystery")
        .unwrap();
    payload_damage[spot] = b'M';
    assert!(matches!(
        Game::new(&ALL_PLUGINS).load(&payload_damage),
        Err(LoadError::Container(ContainerError::CorruptSection { section })) if section == id("unheard_of:data")
    ));
}

#[test]
fn a_channel_whose_meaning_changed_or_whose_registry_is_missing_is_refused() {
    let game = Game::new(&[BaseBlocks]);
    let bytes = game.save(&sample_world(&game));

    let mut changed = Game::new(&[BaseBlocks]);
    let parsed = ParsedWorld::parse(&bytes).unwrap();
    let mut channels = changed.channel_definitions();
    channels[0] = ChannelDefinition::number(id("core:block"), 0);
    let generators = changed.generator_registry();
    let error = parsed
        .load(LoadContext {
            channels,
            registries: vec![(block_kind(), &mut changed.blocks)],
            generators,
            fallback_default_generator: id("base_terrain:flat_v1"),
            assignment_cube_level: 3,
        })
        .unwrap_err();
    assert!(
        matches!(error, LoadError::ChannelKindMismatch { .. }),
        "{error}"
    );

    let mut unsupplied = Game::new(&[BaseBlocks]);
    let error = ParsedWorld::parse(&bytes)
        .unwrap()
        .load(LoadContext {
            channels: unsupplied.channel_definitions(),
            registries: vec![],
            generators: unsupplied.generator_registry(),
            fallback_default_generator: id("base_terrain:flat_v1"),
            assignment_cube_level: 3,
        })
        .unwrap_err();
    assert!(
        matches!(error, LoadError::RegistryNotSupplied { .. }),
        "{error}"
    );
    let _ = &mut unsupplied;
}

#[test]
fn a_block_id_with_no_table_entry_is_refused_rather_than_loaded_dangling() {
    let game = Game::new(&[BaseBlocks]);
    let mut world = game.new_world();
    game.set(&mut world, [1, 1, 1], "core:block", 999);
    let bytes = game.save(&world);
    let error = Game::new(&[BaseBlocks]).load(&bytes).unwrap_err();
    assert!(
        matches!(
            error,
            LoadError::Translation(TranslationError::ValueOutsideRegistry { value: 999, .. })
        ),
        "{error}"
    );
}

#[test]
fn saving_refuses_a_content_channel_whose_registry_table_is_unavailable() {
    let game = Game::new(&[BaseBlocks]);
    let world = game.new_world();
    let error = world.save(&[]).unwrap_err();
    assert!(error.to_string().contains("core:block"), "{error}");
}

#[test]
fn only_real_changes_are_kept_when_pruning() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    world.terrain.assign_unassigned(brick(0, 0, 0)); // flat ground at height 10
    // An edit that changes nothing: a copy of the generated brick.
    let BrickSource::Generated(generated) = world.effective_brick(brick(0, 0, 0)) else {
        panic!("expected terrain")
    };
    world
        .overrides
        .insert_brick(brick(0, 0, 0), Arc::new(generated))
        .unwrap();
    // A real edit next to it.
    game.set_block(&mut world, [40, 2, 3], "fancy_blocks:marble");
    // A brick whose region has no generator cannot be compared and must stay.
    game.set_block(&mut world, [900, 0, 0], "base_blocks:stone");

    assert_eq!(world.prune_unmodified_overrides(), 1);
    assert_eq!(
        world.overrides.sorted_brick_coordinates(),
        [brick(1, 0, 0), brick(28, 0, 0)]
    );
}

#[test]
fn effective_bricks_come_from_edits_then_generators_then_nothing() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    assert!(matches!(
        world.effective_brick(brick(0, 0, 0)),
        BrickSource::Unassigned
    ));
    world.terrain.assign_unassigned(brick(0, 0, 0));
    assert!(matches!(
        world.effective_brick(brick(0, 0, 0)),
        BrickSource::Generated(_)
    ));
    game.set_block(&mut world, [1, 1, 1], "base_blocks:stone");
    assert!(matches!(
        world.effective_brick(brick(0, 0, 0)),
        BrickSource::Edited(_)
    ));
}

#[test]
fn a_loaded_world_can_keep_being_edited_and_saved() {
    let game = Game::new(&ALL_PLUGINS);
    let bytes = game.save(&sample_world(&game));
    let mut later = Game::new(&[BaseBlocks, FancyBlocks]);
    let (mut world, _) = later.load(&bytes).unwrap();
    later.set_block(&mut world, [-70, 3, 3], "fancy_blocks:glass");
    let saved = later.save(&world);

    let described_all = described(&saved, &ALL_PLUGINS);
    let has_new_edit = described_all
        .bricks
        .iter()
        .any(|((coordinate, name, _), (_, changed))| {
            *coordinate == brick(-3, 0, 0)
                && name == "core:block"
                && changed
                    .iter()
                    .any(|(_, value)| value == "fancy_blocks:glass")
        });
    assert!(has_new_edit);
    // Everything saved before is still there too.
    assert!(
        described_all
            .opaque
            .iter()
            .any(|(name, _, _)| name == "magic:spellbook")
    );
}

#[test]
fn a_description_loads_as_the_same_world_shape_with_no_contents() {
    let game = Game::new(&ALL_PLUGINS);
    let world = sample_world(&game);
    let bytes = world.describe(&game.registries()).unwrap();
    let section_names: Vec<String> = read_container(&bytes)
        .unwrap()
        .into_iter()
        .map(|section| section.identifier.to_string())
        .collect();
    assert_eq!(
        section_names,
        [
            "engine:header",
            "engine:registries",
            "engine:channels",
            "engine:generators"
        ]
    );
    assert!(bytes.len() < game.save(&world).len());

    for plugins in [&ALL_PLUGINS[..], &[BaseBlocks]] {
        let mut receiver = Game::new(plugins);
        let (described, report) = receiver.load(&bytes).unwrap();
        assert_eq!(described.world_seed, world.world_seed);
        assert_eq!(described.overrides.brick_count(), 0);
        assert!(described.opaque_sections().is_empty());
        assert_eq!(
            described.preferred_default_generator(),
            world.preferred_default_generator()
        );
        assert_eq!(
            described.terrain.assignment_cube_level(),
            world.terrain.assignment_cube_level()
        );
        // Nothing is assigned yet: the receiver learns assignments as regions come into view.
        assert!(matches!(
            described.effective_brick(brick(0, 0, 0)),
            BrickSource::Unassigned
        ));
        assert_eq!(
            described.overrides.layout().channels().len(),
            4,
            "channels the receiving game lacks are kept, so the layout is always complete"
        );
        if plugins.len() == 1 {
            assert_eq!(report.extra_channels, [id("magic:mana"), id("magic:spell")]);
        }
    }
}

#[test]
fn the_translator_converts_bricks_values_and_generators_from_the_source_numbering() {
    // The source lacks the newcomer plugin, so every block id in its bricks is two lower than ours.
    let source = Game::new(&[BaseBlocks, FancyBlocks, Magic]);
    let mut source_world = source.new_world();
    source.set_block(&mut source_world, [1, 2, 3], "fancy_blocks:marble");
    let source_brick = source_world
        .overrides
        .brick(VoxelCoordinate::new(1, 2, 3).split().0)
        .unwrap();
    let description = source_world.describe(&source.registries()).unwrap();

    let mut receiver = Game::new(&ALL_PLUGINS);
    let (world, _, translator) = ParsedWorld::parse(&description)
        .unwrap()
        .load_with_translator(receiver.load_context())
        .unwrap();
    let block_channel = world
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let (_, local) = VoxelCoordinate::new(1, 2, 3).split();

    let translated = translator
        .translate_brick(Brick::clone(source_brick))
        .unwrap();
    let marble = receiver.raw_block("fancy_blocks:marble");
    assert_ne!(marble, source.raw_block("fancy_blocks:marble"));
    assert_eq!(translated.get(block_channel, local), marble);
    // Untouched voxels are the translated default, air.
    assert_eq!(
        translated.get(block_channel, VoxelCoordinate::new(0, 0, 0).split().1),
        receiver.raw_block("base_blocks:air")
    );

    // The source's channel number 0 is core:block; a single value converts the same way.
    let source_marble = source.raw_block("fancy_blocks:marble");
    assert_eq!(
        translator.translate_value(0, source_marble).unwrap(),
        (block_channel, marble)
    );
    // Plain number channels pass through unchanged.
    let (temperature_channel, value) = translator.translate_value(1, 77).unwrap();
    assert_eq!(
        temperature_channel,
        world
            .overrides
            .layout()
            .channel_index(&id("core:temperature"))
            .unwrap()
    );
    assert_eq!(value, 77);
    assert_eq!(
        translator.translate_generator(0).unwrap(),
        world
            .terrain
            .generators()
            .runtime_id(&id("base_terrain:flat_v1"))
            .unwrap()
    );
}

#[test]
fn the_translator_knows_when_numbering_is_the_same_on_both_sides() {
    let translator_between = |source_plugins: &[Plugin], receiver_plugins: &[Plugin]| {
        let source = Game::new(source_plugins);
        let description = source.new_world().describe(&source.registries()).unwrap();
        let mut receiver = Game::new(receiver_plugins);
        let (_, _, translator) = ParsedWorld::parse(&description)
            .unwrap()
            .load_with_translator(receiver.load_context())
            .unwrap();
        translator
    };
    assert!(translator_between(&ALL_PLUGINS, &ALL_PLUGINS).is_identity());
    // Content the receiver lacks becomes placeholders after its own, and channels it lacks come
    // after its own, so every number still agrees.
    assert!(translator_between(&[BaseBlocks, FancyBlocks, Magic], &[BaseBlocks]).is_identity());
    // A plugin that registers blocks first moves every id.
    assert!(!translator_between(&[BaseBlocks], &[Newcomer, BaseBlocks]).is_identity());
    assert!(!translator_between(&[Newcomer, BaseBlocks], &[BaseBlocks]).is_identity());

    // The same content and the same number of channels, in a different order, is not identity either.
    let source = Game::new(&[BaseBlocks]);
    let description = source.new_world().describe(&source.registries()).unwrap();
    let mut receiver = Game::new(&[BaseBlocks]);
    let mut context = receiver.load_context();
    context.channels.swap(0, 1);
    let (world, _, translator) = ParsedWorld::parse(&description)
        .unwrap()
        .load_with_translator(context)
        .unwrap();
    assert_eq!(world.overrides.layout().channel_count(), 2);
    assert!(!translator.is_identity());
}

#[test]
fn the_translator_refuses_what_the_sources_tables_do_not_account_for() {
    let source = Game::new(&[BaseBlocks]);
    let description = source.new_world().describe(&source.registries()).unwrap();
    let mut receiver = Game::new(&[BaseBlocks]);
    let (_, _, translator) = ParsedWorld::parse(&description)
        .unwrap()
        .load_with_translator(receiver.load_context())
        .unwrap();

    let blocks_in_source = 4; // air, stone, dirt, grass
    assert!(matches!(
        translator.translate_value(0, blocks_in_source),
        Err(TranslationError::ValueOutsideRegistry {
            value: 4,
            entries: 4,
            ..
        })
    ));
    assert!(translator.translate_value(0, blocks_in_source - 1).is_ok());
    assert_eq!(
        translator.translate_value(2, 0),
        Err(TranslationError::ChannelOutsideTable(2))
    );
    assert_eq!(
        translator.translate_generator(1),
        Err(TranslationError::GeneratorOutsideTable(1))
    );

    // A brick with the wrong number of channels, and one holding an id past the table.
    let one_channel = Brick::from_channels(vec![voxel_grid::ChannelStorage::uniform(0)]);
    assert!(matches!(
        translator.translate_brick(one_channel),
        Err(TranslationError::BrickChannelCountMismatch {
            expected: 2,
            found: 1
        })
    ));
    let beyond_table = Brick::from_channels(vec![
        voxel_grid::ChannelStorage::uniform(blocks_in_source),
        voxel_grid::ChannelStorage::uniform(20),
    ]);
    assert!(matches!(
        translator.translate_brick(beyond_table),
        Err(TranslationError::ValueOutsideRegistry { .. })
    ));
}

fn plugin_subset() -> impl Strategy<Value = Vec<Plugin>> {
    proptest::collection::vec(any::<bool>(), 4).prop_map(|keep| {
        let mut plugins = vec![BaseBlocks];
        for (flag, plugin) in keep
            .into_iter()
            .zip([Newcomer, FancyBlocks, Magic, FancyTerrain])
        {
            if flag {
                plugins.push(plugin);
            }
        }
        plugins
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// For random edits and a random set of surviving plugins, load-then-save through the reduced
    /// game and then load with everything must give back exactly the original world.
    #[test]
    fn random_worlds_survive_any_plugin_subset(
        edits in proptest::collection::vec(
            ([-70i64..130, -5i64..70, -40i64..40], 0usize..6, 0u32..300),
            1..30,
        ),
        subset in plugin_subset(),
    ) {
        let game = Game::new(&ALL_PLUGINS);
        let mut world = sample_world(&game);
        let block_names = ["base_blocks:stone", "base_blocks:dirt", "base_blocks:grass", "fancy_blocks:marble", "fancy_blocks:glass", "newcomer:slime"];
        for (at, block, temperature) in edits {
            game.set_block(&mut world, at, block_names[block]);
            game.set(&mut world, at, "core:temperature", temperature);
        }
        let original = comparable(game.describe(&world));
        let bytes = game.save(&world);

        let through = round_trip(&bytes, &subset);
        prop_assert_eq!(comparable(described(&through, &ALL_PLUGINS)), original);
    }
}
