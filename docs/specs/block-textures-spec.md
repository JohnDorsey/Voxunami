# Block textures, and the game's own blocks in data: specification

**Status: written 2026-11-07 from an interview with the owner (the decisions marked "owner" below are theirs, from that day). Steps 0, 1 and 2 are built (see "As built" at the end); the rest is not.** It
extends the data packs (`data-packs-spec.md`, `pack-format.md`) and the content package (`content-package-spec.md`), and it replaces the 2026-10-29
advice of `architecture-review.md` section A4 ("no textures, sub-voxel models") with the owner's decision to have textures. What I checked in the code
or on disk is marked **[code]** or **[disk]**; what I believe without having checked is marked **[belief]**.

## 1. Where things stand

- **[code]** The renderer draws a flat colour per block (a palette of 1,048,576 colours), boxes in eighths of a voxel, and nothing else. `future-goals.md`
  says so ("the renderer has no textures"). The pack reader reports a `texture` key as an unknown key (`architecture-review.md`, the four-mistakes example).
- **[code]** A pack (Phase 29) adds and patches blocks in JSON; the game's own blocks are still Rust: `BaseBlocks::new` (stone, dirt, grass, lamp, wood,
  fire, great_river_water) in `plugins/base_blocks/src/lib.rs`, and `dev_docking_port` in `games/voxunami_cave/src/content.rs`.
- **[code]** A face that is drawn in another colour is three engine properties (`engine:marked_face`, `engine:mark_color`, `engine:mark_both_ends`): one
  face, one colour, optionally its opposite. The wood block uses them for its ends; the docking port for its yellow face. Placement is two more
  (`engine:axis_face`: the block's line runs along the face it was put against, as a log does; `engine:front_face`: the block faces the player). Every voxel
  has an orientation, 0 to 23, saved in the channel `base_blocks:block_orientation` (`players-and-ships.md`).
- **[disk]** `plugins/base_blocks/textures/` holds 16 PNG files, every one 16 x 16. The six this spec is about are fully opaque: `grass.png`, `dirt.png`,
  `stone.png`, `still_water.png`, `log_top.png`, `log_side.png`, and `dev_docking_port_top.png`, `_side.png`, `_bottom.png`. The others (sand, gravel, the
  plants, flowing water) are **not used by this step** (owner). Note the names: the block is `base_blocks:wood` and its pictures are `log_*`; the block
  `great_river_water` has `still_water.png`. So the name of the pictures has to be in the block's definition.
- **[code]** The package a client is sent has no assets and no hash: "built in the phase that adds assets" (`packs-and-content.md`). This is that phase.

## 2. What was decided

| | Decision | By |
|---|---|---|
| D1 | The game's own blocks (stone, dirt, grass, wood, great river water, lamp, fire; Cave's docking port) are **JSON files** read by the pack loader. | owner |
| D2 | A block's definition names its pictures by one **base name**; which faces differ is **never** in the JSON, it follows from the files that exist. | owner |
| D3 | `top` is the face away from the face that was right-clicked to place the block; `side` is any face that shares an edge with the top; `bottom` is opposite the top, and is the top's picture when there is no `_bottom` file. | owner |
| D4 | **Terrain**: the generator writes the orientation, so that a planet's grass has its top away from the planet. | owner |
| D5 | Pictures travel **inline in the welcome**, with size caps. | owner |
| D6 | A pack may replace pictures **by file name**, and a patch may also point a block at another base name. | owner |
| D7 | The game builds its atlases from **loose PNG files**; `atlas.json` is the editor's and decides nothing. | owner |
| D8 | Any power-of-two size from 8 to 256, **at most 4 sizes at once, one atlas per size**, so that texture packs can come later. | owner |
| D9 | **Texture packs** are a later step: a list the player orders, later entries overriding earlier ones, as in Minecraft; the server's pictures are one movable entry in it, at the bottom by default. | owner |
| D10 | A picture on a block that is not a whole cube is placed **from position in the block**. | owner |
| D11 | `color` becomes optional for a textured block. Its default is the average of **five faces: the top once and each of the four sides once** (so a side picture counts four times). An explicit `color` always wins, and is how a modder keeps a dirty-sided block's far view clean. | owner |
| D12 | **Texture distance** is a client setting; beyond it a face **fades** to its flat colour over a band; **levels of detail are never textured.** | owner |
| D13 | **Per-state pictures** are wanted eventually, and are **not in this version**. | owner |
| D14 | The **per-face colours** of the log and the docking port are written in their JSON, and any other block may give them too. | owner |

## 3. The block format

Three keys are new. `pack-format.md` gets them when they are built.

```json
{
  "texture": "log",
  "color": [0.55, 0.38, 0.20],
  "face_colors": { "top": [0.75, 0.58, 0.36], "bottom": [0.75, 0.58, 0.36] },
  "properties": { "base_blocks:flammability": 40, "engine:axis_face": "up" }
}
```

- **`texture`** (optional, a string): the base name. A bare name (`"log"`) is a picture of the pack that defines the block; `"other_pack:stone"` reaches
  a picture of another pack, which must be one the pack depends on (the rule for patches). Without it the block has no pictures and is flat colour.
  A string stays valid for ever: a later version may also accept an object for per-state pictures (D13), so nothing in this version blocks that.
- **`color`** (optional when `texture` is given, as before otherwise): the block's flat colour, used by levels of detail, swatches and particles. If
  omitted it is the average of the five faces' pictures (D11, section 5.3).
- **`face_colors`** (optional, any block, textured or not): the flat colour of faces that differ (D14). Keys are `top`, `side`, `bottom` (as in section 4,
  in the block's own frame, turned with the voxel) or one exact face `negative_x`, `positive_x`, `negative_y`, `positive_y`, `negative_z`, `positive_z` (the
  words of the shapes, `pack-format.md`); an exact face beats its group. The same block may give both. A colour is three numbers from 0 to 1, like `color`.
  The docking port is `"color": [0.17, 0.21, 0.22]` with `"face_colors": { "positive_x": [1.0, 0.847, 0.0] }`; the wood block gives its two ends
  (`191, 148, 92` today).
- **Properties that name a face** accept the word as well as the code: `"engine:axis_face": "up"` for `4`. The codes (1 to 6 for west, east, down, up, north,
  south) are unreadable in a file the game itself ships. *Mine, an addition to Phase 24; the schema lists the words.*

**`face_colors` replaces `engine:marked_face`, `engine:mark_color` and `engine:mark_both_ends`**, which are removed (the project has no worlds to keep before
its first release, `CLAUDE.md`, and one way of colouring a face is better than two). It is also the thing that finishes the old mark: it can colour any
faces, not one face and its opposite. `engine:docking_face`, `engine:front_face` and `engine:axis_face` stay: they say what a face *means*, not what colour it is.
*Mine; section 10 asks the owner to confirm the removal.*

**The game's own blocks** become `plugins/base_blocks/data/blocks/*.json`, and Cave's become `games/voxunami_cave/data/blocks/*.json` (the planet body
already lives in `games/voxunami_cave/data/bodies`). They are read by the loader that reads a pack, so the game's blocks are the first pack and an example
for every modder. Rust looks the ids it needs (stone, grass, a log) up **by name** at start-up. The numbering stays what it is now (air, stone, dirt, grass,
lamp, wood, fire, great_river_water, then Cave's port), by the loader reading the files in a fixed order; the numbering is not a file format (saved files hold
names), but a test pins it so nothing moves by accident. *Mine, a risk: the generators and many tests use `ids.stone` and the like.*

## 4. The pictures

### 4.1 Files and names

A pack's pictures are PNG files in its `textures/` folder (a folder in a pack that nothing reads is an error today; this one is read). A name is lowercase
letters, digits and `_`, as other names are. For the base name `log`:

| Face | Looked for, in order |
|---|---|
| top | `log_top.png`, then `log.png` |
| side | `log_side.png`, then `log.png`, then the top's picture |
| bottom | `log_bottom.png`, then the top's picture |

If none is found the block is flat colour, and `check-pack` says a texture was named that has no file (an error, section 7). Because this is decided by the
files, **adding `log_bottom.png` changes the block with no change to its JSON** (D2). `dev_docking_port_*.png` are exactly the three and `grass.png`,
`dirt.png`, `stone.png` and `still_water.png` are the one-picture case.

*Resolved with the pack's files, on the server, when the packs load*, so a client never searches: the package says, for each block, the picture of each of
top, side and bottom (section 6).

### 4.2 Sizes and what a file may be

A picture must be square, a power of two from 8 to 256 on a side, 8-bit RGB or RGBA (a file of another colour format is an error naming it, not converted).
A world uses **at most four different sizes** (D8); a fifth is an error naming the files. Each size is its own atlas, so pictures of different sizes on
one block are fine. *Mine, **[belief]**: four atlases are four texture bindings in the block shader, one draw call per brick on every adapter. The cap
is a number I chose so that this holds; it must be tried on the four adapters of the dev machine (GPU maths has differed between adapters before) before it is trusted. wgpu's default of 256
layers in a texture array is also a cap on tiles per size, and it is treated as one (an error naming it when hit).*

Alpha: the six pictures are opaque **[disk]**. A picture with clear pixels is accepted (the plants will have them) but this step draws blocks opaque;
translucency and cut-outs are a later step. The flag `see_through` is unchanged.

### 4.3 Overriding (D6)

1. **By file name.** A pack that depends on the pack that owns a base name may have `textures/log_top.png` under the same base name **in the owner's
   namespace**: `my_pack/textures/base_blocks/log_top.png`. Packs apply in their load order (dependency order, ties by id), later wins, as for patches, and
   `check-pack` lists every file that replaces another.
2. **By patch.** `{ "op": "set", "path": "texture", "value": "my_pack:oak" }` points the block at a different base name, with the same `test`/`optional`
   rules as every other patch.

## 5. Colours

### 5.1 Each face has a flat colour

The flat colour of a face is the first that applies: an exact `face_colors` key; the `face_colors` group (`top`, `side`, `bottom`) it is in; the block's
`color`. It is what a textured face fades to (D12) and what a block without pictures is drawn in. (The log's ends keep their lighter colour at a distance
because `face_colors` says so, not because they average to it.)

### 5.2 Palette

Each distinct flat colour takes one palette entry, as the marked faces do today (`BlockTable` appends them after the state colours). The 1,048,576 limit
and the check "a client refuses a package it cannot draw" are unchanged.

### 5.3 The default `color`

If `color` is omitted, it is the average of the top picture (once) and the side picture (four times), alpha-weighted, **in linear light** (the way the
renderer blends), with the bottom ignored. A block with one picture gets that picture's average. *Mine: linear light; a plain average of the sRGB bytes
is darker and would not match the blending. The numbers below are sRGB-byte means (measured, a throwaway script), only to show how far the far view
moves; the linear means were not computed.*

| Block | Colour in Rust now (sRGB bytes) | Mean of the picture(s) now on disk |
|---|---|---|
| stone | 128, 128, 135 | 95, 95, 95 |
| dirt | 115, 77, 46 | 128, 111, 57 |
| grass | 77, 158, 51 | 38, 140, 42 |
| great river water | 20, 71, 217 | 37, 47, 229 |
| wood (top once, side four times) | 140, 97, 51 | 123, 100, 45 |
| dev docking port (top once, side four times) | 43, 54, 56 | 101, 100, 77 |

The far terrain, hotbar swatches and particles of these blocks change colour when this lands (stone gets about a quarter darker). That is intended by D11,
and a block that must keep its old colour writes `color`. Cave's pictures and golden sessions that draw terrain move with it, once, in the step that does
it (section 8), and the step says so.

## 6. Delivery and drawing

**The package** (`game_shell::PackageData`, canonical bytes) gains: for each size in use, its tiles; for each block, its flat colours by face and its
picture of top, side and bottom (an index into the tiles of a size, or none). Decoding refuses what the encoder would not write, as for the rest of the
package; a package that does not fit what the client loaded is a loud error.

**Raw pixels, not PNG, on the wire.** *Mine, a change from what the interview said ("the PNG bytes"), to be confirmed:* the server decodes its own
pack's files and sends 8-bit RGBA, so **a client never runs an image decoder on bytes from a server**. `architecture-review.md` (A4) names that attack
surface (a hostile server against an image decoder). It costs space: a 16 x 16 tile is 1 KiB raw against a few hundred bytes of PNG, and a 256 x 256 one is
256 KiB. Caps (mine, to confirm): 256 tiles per size, 8 MiB for all of a package's tiles; a pack over them does not load. **[belief]** The welcome has no
size limit that this would reach; the step that builds it checks the protocol's limits first.

**Replays** carry the package, so they carry the pictures: a recording does not depend on what is installed (as blocks already do not).

**The client** builds, per size, a 2D texture array (one layer a tile, not one big image, so that no neighbouring tile bleeds into another when mips are
cut). *Mine.* Filtering is nearest when magnified and mip-mapped when minified (mips are made on the client from the tiles).

**Drawing.** A face is drawn with its tile, placed from position in the block (D10): the part of the face's rectangle in the voxel's frame selects the part
of the tile, so a slab shows its slice and a stair step the matching slice. Lighting multiplies the texel as it does a palette colour (**[belief]**
that the fragment shader can do this without a change to the lighting; the renderer is not read for it yet). Beyond the **texture distance** the face fades
to its flat colour over a band that is a fixed fraction of the distance (*mine: 25%; the default distance is mine too, 64 voxels, both to be tuned by eye*),
and blocks in levels of detail above 0 carry no tile at all (D12), only the colour they have today. **Texture distance is a client setting**, in the
client's own settings file, not in the world.

**Texture packs (D9) are the last step**, and nothing before it depends on it. The shape, so nothing earlier forecloses it: a client folder
`texture_packs/<name>/<namespace>/textures/<base>_<face>.png`, an ordered list the player keeps, the server's pictures as one entry of it. A face's picture is
the last entry in the list that has one for `(namespace, base name, face)`. Textures are cosmetic, so no simulation depends on them, and replays are unaffected.

## 7. Orientation: what "top" means

Textures need a top; a block has one only if it is turned. `engine:axis_face` says the block's line (its top and bottom) is along the face it was put against
(a log); `engine:front_face` says it faces the player (the port). D3 is `axis_face` with "up".

- **A block with distinct top and bottom pictures but no `axis_face` or `front_face` has its top at +Y whatever the player does.** I chose not to derive
  placement from which files exist, because then adding a retexture would change how a block is placed and a saved world would look wrong; instead
  `check-pack` **warns** about the combination. *Mine; section 10.*
- **Placed blocks** follow `SetBlock.against` as now. Grass will get `"engine:axis_face": "up"` when it gets a distinct top picture; stone, dirt and
  water have one picture and need none. The port keeps `front_face`, and its top is whatever way "up" went when the block was turned to face the player,
  a rule that needs the roll to be chosen: **the roll is the first of the 24 rotations (in the pinned order of `LatticeRotation::all()`) that gives the
  front the wanted face and, among those, the one keeping the block's own +Y as near to the player's up as it can**. *Mine, **[belief]**; the code that
  turns a front today decides the roll somehow and must be read before this is written.*
- **Terrain (D4).** The planet generator writes, for every block it generates that has an `axis_face`, the orientation that puts +Y on the axis nearest the
  voxel's direction from the planet's centre (the cube face the voxel is in), choosing among the rotations that do it by the pinned order above. The
  overworld's generator writes nothing (orientation 0 is up). A generator that places logs (the trees to come) writes the orientation of the trunk's or the
  branch's line. This is a change to what a generator makes, so it is a new generator version (the project allows replacing one before its first release,
  `CLAUDE.md`, but the golden sessions that pin a planet move once).
- On a planet the nearest axis is up to about 55 degrees from true up (at a corner of the cube-sphere), so a grass top will be tilted there. That is the
  consequence of blocks being axis-aligned, not of this design; trees (`future-goals.md`) have the same problem and this is not solved here.

## 8. Order of work

Each step ends with `cargo test --workspace`, clippy and rustfmt passing, and is committed (`CLAUDE.md`). Tests come first in each.

0. **Lock in today.** Golden tests of the canonical package bytes and of the `BlockTable` of every game (Cave, 2048, Eels), and the numbering of Cave's blocks.
   Done before anything changes; they pass unmodified until the step that is named as moving them.
1. **`face_colors` and face words** (replacing the three mark properties). Rust-defined blocks use them. The log's two ends and the port's face draw exactly
   as before (screenshot test in `tests/pictures.rs`: the same pixels); the package bytes change here, **once, and the pins that change are listed in the
   commit**. Tests: JSON round trip, schema/loader differential on the new keys, turning with the orientation, the removed properties are errors in packs.
2. **The base blocks and the port as JSON.** The package bytes are **equal** to step 1's, to the byte, and the numbering test passes: that is the proof the
   move lost nothing. A property test shuffles the file listing, as the pack tests already do.
3. **`texture`, the files, the resolution, the default colour.** `check-pack` reads `textures/`, applies section 4's rules and reports section 7's problems
   (listed in section 9). Moves the colours (section 5.3 table). No drawing yet. Tests: the resolution table for every combination of files present
   (property test over subsets of `{plain, top, side, bottom}`), a mutation experiment on each row of the chain, sizes and format refusals.
4. **Delivery and drawing.** The package carries tiles; the client builds atlases; position UVs; the fade; the client setting; levels of detail untextured.
   Tests: a GPU probe test on each of the four adapters (`VOX_REQUIRE_GPU=1`; a narrow camera aimed from a high eye, probes kept off edges, since a 64-pixel
   probe on a slanted floor covers only one to three voxels) for a textured face, a half block, the fade and a level of detail; a benchmark with the camera
   and the sun moving (a still one hides cached work), run on the integrated GPU too. The cost
   of the texture binding is **measured** here, and the four-size cap is kept or changed by the measurement.
5. **Orientation.** Grass and any other `axis_face` block placed along the clicked face, the generator writing orientations, a new generator version.
6. **Texture packs on the client** (D9), with the player's ordering saved.

## 9. Everything `check-pack` and the loader say

Errors (the server does not start): a `texture` that names no file at all; a picture that is not square, not a power of two, outside 8 to 256, or not 8-bit
RGB/RGBA; a fifth size; more tiles than a size can hold; an override file for a base name whose pack the pack does not depend on; a `face_colors` key that
is not a group or a face, or a colour outside 0 to 1; a `color` missing on a block with no pictures. Warnings: a PNG in `textures/` that no block names; a
file that replaces another (listed); a block with distinct top or bottom pictures and no `axis_face`/`front_face` (section 7); an explicit `color` more
than a set distance from the average of the pictures (only to remind the modder, never wrong).

## 10. For the owner to confirm

1. **`face_colors` replaces the three mark properties** (section 3). I assumed yes, since you asked for per-face colours to be in the JSON and the project
   has no worlds to keep.
2. **The docking port's yellow face up close.** Textures win over flat colours, and there is no `dev_docking_port_front.png`, so near the port the yellow
   docking face is the side picture and the cue is gone (it stays at a distance, in `face_colors`). Options: a fourth picture (`_front`, found by the same
   rule); draw a marker over a docking face in the renderer; or leave it, since the port is a creative-only dev block.
3. **Raw RGBA on the wire** instead of PNG (section 6), and the caps (256 tiles a size, 8 MiB).
4. **Placement is explicit** (`axis_face`), not derived from the files that exist (section 7).
5. **Linear-light average** for the default colour (section 5.3), and the visible change of far colours.
6. **Face words** in property values (section 3).
7. The **default texture distance (64 voxels) and the fade band (a quarter of it)**, which I would tune by looking.

## 11. Not in this version

Per-state pictures (D13); animated pictures (flowing water); translucent and cut-out pictures (plants); tints by climate; the other textures on disk;
procedural block types, which the species of trees will use through *roles* (trunk, bark, leaf), not block ids; pictures on blocks larger than a metre
(`future-goals.md`); texture packs before step 6. Sub-voxel models, which `architecture-review.md` preferred, are not ruled out for things that are not cubes.

## As built

**Step 0 (2026-11-07).** `block_golden.rs` in each of Cave, 2048 and Eels pins the numbering, the length and hash of the package, and a line per block from `block_models::snapshot` (a `test-support` feature).

**Step 1 (2026-11-07).**
- `BlockDefinition::face_colors` (six `Option<[f32; 3]>`, indexed by `FaceDirection::index`, for the block as it is made) with `with_face_colors`, `set_face_colors` and `color_from_bytes`; one count and then `(face, colour)` in the order of the faces in the byte form
  (decoding refuses a face above 5, a repeat, or an order that is not ascending). The table gives each distinct colour one palette entry after the state colours, in the order of the blocks and then of the faces, and answers `face_materials` /
  `material_of_face` turned by the voxel's orientation; `FaceMark` and `face_mark` are gone, and so are `engine:marked_face`, `engine:mark_color`, `engine:mark_both_ends` and the builders `with_marked_faces` and `with_both_ends_marked`.
- `PropertyKind::Face` (values 0 to 6, byte kind 2 in the schema); the engine's `front_face`, `axis_face` and `docking_face` are of it (`PropertyKey::face`, `engine::FACE_WORDS`, `engine::face_code_from_word`), and a pack writes `"up"` or `4`.
  `BlockTable::handle` now compares the declared kind with the key's own, so a face key asked for as a plain number is refused.
- Packs: `face_colors` in a block file, and `set`, `test` and `remove` of the path `face_colors` in a patch; the schema describes both, and the differential test of the schema against the loader covers them. A patch's text value is read (it is a face's word if the property is
  a face) and refused when applied to any other property, so the schema accepts text there.
- **Checked:** the block table's answers for every block of the three games, in every state and all 24 orientations (1,068 lines), are identical between the code of step 0 and this code, once the removed `mark` token is dropped from the lines of the old code. The
  digests in the `block_golden.rs` table lines moved only because the instrument no longer prints that token; the readable columns and the three palette digests did not move. The package bytes moved (they lose three declarations and a face kind's range, and
  gain a count of coloured faces per block): Cave 3,088 to 2,642, 2048 3,510 to 3,151, Eels 1,547 to 1,189; the stream pins of the golden sessions moved with them and their saved worlds did not.
- Not in step 1, as the spec says: the base blocks are still written in Rust (step 2), `color` is still required, and nothing reads a texture.

**Step 2 (2026-11-07).**
- The eight base blocks are `plugins/base_blocks/data/blocks/*.json` and the dev docking port is `games/voxunami_cave/data/blocks/dev_docking_port.json`, in the format of `pack-format.md`, read with `data_packs::read_block` and the property schema of the
  game, and embedded in the program (`include_str!`), so the program does not look for them on a disk. `BaseBlocks::new()` and `dev_docking_port()` build from them.
- **A deviation from section 3, and why.** The files are read by the reader a pack's files are read by, but **not through the pack loader** (`load_packs`): the loader numbers a pack's blocks in the order of the files' names (air, dirt, fire, grass,
  great_river_water, lamp, stone, wood), and the numbers of the base blocks are fixed (air is 0 so that a world of zeros is sky; the pins say the rest). The order is therefore a list in Rust, `BLOCK_FILES`, and a test keeps it equal to the files in the folder
  (a file nobody listed, or a name without a file, fails it) and to the order of the fields of `BaseBlockIds`. If a pack should ever be able to say its own order, that is a feature of the pack format, and nothing here is in its way.
- What stayed in Rust, and why: `LAMP_EMISSION`, `FIRE_EMISSION` and `FIRE_LIFETIME` (Cave's rules and tests use them), each kept equal to the file by a test; and `SHAPE_NAMES`/`shape_name`, the names of the seven states of a solid block, kept equal to the
  `states` of the four solid blocks by a test. Gone: the colours and flammabilities that only repeated the data (`GREAT_RIVER_WATER_COLOR`, `LOG_END_COLOR`, `GRASS_FLAMMABILITY`, `WOOD_FLAMMABILITY`), `standard_shapes`, `fire_colors`,
  `BlockPropertiesExt` (nothing used it) and the Rust builders of the blocks.
- Colours that were computed (the twelve colours of a fire, the log's ends from bytes, the port's yellow) are written as the shortest decimal of the `f32` they were (`0.8458333`, `0.7490196`), which the reader turns back into the same `f32`; two decimals that name
  the same `f32` are the same colour (`0.36078432` and `0.36078433` are one value, which a mutation experiment found by not being caught, and a one-ulp change, `0.36078435`, is caught).
- **Checked.** Before the switch, a test read each file and compared it with the definition the Rust built (equal, all nine); after it, **the pins of step 1 passed unmodified**: the length and hash of the package of Cave, 2048 and Eels (so the package is
  equal to the byte), the numbering, the table lines, and the stream and saved-world hashes of all the golden sessions. Ten mutations of the data and an unlisted file were caught (the one that is not an equivalent mutant is the log-end colour above).
- A property such as `engine:docking_face` set in `state_properties` is a different set of bytes from the same property in `properties` though it means the same for a block of one state; the port keeps what it had (`front_face` in `properties`, `docking_face` in
  state 0), and a mutation that moves it is caught by the package pin.
