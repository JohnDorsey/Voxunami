# Writing a data pack

A *pack* is a folder of JSON files that adds blocks to a game and changes the blocks it has, with no code. A server loads
packs when it starts; players need nothing installed: a client is sent what blocks look like when it joins, and a
recording carries it. `docs/data-packs-spec.md` has the design and why; this page is for writing one.

## The layout

```
packs/                      the folder you give the server: --packs packs
  my_pack/                  one folder per pack; its name is the pack's id
    pack.json               who the pack is and what it needs
    blocks/marble.json      a block, named by its file: my_pack:marble
    blocks/glow_stone.json
    patches/dim_lamps.json  a change to blocks that already exist
    bodies/moon.json        a planet in the world: my_pack:moon
```

Names are lowercase letters, digits and `_`, starting with a letter, at most 64 characters. A file's name without `.json` is
the block's, patch's or body's name. Names that start with `.` and files that are not `.json` are left alone.

## `pack.json`

```json
{ "id": "my_pack", "depends_on": ["base_blocks", "other_pack"] }
```

`id` must be the folder's name (and not `engine`). `depends_on` lists what must load before this pack: other packs, or the
name of a plugin the game has blocks from (`base_blocks`). A pack may change only blocks of its own and of the names it
depends on (and what those depend on). Packs load in dependency order, ties by id, and the files of a pack in order of their
names, so the result never depends on how the folder happens to be listed.

## A block

```json
{
  "color": [0.9, 0.9, 0.95],
  "see_through": false,
  "states": ["full", "half:negative_y", "stair:positive_x", { "boxes": [[0,0,0, 8,4,8], [2,4,2, 6,8,6]] }],
  "state_colors": [[0.9, 0.9, 0.95], [0.8, 0.8, 0.9]],
  "properties": { "base_blocks:emission": 3, "base_blocks:passable": false },
  "state_properties": { "1": { "base_blocks:emission": 0 } }
}
```

Only `color` is needed. A block is one whole cube unless `states` says more.

- **`color`**: red, green, blue, each 0 to 1.
- **`see_through`**: whether it lets you see and light through (glass). Default false.
- **`states`**: the shape of each state, state 0 first. A name (`full`, `empty`, `half:SIDE`, `stair:SIDE`, with SIDE one of
  `negative_x`, `positive_x`, `negative_y`, `positive_y`, `negative_z`, `positive_z`) or `{"boxes": [...]}`: boxes of six
  whole numbers from 0 to 8 (eighths of a voxel): min x, y, z, then max x, y, z, each min below its max.
- **`state_colors`**: a colour for each state, repeating if there are fewer than states. Each colour takes one of the
  1,048,576 colours the renderer has (a block takes one too).
- **`properties`**: what the game's rules read about the block. The names, kinds and ranges are the game's: `voxunami pack-schema`
  lists them with their documentation. Booleans are `true`/`false` and numbers are whole numbers inside the declared range.
- **`state_properties`**: values that differ in one state, by the state's number.

## A patch

```json
{
  "target": "base_blocks:lamp",
  "optional": false,
  "operations": [
    { "op": "test",   "property": "base_blocks:emission", "value": 64 },
    { "op": "set",    "property": "base_blocks:emission", "value": 90 },
    { "op": "set",    "property": "base_blocks:passable", "state": 0, "value": false },
    { "op": "remove", "property": "base_blocks:flammability" },
    { "op": "set",    "path": "color", "value": [1.0, 0.8, 0.5] },
    { "op": "set",    "path": "see_through", "value": false },
    { "op": "set",    "path": "state_colors", "value": [[0,0,0], [1,1,1]] },
    { "op": "remove", "path": "state_colors" },
    { "op": "add",    "shape": "half:positive_y" }
  ]
}
```

- **`test`** says "I was written against this": the whole patch is refused unless the value is as stated. Use it before
  `set` when you depend on what the block was. A failed `test` is an error even in an optional patch.
- **`set`** changes a property (in every state, or in one with `"state": N`; setting for every state leaves a state's own
  value alone) or one of the fields `color`, `see_through`, `state_colors`.
- **`remove`** takes a property back to the game's default (for every state, or one state), or removes the state colours. It is an
  error to remove what is not set. `color` and `see_through` cannot be removed (set them).
- **`add`** appends a state with this shape. Later operations in the same patch may use it.
- A patch is applied as a whole or not at all: if one operation fails, none of it happened.
- Two packs that both set a tag (a true/false property) to true agree; a pack that wants it off sets it false or removes it.
- **`optional`**: skip the patch, with a warning on the server's console, if its target block does not exist or it names a
  property the game does not declare (the plugin is not loaded). This is how a pack improves another mod's blocks when that
  mod is installed, and does nothing when it is not. Without it, either is an error (so a typo is never silent).

## A body

A body is a planet: a ship in a level of the world that is held where it is put and has ground of its own, which a *family* of terrain
generators makes from a few numbers (so a planet of another size or seed is a file, not a change of code). Cave's own planet is a body, written
in this same format (`games/voxunami_cave/data/bodies/planet.json`).

```json
{
  "level": "voxunami_cave:mini_cosmos",
  "position": [400000, 0, 0],
  "terrain": {
    "family": "base_blocks:planet_v1",
    "parameters": { "seed": 99, "radius": 10000, "hill_height": 20, "largest_hill_wavelength_log2": 7,
                    "hill_octave_count": 4, "cave_wavelength_log2": 5, "cave_half_thickness": 0.05 }
  },
  "extent_bricks": 340,
  "surface_gravity": 8
}
```

- **`level`**: the level it is in. Cave has `voxunami_cave:mini_cosmos` (space, where its planet is) and `voxunami_cave:overworld`.
- **`position`**: where its centre is in the level, in voxels (x, y, z). The origin if left out.
- **`terrain.family`**: the family of generators that makes its ground. Cave has one, `base_blocks:planet_v1`: a ball whose surface is hills (noise bent
  round it) with sheet caves under it. `pack-schema` lists the families a game has.
- **`terrain.parameters`**: what the family takes. For planets: `seed` (a whole number, or `"world"` for the seed of the world it is made in, which makes
  the planet the world's own) and `radius` (voxels, from the centre to the surface where the hills add nothing) are required; `hill_height` (voxels, the height
  of the largest hills), `largest_hill_wavelength_log2` (the largest hills are 2 to this power voxels across; 10), `hill_octave_count` (how many sizes of hill,
  each half the one before; 0), `cave_wavelength_log2` (5) and `cave_half_thickness` (0 for no caves) make a smooth ball if left out.
- **`extent_bricks`**: how many bricks (32 voxels) either way of its centre its ground reaches. Ground beyond is not made, so this wants to be the radius and the
  height of the hills, with a brick or two to spare.
- **`surface_gravity`** (metres a second a second) **or `mass`** (kilograms): one of them. A planet given its pull weighs what that takes at its radius with the
  game's gravity. (In Cave a body pulls at all only if it weighs at least 10^19 kilograms, which is a pull of ten at a radius of some eight kilometres.)

**`rail`** (optional) says how the body moves, as a function of the time of the world (seconds from when the world was made, counted in ticks of a twentieth of a
second in Cave; it is saved with the world, so a restart goes on from where it was):

```json
"rail": {
  "spin":  { "axis": [0, 1, 0], "period": 86400, "angle": 15 },
  "orbit": { "around": "my_pack:star", "normal": [0, 1, 0], "radius": 1.5e9, "period": 31557600, "phase": 90 }
}
```

- **`spin`**: the body turns once in `period` seconds (negative the other way) about `axis`, which is where its own up goes (a tilt is an axis that is not `[0, 1, 0]`); `angle` is
  how far it has turned at time 0, in degrees (0).
- **`orbit`**: it goes round `around` on a circle of `radius` voxels (at most 10^15), once in `period` seconds (negative the other way), counter-clockwise looking down `normal`
  (up if left out), from `phase` degrees at time 0 (0). The circle starts toward the +x axis taken into its plane. `around` is a point of the level (`[x, y, z]`) or another body
  (`"my_pack:star"`), which must be made before this one (bodies are made in the order of the packs and then of their files' names, so name them to sort). A body that goes round
  has its place from the orbit, and gives no `position`; a body that only spins stays where it was put. A moon goes round a planet that goes round a star.
- A body on a rail is in free fall with what it goes round: what stands on it feels its pull and not the pull of what it goes round.
- **What a planet on a rail carries.** Everything within one and a half times the reach of its ground (`extent_bricks`; at most 120,000 voxels) of its middle goes with it:
  players and items in the air, and ships, free or held. Ships are simulated in a frame that goes with the planet, so they fall to it, rest on it and are carried round by its
  turn however fast it goes, and a ship that leaves goes on in the world at the speed it had. Where spheres overlap (a moon in its planet's) the smallest carries; what is
  carried stays so a quarter farther out than it was taken in. Blocks built in space, which stand still in the world, are not solid to what a planet carries (blocks built on
  the planet are). A ship made empty near a planet that moves starts at rest in the world, so it is going at the planet's speed against it; one carved from a planet or from
  another ship has the speed of the blocks it was carved from. There is no speed limit (Rapier's default of 400 metres a second is turned off); past some 380 voxels a second a tick can no longer be cut finely enough to keep a ship from going through thin ground, and the result is not promised to be the same on every machine.
- *Not yet:* ellipses; ships on rails of their own; time faster than real. `docs/rails-and-recipes-spec.md`.
- **A solar system** is a few bodies that go round each other: `games/voxunami_cave/examples/packs/solar_system` has a star, a planet and a moon (the star is a large planet: a ball of ground, with no light of its own yet). Periods there are those the game's pull would keep, `T = 2 pi sqrt(r^3 / (G M))` with `G` 6.674e-11 and the mass the file's `surface_gravity` gives (`g r r / G`). Name the files so that what is gone round sorts first (`star`, `star_planet`, `star_planet_moon`).

A body is made when a *world* is made: the planets of a world that exists are not changed by a pack added later, and do not go if the pack is taken away (the
world keeps the recipe of each planet's generator, so it makes the same ground). A body with no `rail` stands still (`docs/rails-and-recipes-spec.md`).
Bodies come into a new world after the game's own, in the order the packs load.

## Checking a pack

```
voxunami check-pack cave packs        what the server would say, for every pack in `packs`
voxunami pack-schema cave schemas     writes pack.schema.json, block.schema.json, patch.schema.json, body.schema.json
```

`check-pack` loads the packs exactly as a server does and prints *every* problem, each with the file and the place in it:

```
my_pack/blocks/marble.json: /properties/base_blocks:emission: 300 is outside 0 to 255
my_pack/patches/dim.json: /operations/0: the test failed: base_blocks:emission is 64, not 65
```

The schemas are made from the properties, the families of generators and the levels the game declares, never written by hand. Put one in a file's `$schema` key (a path
relative to the file works in most editors) and the editor completes and checks it as you type:

```json
{ "$schema": "../../schemas/block.schema.json", "color": [0.9, 0.9, 0.95] }
```

A schema can check the shape of a file, not everything: that a box has some size, that a state number is one the block has,
that a name is free, that dependencies exist and do not form a loop, and that a `test` holds are checked only by `check-pack` (and
the server).

## Running a server with packs

```
voxunami serve cave --packs packs
voxunami cave --packs packs            playing, with a server in this process
voxunami cave --host --packs packs
```

A server that finds any problem does not start. Blocks are numbered after the game's own, in the order the packs load; a
world saved with a pack's blocks loads without the pack (those blocks show as magenta cubes and are kept exactly as they were) and
the blocks come back when the pack is loaded again.

## Limits

- At most 1,048,576 colours in all: every block takes one, and so does each state colour.
- A pack changes what blocks look like and the properties the game's rules read, so a block's *behaviour* is as rich as the
  properties the game gives (light, burning, walking through). New kinds of behaviour need code, which is later.
