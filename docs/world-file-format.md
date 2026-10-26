# World file format (version 1)

Implemented by `engine/world_storage` (container, engine sections) and `engine/voxel_grid` (brick encoding).
The governing rule: **loading and saving a world must never lose data, even when the plugins that wrote it are not
installed.** All integers are little-endian unless stated; "varint" is unsigned LEB128, "zigzag" is a signed varint, a
"string" is a varint byte length followed by UTF-8, and an "identifier" is a string holding `namespace:name`.

## Container

```
magic "VOXUNAMI" (8 bytes) | container version (u32, currently 1) | section count (varint)
per section: identifier | section version (varint) | payload length (varint)
             | CRC-32 of identifier text, version (u32) and payload (u32) | payload
```

Sections have distinct identifiers. The checksum covers the identifier and version as well as the payload, so any
single damaged byte anywhere in a section is detected. Unknown sections are carried along untouched.

## Engine sections (all version 1, except `engine:generators`, which is version 2, and the sections that hold poses, which are version 2)

**Poses (2026-10-23, Phase 38a).** An orientation is four `f64` (it was four `f32`), so that a turned planet is placed exactly (an `f32` turn can put a point 5 x 10^7 m out
by metres, measured). The sections that hold poses are `engine:entities`, `engine:grids` and a ship file's `engine:grid_flight`: this program writes them as version 2 and
reads version 1 too, an `f32` turn read as exactly the same turn. A version it does not know is refused. The other sections are still version 1.

A world file has a list of levels, in `engine:levels`. Files written before levels existed have an `engine:generators` and
an `engine:override_bricks` instead, which describe one level; they load as a world of one level that the game names, and
are saved in the new form. A file with both is refused.

Everything else in the file refers to content by *file id*: an index into a registry table below.

| Section | Contents |
|---|---|
| `engine:header` | world seed (`u64`), engine version text |
| `engine:registries` | count, then per registry kind: identifier, entry count, the entries' identifiers in file-id order |
| `engine:channels` | count, then per channel: identifier, default value (`u32`), value kind (`0` number, or `1` + the registry kind its ids come from) |
| `engine:generators` | default terrain generator (identifier), assignment cube level (`u8`), leaf count, then per leaf of the generator-assignment octree, in Morton order: one varint, (generator file id plus one, `0` = unassigned) times 64 plus the cube's level. The leaves tile the whole space, so each leaf's place follows from the levels before it. A leaf count of 0 means no assignments are given (a description of a world's shape). Version 1, which this program still reads, gave each leaf's origin x, y, z (zigzag) and level (`u8`) before its generator number, about ten times the size. |
| `engine:levels` | count (at least 1), then per level in order, the first being the main level: its identifier, its generator setup as length-prefixed bytes (the payload of `engine:generators`, version 2), and its edited bricks as length-prefixed bytes (the payload of `engine:override_bricks`). Level identifiers are distinct. |
| `engine:entities` | the entity counter (varint: the id the next entity gets, never reused), a count, then per entity in strictly increasing id order, each id below the counter: id (varint), kind (identifier), pose (grid id varint; brick x, y, z zigzag; three `f32` offsets in [0, 32); four `f64` of a unit quaternion since version 2 of the section, four `f32` in version 1), a reserved byte (0 now; 1 will mean a world-space fallback pose follows, for an entity in a grid that is not a root grid, and is refused until then rather than dropped), and its components (a count, then identifier and length-prefixed bytes, in order of identifier). Written only when the world has entities or has had any, so a world that never had any is the same bytes as before; not in a description. Entities of kinds, and components, that no loaded plugin has are kept unread. |
| `engine:override_bricks` | count, then per brick in strictly increasing coordinate order: x, y, z (zigzag), length-prefixed brick bytes |
| `engine:generator_recipes` | the terrain generators that are data: a count, then per recipe, in the order they were added to the world: the instance name (the name the generator has in the table of generators), the family (code that makes generators from parameters) and the parameters (length-prefixed bytes, whose form the family decides). Instance names are distinct. Written only when the world has any, and **also in a description** (a client makes the same generators from it); a world without any is the same bytes as before. A family the loading program lacks, or one that refuses the parameters, leaves a placeholder in the table (reported) and the recipe is kept and written back as it was read. A recipe that names a generator the program makes itself refuses the file. Phase 40b. |
| `engine:clock` | the world's clock: whole ticks since the world was made (one varint; a tick is a twentieth of a second). Written after `engine:entities` only when the clock is not 0, so a world that has not ticked is the same bytes as before; not in a description (a client is told the time another way, when anything needs it). A file without it is at tick 0. Trailing bytes are refused. Phase 40a. |

The generator table is the registry kind `engine:terrain_generator`.

**A ship file's `engine:grid_terrain`** (optional, version 1; Phase 39a): for a planet, the terrain generator version that makes the bricks the ship does not store, by name (an identifier, looked up in the reading world's generator table like the content of a
channel), the box of bricks it makes (the lowest and the highest corner, each three zigzag varints) and the mass in kilograms (`f32` bits; finite and not negative). The ship's `engine:ship` section then holds only its edits. A file with no such section is a ship that
nothing generates.

**The world file's list of ships** (`engine:grids`) gives each ship a flags byte after its file's checksum: bit 0 is that it waits for those aboard, bit 1 is that it has terrain (a planet; Phase 39f). A byte over 3 is refused. A world from before the second bit reads as it always did (0 or 1), and a world with a planet is not read by a program from before it.

A **brick** is its channel count (`u16`) then each channel's storage: tag `0` uniform (`u32`); tag `1` paletted (palette
length `u16`, 2 to 256; index width `u8`: 1, 2, 4 or 8; palette `u32`s; `512 x width` packed `u64` index words); tag `2`
dense (32768 `u32`s). Storage is always written in its minimal form, so equal contents give identical bytes.

Terrain that the generator reproduces is **not** stored. A brick in `engine:override_bricks` replaces the generated
one completely, and is only stored if it differs from what the generator makes (`WorldData::prune_unmodified_overrides`).

## Descriptions

`WorldData::describe` writes the same container with only `engine:header`, `engine:registries`, `engine:channels` and
`engine:levels` (each level with no assignment leaves and no bricks): the world's shape without its contents. A server sends it to a joining
client (`docs/protocol-and-replay.md`), which loads it exactly like a save, and keeps the `SourceTranslator` that loading
builds to translate the bricks, values and generator ids the server sends afterwards.

## What happens when something is missing

| Situation on load | Behaviour |
|---|---|
| A registry entry (block type, ...) whose plugin is not loaded | A content-less placeholder is registered under the same identifier; voxels keep a valid id that renders as magenta and is saved back under its original name. Reported in `LoadReport::missing_content`. |
| A terrain generator whose plugin is not loaded | Same: a placeholder; its regions report `MissingGenerator`, nothing is lost. |
| The world's preferred default generator is unavailable | The game's fallback is used for new regions, the preference is remembered in the file, and `LoadReport::unavailable_default_generator` says so. |
| A channel the game does not define | Appended to the layout after the game's channels, with all its data. |
| A registry kind the game supplies no registry for | Its table and the ids that use it are kept as they are. |
| A section nobody owns (plugin data, or unknown engine-namespace sections) | Kept byte for byte, in order. |
| A plugin newly added | Its content simply is not in the file; its channels start at their defaults. |
| An `engine:*` section with a newer version than this program supports | Refused (`UnsupportedEngineSection`): loading would lose data. |
| A channel the game defines with a different value meaning than the file | Refused (`ChannelKindMismatch`). |
| An id outside its registry table, a damaged checksum, a truncated file, malformed data | Refused with an error naming what is wrong; never a panic, never silently different data. |

## Guarantees checked by tests

- Load then save with the same plugins gives identical bytes.
- For random edits and every tested subset or ordering of plugins, saving through the reduced game and loading with all
  plugins restores the original world exactly (compared by identifier, not by runtime id).
- Enabling a plugin that shifts every runtime id does not change what a world means.
- Plugin sections survive byte for byte and in order.
- Damaging any single byte of the container, or truncating it anywhere, is an error.

## Known limits and future work

- A world is one file, written whole. Large worlds need regions saved incrementally; the section mechanism allows
  adding that as new section kinds with a new container version.
- `WorldData::save` returns bytes; `file::write_atomically` writes them to disk all-or-nothing (temporary file, flush,
  rename), so a crash during a save leaves the previous world intact.
- The brick section is not streamed: all edited bricks are decoded at load.
- Save and load of entities, structures and plugin state are the plugins' to define as sections.
