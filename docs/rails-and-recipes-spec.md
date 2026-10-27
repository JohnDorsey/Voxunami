# Phase 40: planets on rails, and planets from data

Status: started 2026-10-26 (the owner: "work on planets on rails and data-driven planets"). What I checked in the code is marked **[code]**; what I believe is
marked **[belief]**. This builds on `planet-spec.md` (planets are ships), `moving-grids-spec.md` and `future-goals.md` ("Space exploration").

## 1. What the owner has said that this serves

- Planets are a kind of ship with slightly different rules; **all planets will be on rails**; they are never docked to each other or to a moon; a ship that docks with
  one is written into its grid (owner, 2026-10-25). Built: all but the rails.
- Cave is to become a space-exploration game (KSP, KSA): ships will someday go on rails too, when far from the players or in a time warp (`future-goals.md`).
- A planet's generator parameters are not in the file: `GridTerrain` holds the number of a generator in the world's table, and that generator is *code* with its
  numbers built in (`base_blocks:small_planet_v1` is `PlanetSettings::one_percent_of_earth(world seed)`). So a planet of another size or another seed is a
  change of Rust (the "(n)" of `planet-spec.md`).

## 2. What is there now

- **[code]** A planet is a `MovingGrid` with `terrain: Some(GridTerrain { generator, extent, mass })`, `held`, a pose and velocities that nothing changes. Its body is a
  fixed body in each frame of its level (`grid_bodies`, `sync_planets`); it is moved by `set_state` (a teleport) if the world moves it.
- **[code]** The terrain generators of a world are `Registry<Arc<dyn TerrainGenerator>>`, built by the game's code from the world's seed
  (`BaseBlocks::terrain_generators(seed, block_channel)`); the world file holds their identifiers (`engine:generators`), a ship file names its generator by identifier,
  the wire (protocol 8) sends a planet's generator by name, and the client looks the name up in the registry it builds from the same code. *Nothing about a generator's
  numbers is in any file, any message or any package.*
- **[code]** The tick (`ServerWorld::tick`) starts at zero at each start of a server and is not saved. Nothing in the world has a time.
- **[code]** `data_packs` loads JSON packs (blocks and patches) on the server only; a client learns the result through the content package, which "holds no
  generators" (`content-package-spec.md`).
- **[code]** The bodies of a level are in frames, one for each place (`Frame`): `BodyWorld` is `f32` around a brick of origin, and a frame's origin never moves.

## 3. The design

### 3.1 A world clock (40a)

A planet on rails is a function of time, and time must survive a restart: a world saved at tick 100,000 and loaded must put the planets where they were, not at
their start. So **`WorldData` has a clock: whole ticks since the world was made** (`u64`), saved in the world file (`engine:clock`, version 1, optional: a file without it
is at tick 0), advanced by the server once a tick, and read by rules as `ServerWorld::world_tick()`. (The server's `tick` stays the count since the start; recordings,
messages and `Tick` are unchanged.) A tick is a twentieth of a second. **Time warp** (later) is a clock that advances by more than one a tick; nothing here prevents it,
and nothing that reads the clock may read the wall clock or count ticks itself.

### 3.2 Generator recipes (40b): planets from data

A **recipe** is `(instance identifier, family identifier, parameters)`:

- A **family** is code, and a file format like every generator: `base_blocks:planet_v1` is the algorithm of `terrain::planet` and never changes once a world has used it.
  It is registered with the game next to the generators (a `GeneratorFamilies` registry: `build(seed-independent parameters, block channel, materials) -> generator`),
  and it gives its parameters a canonical byte form and a JSON form (so a pack can write them).
- An **instance** is a name chosen by the data (`voxunami_cave:mini_cosmos/planet`); it is the identifier in the world's table of generators, so a ship file, the world file
  and the wire name it as they name any generator and nothing in them changes.
- **The recipes are in the world** (`engine:generator_recipes`, version 1, optional): a world is self-contained, and a pack that is edited afterwards does not change
  a planet that exists (the hard rule: a released generator is never edited). On load each recipe whose family the game has is built and put in the table under its
  instance name; one whose family is missing becomes a placeholder, as a missing generator does today, and its bricks are kept.
- **The recipes are in the world's description** (what a server sends a joining client as the shape of the world, and a recording carries), so the client builds the same instances with the families it has because it is the same program. *Changed from the first text of this document, which had a new list in the content package and protocol 9: the description already travels, is already loaded with the game's own content, and needs no new message.*
- The **seed** is a parameter, so two planets of one family differ. Data may say `"seed": "world"`, which is resolved to the world's seed (plus a salt of the instance's name) *when the
  recipe is made*, and the resolved number is what is stored.
- A recipe can be made while a server runs (a command, a plugin): the table gains an entry, clients are told by a message. *Built later than the rest of 40b*.

### 3.3 Bodies in data (40c)

A pack (or the game's built-in data) may hold **bodies**: one JSON file for each planet, naming its recipe, its extent, its mass (or the gravity at its surface and
the family's radius, so that M = g R^2 / G), the level it is in, where it is and what it does (section 3.4). Cave's `mini_cosmos` is rewritten as data: the same planet, the
same numbers (a test pins the generator's output for it, bit for bit, against `small_planet_v1`, so that the new route can be shown to make the planet that worlds have).
`check-pack` knows the kind, and the generated schema describes it. The world takes the bodies when it is *made*, not every time it is loaded.

### 3.4 Rails (40d and 40e)

**A rail is part of a planet** (`MovingGrid::rail: Option<Rail>`, a ship-file section `engine:rail`, version 1):

- `spin`: the planet turns about an axis (in its parent's frame) once in `period` seconds, from an `angle` at tick 0.
- `orbit`: the planet's centre goes round a `parent` (another planet of the level, or a fixed point) on a circle of `radius` in a plane (a normal), once in `period` seconds, from
  a `phase` at tick 0. *Circular only to begin with*; the type is an enum so that Kepler's ellipses (`eccentricity`, `argument of periapsis`) can be added without changing a file.
- Position and orientation are **a pure function of the world clock**: `libm` sine and cosine in `f64`, the same bits on every machine; no state is integrated, so there
  is nothing to drift and nothing to save but the clock. The velocity is the analytic derivative.
- A **rails system** (Physics phase, before the bodies) sets the pose and velocities of every planet that has a rail from the clock. A planet's pose then changes every tick,
  and the replication of poses already tells clients of a changed pose (**[belief]**; I will check it first).

**The bodies of a planet that moves** (40e) are the hard part. *As built (2026-10-26)*, which differs from the first design in three places that the work found (marked *changed*):

1. **A planet on a rail is a kinematic body** (velocity-based; `BodyKind::Kinematic` in `physics`): Rapier moves it by its velocities, a ship resting on it is carried by friction and one it turns into is hit. Every planet in a frame that follows a planet is kinematic too (they move against the frame); a planet with no rail in a frame that does not follow stands fixed, as before. *Changed:* it is set every tick to where it was **at the start** of the tick's step (its pose at the end of the tick, backed up by its velocities for one tick), because the world has it where it is at the end: set to the end pose, the step carried the ground a tick ahead of the planet the world shows, and what rested on a turning planet was a tick of its turn off.
2. **A frame follows the planet that carries the ships in it** (`Frame::follow`, `Follow{planet, anchor, shift, velocity}`). The places of such a frame are the world's less how far the planet has gone since the frame was made (`shift`, a `voxel_space::Displacement`: whole bricks and an `f32` rest, exact however far the planet is from the origin) and its velocities are the world's less the planet's. The planet's centre does not move in it, it only turns. A ship at rest on the planet is at rest in the frame whatever the planet's speed; the frame is in free fall with the planet, so the gravity in it is `gravity_in_the_free_fall_of` (everything's pull but that of what the planet goes round).
3. ***Changed:* the server decides who a planet carries, and carries it** (`ServerWorld::apply_rails`, called at the start of the tick, before anything else looks at the world): everything in a level within the **sphere of influence** of a planet on a rail (one and a half times the reach of its ground, at most 120,000 voxels; the smallest sphere when in several; kept a quarter farther out than it is entered, so a thing at the edge is not taken and let go in turn) is moved by the planet's displacement of the tick: players and items (in the level's coordinates), and **ships, held and frozen ones too**. Only ships with no terrain and no rail of their own are carried; velocities are left as they are, being against the planet. **What keeps a state of its own for an entity must be told, or it undoes the carry:** the character simulation Cave keeps for each player writes its own position over the entity's record every tick, so a record moved alone was moved back (found 2026-10-26: a walker dropped over a planet that goes round landed a whole fall's travel of the planet from where it came in, and a test that only asked whether it landed aboard passed). `ServerWorld::carried_by_rails(entity)` says by how much the world carried an entity this tick (forgotten when the entity is put somewhere, and gone in the next call of `apply_rails`), and the body of a player is moved by it (`CharacterBody::displace`, in `Movement::step`); it is told and not found by comparing the body with the record, because a record that differs from the body is not always the newer (a player who has just come in). `ServerWorld::carrier_of(ship)` says who carried a ship; the plugin puts a ship in a frame that follows that planet (or in none), and takes it out when the planet changes. The first design had the plugin alone move the ships, which left every ship a tick of the planet's motion behind it in the phases before the bodies (6 voxels at 125 metres a second; 1,500 at an orbital 30 kilometres a second), so a walker next to a raft saw it displaced.
4. **The terrain of the level (blocks put in space) is not in such a frame**: it stands still in the world. A ship that goes with a planet does not collide with blocks built in space near it. (Blocks built *on* the planet are the planet's and are solid.)
5. **A ship in flight that leaves the sphere** is in a frame that does not follow, as before, and goes on in the world at the speed it had (the plugin writes a ship's velocity as what it has against the frame plus the planet's). A ship that flies in is taken in the same way. A player aboard the planet is in its coordinates (nothing to do); a player in the air above it is carried by the server, and falls to it. *Changed:* a **held** ship in a sphere goes with the planet (held still against it, at the planet's speed in the world, so that it goes on with the planet when it is let go); one outside every sphere stays where it was put in the world.

*What is not done:* a ship made at rest in the world near a planet that moves is going at the planet's speed against it (a ship carved from a planet or from another ship takes the speed of the blocks it is carved from, the planet's turn included, and is at rest against the ground it came from; one made empty with `create_grid`, or carved from the level's own blocks, is at rest in the world, and near a planet that moves it is going at the planet's speed against it; **verified** in `detach_blocks`, not yet by a test near a planet on a rail); an entity that leaves a sphere loses the speed it had against the planet (entities' velocities are against the carrier); the surface of a planet larger than a frame (4,096 bricks) has no body; two ships on the two sides of a sphere's edge are in different frames and do not touch.

### 3.5 What is not in this phase

Ships on rails (the orbit of a ship, and the conversions: `future-goals.md`); time warp; the sky and the sun; the light of a star; elliptical orbits; a planet's
atmosphere; the client drawing a planet from another planet's surface (the positions of far planets are known to it by the poses it is sent, which is what it needs).

## 4. Steps

Each ends with tests (first), clippy, rustfmt, the Windows and Linux suites, mutation experiments, documents, and a commit.

- **40a The world clock. Done 2026-10-26.** `engine:clock`, `WorldData::clock`, the server advances it, `ServerWorld::world_tick`. Nothing else changes. *Verified:* a world that has not ticked writes no section and a resave gives the same bytes; a damaged section and one of a newer version are refused, not dropped; a Cave world saved and loaded goes on from its tick. The one visible change is that the saved-world hash of every golden session moved (a world that has ticked has a clock section); the stream hash and length did not.
- **40b Generator recipes. Done 2026-10-26.** `terrain::GeneratorFamily` (code that builds a generator from bytes) and `PlanetFamily`; the canonical bytes of `PlanetSettings` (35 bytes, refusing numbers that are no planet); `engine:generator_recipes` in the world file and its description; `WorldData::add_generator_recipe`; `LoadContext::families` and `PackageSpec::families`; `base_blocks:planet_v1`; `CaveContent::add_generator_recipe`. A recipe with the settings of `PlanetSettings::one_percent_of_earth(seed)` makes `small_planet_v1`'s bricks, coarse nodes and core bit for bit (a test pins it). *Not done, and a change from the plan above:* Cave's **default** planet is still `small_planet_v1`, not a recipe: the switch is what the data of 40c is for (a world's bodies come from data there), and until then every golden session stays what it was. Worlds made before it load as they were; so do worlds with no recipes (no section).
- **40c Bodies in data. Done 2026-10-26.** The JSON kind (`bodies/NAME.json` in a pack), its generated schema and `check-pack`; Cave's own planet is a body of its own data (`games/voxunami_cave/data/bodies/planet.json`) and makes `small_planet_v1`'s bricks bit for bit, its mass to the bit and its 2,100-brick extent; a pack puts a second planet (of 10,000 voxels, 400 km away) in a new world and a walker lands on it. *As built, and different from the text above:* a family has a **data side** (`data_packs::FamilyData`: the schema of its parameters and how a body file's JSON becomes its bytes, with every problem and the pointer to it), implemented by a plugin next to the family's code; `"seed": "world"` is exactly the seed of the world (no salt, so that Cave's planet is the old one); a body says `surface_gravity` or `mass`, and the family says the radius the pull is for; the extent is `extent_bricks`, stated; `PackageSpec` has `body_support` (the data sides of the families and the levels), `builtin_bodies` and `pull_strength` (the game's G); a package makes its bodies in a world with `add_bodies`, which makes none if one cannot be, and `apply_packs` tries them in a world that is thrown away so that a pack that cannot be made is refused when it is loaded, not when a world is. Worlds made before it are as they were (the planet of an old world is `small_planet_v1`); a new world's is the recipe `voxunami_cave:planet`.
- **40d Rails: the function of the clock and the rails system. Done 2026-10-26.** `world_storage::rail` (`Rail{spin, orbit}`, `Spin`, `Orbit`, `Centre::{Point, Grid}`, `Rail::state(seconds, centre, centre_velocity)`; pinned to the bit against an independent calculation; the bytes of a rail), `MovingGrid::rail`, the ship file's `engine:rail`, `WorldData::put_ships_on_their_rails(seconds)` (in order of the ships' numbers, so what a ship goes round is in place first; returns the ships whose place the world cannot hold), `ServerWorld::{set_grid_rail, apply_rails}` and `Server::step` (once after the clock moves and once after the rules of the tick, which are the same bits again unless a ship came into memory in the tick), the `rail` of a body file (`pack-format.md`), the planet's body in `grid_bodies` moved by `set_state` as it was (it sees the new pose every tick), and **the free-fall rule of gravity**: what is on a ship that is on a rail (`gravity_in`) feels the pull of the ship and of everything but what the ship goes round, and what that goes round in turn. *Not done (40e), and a limit of this step:* the body of a moving planet is a teleport each tick, not a kinematic body in a frame that follows it, so a free ship near a planet that moves is not carried, a planet more than 131 km from the origin of a frame still has no body, and a player in the air over a planet that moves is left behind it (a player aboard is carried; an item aboard too). The default world has no rail: the planet of Cave stands still until the owner gives it one in its data.
- **40e The bodies of a planet on rails. Done 2026-10-26.** `physics::BodyKind::Kinematic` (a velocity-based body: goes at its velocity, carries what rests on it and turns about its middle) and `CharacterBody::displace` (a player's body carried with the world, by what `carried_by_rails` says, in Cave's `Movement::step`); `voxel_space::Displacement` (the exact vector between two positions: whole bricks and an `f32` rest); `ServerWorld::{influence_radius, planet_carrying, carrier_of, carried_by_rails, gravity_in_the_free_fall_of}` and the carrying in `apply_rails` (players, items and ships, free or held, within the sphere of a planet on a rail, moved by its displacement of the tick; gravity in the level for what is carried leaves out what the planet goes round); `grid_bodies`: `Frame::follow` (`Follow{planet, anchor, shift, velocity}`), ships put in the frame of the planet that carries them and out of it when it changes or they go twice 500 bricks from its origin, a kinematic body for every planet on a rail and every planet in a frame that follows, set each tick to where it was at the start of the tick's step (`planet_state`, `state_before`), no terrain of the level in a following frame, gravity from `gravity_in_the_free_fall_of`, the whole of a following frame written back each tick (held and sleeping ships too), a ship moved by something else noticed by its place in the frame, docking with the velocities of the world. Section 3.4 says it as built. *Not done:* what is in 40f (a solar system in data; terrainless bodies on rails), the sky and light of a star, ships on rails, time faster than real; the limits at the end of 3.4.
- **40f A solar system in data. Done 2026-10-27.** The example pack `games/voxunami_cave/examples/packs/solar_system` (`voxunami check-pack cave games/voxunami_cave/examples/packs` passes; `--packs` takes the same folder): a star of 40,000 voxels (turns once in 250,000 s) at x = -3,000,000, a planet of 16,000 (tilted axis, turns in 36,000 s) that goes round it at 1,500,000 voxels in 52,700 s, and a moon of 9,000 (turns once as it goes round: the same face to the planet) that goes round the planet at 80,000 in 2,565 s in a plane tilted a tenth. The orbital periods are Kepler's (`T = 2 pi sqrt(r^3 / (G M))`, the game's G and the masses the file's `surface_gravity` gives) rounded; a test keeps them within 1% so that the system is one the pull would keep. *Decided differently from the default of question 2 below:* **the star is a ball of ground of the planet family, not a body without terrain**: it needed no new code, and every place that treats a grid with no terrain as a ship (merge, split, hold, carry, the lifecycle) leaves it alone as it leaves any planet. A body without terrain is for when the star is drawn as a light and gives some (the sky and light of a star are not built): it needs a marker then anyway (what is light, what is only a ship that weighs a lot) and the guards come with it. *Measured (release, one server, no client):* the system costs 0.002 ms a tick over a world with no pack (0.021 against 0.019); a raft resting on the moon, in the moon's frame, 0.24 ms a tick. *Found:* bodies are made in the order of their files' names and a body can only go round one made before it, so the files are `star`, `star_planet`, `star_planet_moon` (a prefix sorts first); the order should come from what goes round what, which is a small change for later.

## 5. Questions for the owner (my default first; I build the default unless told)

1. **Circular orbits first, ellipses later?** *Default: yes.* An elliptical orbit is a function of time too (Kepler's equation needs an iteration or a series; it is doable
   with `libm`), but a circle is enough to put a moon round a planet and a planet round a star.
2. **The sun.** *Built (40f) as a ball of ground, not the default below; say if you want the other.* A body with a mass and no terrain (a point that pulls) or a planet with a large ball of ground? *Default: a body with a mass and a radius and no terrain* (it
   cannot be walked on, and is drawn by the client as a light); it is a ship with `terrain: None`, `held`, a mass, so the pull code already treats it (the threshold of
   `PULLING_MASS` makes a raft pull nothing and a star pull).
3. **Blocks built in space beside a planet on an orbit.** They are the level's terrain and stay still in the world while the planet goes by. *Default: so be it*; the
   alternative is that building in space is done in a ship (`/ship create` already makes one of what you built). I think that is right, and the level's terrain in space is
   for stations that are *not* near a moving planet.
4. **The real time scale.** A planet of 63 km radius that orbits at a realistic speed... *Default: whatever the data says*; Cave's `mini_cosmos` begins with **no rail**
   (as now) so that the game does not change until the owner puts a rail in its data.
5. **Where bodies are defined for the owner's own worlds.** *Default: a `bodies/` folder in a pack, as `blocks/` is*, read by the server when a world is made.

## 6. Risks I see

- **Precision.** Orbit radii of 10^9 m are fine in `f64` and in a `WorldPosition` (a brick and an `f32` offset in it), but a frame's local `f32` coordinates are only exact within
  tens of kilometres of its origin, so a frame follows a planet and does not use world coordinates. **[belief]** this works; the proof is the test that a ship rests on a planet
  that is moving at 30 km/s for a minute, and is where it should be.
- **Rapier and a fast kinematic body.** The planet turns and carries its ground; at the equator of a 63 km planet that turns once an hour the ground moves at 110 m/s, which
  is fast for the contact solver (the plugin cuts a tick into steps for a fast one, up to 24; Rapier's own default cap of 400 m/s is turned off). **[belief]** I will measure before choosing a default spin.
- **The wire.** A planet whose pose changes every tick is a pose in every tick's messages for every client that watches the level (a few dozen bytes). The nodes of a planet
  are drawn from its pose, so the client re-poses its meshes every tick; **[belief]** that costs nothing, as a ship's does.
