//! A game's content as one declared package: what a game that has one block registry, one set of
//! channels and one terrain generator says to the shell, instead of writing the same
//! [`GameContent`] by hand.
//!
//! A [`PackageSpec`] is everything a game declares about its world: the level players start in,
//! the channels of every voxel, which of them the view reads, the terrain generators, and how
//! the blocks look. [`ContentPackage`] builds worlds and loads them from it. This is the shape a
//! data-driven mod will fill in later, so what a spec holds is what a mod's manifest will need.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use block_models::{BlockDefinition, PropertyDeclaration, PropertySchema};
use content_registry::{IdentifierRegistry, Registry};
use namespaced_id::NamespacedId;
use terrain::{GeneratorFamily, GeneratorId, TerrainGenerator, WorldTerrain};
use voxel_grid::{ChannelDefinition, ChannelIndex, Grid, GridLayout};
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, WorldPosition,
};
use world_storage::rail::{Centre, Orbit, Rail, Spin};
use world_storage::{
    GeneratorRecipe, GridTerrain, LevelData, LoadContext, MovingGrid, PilotLogout, RecipeError,
    WorldData,
};
use world_view::ViewChannels;

use crate::definition::GameContent;
use crate::package_data::{PackageData, check_materials};
use data_packs::{
    Around, Body, BodyRail, BodySupport, PackErrors, RawPack, Weight, load_packs, read_body,
    read_family_parameters,
};

/// Makes a game's terrain generators for a world seed. The layout is the world's, for finding the
/// channels a generator writes.
pub type GeneratorFactory =
    Arc<dyn Fn(u64, &GridLayout) -> Registry<Arc<dyn TerrainGenerator>> + Send + Sync>;

/// Makes a game's families of terrain generators, which a world's recipes are made with. The layout is the world's, for finding the channels a generator writes.
pub type FamilyFactory =
    Arc<dyn Fn(&GridLayout) -> Vec<(NamespacedId, Arc<dyn GeneratorFamily>)> + Send + Sync>;

/// For a game that has no families: its generators are all code.
pub fn no_families() -> FamilyFactory {
    Arc::new(|_layout| Vec::new())
}

/// What a game declares about its world.
#[derive(Clone)]
pub struct PackageSpec {
    /// The level players start in, and the only one a new world has.
    pub level: NamespacedId,
    /// The kind of the block registry in world files (such as `my_game:block`).
    pub block_registry_kind: NamespacedId,
    /// The channels of every brick, in order. The first is conventionally the block channel.
    pub channels: Vec<ChannelDefinition>,
    /// Which channel holds each voxel's block, and where the game has them, its state and light.
    pub block_channel: NamespacedId,
    pub state_channel: Option<NamespacedId>,
    /// The channel holding each voxel's orientation, where the game's blocks can be turned.
    pub orientation_channel: Option<NamespacedId>,
    pub light_channel: Option<NamespacedId>,
    /// The properties of blocks that the game's plugins declare, besides the engine's.
    pub properties: Vec<PropertyDeclaration>,
    pub generators: GeneratorFactory,
    /// The families of terrain generators whose settings a world keeps as data (see [`WorldData::add_generator_recipe`]).
    pub families: FamilyFactory,
    /// How the parameters of those families are written in the body files of packs, and the levels a new world has (the names a body's `level` may take).
    pub body_support: BodySupport,
    /// The bodies a new world has before any pack adds its own: the name of each and the text of its file (as a pack's `bodies/` folder holds them).
    pub builtin_bodies: Vec<(NamespacedId, &'static str)>,
    /// How hard gravity pulls between bodies (the game's gravity constant, `GravitySettings::pull_strength`): what a body that is told how hard it pulls at its
    /// surface weighs is worked out with it.
    pub pull_strength: f64,
    /// The generator new regions are given, and the one used if a world's own is missing.
    pub default_generator: NamespacedId,
    /// Generators are assigned to cubes of 8 to the power of this many bricks per side (see
    /// `WorldTerrain`).
    pub assignment_cube_level: u8,
    /// How bright a face is that no light reaches, 0 to 255.
    pub ambient_light: u8,
}

impl fmt::Debug for PackageSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PackageSpec")
            .field("level", &self.level)
            .field("channels", &self.channels.len())
            .field("default_generator", &self.default_generator)
            .finish_non_exhaustive()
    }
}

/// A game's blocks and its declaration, as the shell's [`GameContent`].
#[derive(Debug)]
pub struct ContentPackage {
    blocks: Registry<BlockDefinition>,
    spec: PackageSpec,
    /// The bodies a new world has: the game's own, then the packs', in the order they are made.
    bodies: Vec<(NamespacedId, Body)>,
}

impl ContentPackage {
    /// Panics if one of the game's own bodies is not a good body file, which is a mistake in the game's declaration.
    pub fn new(blocks: Registry<BlockDefinition>, spec: PackageSpec) -> Self {
        let bodies = spec
            .builtin_bodies
            .iter()
            .map(|(identifier, text)| {
                let file = format!(
                    "{}/bodies/{}.json",
                    identifier.namespace(),
                    identifier.name()
                );
                let (_, body) = read_body(
                    &file,
                    identifier.namespace(),
                    identifier.name(),
                    text,
                    &spec.body_support.context(),
                )
                .unwrap_or_else(|errors| {
                    panic!("the game's own body {identifier} is wrong:\n{errors}")
                });
                (identifier.clone(), body)
            })
            .collect();
        Self {
            blocks,
            spec,
            bodies,
        }
    }

    fn layout(&self) -> Arc<GridLayout> {
        Arc::new(GridLayout::new(self.spec.channels.clone()).expect("the channels are distinct"))
    }

    /// The channel with an identifier. Panics if the package has no such channel, which is a
    /// mistake in the game's declaration.
    pub fn channel(&self, identifier: &NamespacedId) -> ChannelIndex {
        self.layout()
            .channel_index(identifier)
            .unwrap_or_else(|| panic!("the package has no channel {identifier}"))
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.channel(&self.spec.block_channel)
    }

    /// The channel holding each voxel's orientation. Panics if the game has none.
    pub fn orientation_channel(&self) -> ChannelIndex {
        self.channel(
            self.spec
                .orientation_channel
                .as_ref()
                .expect("the package declares an orientation channel"),
        )
    }

    /// The channel holding each voxel's state. Panics if the game has none.
    pub fn state_channel(&self) -> ChannelIndex {
        self.channel(
            self.spec
                .state_channel
                .as_ref()
                .expect("the package declares a state channel"),
        )
    }

    fn generators(&self, seed: u64) -> Registry<Arc<dyn TerrainGenerator>> {
        (self.spec.generators)(seed, &self.layout())
    }

    fn families(&self) -> Vec<(NamespacedId, Arc<dyn GeneratorFamily>)> {
        (self.spec.families)(&self.layout())
    }

    /// Makes the package's bodies in a world that is being made, each as a held ship with the ground of its own generator (a recipe, named as the body is), in the
    /// order of the package: the game's own, then those of the packs. A body that is told how hard it pulls at its surface weighs what that takes at the game's
    /// pull strength: `g * r * r / pull_strength`, with `r` the radius its family reads. Nothing is made if any of them cannot be: the message names the body.
    pub fn add_bodies(&self, world: &mut WorldData) -> Result<(), String> {
        let pull_strength = self.spec.pull_strength;
        if !(pull_strength > 0.0 && pull_strength.is_finite()) {
            return Err(format!(
                "the pull of gravity is above nothing, not {pull_strength}"
            ));
        }
        struct Ready {
            identifier: NamespacedId,
            recipe: GeneratorRecipe,
            level: LevelIndex,
            body: Body,
            mass: f32,
        }
        let mut ready: Vec<Ready> = Vec::new();
        for (identifier, body) in &self.bodies {
            let trouble = |why: String| format!("{identifier}: {why}");
            let level = world
                .level_index(&body.level)
                .ok_or_else(|| trouble(format!("the world has no level {}", body.level)))?;
            let parameters =
                read_family_parameters(&self.spec.body_support.families, body, world.world_seed)
                    .map_err(trouble)?;
            let taken = world
                .main()
                .terrain
                .generators()
                .runtime_id(identifier)
                .is_some();
            if taken {
                return Err(trouble(
                    RecipeError::NameTaken(identifier.clone()).to_string(),
                ));
            }
            let mass = match body.weight {
                Weight::Mass(mass) => mass,
                Weight::SurfaceGravity(gravity) => {
                    gravity * parameters.surface_radius * parameters.surface_radius / pull_strength
                }
            };
            if !(mass > 0.0 && mass <= f64::from(f32::MAX)) {
                return Err(trouble(format!(
                    "{mass} kilograms is not a weight a ship can have"
                )));
            }
            if let Some(rail) = &body.rail {
                if let Some(orbit) = &rail.orbit
                    && let Around::Body(centre) = &orbit.around
                    && !ready.iter().any(|earlier| earlier.identifier == *centre)
                {
                    return Err(trouble(format!(
                        "it goes round {centre}, which is no body made before it"
                    )));
                }
                // (What it goes round is a body made before it, which is not a ship yet: the rest of the rail is made right here for what it can say.)
                rail_of(rail, |_| Centre::Point([0.0; 3])).map_err(trouble)?;
            }
            ready.push(Ready {
                identifier: identifier.clone(),
                recipe: GeneratorRecipe {
                    instance: identifier.clone(),
                    family: body.family.clone(),
                    parameters: parameters.bytes,
                },
                level,
                body: body.clone(),
                mass: mass as f32,
            });
        }
        let layout = Arc::clone(world.main().overrides.layout());
        let mut made: BTreeMap<NamespacedId, GridId> = BTreeMap::new();
        for Ready {
            identifier,
            recipe,
            level,
            body,
            mass,
        } in ready
        {
            let generator = self
                .add_generator_recipe(world, recipe)
                .map_err(|error| format!("{identifier}: {error}"))?;
            let rail = body
                .rail
                .as_ref()
                .map(|rail| {
                    rail_of(rail, |name| {
                        Centre::Grid(*made.get(name).expect("it was found to be made before"))
                    })
                })
                .transpose()
                .map_err(|error| format!("{identifier}: {error}"))?;
            let ship = world.grids_mut().create(MovingGrid {
                level,
                pose: GridPose::new(
                    WorldPosition::from_voxels(body.position),
                    Orientation::IDENTITY,
                ),
                linear_velocity: [0.0; 3],
                angular_velocity: [0.0; 3],
                mass,
                centre_of_mass: [0.0; 3],
                held: true,
                frozen: false,
                on_pilot_logout: PilotLogout::Stay,
                voxels: Grid::new(Arc::clone(&layout)),
                terrain: Some(GridTerrain {
                    generator,
                    extent: BrickBox::around(BrickCoordinate::ORIGIN, body.extent_bricks),
                    mass,
                }),
                rail,
                sections: Vec::new(),
            });
            made.insert(identifier, ship);
        }
        // The ships that are on rails are where their rails say at the start of the world (a new world is at time 0).
        if let Some(grid) = world.put_ships_on_their_rails(0.0).first() {
            return Err(format!(
                "the rail of ship {grid} puts it where the world cannot hold it"
            ));
        }
        Ok(())
    }

    /// Whether the bodies of the package can be made in a new world: makes them in one with the levels the package says it has, and throws it away.
    fn check_bodies(&self) -> Result<(), String> {
        if self.bodies.is_empty() {
            return Ok(());
        }
        let levels: Vec<(NamespacedId, NamespacedId)> = self
            .spec
            .body_support
            .levels
            .iter()
            .map(|level| (level.clone(), self.spec.default_generator.clone()))
            .collect();
        self.add_bodies(&mut self.new_world_of(0, &levels))
    }

    /// Adds a terrain generator made from a recipe to a world of this package (see [`WorldData::add_generator_recipe`]), with the package's families.
    pub fn add_generator_recipe(
        &self,
        world: &mut WorldData,
        recipe: GeneratorRecipe,
    ) -> Result<GeneratorId, RecipeError> {
        world.add_generator_recipe(recipe, &self.families())
    }

    /// A level that starts with nothing edited and `default` for the generator of its regions.
    fn new_level(&self, seed: u64, default: &NamespacedId) -> LevelData {
        let generators = self.generators(seed);
        let default = generators
            .runtime_id(default)
            .expect("the package's generators include the default of every level");
        let terrain = WorldTerrain::new(generators, default, self.spec.assignment_cube_level)
            .expect("the default generator is available");
        LevelData::new(terrain, Grid::new(self.layout()))
    }

    /// A new world with these levels, each with its identifier and the generator that is the default of its regions, in this order: the first is the main level,
    /// where players start. Nothing is edited, and there are no bodies yet ([`ContentPackage::add_bodies`]). (The world of [`GameContent::new_world`] is the one
    /// level of the spec, with the spec's default generator.)
    ///
    /// Panics with no levels, or with a generator the package does not have, which are mistakes in the game's declaration.
    pub fn new_world_of(&self, seed: u64, levels: &[(NamespacedId, NamespacedId)]) -> WorldData {
        let [(first_id, first_default), rest @ ..] = levels else {
            panic!("a world has at least one level");
        };
        let mut world = WorldData::new(seed, first_id.clone(), self.new_level(seed, first_default));
        for (level_id, default) in rest {
            world
                .add_level(level_id.clone(), self.new_level(seed, default))
                .expect("the levels of a package have distinct identifiers and the same channels");
        }
        world
    }

    /// Adds the blocks of data packs and applies their patches (see the `data_packs` crate). The game's own blocks
    /// keep their numbers and the packs' blocks follow them. Returns what was skipped on purpose, one line each.
    /// Nothing changes if there is an error: the packs are not loaded at all. Call it before a world is loaded,
    /// so that its blocks are not taken for blocks of a plugin that is missing.
    pub fn apply_packs(&mut self, packs: &[RawPack]) -> Result<Vec<String>, PackErrors> {
        let package = self.package_data();
        let base: Vec<(NamespacedId, BlockDefinition)> = package
            .blocks
            .iter()
            .filter_map(|(identifier, definition)| Some((identifier.clone(), definition.clone()?)))
            .collect();
        let loaded = load_packs(
            packs,
            &base,
            &package.properties,
            &self.spec.body_support.context(),
        )?;

        let mut blocks = Registry::new();
        let changed: std::collections::HashMap<&NamespacedId, &BlockDefinition> = loaded
            .blocks
            .iter()
            .map(|(identifier, definition)| (identifier, definition))
            .collect();
        for (_, entry) in self.blocks.iter() {
            let identifier = entry.identifier().clone();
            match (entry.content(), changed.get(&identifier)) {
                (Some(_), Some(definition)) => {
                    blocks
                        .register(identifier, (*definition).clone())
                        .expect("the blocks of a registry are distinct");
                }
                (Some(definition), None) => {
                    blocks
                        .register(identifier, definition.clone())
                        .expect("the blocks of a registry are distinct");
                }
                (None, _) => {
                    blocks.register_missing(identifier);
                }
            }
        }
        for (identifier, definition) in &loaded.blocks {
            if blocks.runtime_id(identifier).is_none() {
                blocks
                    .register(identifier.clone(), definition.clone())
                    .expect("the identifier was just found to be free");
            }
        }
        let candidate = Self {
            blocks,
            spec: self.spec.clone(),
            bodies: self
                .bodies
                .iter()
                .cloned()
                .chain(loaded.bodies.iter().cloned())
                .collect(),
        };
        // The renderer cannot draw more colours than it has: say so now, not when a window opens.
        let table = candidate
            .package_data()
            .block_table()
            .map_err(|error| pack_problem(error.to_string()))?;
        check_materials(&table).map_err(|error| pack_problem(error.to_string()))?;
        // The bodies must be makeable, which only a world can say (their weight, the names of their generators): in a world of no seed that is thrown away.
        candidate.check_bodies().map_err(pack_problem)?;
        *self = candidate;
        Ok(loaded.warnings)
    }
}

/// A rail as the world holds it, from the numbers of a body file (seconds, degrees, bodies by name): the centre an orbit goes round, when it is a body, is
/// what `centre_of` makes of its name.
fn rail_of(data: &BodyRail, centre_of: impl Fn(&NamespacedId) -> Centre) -> Result<Rail, String> {
    let radians = |degrees: f64| degrees * (std::f64::consts::PI / 180.0);
    let spin = data
        .spin
        .as_ref()
        .map(|spin| Spin::new(spin.axis, spin.period, radians(spin.angle_degrees)))
        .transpose()
        .map_err(|error| error.to_string())?;
    let orbit = data
        .orbit
        .as_ref()
        .map(|orbit| {
            let around = match &orbit.around {
                Around::Point(point) => Centre::Point(*point),
                Around::Body(name) => centre_of(name),
            };
            Orbit::new(
                around,
                orbit.normal,
                orbit.radius,
                orbit.period,
                radians(orbit.phase_degrees),
            )
        })
        .transpose()
        .map_err(|error| error.to_string())?;
    Ok(Rail { spin, orbit })
}

fn pack_problem(message: String) -> PackErrors {
    PackErrors(vec![data_packs::PackError {
        file: "the packs together".to_owned(),
        path: String::new(),
        message,
    }])
}

impl GameContent for ContentPackage {
    fn view_channels(&self) -> ViewChannels {
        ViewChannels::new(
            self.block_channel(),
            self.spec.state_channel.as_ref().map(|id| self.channel(id)),
            self.spec.light_channel.as_ref().map(|id| self.channel(id)),
        )
        .with_orientations(
            self.spec
                .orientation_channel
                .as_ref()
                .map(|id| self.channel(id)),
        )
    }

    fn package_data(&self) -> PackageData {
        let mut properties = PropertySchema::new();
        for declaration in &self.spec.properties {
            properties
                .declare(declaration.clone())
                .unwrap_or_else(|error| {
                    panic!("the package's properties are inconsistent: {error}")
                });
        }
        PackageData {
            properties,
            block_registry: self.spec.block_registry_kind.clone(),
            blocks: self
                .blocks
                .iter()
                .map(|(_, entry)| (entry.identifier().clone(), entry.content().cloned()))
                .collect(),
            block_channel: self.spec.block_channel.clone(),
            state_channel: self.spec.state_channel.clone(),
            orientation_channel: self.spec.orientation_channel.clone(),
            light_channel: self.spec.light_channel.clone(),
            ambient_light: self.spec.ambient_light,
        }
    }

    fn new_world(&self, seed: u64) -> WorldData {
        self.new_world_of(
            seed,
            &[(self.spec.level.clone(), self.spec.default_generator.clone())],
        )
    }

    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        let generators = self.generators(seed);
        let families = self.families();
        LoadContext {
            channels: self.spec.channels.clone(),
            registries: vec![(self.spec.block_registry_kind.clone(), &mut self.blocks)],
            generators,
            families,
            fallback_default_generator: self.spec.default_generator.clone(),
            assignment_cube_level: self.spec.assignment_cube_level,
            legacy_level: self.spec.level.clone(),
        }
    }

    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        vec![(self.spec.block_registry_kind.clone(), &self.blocks)]
    }

    fn body_support(&self) -> BodySupport {
        self.spec.body_support.clone()
    }

    fn body_names(&self) -> Vec<NamespacedId> {
        self.bodies
            .iter()
            .map(|(identifier, _)| identifier.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::Brick;
    use voxel_space::BrickCoordinate;
    use world_storage::ParsedWorld;

    use super::*;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    #[derive(Debug)]
    struct Empty;

    impl TerrainGenerator for Empty {
        fn generate_brick(&self, layout: &GridLayout, _coordinate: BrickCoordinate) -> Brick {
            Brick::new(layout)
        }
    }

    fn package() -> ContentPackage {
        let mut blocks = Registry::new();
        blocks
            .register(id("test_game:air"), BlockDefinition::air())
            .unwrap();
        blocks
            .register(id("test_game:rock"), BlockDefinition::cube([0.5; 3]))
            .unwrap();
        ContentPackage::new(
            blocks,
            PackageSpec {
                level: id("test_game:main"),
                block_registry_kind: id("test_game:block"),
                channels: vec![
                    ChannelDefinition::content(id("test_game:block"), 0, id("test_game:block")),
                    ChannelDefinition::number(id("test_game:state"), 0),
                ],
                block_channel: id("test_game:block"),
                state_channel: Some(id("test_game:state")),
                orientation_channel: None,
                light_channel: None,
                properties: Vec::new(),
                generators: Arc::new(|_seed, _layout| {
                    let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
                    generators
                        .register(id("test_game:empty_v1"), Arc::new(Empty))
                        .unwrap();
                    generators
                }),
                families: Arc::new(|_layout| {
                    let family: Arc<dyn GeneratorFamily> = Arc::new(NothingFamily);
                    vec![(id("test_game:nothing_family_v1"), family)]
                }),
                body_support: BodySupport {
                    families: vec![(id("test_game:nothing_family_v1"), Arc::new(RockData))],
                    levels: vec![id("test_game:main")],
                },
                builtin_bodies: vec![(id("test_game:boulder"), BOULDER)],
                pull_strength: 2.0e-10,
                default_generator: id("test_game:empty_v1"),
                assignment_cube_level: 3,
                ambient_light: 90,
            },
        )
    }

    /// The data side of the same family: the radius is all it takes, and the bytes are the radius and the seed of the world.
    #[derive(Debug)]
    struct RockData;

    impl data_packs::FamilyData for RockData {
        fn schema(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object" })
        }

        fn read(
            &self,
            parameters: &serde_json::Value,
            world_seed: u64,
        ) -> Result<data_packs::FamilyParameters, Vec<data_packs::FamilyProblem>> {
            let radius = parameters["radius"].as_f64().ok_or_else(|| {
                vec![data_packs::FamilyProblem {
                    path: "/radius".to_owned(),
                    message: "a radius".to_owned(),
                }]
            })?;
            let mut bytes = radius.to_bits().to_le_bytes().to_vec();
            bytes.extend_from_slice(&world_seed.to_le_bytes());
            Ok(data_packs::FamilyParameters {
                bytes,
                surface_radius: radius,
            })
        }
    }

    /// The package's own body: a boulder of 50 voxels, at (100, 0, -40) in the level of the package.
    const BOULDER: &str = r#"{ "level": "test_game:main", "position": [100, 0, -40],
        "terrain": { "family": "test_game:nothing_family_v1", "parameters": { "radius": 50 } },
        "extent_bricks": 3, "surface_gravity": 4 }"#;

    /// A family whose generators make nothing, however they are set.
    #[derive(Debug)]
    struct NothingFamily;

    impl GeneratorFamily for NothingFamily {
        fn build(
            &self,
            _parameters: &[u8],
        ) -> Result<Arc<dyn TerrainGenerator>, terrain::FamilyError> {
            Ok(Arc::new(Empty))
        }
    }

    #[test]
    fn a_recipe_made_with_the_packages_families_is_in_the_world_and_is_made_again_when_it_is_loaded()
     {
        let content = package();
        let mut world = content.new_world(42);
        let recipe = GeneratorRecipe {
            instance: id("test_game:the_moon"),
            family: id("test_game:nothing_family_v1"),
            parameters: vec![1, 2, 3],
        };
        let made = content
            .add_generator_recipe(&mut world, recipe.clone())
            .unwrap();
        assert_eq!(
            world
                .main()
                .terrain
                .generators()
                .runtime_id(&id("test_game:the_moon")),
            Some(made)
        );
        // A family the package does not have is refused.
        let mut stranger = recipe.clone();
        stranger.instance = id("test_game:the_sun");
        stranger.family = id("somebody_else:star_family_v1");
        assert_eq!(
            content.add_generator_recipe(&mut world, stranger),
            Err(RecipeError::UnknownFamily(id(
                "somebody_else:star_family_v1"
            )))
        );

        let bytes = world.save(&content.registries()).unwrap();
        let mut later = package();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let (loaded, report) = parsed.load(later.load_context(42)).unwrap();
        assert!(report.missing_content.is_empty() && report.refused_recipes.is_empty());
        assert_eq!(loaded.generator_recipes(), &[recipe]);
        let generators = loaded.main().terrain.generators();
        let number = generators.runtime_id(&id("test_game:the_moon")).unwrap();
        assert!(!generators.entry(number).unwrap().is_missing());
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
    }

    #[test]
    fn the_package_says_what_its_spec_declares() {
        let content = package();
        assert_eq!(content.block_channel().get(), 0);
        assert_eq!(content.state_channel().get(), 1);
        assert_eq!(content.ambient_light(), 90);
        let view = content.view_channels();
        assert_eq!(
            (view.blocks.get(), view.states.map(|s| s.get()), view.light),
            (0, Some(1), None)
        );
        assert_eq!(content.block_table().block_count(), 2);
        assert_eq!(content.registries().len(), 1);
    }

    #[test]
    #[should_panic(expected = "the package has no channel test_game:nothing")]
    fn asking_for_a_channel_that_was_not_declared_is_a_loud_mistake() {
        package().channel(&id("test_game:nothing"));
    }

    #[test]
    fn a_new_world_is_in_the_declared_level_unedited_and_saves_and_loads_back_the_same() {
        let content = package();
        let world = content.new_world(42);
        assert_eq!(world.world_seed, 42);
        assert_eq!(world.main().overrides.brick_count(), 0);
        let bytes = world.save(&content.registries()).unwrap();

        let mut later = package();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let seed = parsed.world_seed();
        let (loaded, report) = parsed.load(later.load_context(seed)).unwrap();
        assert_eq!(seed, 42);
        assert!(report.missing_content.is_empty() && report.extra_channels.is_empty());
        assert_eq!(
            loaded.main().preferred_default_generator(),
            &id("test_game:empty_v1")
        );
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
    }

    #[test]
    fn a_new_world_of_several_levels_has_them_in_that_order_each_with_the_default_generator_it_was_given()
     {
        let mut content = package();
        content.spec.generators = Arc::new(|_seed, _layout| {
            let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
            generators
                .register(id("test_game:empty_v1"), Arc::new(Empty))
                .unwrap();
            generators
                .register(id("test_game:other_v1"), Arc::new(Empty))
                .unwrap();
            generators
        });
        let world = content.new_world_of(
            7,
            &[
                (id("test_game:space"), id("test_game:other_v1")),
                (id("test_game:main"), id("test_game:empty_v1")),
            ],
        );
        let levels: Vec<(String, String)> = world
            .levels()
            .map(|(_, level_id, level)| {
                (
                    level_id.to_string(),
                    level.preferred_default_generator().to_string(),
                )
            })
            .collect();
        assert_eq!(
            levels,
            [
                (
                    "test_game:space".to_owned(),
                    "test_game:other_v1".to_owned()
                ),
                ("test_game:main".to_owned(), "test_game:empty_v1".to_owned()),
            ]
        );
        // The first is where players start: the main level.
        assert_eq!(
            world.level_id(voxel_space::LevelIndex::MAIN),
            Some(&id("test_game:space"))
        );
        assert_eq!(world.world_seed, 7);
        // And it saves and loads back the same, with both levels.
        let bytes = world.save(&content.registries()).unwrap();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let (loaded, report) = parsed.load(content.load_context(7)).unwrap();
        assert!(report.missing_content.is_empty());
        assert_eq!(loaded.levels().count(), 2);
        assert_eq!(loaded.save(&content.registries()).unwrap(), bytes);
    }

    #[test]
    fn the_generators_are_made_for_the_seed_of_the_world() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut content = package();
        let remembered = Arc::clone(&seen);
        content.spec.generators = Arc::new(move |seed, layout| {
            remembered
                .lock()
                .unwrap()
                .push((seed, layout.channel_count()));
            let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
            generators
                .register(id("test_game:empty_v1"), Arc::new(Empty))
                .unwrap();
            generators
        });
        let world = content.new_world(7);
        let bytes = world.save(&content.registries()).unwrap();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let _ = parsed.load(content.load_context(7)).unwrap();
        assert_eq!(*seen.lock().unwrap(), [(7, 2), (7, 2)]);
    }

    // ---- bodies ----

    fn pull() -> f64 {
        package().spec.pull_strength
    }

    #[test]
    fn the_bodies_of_a_package_are_ships_in_a_new_world_with_a_generator_a_weight_and_a_place() {
        let content = package();
        let mut world = content.new_world(42);
        assert!(
            world.grids().is_empty(),
            "a new world has no bodies until they are made"
        );
        content.add_bodies(&mut world).unwrap();

        assert_eq!(world.grids().len(), 1);
        let (_, ship) = world.grids().iter().next().unwrap();
        assert_eq!(ship.level, voxel_space::LevelIndex::MAIN);
        assert!(ship.held && !ship.frozen);
        assert_eq!(ship.centre_of_mass, [0.0; 3]);
        assert_eq!(ship.linear_velocity, [0.0; 3]);
        assert_eq!(ship.angular_velocity, [0.0; 3]);
        assert_eq!(ship.voxels.brick_count(), 0);
        // 100 voxels along x is 3 bricks and 4 voxels; -40 along z is brick -2 and 24 voxels.
        assert_eq!(
            ship.pose,
            voxel_space::GridPose::new(
                voxel_space::WorldPosition::new(
                    voxel_space::BrickCoordinate::new(3, 0, -2),
                    [4.0, 0.0, 24.0]
                ),
                voxel_space::Orientation::IDENTITY
            )
        );
        // As heavy as it must be to pull 4 metres a second a second at its surface, 50 voxels from its middle.
        let mass = (4.0f64 * 50.0 * 50.0 / pull()) as f32;
        assert_eq!(ship.mass, mass);
        let terrain = ship.terrain.expect("a body has terrain");
        assert_eq!(terrain.mass, mass);
        assert_eq!(
            terrain.extent,
            voxel_space::BrickBox::around(voxel_space::BrickCoordinate::ORIGIN, 3)
        );
        // Its generator is the recipe of the body, made for the seed of this world.
        let generators = world.main().terrain.generators();
        assert_eq!(
            generators.entry(terrain.generator).unwrap().identifier(),
            &id("test_game:boulder")
        );
        let mut parameters = 50.0f64.to_bits().to_le_bytes().to_vec();
        parameters.extend_from_slice(&42u64.to_le_bytes());
        assert_eq!(
            world.generator_recipes(),
            &[GeneratorRecipe {
                instance: id("test_game:boulder"),
                family: id("test_game:nothing_family_v1"),
                parameters,
            }]
        );
        // Another world is another seed.
        let mut other = content.new_world(43);
        content.add_bodies(&mut other).unwrap();
        assert_ne!(other.generator_recipes(), world.generator_recipes());
    }

    #[test]
    fn a_world_with_bodies_saves_and_loads_with_them_and_a_load_does_not_make_them_again() {
        let content = package();
        let mut world = content.new_world(42);
        content.add_bodies(&mut world).unwrap();
        let bytes = world.save_with_ships(&content.registries()).unwrap();
        let mut file = Vec::new();
        let mut ships = world_storage::MemoryShipFiles::new();
        bytes
            .write_with(&mut ships, |written| {
                file = written.to_vec();
                Ok(())
            })
            .unwrap();

        let mut later = package();
        let parsed = ParsedWorld::parse(&file).unwrap();
        let (loaded, report) = parsed.load(later.load_context(42)).unwrap();
        assert!(report.missing_content.is_empty() && report.refused_recipes.is_empty());
        assert_eq!(loaded.generator_recipes(), world.generator_recipes());
        // One ship, in its file (none in memory), and the load did not add the package's bodies to it.
        assert_eq!(loaded.grids().len(), 0);
        assert_eq!(loaded.grids().parked().count(), 1);
    }

    #[test]
    fn the_bodies_of_packs_follow_the_games_own_and_may_weigh_what_they_say() {
        let mut content = package();
        content
            .apply_packs(&[RawPack {
                folder: "my_pack".to_owned(),
                manifest: r#"{ "id": "my_pack" }"#.to_owned(),
                blocks: Vec::new(),
                patches: Vec::new(),
                bodies: vec![RawFile {
                    name: "pebble".to_owned(),
                    text: r#"{ "level": "test_game:main",
                        "terrain": { "family": "test_game:nothing_family_v1", "parameters": { "radius": 2 } },
                        "extent_bricks": 1, "mass": 12 }"#
                        .to_owned(),
                }],
            }])
            .unwrap();
        let mut world = content.new_world(5);
        content.add_bodies(&mut world).unwrap();
        let names: Vec<String> = world
            .generator_recipes()
            .iter()
            .map(|recipe| recipe.instance.to_string())
            .collect();
        assert_eq!(names, ["test_game:boulder", "my_pack:pebble"]);
        let ships: Vec<f32> = world.grids().iter().map(|(_, ship)| ship.mass).collect();
        assert_eq!(ships.len(), 2);
        assert_eq!(ships[1], 12.0);
        // At the origin, because the file said no place.
        let (_, pebble) = world.grids().iter().nth(1).unwrap();
        assert_eq!(
            pebble.pose.position.brick(),
            voxel_space::BrickCoordinate::ORIGIN
        );
    }

    #[test]
    fn a_pack_that_puts_a_body_in_a_level_the_game_does_not_have_is_refused_and_nothing_is_loaded()
    {
        let mut content = package();
        let error = content
            .apply_packs(&[RawPack {
                folder: "my_pack".to_owned(),
                manifest: r#"{ "id": "my_pack" }"#.to_owned(),
                blocks: vec![RawFile {
                    name: "marble".to_owned(),
                    text: r#"{ "color": [1, 1, 1] }"#.to_owned(),
                }],
                patches: Vec::new(),
                bodies: vec![RawFile {
                    name: "pebble".to_owned(),
                    text: r#"{ "level": "test_game:below",
                        "terrain": { "family": "test_game:nothing_family_v1", "parameters": { "radius": 2 } },
                        "extent_bricks": 1, "mass": 12 }"#
                        .to_owned(),
                }],
            }])
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "my_pack/bodies/pebble.json: /level: the game has no level \"test_game:below\"; its levels are \"test_game:main\""
        );
        // (Neither the block nor the body of the pack is in.)
        assert_eq!(block_names(&content), ["test_game:air", "test_game:rock"]);
        let mut world = content.new_world(5);
        content.add_bodies(&mut world).unwrap();
        assert_eq!(world.grids().len(), 1);
    }

    #[test]
    fn bodies_that_cannot_be_made_in_a_world_change_nothing_and_say_why() {
        let content = package();
        // A world that has not the level the body is in.
        let mut elsewhere =
            content.new_world_of(5, &[(id("test_game:elsewhere"), id("test_game:empty_v1"))]);
        let error = content.add_bodies(&mut elsewhere).unwrap_err();
        assert_eq!(
            error,
            "test_game:boulder: the world has no level test_game:main"
        );
        assert!(elsewhere.grids().is_empty() && elsewhere.generator_recipes().is_empty());
        // A world that already has them.
        let mut world = content.new_world(5);
        content.add_bodies(&mut world).unwrap();
        let error = content.add_bodies(&mut world).unwrap_err();
        assert_eq!(
            error,
            "test_game:boulder: the world already has a terrain generator called test_game:boulder"
        );
        assert_eq!(world.grids().len(), 1);
        assert_eq!(world.generator_recipes().len(), 1);
        // A pull that is no pull.
        let mut no_pull = package();
        no_pull.spec.pull_strength = 0.0;
        let mut third =
            no_pull.new_world_of(5, &[(id("test_game:main"), id("test_game:empty_v1"))]);
        assert_eq!(
            no_pull.add_bodies(&mut third).unwrap_err(),
            "the pull of gravity is above nothing, not 0"
        );
        assert!(third.grids().is_empty());
    }

    #[test]
    fn a_body_that_cannot_be_made_stops_the_others_from_being_made_before_it() {
        let mut content = package();
        content
            .apply_packs(&[RawPack {
                folder: "my_pack".to_owned(),
                manifest: r#"{ "id": "my_pack" }"#.to_owned(),
                blocks: Vec::new(),
                patches: Vec::new(),
                bodies: vec![RawFile {
                    name: "pebble".to_owned(),
                    text: r#"{ "level": "test_game:main",
                        "terrain": { "family": "test_game:nothing_family_v1", "parameters": { "radius": 2 } },
                        "extent_bricks": 1, "mass": 12 }"#
                        .to_owned(),
                }],
            }])
            .unwrap();
        // A world in which the second body's name is already taken (by a recipe that is not a body's).
        let mut world = content.new_world(5);
        let taken = GeneratorRecipe {
            instance: id("my_pack:pebble"),
            family: id("test_game:nothing_family_v1"),
            parameters: Vec::new(),
        };
        content
            .add_generator_recipe(&mut world, taken.clone())
            .unwrap();
        let error = content.add_bodies(&mut world).unwrap_err();
        assert_eq!(
            error,
            "my_pack:pebble: the world already has a terrain generator called my_pack:pebble"
        );
        // The first body, which could have been made, was not.
        assert!(world.grids().is_empty());
        assert_eq!(world.generator_recipes(), &[taken]);
    }

    #[test]
    fn a_pack_whose_body_is_too_heavy_to_be_a_ship_is_refused_when_it_is_loaded() {
        let mut content = package();
        let error = content
            .apply_packs(&[RawPack {
                folder: "my_pack".to_owned(),
                manifest: r#"{ "id": "my_pack" }"#.to_owned(),
                blocks: Vec::new(),
                patches: Vec::new(),
                bodies: vec![RawFile {
                    name: "dense".to_owned(),
                    text: r#"{ "level": "test_game:main",
                        "terrain": { "family": "test_game:nothing_family_v1", "parameters": { "radius": 2 } },
                        "extent_bricks": 1, "surface_gravity": 1e40 }"#
                        .to_owned(),
                }],
            }])
            .unwrap_err();
        assert_eq!(error.0.len(), 1);
        assert!(
            error.to_string().contains("my_pack:dense: ")
                && error
                    .to_string()
                    .contains("is not a weight a ship can have"),
            "{error}"
        );
        // Nothing of it is in.
        assert_eq!(content.body_names(), [id("test_game:boulder")]);
    }

    // ---- bodies on rails ----

    /// A pack of bodies, each with the text of its file.
    fn pack_of_bodies(bodies: &[(&str, &str)]) -> RawPack {
        RawPack {
            folder: "my_pack".to_owned(),
            manifest: r#"{ "id": "my_pack" }"#.to_owned(),
            blocks: Vec::new(),
            patches: Vec::new(),
            bodies: bodies
                .iter()
                .map(|(name, text)| RawFile {
                    name: (*name).to_owned(),
                    text: (*text).to_owned(),
                })
                .collect(),
        }
    }

    /// A body of the package's family in the package's level, with this rail (and this other text before it) written in.
    fn body_with(extra: &str, rail: &str) -> String {
        format!(
            r#"{{ "level": "test_game:main", {extra}
                "terrain": {{ "family": "test_game:nothing_family_v1", "parameters": {{ "radius": 50 }} }},
                "extent_bricks": 3, "mass": 1e20, "rail": {rail} }}"#
        )
    }

    fn degrees(angle: f64) -> f64 {
        angle * (std::f64::consts::PI / 180.0)
    }

    #[test]
    fn a_body_with_a_rail_is_made_on_it_and_put_where_the_rail_says_at_the_start_of_the_world() {
        let mut content = package();
        content
            .apply_packs(&[pack_of_bodies(&[
                (
                    "planet",
                    &body_with(
                        "",
                        r#"{ "spin": { "axis": [0, 1, 0], "period": 3600, "angle": 90 },
                            "orbit": { "around": [1000, 0, -50], "normal": [0, 1, 0], "radius": 2e6, "period": 40000, "phase": 90 } }"#,
                    ),
                ),
                (
                    "rock",
                    &body_with(
                        r#""position": [7, 8, 9],"#,
                        r#"{ "spin": { "axis": [1, 0, 0], "period": -60 } }"#,
                    ),
                ),
            ])])
            .unwrap();
        let mut world = content.new_world(5);
        content.add_bodies(&mut world).unwrap();
        // [the boulder, the planet, the rock]
        let ships: Vec<_> = world
            .grids()
            .iter()
            .map(|(id, ship)| (id, ship.clone()))
            .collect();
        assert_eq!(ships.len(), 3);
        let (_, boulder) = &ships[0];
        assert_eq!(boulder.rail, None, "a body with no rail has none");

        let (_, planet) = &ships[1];
        let rail = planet.rail.expect("the planet is on a rail");
        let spin = rail.spin.unwrap();
        assert_eq!(spin.period, 3600.0);
        assert!((spin.angle - degrees(90.0)).abs() < 1e-12, "{}", spin.angle);
        let orbit = rail.orbit.unwrap();
        assert_eq!(
            orbit.around,
            world_storage::rail::Centre::Point([1000.0, 0.0, -50.0])
        );
        assert_eq!((orbit.radius, orbit.period), (2.0e6, 40_000.0));
        assert!((orbit.phase - degrees(90.0)).abs() < 1e-12);
        // At the start it is where the rail puts it, and moving as the rail says, not where the file's nothing put it.
        let state = rail.state(0.0, [1000.0, 0.0, -50.0], [0.0; 3]);
        assert_eq!(
            planet.pose.position,
            voxel_space::WorldPosition::from_voxels(state.position.unwrap())
        );
        assert_eq!(planet.pose.orientation, state.orientation.unwrap());
        assert_eq!(
            planet.linear_velocity,
            state.velocity.map(|component| component as f32)
        );
        assert_ne!(planet.linear_velocity, [0.0; 3]);

        // A body that only turns stays where the file put it and does not move.
        let (_, rock) = &ships[2];
        assert_eq!(
            rock.pose.position,
            voxel_space::WorldPosition::from_voxels([7.0, 8.0, 9.0])
        );
        assert_eq!(rock.linear_velocity, [0.0; 3]);
        assert!(rock.rail.unwrap().orbit.is_none());
        assert_ne!(rock.angular_velocity, [0.0; 3]);
    }

    #[test]
    fn a_body_that_goes_round_another_body_goes_round_the_ship_that_was_made_for_it() {
        let mut content = package();
        content
            .apply_packs(&[pack_of_bodies(&[
                (
                    "planet",
                    &body_with(
                        "",
                        r#"{ "orbit": { "around": [0, 0, 0], "radius": 1e6, "period": 40000 } }"#,
                    ),
                ),
                (
                    "zmoon",
                    &body_with(
                        "",
                        r#"{ "orbit": { "around": "my_pack:planet", "radius": 2e4, "period": 500 } }"#,
                    ),
                ),
            ])])
            .unwrap();
        let mut world = content.new_world(5);
        content.add_bodies(&mut world).unwrap();
        let ships: Vec<_> = world
            .grids()
            .iter()
            .map(|(id, ship)| (id, ship.clone()))
            .collect();
        let (planet_id, planet) = &ships[1];
        let (_, moon) = &ships[2];
        assert_eq!(
            moon.rail.unwrap().orbit.unwrap().around,
            world_storage::rail::Centre::Grid(*planet_id)
        );
        // Twenty thousand voxels from the planet at the start, and with its speed and its own.
        let apart: Vec<f64> = (0..3)
            .map(|axis| {
                moon.pose.position.in_voxels()[axis] - planet.pose.position.in_voxels()[axis]
            })
            .collect();
        let distance = (apart[0] * apart[0] + apart[1] * apart[1] + apart[2] * apart[2]).sqrt();
        assert!((distance - 2.0e4).abs() < 1e-2, "{distance}");
        let own = std::f64::consts::TAU * 2.0e4 / 500.0;
        let relative: Vec<f64> = (0..3)
            .map(|axis| {
                f64::from(moon.linear_velocity[axis]) - f64::from(planet.linear_velocity[axis])
            })
            .collect();
        let speed =
            (relative[0] * relative[0] + relative[1] * relative[1] + relative[2] * relative[2])
                .sqrt();
        assert!((speed - own).abs() < 1e-3, "{speed} against {own}");
    }

    #[test]
    fn a_body_cannot_go_round_a_body_that_is_not_there_or_not_there_yet_or_itself() {
        let orbit_around = |name: &str| {
            body_with(
                "",
                &format!(r#"{{ "orbit": {{ "around": "{name}", "radius": 100, "period": 60 }} }}"#),
            )
        };
        for (about, files, message) in [
            (
                "nothing",
                vec![("moon", orbit_around("my_pack:nothing"))],
                "my_pack:moon: it goes round my_pack:nothing, which is no body made before it",
            ),
            (
                "itself",
                vec![("moon", orbit_around("my_pack:moon"))],
                "my_pack:moon: it goes round my_pack:moon, which is no body made before it",
            ),
            (
                "a body that comes after it (bodies are made in the order of their files' names)",
                vec![
                    ("amoon", orbit_around("my_pack:planet")),
                    (
                        "planet",
                        body_with(
                            "",
                            r#"{ "orbit": { "around": [0, 0, 0], "radius": 100, "period": 60 } }"#,
                        ),
                    ),
                ],
                "my_pack:amoon: it goes round my_pack:planet, which is no body made before it",
            ),
        ] {
            let mut content = package();
            let files: Vec<(&str, &str)> = files
                .iter()
                .map(|(name, text)| (*name, text.as_str()))
                .collect();
            let error = content.apply_packs(&[pack_of_bodies(&files)]).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("the packs together: {message}"),
                "{about}"
            );
            // Nothing of the pack is in.
            assert_eq!(content.body_names(), [id("test_game:boulder")]);
        }
    }

    #[test]
    fn a_rail_whose_numbers_make_no_motion_is_refused_when_the_pack_is_loaded() {
        // A radius so great that the place is no place the world can hold.
        let mut content = package();
        let error = content
            .apply_packs(&[pack_of_bodies(&[(
                "far",
                &body_with(
                    "",
                    r#"{ "orbit": { "around": [0, 0, 0], "radius": 1e300, "period": 60 } }"#,
                ),
            )])])
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "my_pack/bodies/far.json: /rail/orbit/radius: 1e+300 is more than 1000000000000000 voxels"
        );
        assert_eq!(content.body_names(), [id("test_game:boulder")]);
    }

    #[test]
    fn a_pack_cannot_make_a_body_that_has_the_name_of_one_the_game_has() {
        let mut content = package();
        // (A pack of the game's own namespace, which nothing forbids.)
        let error = content
            .apply_packs(&[RawPack {
                folder: "test_game".to_owned(),
                manifest: r#"{ "id": "test_game" }"#.to_owned(),
                blocks: Vec::new(),
                patches: Vec::new(),
                bodies: vec![RawFile {
                    name: "boulder".to_owned(),
                    text: BOULDER.to_owned(),
                }],
            }])
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the packs together: test_game:boulder: the world already has a terrain generator called test_game:boulder"
        );
        assert_eq!(content.body_names(), [id("test_game:boulder")]);
    }

    #[test]
    fn a_package_with_no_bodies_takes_packs_as_it_did_and_says_what_its_bodies_may_name() {
        let mut spec = package().spec;
        spec.builtin_bodies.clear();
        spec.body_support = BodySupport::default();
        let mut bare = ContentPackage::new(Registry::new(), spec);
        assert!(bare.body_names().is_empty());
        assert!(bare.body_support().families.is_empty() && bare.body_support().levels.is_empty());
        bare.apply_packs(&[pack("my_pack", "", &[], &[])]).unwrap();
        // And a package that has some says what a body of a pack may name.
        let support = package().body_support();
        assert_eq!(support.levels, [id("test_game:main")]);
        assert_eq!(
            support
                .families
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>(),
            [id("test_game:nothing_family_v1")]
        );
    }

    #[test]
    #[should_panic(expected = "the game's own body test_game:bad is wrong")]
    fn a_game_whose_own_body_is_wrong_is_a_loud_mistake() {
        let mut content = package();
        content.spec.builtin_bodies = vec![(id("test_game:bad"), "{}")];
        let _ = ContentPackage::new(Registry::new(), content.spec);
    }

    // ---- data packs ----

    use data_packs::RawFile;

    fn pack(
        folder: &str,
        depends_on: &str,
        blocks: &[(&str, &str)],
        patches: &[(&str, &str)],
    ) -> RawPack {
        let files = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(name, text)| RawFile {
                    name: (*name).to_owned(),
                    text: (*text).to_owned(),
                })
                .collect()
        };
        RawPack {
            folder: folder.to_owned(),
            manifest: format!(r#"{{ "id": "{folder}", "depends_on": [{depends_on}] }}"#),
            blocks: files(blocks),
            patches: files(patches),
            bodies: Vec::new(),
        }
    }

    fn block_names(content: &ContentPackage) -> Vec<String> {
        content
            .package_data()
            .blocks
            .iter()
            .map(|(identifier, _)| identifier.to_string())
            .collect()
    }

    #[test]
    fn packs_add_blocks_after_the_games_own_and_leave_their_numbers_alone() {
        let mut content = package();
        let before = content.package_data();
        let warnings = content
            .apply_packs(&[pack(
                "my_pack",
                "",
                &[("marble", r#"{ "color": [0.9, 0.9, 1.0] }"#)],
                &[],
            )])
            .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(
            block_names(&content),
            ["test_game:air", "test_game:rock", "my_pack:marble"]
        );
        let after = content.package_data();
        assert_eq!(after.blocks[..2], before.blocks[..]);
        assert_eq!(
            after.blocks[2].1,
            Some(BlockDefinition::cube([0.9, 0.9, 1.0]))
        );
        // The table has the new block too (it is what a server's plugins and its clients see).
        assert_eq!(content.block_table().block_count(), 3);
    }

    #[test]
    fn a_pack_can_patch_the_games_blocks_and_what_it_skipped_on_purpose_is_returned() {
        let mut content = package();
        let patch = r#"{ "target": "test_game:rock", "operations": [
            { "op": "set", "path": "color", "value": [1, 0, 0] } ] }"#;
        let skipped = r#"{ "target": "test_game:ghost", "optional": true, "operations": [
            { "op": "set", "path": "color", "value": [1, 0, 0] } ] }"#;
        let warnings = content
            .apply_packs(&[pack(
                "my_pack",
                r#""test_game""#,
                &[],
                &[("a_red", patch), ("b_skipped", skipped)],
            )])
            .unwrap();
        assert_eq!(
            warnings,
            [
                "my_pack/patches/b_skipped.json: skipped: there is no block test_game:ghost (is its pack or plugin loaded?)"
            ]
        );
        assert_eq!(
            content.package_data().blocks[1].1,
            Some(BlockDefinition::cube([1.0, 0.0, 0.0]))
        );
    }

    #[test]
    fn packs_that_cannot_be_loaded_change_nothing() {
        let mut content = package();
        let before = content.package_data();
        let errors = content
            .apply_packs(&[pack(
                "my_pack",
                "",
                &[("bad", r#"{ "colour": [0,0,0] }"#)],
                &[],
            )])
            .unwrap_err();
        assert!(
            errors.to_string().contains("my_pack/blocks/bad.json"),
            "{errors}"
        );
        assert_eq!(content.package_data(), before);
    }

    #[test]
    fn packs_whose_blocks_need_more_colours_than_can_be_drawn_are_refused_and_change_nothing() {
        let mut content = package();
        let before = content.package_data();
        // Two blocks of the game, one of the pack, and as many colours for the states of one as can be drawn.
        let most = renderer::MAXIMUM_GAME_MATERIALS;
        let colors = vec!["[0, 0, 0]"; most].join(", ");
        let heavy = format!(r#"{{ "color": [0,0,0], "state_colors": [{colors}] }}"#);
        let errors = content
            .apply_packs(&[pack("my_pack", "", &[("heavy", &heavy)], &[])])
            .unwrap_err();
        assert_eq!(
            errors.to_string(),
            format!(
                "the packs together: the blocks need {} colours and at most {most} can be drawn",
                most + 3
            )
        );
        assert_eq!(content.package_data(), before);
    }

    #[test]
    fn a_block_of_a_missing_plugin_keeps_its_place_when_packs_are_added() {
        let mut content = package();
        content.blocks.register_missing(id("gone_plugin:thing"));
        content
            .apply_packs(&[pack(
                "my_pack",
                "",
                &[("marble", r#"{ "color": [0,0,0] }"#)],
                &[],
            )])
            .unwrap();
        assert_eq!(
            block_names(&content),
            [
                "test_game:air",
                "test_game:rock",
                "gone_plugin:thing",
                "my_pack:marble"
            ]
        );
        assert_eq!(content.package_data().blocks[2].1, None);
    }
}
