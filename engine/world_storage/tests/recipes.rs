//! Generator recipes: a world keeps, besides the names of its terrain generators, the recipes of the ones that are data (an instance name, a family, and the
//! parameters as bytes), and makes those generators again when it is loaded, or when a client is told of it. A planet of another size is a recipe, not a change
//! of code. (The family here is the fancy terrain plugin's: a flat world whose height is its parameters.)

use world_storage::container::{read_container, write_container};
use world_storage::engine_sections::{
    decode_generator_recipes, encode_generator_recipes, generator_recipes_id,
};
use world_storage::test_support::{ALL_PLUGINS, Game, Plugin, brick, id, sample_world};
use world_storage::{GeneratorRecipe, LoadError, RecipeError, Section};

use content_registry::IdentifierRegistry;
use region_octree::BrickCube;
use terrain::GenerationOutcome;
use voxel_grid::Brick;

fn sections_of(bytes: &[u8]) -> Vec<Section> {
    read_container(bytes).unwrap()
}

fn has_recipes_section(bytes: &[u8]) -> bool {
    sections_of(bytes)
        .iter()
        .any(|section| section.identifier == generator_recipes_id())
}

fn without_the_fancy_terrain() -> Game {
    Game::new(&[
        Plugin::Newcomer,
        Plugin::BaseBlocks,
        Plugin::FancyBlocks,
        Plugin::Magic,
    ])
}

/// The brick the generator `instance` makes at `at`, got by giving the whole cube round it to that generator in the world's main level.
fn brick_of(world: &mut world_storage::WorldData, instance: &str, at: [i128; 3]) -> Brick {
    let layout = std::sync::Arc::clone(world.main().overrides.layout());
    let generator = world
        .main()
        .terrain
        .generators()
        .runtime_id(&id(instance))
        .unwrap();
    let coordinate = brick(at[0], at[1], at[2]);
    let cube = BrickCube::containing(coordinate, world.main().terrain.assignment_cube_level());
    world
        .main_mut()
        .terrain
        .set_assignment(cube, Some(generator));
    match world.main().terrain.generate(&layout, coordinate) {
        GenerationOutcome::Generated(brick) => brick,
        other => panic!("expected a brick, got {other:?}"),
    }
}

#[test]
fn a_recipe_adds_its_generator_to_every_level_after_the_others_and_moves_nothing() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    world
        .add_level(
            id("test_content:second"),
            game.new_level("base_terrain:flat_v1"),
        )
        .unwrap();
    let before: Vec<_> = world
        .main()
        .terrain
        .generators()
        .iter()
        .map(|(id, entry)| (id, entry.identifier().clone()))
        .collect();

    let recipe = game.flat_recipe("test_content:high_ground", 25.0);
    let added = game.add_recipe(&mut world, recipe.clone()).unwrap();

    assert_eq!(added.raw() as usize, before.len());
    for (_, _, level) in world.levels() {
        let generators = level.terrain.generators();
        assert_eq!(generators.len(), before.len() + 1);
        assert_eq!(
            generators.runtime_id(&id("test_content:high_ground")),
            Some(added)
        );
        assert!(!generators.entry(added).unwrap().is_missing());
        for (number, name) in &before {
            assert_eq!(generators.runtime_id(name), Some(*number));
        }
    }
    assert_eq!(world.generator_recipes(), &[recipe]);
}

#[test]
fn a_recipe_that_cannot_be_added_changes_nothing_and_says_why() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    world
        .add_level(
            id("test_content:second"),
            game.new_level("base_terrain:flat_v1"),
        )
        .unwrap();
    let first = game.flat_recipe("test_content:high_ground", 25.0);
    game.add_recipe(&mut world, first.clone()).unwrap();
    let tables = |world: &world_storage::WorldData| -> Vec<usize> {
        world
            .levels()
            .map(|(_, _, level)| level.terrain.generators().entry_count())
            .collect()
    };
    let before = tables(&world);

    // A family nobody has.
    let mut unknown = game.flat_recipe("test_content:other", 1.0);
    unknown.family = id("nobody:family_v1");
    assert_eq!(
        game.add_recipe(&mut world, unknown),
        Err(RecipeError::UnknownFamily(id("nobody:family_v1")))
    );
    // Parameters the family refuses.
    let mut refused = game.flat_recipe("test_content:other", 1.0);
    refused.parameters = vec![1, 2, 3];
    match game.add_recipe(&mut world, refused) {
        Err(RecipeError::Refused { instance, message }) => {
            assert_eq!(instance, id("test_content:other"));
            assert!(message.contains("8 bytes"), "{message}");
        }
        other => panic!("expected the parameters to be refused, got {other:?}"),
    }
    // A name a generator of the game has, one an earlier recipe has.
    for taken in ["base_terrain:flat_v1", "test_content:high_ground"] {
        let recipe = game.flat_recipe(taken, 3.0);
        assert_eq!(
            game.add_recipe(&mut world, recipe),
            Err(RecipeError::NameTaken(id(taken)))
        );
    }

    assert_eq!(tables(&world), before);
    assert_eq!(world.generator_recipes(), &[first]);
}

#[test]
fn a_world_with_a_recipe_comes_back_from_its_file_with_the_generator_made_again() {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    game.add_recipe(
        &mut world,
        game.flat_recipe("test_content:high_ground", 25.0),
    )
    .unwrap();
    game.add_recipe(&mut world, game.flat_recipe("test_content:low_ground", 3.0))
        .unwrap();
    // A region that the recipe's generator makes.
    let high = brick_of(&mut world, "test_content:high_ground", [7, 0, 7]);
    let bytes = game.save(&world);
    assert!(has_recipes_section(&bytes));

    let (mut loaded, report) = game.load(&bytes).unwrap();
    assert!(report.missing_content.is_empty(), "{report:?}");
    assert!(report.refused_recipes.is_empty(), "{report:?}");
    assert_eq!(loaded.generator_recipes(), world.generator_recipes());
    // Saved again, the file is the one that was read.
    assert_eq!(game.save(&loaded), bytes);
    let again = brick_of(&mut loaded, "test_content:high_ground", [7, 0, 7]);
    assert_eq!(again, high, "the same generator, so the same bricks");
    let low = brick_of(&mut loaded, "test_content:low_ground", [7, 0, 7]);
    assert_ne!(low, high, "and the two recipes make different worlds");
}

#[test]
fn a_description_carries_the_recipes_so_that_a_client_makes_the_same_generators() {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    game.add_recipe(
        &mut world,
        game.flat_recipe("test_content:high_ground", 25.0),
    )
    .unwrap();
    let server_brick = brick_of(&mut world, "test_content:high_ground", [2, 0, 9]);

    let description = world.describe(&game.registries()).unwrap();
    assert!(has_recipes_section(&description));
    let (mut client_world, report) = game.load(&description).unwrap();
    assert!(report.refused_recipes.is_empty() && report.missing_content.is_empty());
    assert_eq!(client_world.generator_recipes(), world.generator_recipes());
    let client_brick = brick_of(&mut client_world, "test_content:high_ground", [2, 0, 9]);
    assert_eq!(client_brick, server_brick);
}

#[test]
fn a_world_with_no_recipes_writes_no_section_so_it_saves_as_it_always_did() {
    let game = Game::new(&ALL_PLUGINS);
    let world = sample_world(&game);
    assert!(world.generator_recipes().is_empty());
    assert!(!has_recipes_section(&game.save(&world)));
    assert!(!has_recipes_section(
        &world.describe(&game.registries()).unwrap()
    ));
}

#[test]
fn a_recipe_of_a_family_this_game_lacks_is_kept_with_a_placeholder_for_its_generator() {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    game.add_recipe(
        &mut world,
        game.flat_recipe("test_content:high_ground", 25.0),
    )
    .unwrap();
    let bytes = game.save(&world);

    // The plugin that has the family is not loaded this time.
    let mut without = without_the_fancy_terrain();
    let (loaded, report) = without.load(&bytes).unwrap();
    assert!(
        report.missing_content.contains(&(
            world_storage::engine_sections::terrain_generator_registry_kind(),
            id("test_content:high_ground")
        )),
        "{report:?}"
    );
    assert!(
        report.refused_recipes.is_empty(),
        "no family is not a refusal"
    );
    let generators = loaded.main().terrain.generators();
    let placeholder = generators
        .runtime_id(&id("test_content:high_ground"))
        .unwrap();
    assert!(generators.entry(placeholder).unwrap().is_missing());
    // Nothing is lost: the recipe is in the world, and saved again the file is the one that was read.
    assert_eq!(loaded.generator_recipes(), world.generator_recipes());
    assert_eq!(without.save(&loaded), bytes);

    // And a game that has the plugin again reads it as before.
    let (back, report) = game.load(&without.save(&loaded)).unwrap();
    assert!(report.missing_content.is_empty());
    assert!(
        !back
            .main()
            .terrain
            .generators()
            .entry(placeholder)
            .unwrap()
            .is_missing()
    );
}

/// The file of a world with one recipe, with the payload of its recipes section replaced by `recipes`.
fn file_with_recipes(game: &mut Game, recipes: &[GeneratorRecipe]) -> Vec<u8> {
    let mut world = sample_world(game);
    game.add_recipe(
        &mut world,
        game.flat_recipe("test_content:high_ground", 25.0),
    )
    .unwrap();
    let mut sections = sections_of(&game.save(&world));
    for section in &mut sections {
        if section.identifier == generator_recipes_id() {
            section.payload = encode_generator_recipes(recipes);
        }
    }
    write_container(&sections)
}

#[test]
fn a_recipe_the_family_refuses_is_reported_and_kept_not_believed() {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut bad = game.flat_recipe("test_content:high_ground", 25.0);
    bad.parameters = vec![9, 9];
    let bytes = file_with_recipes(&mut game, &[bad.clone()]);

    let (loaded, report) = game.load(&bytes).unwrap();
    assert_eq!(report.refused_recipes.len(), 1, "{report:?}");
    assert_eq!(report.refused_recipes[0].0, id("test_content:high_ground"));
    assert!(report.refused_recipes[0].1.contains("8 bytes"));
    let generators = loaded.main().terrain.generators();
    let number = generators
        .runtime_id(&id("test_content:high_ground"))
        .unwrap();
    assert!(generators.entry(number).unwrap().is_missing());
    assert_eq!(loaded.generator_recipes(), &[bad]);
}

#[test]
fn a_recipe_the_files_table_of_generators_does_not_list_is_made_after_the_others() {
    let mut game = Game::new(&ALL_PLUGINS);
    let listed = game.flat_recipe("test_content:high_ground", 25.0);
    let unlisted = game.flat_recipe("test_content:unlisted", 3.0);
    let bytes = file_with_recipes(&mut game, &[listed.clone(), unlisted.clone()]);

    let (mut loaded, report) = game.load(&bytes).unwrap();
    assert!(report.missing_content.is_empty() && report.refused_recipes.is_empty());
    assert_eq!(loaded.generator_recipes(), &[listed, unlisted]);
    let generators = loaded.main().terrain.generators();
    let last = generators.len() - 1;
    let number = generators.runtime_id(&id("test_content:unlisted")).unwrap();
    assert_eq!(number.raw() as usize, last, "after the others");
    assert!(!generators.entry(number).unwrap().is_missing());
    // And it makes a world of its own.
    let low = brick_of(&mut loaded, "test_content:unlisted", [1, 0, 1]);
    let high = brick_of(&mut loaded, "test_content:high_ground", [1, 0, 1]);
    assert_ne!(low, high);
    // The world that saves is whole: the table lists the generator now, so a load of it finds the same.
    let (again, _) = game.load(&game.save(&loaded)).unwrap();
    assert_eq!(again.generator_recipes(), loaded.generator_recipes());
}

#[test]
fn a_recipe_that_names_a_generator_the_game_has_is_a_damaged_file() {
    let mut game = Game::new(&ALL_PLUGINS);
    let shadow = game.flat_recipe("base_terrain:flat_v1", 25.0);
    let bytes = file_with_recipes(&mut game, &[shadow]);
    match game.load(&bytes) {
        Err(LoadError::RecipeShadowsGenerator(name)) => {
            assert_eq!(name, id("base_terrain:flat_v1"));
        }
        other => panic!("expected the file to be refused, got {other:?}"),
    }
}

#[test]
fn the_section_is_the_recipes_in_order_and_a_damaged_one_is_refused() {
    let game = Game::new(&ALL_PLUGINS);
    let recipes = vec![
        game.flat_recipe("test_content:high_ground", 25.0),
        GeneratorRecipe {
            instance: id("test_content:nothing"),
            family: id("fancy_terrain:flat_family_v1"),
            parameters: Vec::new(),
        },
        GeneratorRecipe {
            instance: id("test_content:long"),
            family: id("other_plugin:long_family_v1"),
            parameters: (0..=255).cycle().take(1_000).collect(),
        },
    ];
    let bytes = encode_generator_recipes(&recipes);
    assert_eq!(decode_generator_recipes(&bytes).unwrap(), recipes);
    assert_eq!(
        decode_generator_recipes(&encode_generator_recipes(&[])).unwrap(),
        Vec::<GeneratorRecipe>::new()
    );

    assert!(
        decode_generator_recipes(&bytes[..bytes.len() - 1]).is_err(),
        "truncated"
    );
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(decode_generator_recipes(&longer).is_err(), "trailing bytes");
    assert!(decode_generator_recipes(&[]).is_err(), "empty");
    // Two recipes for one instance are one too many.
    let twice = encode_generator_recipes(&[recipes[0].clone(), recipes[0].clone()]);
    assert!(decode_generator_recipes(&twice).is_err(), "a name twice");
}

#[test]
fn a_recipes_section_of_a_newer_version_is_refused_not_dropped() {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    game.add_recipe(
        &mut world,
        game.flat_recipe("test_content:high_ground", 25.0),
    )
    .unwrap();
    let mut sections = sections_of(&game.save(&world));
    for section in &mut sections {
        if section.identifier == generator_recipes_id() {
            section.version = world_storage::engine_sections::SUPPORTED_VERSION + 1;
        }
    }
    assert!(matches!(
        game.load(&write_container(&sections)),
        Err(LoadError::UnsupportedEngineSection { .. })
    ));
}
