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

**The bodies of a planet that moves** (40e) are the hard part, and this is the design I would build:

1. **The planet's body is kinematic** (velocity-based): Rapier moves it by its velocities and a ship resting on it is carried by friction, and a ship it turns into is hit.
2. **A frame follows the planet.** A frame near a planet on an orbit is an inertial frame that moves with the planet's centre: it has a **drift** (the planet's velocity, in
   `f64`), and the positions of its bodies in the world are the frame's origin (which moves by the drift each tick) plus their local positions. In the frame the planet's centre does not
   move; it only turns. Ships feel the planet's pull and nothing else: the frame is in free fall with the planet, as in the sphere of influence of a patched-conic game. The
   pull of the star on a ship near the planet is not added (it is the same as the planet's, to the first order).
3. **The terrain of the level (blocks put in space) is not in such a frame**: it is at rest in the world, and a frame that moves would have to move it. A frame that follows a
   planet has no colliders of the level's terrain. (Blocks built in space near a planet on an orbit are a thing to settle with the owner; see the questions.)
4. A ship in flight that leaves the sphere of influence of a planet is in a frame of its own (`planet-spec.md`, "Frames"), as now.
5. A player aboard the planet is in the planet's coordinates, which move with it: nothing to do. A player in the air above it is in the level's frame as now, and falls
   toward a planet that is moving at 30 km/s: it is a player in a **frame that follows the planet** too, for as long as the planet's pull is what holds them (they are in
   its sphere); this is the same rule as for ships.

### 3.5 What is not in this phase

Ships on rails (the orbit of a ship, and the conversions: `future-goals.md`); time warp; the sky and the sun; the light of a star; elliptical orbits; a planet's
atmosphere; the client drawing a planet from another planet's surface (the positions of far planets are known to it by the poses it is sent, which is what it needs).

## 4. Steps

Each ends with tests (first), clippy, rustfmt, the Windows and Linux suites, mutation experiments, documents, and a commit.

- **40a The world clock. Done 2026-10-26.** `engine:clock`, `WorldData::clock`, the server advances it, `ServerWorld::world_tick`. Nothing else changes. *Verified:* a world that has not ticked writes no section and a resave gives the same bytes; a damaged section and one of a newer version are refused, not dropped; a Cave world saved and loaded goes on from its tick. The one visible change is that the saved-world hash of every golden session moved (a world that has ticked has a clock section); the stream hash and length did not.
- **40b Generator recipes. Done 2026-10-26.** `terrain::GeneratorFamily` (code that builds a generator from bytes) and `PlanetFamily`; the canonical bytes of `PlanetSettings` (35 bytes, refusing numbers that are no planet); `engine:generator_recipes` in the world file and its description; `WorldData::add_generator_recipe`; `LoadContext::families` and `PackageSpec::families`; `base_blocks:planet_v1`; `CaveContent::add_generator_recipe`. A recipe with the settings of `PlanetSettings::one_percent_of_earth(seed)` makes `small_planet_v1`'s bricks, coarse nodes and core bit for bit (a test pins it). *Not done, and a change from the plan above:* Cave's **default** planet is still `small_planet_v1`, not a recipe: the switch is what the data of 40c is for (a world's bodies come from data there), and until then every golden session stays what it was. Worlds made before it load as they were; so do worlds with no recipes (no section).
- **40c Bodies in data. Done 2026-10-26.** The JSON kind (`bodies/NAME.json` in a pack), its generated schema and `check-pack`; Cave's own planet is a body of its own data (`games/voxunami_cave/data/bodies/planet.json`) and makes `small_planet_v1`'s bricks bit for bit, its mass to the bit and its 2,100-brick extent; a pack puts a second planet (of 10,000 voxels, 400 km away) in a new world and a walker lands on it. *As built, and different from the text above:* a family has a **data side** (`data_packs::FamilyData`: the schema of its parameters and how a body file's JSON becomes its bytes, with every problem and the pointer to it), implemented by a plugin next to the family's code; `"seed": "world"` is exactly the seed of the world (no salt, so that Cave's planet is the old one); a body says `surface_gravity` or `mass`, and the family says the radius the pull is for; the extent is `extent_bricks`, stated; `PackageSpec` has `body_support` (the data sides of the families and the levels), `builtin_bodies` and `pull_strength` (the game's G); a package makes its bodies in a world with `add_bodies`, which makes none if one cannot be, and `apply_packs` tries them in a world that is thrown away so that a pack that cannot be made is refused when it is loaded, not when a world is. Worlds made before it are as they were (the planet of an old world is `small_planet_v1`); a new world's is the recipe `voxunami_cave:planet`.
- **40d Rails: the function of the clock and the rails system. Done 2026-10-26.** `world_storage::rail` (`Rail{spin, orbit}`, `Spin`, `Orbit`, `Centre::{Point, Grid}`, `Rail::state(seconds, centre, centre_velocity)`; pinned to the bit against an independent calculation; the bytes of a rail), `MovingGrid::rail`, the ship file's `engine:rail`, `WorldData::put_ships_on_their_rails(seconds)` (in order of the ships' numbers, so what a ship goes round is in place first; returns the ships whose place the world cannot hold), `ServerWorld::{set_grid_rail, apply_rails}` and `Server::step` (once after the clock moves and once after the rules of the tick, which are the same bits again unless a ship came into memory in the tick), the `rail` of a body file (`pack-format.md`), the planet's body in `grid_bodies` moved by `set_state` as it was (it sees the new pose every tick), and **the free-fall rule of gravity**: what is on a ship that is on a rail (`gravity_in`) feels the pull of the ship and of everything but what the ship goes round, and what that goes round in turn. *Not done (40e), and a limit of this step:* the body of a moving planet is a teleport each tick, not a kinematic body in a frame that follows it, so a free ship near a planet that moves is not carried, a planet more than 131 km from the origin of a frame still has no body, and a player in the air over a planet that moves is left behind it (a player aboard is carried; an item aboard too). The default world has no rail: the planet of Cave stands still until the owner gives it one in its data.
- **40e The bodies of a planet on rails. (Next.)** The kinematic body; the frame that follows; ships and players on and over a planet that spins and orbits.
- **40f Documents, measurements, the owner's play.** A solar system in data (a star, a planet, a moon) to look at.

## 5. Questions for the owner (my default first; I build the default unless told)

1. **Circular orbits first, ellipses later?** *Default: yes.* An elliptical orbit is a function of time too (Kepler's equation needs an iteration or a series; it is doable
   with `libm`), but a circle is enough to put a moon round a planet and a planet round a star.
2. **The sun.** A body with a mass and no terrain (a point that pulls) or a planet with a large ball of ground? *Default: a body with a mass and a radius and no terrain* (it
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
  is fast for the contact solver (Rapier caps a body at 400 m/s and the plugin cuts a tick into steps for a fast one). **[belief]** I will measure before choosing a default spin.
- **The wire.** A planet whose pose changes every tick is a pose in every tick's messages for every client that watches the level (a few dozen bytes). The nodes of a planet
  are drawn from its pose, so the client re-poses its meshes every tick; **[belief]** that costs nothing, as a ship's does.
