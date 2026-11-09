# Block textures, and the game's own blocks in data: specification

**Status: written 2026-11-07 from an interview with the owner (the decisions marked "owner" below are theirs, from that day). Steps 0 to 4 are built, and step 5 only for the docking port (see "As built" at the end); the rest is not.** It
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
translucency and cut-outs are a later step. The flag `sight_through` is unchanged. *(Cut-outs are built, 2026-11-08: see "As built", "Leaves and glass".)*

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
  water have one picture and need none. **The port has both `front_face` and `axis_face` (decided by the owner, 2026-11-08, replacing my roll rule):
  its top goes away from the surface that was clicked, as a log's does, and its front (the yellow, docking face) is turned to the side of the block the
  player is on as far as that allows, which is one of the four sides of the top.** Mine, to confirm: *the side the player is on* is the one most the opposite
  of the way they look; against a wall that is the face that is up if they look down at it; a player who looks along the top (straight down at the floor) has
  no side, and the front is the first that the pinned order of `LatticeRotation::all()` gives. (Before, the front faced the player by itself and the roll
  was whatever the pinned order gave first: a port put to face west had its top at the bottom, and one put to face north or south had its top on a side.)
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
5. **Orientation.** Grass and any other `axis_face` block placed along the clicked face, the generator writing orientations, a new generator version. *(Built only for the port, 2026-11-08; the owner chose to skip the rest until a grass top picture or trees give it something to show.)*
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
8. **The picture shader costs 0.84 ms (10 percent) on the integrated GPU at 1280 by 720 whatever the distance** (nothing measurable on the discrete one), and `.textures off` takes it away. Where it goes, and a version that costs 0.63 that was built and **not kept** (your call, 2026-11-08), are in "As built" (step 4,
   "Where the cost goes"). Whether the cost is acceptable until an optimisation pass is yours to say.
9. The window command is `.textures`, not a name you chose.

## 11. Not in this version

Per-state pictures (D13); animated pictures (flowing water); translucent (blended) pictures, as against the cut-outs of "Leaves and glass"; tints by climate; the other textures on disk;
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

**Step 3 (2026-11-07), in three pieces.**
- **a. The definition.** `BlockDefinition` has `texture` (the base name, a namespaced id) and `color_from_pictures` (the colour is the average of the pictures and follows them if they change; only a block with a texture has it; choosing a colour or taking the
  texture away ends it). Both are in the byte form: two bytes more for each block in the package (Cave 2,642 to 2,660, 2048 3,151 to 3,159, Eels 1,189 to 1,199, predicted from the number of blocks before the run), and the sessions' streams longer by the same.
  The package carries the base name and the flag because a client's texture pack (D9) finds pictures by it and may work the colour out from its own pictures.
- **b. Pictures in packs** (`engine/data_packs`: `tile.rs`, `pictures.rs`). `Tile::from_png` reads a square 8-bit RGB or RGBA PNG of 8 to 256 pixels (power of two) after looking at the header, and refuses the rest with the reason; the average is alpha-weighted
  and in linear light (`libm::pow`, for the same reason as the rest of the project). `resolve_pictures` is the table of 4.1 (the 16 combinations of files are a test written apart from the code). A pack's `textures/` folder is read by the directory reader:
  `name.png` is `pack:name`, and `namespace/name.png` puts a picture in that namespace (replacing one that is there, said in a warning; only for a namespace the pack depends on). The loader (`load_packs`, which now takes the game's pictures too) puts the packs'
  pictures in load order, then settles every block with a texture: finds its pictures (an error naming the block and what was looked for), works out the colours that follow, checks the limits (at most 4 sizes and 256 pictures of a size among the pictures in use;
  the files of the fifth size are named) and returns `Loaded.textures` (each block's pictures, and the tiles in use) for step 4. Patches `set`, `test` and `remove` the path `texture` (a patch writes the namespace). The schemas describe the key (and `color` is not
  required when `texture` is there) and the operations.
- **c. The game's blocks.** `texture` replaced `color` in the files of stone, dirt, grass, wood (`log`), great river water (`still_water`) and the dev docking port (`base_blocks:dev_docking_port`: its pictures stay in `base_blocks/textures`, where the owner made
  them, and Cave's block names them by namespace); `base_blocks` embeds the nine PNG files (`textures()`) and `BaseBlocks::new()` and `dev_docking_port()` settle the colours against them; Cave gives the pictures to its packs. A test keeps the embedded list equal to the
  files of those names and to the pictures the blocks use. Lamp, fire and air have no texture and their colours are what their files say.
- **What the colours became** (measured, in linear light; before is the colour in Rust): stone `#808087` to `#606060`, dirt `#734d2e` to `#81703b`, grass `#4d9e33` to `#278d2b`, wood `#8c6133` to `#877333`, great river water `#1447d9` to `#283de8`, the port `#2b3638` to
  `#797658`. (The table of 5.3 was an estimate from byte means; these are the numbers.) Stone is about a quarter darker, as said; the port is no longer gunmetal gray, it is the colour of its pictures, and the test that said so now checks that the colour is the average of
  its pictures.
- **Checked.** The numbering and the face-material and behaviour columns of every block's line are unchanged; only the colour column of the six blocks and the palette digest moved. Cave's package went from 2,660 to 2,782 bytes (the names of the pictures) and its session
  stream by the same 122 (7,205,489 to 7,205,611), predicted from the package and then seen, with the saved world unmoved. 2048 and Eels did not move. A pack that repaints `base_blocks:stone` makes the game's stone and a pack block that uses the picture white, through Cave's
  real `apply_packs`. Nineteen mutations of the picture code (the weights of the average, the order of the chain, linear light, who may replace what, both limits, the wiring of the loader, the patch rules, the directory reader, Cave's hand-over of the pictures, the
  embedded list) are all caught; one gap was found and closed (a name paired with the wrong embedded file).
- **Not in step 3:** nothing draws a picture or sends one to a client (step 4); the `warnings` of the loader are shown by `check-pack` and the server as the other warnings are; `check-pack`'s unused-picture warning does not cover the game's own pictures.

**Step 4 (2026-11-07), in four pieces.**
- **a. The package carries the pictures.** `PackageData::tile_classes` holds the tiles in use as raw 8-bit RGBA, by size (smallest first), after the ambient light; each block's definition carries `pictures` (top, side and bottom as a size number and a place,
  `TileRef`). Decoding refuses a size that is not a power of two from 8 to 256, sizes repeated or out of order, an empty size or more than 256 pictures of one, more than four sizes, more than 8 MiB of pixels in all, and a block whose pictures are not among the
  tiles. The settling numbers the tiles in use by size and then by name. Cave's package went from 2,782 to 12,046 bytes (predicted: 9 flags, 6 blocks of 6 bytes of places, 9 tiles of 1,024 bytes and framing); 2048 and Eels by 5 and 6 bytes. A test written for the
  package found that `average_color` weighted a picture by its pixels (a 16 by 16 top and an 8 by 8 side were not one face and four faces); it counts a face as a face now.
- **b. The table.** A palette entry can be a colour with a picture (a `FacePicture` word of 17 bits: the size's number, the place, which way the image runs across the face and which way up). For a block with pictures the table gives every face of every model in
  every orientation such an entry (its flat colour and the picture of the made face the turn brings there, with `across` and `up` turned too). **Levels of detail need no code to stay untextured**: a coarse cell is drawn in the block's own entry, which has none.
  `BlockTable::palette_pictures()` is parallel to `palette()`. Tested by geometry: in all 24 orientations the image is never mirrored, the top's picture is on the face the top went to, a log on its side has its grain along it. Cave's palette is 263 entries, 240 of
  them with a picture.
- **c. The renderer** (`engine/renderer`: `block_textures.rs`, `pictures_on.wgsl`, `pictures_off.wgsl`). One 2D texture array for each size, one layer for each picture, a chain of smaller pictures under each averaged in linear light (a test: black beside white
  becomes the sRGB byte 188, not 128); nearest when magnified, mixed when minified, no tile bleeding into another. The picture's word travels in the bits of the fourth channel of the palette entry (`1.0`'s bits when there is none). The place on the face is the position
  in the brick (`fract` of it), so a slab shows its slice (the vertex shader works the pair out and the fragment shader gets it interpolated; see "Where the cost goes" for the other way). The share of the picture is `1 - smoothstep(distance, 1.25 distance, d)`; **the gradient is taken before anything branches** (taking it in a branch is not allowed), and a face past the band does not read the
  texture. **The reading of pictures is a second fragment shader.** A shader that merely *used* the texture bindings made the two shadow tests fail on the integrated AMD GPU (3 to 4 of 10 runs; none with the committed renderer, none with the same shader with no
  picture code or with only the derivatives; a version with one sampling call failed 4 of 12), with no picture drawn. So the voxel shader has a hook, `picture_albedo`, and is built with `pictures_off.wgsl` (the face's own colour, no texture at all) or `pictures_on.wgsl`;
  the pipelines of the second are made when a palette first has a picture and chosen only when a distance is set, so a palette with no picture draws as it did and **`.textures off` is a real switch** (measured: equal to before, below). Then the two shadow tests failed
  0 of 12 times, and the picture tests 0 of 18 on the integrated GPU (a stand-in wait for the textures to reach the GPU was tried and is not needed).
  GPU tests on every adapter (`gpu_rendering.rs`): the four corners of a picture on a side towards +z, towards -z and +x and on top (upright, not mirrored), the fade (a partial share in the band, gone beyond it, none at distance 0 or when a palette has no
  pictures, back when the distance is), sizes and places found by their words, a bottom slab and a top slab (the lower and the upper half of the picture) and a coarse cell that stays flat. **Ten mutations of the shader and renderer are all caught** (coordinates read
  swapped, across and up swapped, up not turned, size ignored, place ignored, no fade, fade reversed, pictures never in use, distance ignored, bits not masked).
- **d. The client.** `ClientSettings::texture_distance` (default 64) and the window command `.textures [VOXELS|off|default]` (tested); `configure_renderer` hands the renderer the palette's words and the package's tiles (`picture_sizes`, tested), and the distance is set every
  frame with the light. **A screenshot of the real game** (headless, noon) shows the grass and dirt in their pictures near and the flat colour beyond.
- **Measured** (`--benchmark`, 1280 by 720, view radius 8, noon, the camera turning, dev profile (optimised); before is the commit of piece b): the discrete GPU (RTX 3090) **2.77 ms before and 2.78 with pictures**; the integrated GPU (AMD) **10.5 ms before and
  11.4 with pictures** by the benchmark as it stood, which does not change with the distance (0, 16, 64 or 128 voxels: 11.3, 11.5, 11.4, 11.5); with `.textures off` it is 10.5, as before. **That benchmark waited for the GPU after every frame, and on this machine the
  wait comes back in whole milliseconds (the medians of every executable fell on 11, 12 and 13 ms in turn), so a difference of under a millisecond could not be read from it**; the +0.9 happened to be near the truth, but I did not know that. **`--benchmark` now waits once for each ten frames** (`headless.rs`; its line says "batches of 10 frames", and "worst batch" is the worst of the batches, each spread over its ten frames), and the numbers from it, the same executable with the distance 0 and 64, six runs each, are: the discrete GPU **1.98 ms with pictures off and 1.97 on**,
  the integrated GPU **8.08 off and 8.92 on (+0.84, 10 percent)** (the old single-frame figures were 0.8 ms higher on the discrete GPU, which is what a wait for one frame costs there).
- **Where the cost goes** (integrated GPU, ten frames to a wait, six runs of each, interleaved; pictures off is 8.1 ms a frame; each piece was a build with the rest of the picture shader taken out, so the pieces are not quite additive): passing the picture's coordinate pair from
  the vertex shader **+0.35 ms**, the picture word +0.1, the derivatives +0.06, the texture read +0.3 (one array costs the same as four), the fade maths nothing that shows, and **the sampler's filtering nothing** (nearest everywhere is the same).
  **A cheaper version was built, tested and measured, and is not kept** (commit `9213a59`, reverted on the owner's decision, 2026-11-08, on my advice). The fragment shader worked the pair out itself, from the interpolated position and the placement turned back
  (`transpose(placement.rotation) * (in.relative - placement.offset.xyz) / placement.offset.w`, then `along` for the two ways of the picture word; it needs the placement visible to the fragment stage): +0.87 became +0.63 (8.1 to 8.75, 7 percent), the picture tests passed and five
  mutations of the new lines were caught. **Why it was reverted:** it saves 0.24 ms, 3 percent of a frame on one GPU, in a shader that shadows, global illumination, MSAA and waving vegetation are each going to rework; and it makes the picture's place depend on the *final* position of the
  pixel, so a vertex shader that moves a leaf would make its picture swim with the wave, and the way back is to pass the pair again (the 0.35). **Tried and not kept either:** passing the material number and reading the palette in the fragment shader (the colour and the picture word are then not passed): 8.64
  with pictures **but 8.29 with none** (it costs every game and `.textures off` 0.13 ms). **What was kept from that work:** the batched benchmark, and the test of a picture on a block of a turned ship (`a_picture_on_a_block_of_a_turned_ship_reads_the_same_from_outside_as_on_the_ground`), which any version must pass.
  **[belief]** the four-size cap is not what costs: the arrays are bound whether one size or four is in use. Not measured: a world with four sizes in use, a camera that stays near much stone at the edge of the distance, the sun moving (the picture shader changes nothing the shadows draw).
- **Not in step 4:** a `--textures` option for the headless run (the distance is the client's setting in the window; the headless picture takes the default); a settings file (the client has none, `.ambient` and `.shadows` are not kept either); the mip chains are made on the
  client when the pictures are sent to the GPU (the time that takes is not measured, for nine tiles or for 256 pictures of 256 pixels).

**Step 5 (2026-11-08), only the port.** The owner chose, when asked what to build of step 5, to **skip the rest for now** (no terrain block has an axis and grass has one picture, so the generator writing orientations would show nothing yet; grass does not get `axis_face`,
and no generator version was added) and to **change how the dev docking port is placed**: its top goes away from the clicked surface, as a log's does.
- `Placement` has a fourth value, `AlongPlacedFaceFacingPlayer { axis, front }`, for a block that has both properties (the front rule won before, silently, for a block that had both); `orientation_of_placement` takes the turns that carry the axis to the clicked face (the top is up when the request did
  not say what was clicked) and, of those, the one that carries the front to the side of the block the player is on, the first of them when none is more than another. The port's file has `"engine:axis_face": "up"`.
- **Pins moved by an amount I predicted wrongly:** I counted 2 bytes for the property; a property is written by its name (17 bytes) and a flag for every state, its value and a count of states set, **20 bytes**. Cave's package is 12,046 to 12,066 bytes and the golden session's stream 7,214,875 to 7,214,895; the saved world did not move; the table line of the port
  says `axis PositiveY` and its placement; 2048 and Eels did not move.
- Tests first (all failed before the change): the top is the clicked face for all six faces and five ways of looking; the front is the side the player is on on the ground, against a wall (looking down at it, up; looking up, down), and along a wall; not told what was clicked, the top is up; looking along the top gives the first turn, every time;
  the table says which placement a block has (both). The test that places the port in the real game now says its top is up (the front is one of the four sides). **Eight mutations** (the player on the other side, a tie kept by the last, the top down when not told, any turn and not only those that carry the line, the side
  that least agrees, the front rule winning, the line rule winning, the wrong face as the front) **are all caught**; a ninth, working out the player's side without the part of their look along the line, changed nothing and is an equivalent mutant (every side of the line is across it), so the code does not do it.
- **A consequence the owner may not have meant:** a port can no longer be put with its yellow face up or down against the floor or ceiling (the front is always across the top), so two ports cannot be stacked yellow to yellow by putting them on the floor; put against a wall the yellow face can be up or down, by looking down or up at the wall.

**Leaves and glass (2026-11-08, owner's request; the first blocks with clear pixels).**
- **Transparency modes in block data (the owner's decisions, 2026-11-08).** A block says `"transparency": "opaque" | "leaves" | "glass"` (default opaque), and the mode is a fixed answer to three questions
  (`block_models::Transparency`, tabled in `pack-format.md`): whether the clear pixels of its pictures are holes, whether the faces between two voxels of the block are drawn, and whether its faces are drawn from behind.
  Leaves: yes, yes, yes. Glass: yes, no, yes. Opaque: no, no, no (so the alpha of a picture on an opaque block is ignored, and a stray alpha channel makes no holes). All draw the faces that look at the player.
  *Built first as two engine properties, `engine:inner_faces` and `engine:back_faces`, and replaced the same day, by the owner's choice of named modes.*
- **`see_through` is now two flags**, `sight_through` (hides nothing behind it) and `light_through` (light passes and the voxel holds light itself; `BlockTable::connections`), the owner's names. They were one flag with both meanings; air, leaves and
  glass have both. A mode other than opaque needs `sight_through` (the loader refuses it, and so does a patch whose result lacks it; the block schema says so with `if`/`then`).
- **Cut-outs.** The picture word of a palette entry has a bit for them (bit 17, `FacePicture::cut_out`, set when the block's mode is not opaque). The fragment shader reads the picture's alpha and does not draw a pixel under one half (`discard`) where that bit is set;
  the alpha is faded with the picture over the fade band, so beyond the texture distance a face is whole (as it is in the coarse levels). Both pictures on disk have only alpha 0 and 255 **[measured]**. The mips are
  made weighting each pixel by its opacity, as the black the clear pixels are stored as would darken the edges at a distance (`mip_chain`; four clear pixels give the plain average).
- **A sheet from behind is a quad on the same plane looking the other way** (`mesh_partial_blocks`): no pipeline change, since a face looking at the camera passes both the per-brick choice of face directions and the
  GPU's culling, and a detailed quad already says where its plane is whichever way it looks. It is lit by its own voxel (a face outward is lit by the voxel it looks at, so a block that is not `light_through` is dark from inside). It is **not** drawn where the voxel in front is a solid
  sight-through block that covers the whole rectangle (that block draws its own face on the plane, looking the same way, and two sheets there would fight for the pixels). The picture on a sheet from behind is read
  as the face it looks as (not mirrored), so every plane of a mass of leaves has the same holes in the same places and one sees straight down a column of them.
- **Cost:** these blocks are detailed quads (16 bytes), not greedy-merged: a lone leaf block is 12 quads, a solid 32-voxel cube of leaves (every face of every voxel) is 196,608 quads, 3 MB. Routing
  sight-through whole cubes through the merged path would need a flag on the quad for the sheet from behind; not done. How much it costs on the GPU is measured below, under the shadows.
- **Cut-out shadows (built the same day, as the owner's second phase).** The shadow passes had no fragment stage; for blocks that cut holes they now have one. When a palette entry with the hole bit (17) is set and pictures are drawn, the sun's and the sky's maps are drawn with a second set of
  pipelines (`ShadowPipelines::new(.., cutout: true)`) whose vertex stage also finds the face's picture and where it is on it, and whose fragment stage reads the picture and `discard`s a pixel under half alpha by the rule of the main pass, including its distance: beyond the picture distance (and with
  pictures off, distance 0) a face casts a whole shadow, as it is drawn whole. The pipelines have their own bind group (the frame's copies, the palette, the pictures), made with the first such picture, so a renderer with no such blocks draws its maps with exactly the pipelines it always had (the
  integrated AMD GPU's trouble with unused picture bindings, `test-flakes.md`, is the reason for the separate shader there too). Setting pictures, or the picture distance, draws the kept maps again. The sky's maps use the same pipelines and no test looks at them apart from the sun's. Test: a roof of leaves, glass and an opaque block over a floor, with a clear picture, an opaque one and pictures off
  (the floor under it is lit, dark, and dark), on every adapter; and a mutation that never uses the new pipelines is caught by it. **Measured (2026-11-08, a throwaway GPU test, removed again; 1280 by 720, all four adapters, the camera and the sun moved every frame, 30 batches of 10 frames, wall time with the GPU waited for):** a scene of 36 solid canopies of leaves of 9 by 9 by 5 voxels on nine bricks of floor (99,846 quads; a dense stand, not a realistic tree), against the same volume as an opaque block (270 quads, merged). Mean frame, in ms:

| | leaves, shadows off | leaves, cut-out shadows | leaves, the plain shadows they had before | opaque canopy, shadows off | opaque canopy, shadows on |
|---|---|---|---|---|---|
| AMD integrated, Vulkan | 3.2 | 3.9 to 4.1 | 3.9 to 4.0 | 0.47 | 0.63 |
| AMD integrated, DX12 | 3.0 to 3.1 | 3.8 to 4.0 | 3.8 to 3.9 | 0.43 | 0.60 |
| RTX 3090 (both) | 0.11 to 0.14 | 0.13 to 0.17 | 0.13 to 0.17 | 0.04 to 0.11 | 0.05 to 0.12 |

What it says: **the cut-out shadow pipelines cost 0.01 to 0.09 ms (at most 2 percent) over the plain shadows leaves had before**, on the integrated GPU and on the RTX. **Casting shadows from this many quads costs about 0.8 to 0.9 ms (27 percent) on the integrated GPU either way**, which is the cost of 100,000 quads as casters, not of the picture lookup. **The larger cost is the leaves themselves**: with shadows off the canopies cost 2.6 ms more than the same volume as one opaque block on the integrated GPU (3.1 against 0.43; nothing on the RTX), which is every face of every voxel drawn (the inner faces) and thrown away by the shader where it is clear. Caveats: one dense synthetic scene, so a real tree (a thin shell of leaves) costs a fraction of this;
the "plain shadows" column is the new code with the new pipelines switched off by a temporary patch, not the old binary; walking and flying speeds gave the same numbers within noise, so the maps were probably drawn again nearly every frame (the whole cost of a refresh, not an average over frames that keep their maps), which I believe and did not check.
- **Not done:** blending, a different texture on the back of a face, a per-face turn of the picture so that a mass of leaves does not show columns, a dithered far view (owner's
  unscheduled list, `future-goals.md`), and a light that leaves dim (light is passed whole or not at all).
- **Pins moved, once:** every game's package changes (the definitions carry two more bytes each, and `sight_through` is named in the table lines); Cave's gains the two blocks (numbers 8 and 9; the dev blocks are 10 to 12), and its saved world's hash moved because the registry has two more blocks.
  **The pins of `block_golden.rs` and the golden stream of Cave were already stale** (the grass, the log and others had been redrawn since 2026-11-07; checked at the commit before this one), so their colours
  and digests moved for that reason too and were pinned again here.
