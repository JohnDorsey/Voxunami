//! The invariants this crate exists for, checked against a small pretend game whose plugins can be
//! switched on and off between a save and a load.

use std::sync::Arc;

use proptest::prelude::*;
use voxel_grid::{Brick, ChannelDefinition};
use voxel_space::VoxelCoordinate;
use world_storage::container::{read_container, write_container};
use world_storage::engine_sections::{
    self, decode_levels, encode_generators, encode_override_bricks,
};
use world_storage::test_support::Plugin::*;
use world_storage::test_support::{
    ALL_PLUGINS, Description, Game, Plugin, block_kind, brick, comparable, id, main_level_id,
    sample_world, spell_kind,
};
use world_storage::{
    BrickSource, ContainerError, LevelError, LevelIndex, LoadContext, LoadError, ParsedWorld,
    Section, TranslationError, WorldData,
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
    assert_eq!(report.unavailable_default_generators, []);
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
        report.unavailable_default_generators,
        [(main_level_id(), id("fancy_terrain:cliffs_v1"))]
    );
    assert!(report.changed_channel_defaults.is_empty());

    // The missing content is present as placeholders, not silently turned into something else.
    let marble = reduced
        .blocks
        .runtime_id(&id("fancy_blocks:marble"))
        .unwrap();
    assert!(reduced.blocks.entry(marble).unwrap().is_missing());
    assert!(
        matches!(partial.main().effective_brick(brick(100, 0, 0)), BrickSource::MissingGenerator(name) if name == id("fancy_terrain:cliffs_v1"))
    );
    assert!(matches!(
        partial.main().effective_brick(brick(0, 0, 0)),
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
            legacy_level: main_level_id(),
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
            legacy_level: main_level_id(),
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
    world.main_mut().terrain.assign_unassigned(brick(0, 0, 0)); // flat ground at height 10
    // An edit that changes nothing: a copy of the generated brick.
    let BrickSource::Generated(generated) = world.main().effective_brick(brick(0, 0, 0)) else {
        panic!("expected terrain")
    };
    world
        .main_mut()
        .overrides
        .insert_brick(brick(0, 0, 0), Arc::new(generated))
        .unwrap();
    // A real edit next to it.
    game.set_block(&mut world, [40, 2, 3], "fancy_blocks:marble");
    // A brick whose region has no generator cannot be compared and must stay.
    game.set_block(&mut world, [900, 0, 0], "base_blocks:stone");

    assert_eq!(world.prune_unmodified_overrides(), 1);
    assert_eq!(
        world.main().overrides.sorted_brick_coordinates(),
        [brick(1, 0, 0), brick(28, 0, 0)]
    );
}

#[test]
fn effective_bricks_come_from_edits_then_generators_then_nothing() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    assert!(matches!(
        world.main().effective_brick(brick(0, 0, 0)),
        BrickSource::Unassigned
    ));
    world.main_mut().terrain.assign_unassigned(brick(0, 0, 0));
    assert!(matches!(
        world.main().effective_brick(brick(0, 0, 0)),
        BrickSource::Generated(_)
    ));
    game.set_block(&mut world, [1, 1, 1], "base_blocks:stone");
    assert!(matches!(
        world.main().effective_brick(brick(0, 0, 0)),
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
    let has_new_edit =
        described_all.levels[0]
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
            "engine:levels"
        ]
    );
    assert!(bytes.len() < game.save(&world).len());

    for plugins in [&ALL_PLUGINS[..], &[BaseBlocks]] {
        let mut receiver = Game::new(plugins);
        let (described, report) = receiver.load(&bytes).unwrap();
        assert_eq!(described.world_seed, world.world_seed);
        assert_eq!(described.main().overrides.brick_count(), 0);
        assert!(described.opaque_sections().is_empty());
        assert_eq!(
            described.main().preferred_default_generator(),
            world.main().preferred_default_generator()
        );
        assert_eq!(
            described.main().terrain.assignment_cube_level(),
            world.main().terrain.assignment_cube_level()
        );
        // Nothing is assigned yet: the receiver learns assignments as regions come into view.
        assert!(matches!(
            described.main().effective_brick(brick(0, 0, 0)),
            BrickSource::Unassigned
        ));
        assert_eq!(
            described.main().overrides.layout().channels().len(),
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
        .main()
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
        .main()
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
            .main()
            .overrides
            .layout()
            .channel_index(&id("core:temperature"))
            .unwrap()
    );
    assert_eq!(value, 77);
    assert_eq!(
        translator.translate_generator(0).unwrap(),
        world
            .main()
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
    assert_eq!(world.main().overrides.layout().channel_count(), 2);
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

/// A world with a second level, which uses the flat generator where the main level uses the
/// cliffs, and edits in each level.
fn two_level_world(game: &Game) -> (WorldData, LevelIndex) {
    let mut world = sample_world(game);
    let nether = world
        .add_level(
            id("test_content:nether"),
            game.new_level("base_terrain:flat_v1"),
        )
        .unwrap();
    game.set_in_level(
        &mut world,
        nether,
        [7, 7, 7],
        "core:block",
        game.raw_block("base_blocks:grass"),
    );
    world
        .level_mut(nether)
        .unwrap()
        .terrain
        .assign_unassigned(brick(3, 0, 0));
    (world, nether)
}

#[test]
fn levels_have_indices_in_order_and_names_that_resolve() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, nether) = two_level_world(&game);
    assert_eq!(world.level_count(), 2);
    assert_eq!((LevelIndex::MAIN.get(), nether.get()), (0, 1));
    assert_eq!(world.level_id(LevelIndex::MAIN), Some(&main_level_id()));
    assert_eq!(world.level_id(nether), Some(&id("test_content:nether")));
    assert_eq!(world.level_index(&main_level_id()), Some(LevelIndex::MAIN));
    assert_eq!(world.level_index(&id("test_content:nether")), Some(nether));
    assert_eq!(world.level_index(&id("test_content:missing")), None);
    assert!(world.level(LevelIndex::new(2)).is_none());
    assert!(world.level_id(LevelIndex::new(2)).is_none());
    let listed: Vec<_> = world
        .levels()
        .map(|(index, name, _)| (index, name.to_string()))
        .collect();
    assert_eq!(
        listed,
        [
            (LevelIndex::MAIN, "test_content:main".to_owned()),
            (nether, "test_content:nether".to_owned())
        ]
    );
    assert!(std::ptr::eq(
        world.main(),
        world.level(LevelIndex::MAIN).unwrap()
    ));
}

#[test]
fn a_level_can_only_be_added_if_it_matches_the_world() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    assert_eq!(
        world
            .add_level(main_level_id(), game.new_level("base_terrain:flat_v1"))
            .unwrap_err(),
        LevelError::Duplicate(main_level_id())
    );
    // Without the fancy terrain plugin the generator versions differ.
    let without_fancy_terrain = Game::new(&[Newcomer, BaseBlocks, FancyBlocks, Magic]);
    assert_eq!(
        world
            .add_level(
                id("test_content:other"),
                without_fancy_terrain.new_level("base_terrain:flat_v1")
            )
            .unwrap_err(),
        LevelError::DifferentGenerators(id("test_content:other"))
    );
    // Without the magic plugin the channels differ.
    let without_magic = Game::new(&[BaseBlocks]);
    assert_eq!(
        world
            .add_level(
                id("test_content:other"),
                without_magic.new_level("base_terrain:flat_v1")
            )
            .unwrap_err(),
        LevelError::DifferentChannels(id("test_content:other"))
    );
    assert_eq!(world.level_count(), 1, "nothing was added");
}

#[test]
fn levels_are_saved_and_loaded_each_with_its_own_edits_and_terrain() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, nether) = two_level_world(&game);
    let bytes = game.save(&world);

    let mut again = Game::new(&ALL_PLUGINS);
    let (loaded, report) = again.load(&bytes).unwrap();
    assert!(report.unavailable_default_generators.is_empty());
    assert_eq!(loaded.level_count(), 2);
    assert_eq!(loaded.level_id(nether), Some(&id("test_content:nether")));
    // Each level has only its own edits.
    let main_bricks = loaded.main().overrides.sorted_brick_coordinates();
    let nether_bricks = loaded
        .level(nether)
        .unwrap()
        .overrides
        .sorted_brick_coordinates();
    assert!(main_bricks.len() > 1);
    assert_eq!(nether_bricks, [brick(0, 0, 0)]);
    assert_ne!(
        loaded.main().overrides.brick(brick(0, 0, 0)),
        loaded
            .level(nether)
            .unwrap()
            .overrides
            .brick(brick(0, 0, 0))
    );
    // And its own generators.
    assert_eq!(
        loaded.main().preferred_default_generator(),
        &id("fancy_terrain:cliffs_v1")
    );
    assert_eq!(
        loaded.level(nether).unwrap().preferred_default_generator(),
        &id("base_terrain:flat_v1")
    );
    assert_eq!(
        again.describe(&loaded).levels[1].assignments.len(),
        game.describe(&world).levels[1].assignments.len()
    );
    assert_eq!(again.describe(&loaded), game.describe(&world));
    assert_eq!(again.save(&loaded), bytes);

    // A description names the levels and their generators, and holds none of their contents.
    let described = world.describe(&game.registries()).unwrap();
    let mut receiver = Game::new(&ALL_PLUGINS);
    let (shape, _) = receiver.load(&described).unwrap();
    assert_eq!(shape.level_count(), 2);
    assert_eq!(shape.level_id(nether), Some(&id("test_content:nether")));
    for (_, _, level) in shape.levels() {
        assert_eq!(level.overrides.brick_count(), 0);
        assert!(level.terrain.assignments().leaves().len() == 1);
    }
    assert_eq!(
        shape.level(nether).unwrap().preferred_default_generator(),
        &id("base_terrain:flat_v1")
    );
}

#[test]
fn loading_without_plugins_keeps_every_levels_data() {
    let game = Game::new(&ALL_PLUGINS);
    let (world, _) = two_level_world(&game);
    let bytes = game.save(&world);
    let mut reduced = Game::new(&[BaseBlocks]);
    let (_, report) = reduced.load(&bytes).unwrap();
    // Only the main level asked for a generator that is gone.
    assert_eq!(
        report.unavailable_default_generators,
        [(main_level_id(), id("fancy_terrain:cliffs_v1"))]
    );
    assert_eq!(
        comparable(described(&round_trip(&bytes, &[BaseBlocks]), &ALL_PLUGINS)),
        comparable(game.describe(&world))
    );
}

/// The sections of a one-level world as files had them before levels: the generators and the
/// edited bricks in sections of their own.
fn before_levels(bytes: &[u8]) -> Vec<u8> {
    let sections = read_container(bytes).unwrap();
    let mut legacy = Vec::new();
    for section in sections {
        if section.identifier != engine_sections::levels_id() {
            legacy.push(section);
            continue;
        }
        let mut levels = decode_levels(&section.payload).unwrap();
        assert_eq!(
            levels.len(),
            1,
            "only a world of one level can be written the old way"
        );
        let level = levels.remove(0);
        legacy.push(Section {
            identifier: engine_sections::generators_id(),
            version: engine_sections::GENERATORS_VERSION,
            payload: encode_generators(&level.generators),
        });
        let bricks: Vec<_> = level
            .bricks
            .iter()
            .map(|(at, brick)| (*at, brick))
            .collect();
        legacy.push(Section {
            identifier: engine_sections::override_bricks_id(),
            version: engine_sections::SUPPORTED_VERSION,
            payload: encode_override_bricks(&bricks),
        });
    }
    write_container(&legacy)
}

#[test]
fn a_file_from_before_levels_loads_as_one_level_the_game_names_and_is_saved_in_the_new_form() {
    let game = Game::new(&ALL_PLUGINS);
    let world = sample_world(&game);
    let new_form = game.save(&world);
    let old_form = before_levels(&new_form);
    let names = |bytes: &[u8]| -> Vec<String> {
        read_container(bytes)
            .unwrap()
            .into_iter()
            .map(|section| section.identifier.to_string())
            .collect()
    };
    assert!(names(&old_form).contains(&"engine:override_bricks".to_owned()));
    assert!(!names(&old_form).contains(&"engine:levels".to_owned()));

    let mut loading = Game::new(&ALL_PLUGINS);
    let (loaded, report) = loading.load(&old_form).unwrap();
    assert!(report.unavailable_default_generators.is_empty());
    assert_eq!(loaded.level_count(), 1);
    assert_eq!(loaded.level_id(LevelIndex::MAIN), Some(&main_level_id()));
    assert_eq!(loading.describe(&loaded), game.describe(&world));
    // Saving writes the new form, and is the same file as saving the world in the first place.
    assert_eq!(loading.save(&loaded), new_form);
}

#[test]
fn a_file_with_a_list_of_levels_and_the_old_sections_too_is_refused() {
    let game = Game::new(&ALL_PLUGINS);
    let bytes = game.save(&sample_world(&game));
    let mut sections = read_container(&bytes).unwrap();
    sections.push(Section {
        identifier: engine_sections::override_bricks_id(),
        version: engine_sections::SUPPORTED_VERSION,
        payload: encode_override_bricks(&[]),
    });
    let error = ParsedWorld::parse(&write_container(&sections)).unwrap_err();
    assert!(
        matches!(error, LoadError::LevelsAndLegacySections),
        "{error}"
    );
}

#[test]
fn pruning_unchanged_bricks_covers_every_level() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    let nether = world
        .add_level(
            id("test_content:nether"),
            game.new_level("base_terrain:flat_v1"),
        )
        .unwrap();
    // In each level, an edited brick that is exactly what the generator makes.
    for level in [LevelIndex::MAIN, nether] {
        let level = world.level_mut(level).unwrap();
        level.terrain.assign_unassigned(brick(0, 0, 0));
        let BrickSource::Generated(generated) = level.effective_brick(brick(0, 0, 0)) else {
            panic!("the brick is generated");
        };
        level
            .overrides
            .insert_brick(brick(0, 0, 0), Arc::new(generated))
            .unwrap();
    }
    assert_eq!(world.prune_unmodified_overrides(), 2);
    assert_eq!(world.main().overrides.brick_count(), 0);
    assert_eq!(world.level(nether).unwrap().overrides.brick_count(), 0);
}
