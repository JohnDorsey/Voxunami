# Data packs and the content package: what was decided

How a game's content reaches a server (data packs) and a client (the package in the welcome). Specs: `data-packs-spec.md`, `content-package-spec.md`; the guide is `pack-format.md`. Moved from `design-decisions.md`, which keeps what holds for every game; the stories behind these entries are in `history/decision-diary.md`. Status key as in `design-decisions.md`.

## Decided (data packs; built in Phase 29)

- **A pack is data, loaded by the server only.** A folder of JSON adds blocks and patches blocks; no code runs. A client needs no pack: it is sent
  the result in the package (Phase 28), which is why packs needed nothing on the wire. A server that finds any problem in its packs does not start.
- **Every problem is reported with its file and the path inside it, and all of them at once.** The reader walks a `serde_json::Value` itself (serde
  derive would stop at the first error and loses the path); unknown keys are errors with a "did you mean" when one is near; a failed patch changes nothing.
- **The loader is deterministic in what it is given, not in how it is listed:** packs in dependency order with ties by id, the files of a pack by
  name, so numbering and results are the same on every machine and a property test shuffles the listing. A pack may only add blocks in its own namespace
  (its id), and patch only namespaces it names as needs (directly or through its needs), so order cannot decide who wins.
- **Optional patches are the only quiet failure,** and they are not quiet: a patch that names a block or property that is not there is an error unless it
  says `"optional": true`, and then the skip is returned and printed as a warning. A `test` operation that fails is always an error.
- **The schema is generated, never written:** `pack_schemas(&PropertySchema)` makes the three JSON Schemas from the declared keys, kinds, ranges, defaults
  and docs and the shape names. A differential test keeps the schema and the loader agreeing on a corpus of files and on files damaged in one place;
  what a schema cannot say (a box with no size, a state the block lacks, a name taken, a cycle, a failed test) is listed in the test with its reason.
- **What a player can place is a property of the block** (`voxunami_cave:placeable`), set by Cave on its six blocks and by a pack on its own. Cave's items,
  its drops and its number keys come from it; the client reads the list from the server's package, so a client with no pack has the same keys.
- **A client refuses a package it cannot draw** (more colours than the renderer has) with an error instead of a panic, and the same check runs when packs load.
- Mine, M0 housekeeping (2026-10-30; `architecture-review.md`): **a folder in a pack that nothing reads is an error** (`check-pack` names it and suggests the nearest of `blocks`, `patches`,
  `bodies`), **`pack.json` accepts `"format": 1` and a free-text `"version"`**, and the examples in `pack-format.md` are checked by a test.
- Mine, M0 (2026-10-30): **a server that panics is noticed and nothing is saved from it**: a panic leaves a tick half done, and a file written from it could replace a good world. `ServerThread::stop`
  returns the failure, `has_failed` lets `serve` end by itself, and both say which save is the world. Open: a game played with no `--autosave` saves only on exit, so a failure there loses the session.
- Owner and mine, F5 (2026-10-30): **Cave owns Cave's ground.** No world is worth keeping before the first release (owner), so generator versions may change until then. The planet, the great
  rivers, the hills and the planet family are in `games/voxunami_cave/src/generators` under Cave's own ids (`voxunami_cave:rolling_hills_v1`, `small_planet_v1`, `empty_v1`, `great_rivers_v1`, the
  family `planet_v1`); `base_blocks` is only blocks and properties. The engine keeps the machinery and **`SurfaceLayers { air, surface, subsurface, deep }`**, so it names no block; its own tests use
  fixtures under `terrain`'s `test-support` feature. Checked: with the old ids written back Cave's golden session gave its old pins to the digit, so only the names changed.

## Decided (block textures and the game's own blocks in data; owner, 2026-11-07; not built; `block-textures-spec.md`)

- **The game's own blocks become JSON** read by the pack loader; Rust looks ids up by name. A block names its pictures by one **base name**
  (`"texture": "log"`); which faces differ follows from the files that exist (`_top`, `_side`, `_bottom`), never from the JSON. `top` is the face away from the face
  clicked to place the block. `color` is optional on a textured block (default: the top once and each side once, bottom ignored). **Per-face colours are in the JSON** (`face_colors`).
- **Pictures** travel inline in the welcome, one atlas per size (powers of two, 8 to 256, at most four sizes); a pack replaces pictures by file name or by patch; the client's texture
  distance is a setting, the texture fades to the flat colour beyond it, and levels of detail are never textured. Per-state pictures and the client's ordered texture packs come later.
- Terrain: the generator writes the orientation of blocks that have an axis, so grass tops point away from a planet. This replaces `architecture-review.md` A4's advice against textures.

- **Built 2026-11-07 (step 1 of `block-textures-spec.md`):** `face_colors` (a definition's six optional face colours, written in a block file by group or by face, set/tested/removed by
  patches) replaced the three mark properties, and face-valued properties take a word. The block table's answers are unchanged, to the orientation, for every block of Cave, 2048 and Eels (checked by dumping
  them from the old and the new code); the package got shorter by the three declarations.

- **Built 2026-11-07 (step 2 of `block-textures-spec.md`):** the game's own blocks (the eight of `base_blocks`, and Cave's dev docking port) are JSON files in the pack format, embedded in the program and read by the pack reader. They are registered in
  a list in Rust, not numbered by the loader, because the loader orders a pack's files by name and these numbers are fixed. The package, the numbering, the block table and the golden sessions are unchanged to the byte, with no pin touched.

- **Built 2026-11-07 (step 3 of `block-textures-spec.md`):** a block names its pictures with `texture`, and the picture of each face is the first file that exists (`_top`; `_side` then the plain name; `_bottom` then the top's), so a file added
  changes a block with no change to its JSON. A pack's `textures` folder holds PNGs (square, 8-bit RGB or RGBA, 8 to 256 pixels, a power of two) and folders named for namespaces, whose files replace or add pictures of that namespace. `color` may be left out of a block with a
  texture and is then the average of the pictures (top once, each side once, bottom not at all, in linear light) and follows them if another pack replaces one. At most 4 sizes and 256 pictures of a size are in use. The six blocks that have pictures in `base_blocks/textures`
  (grass, dirt, stone, great river water, wood, and Cave's dev docking port) use them; their far colours changed with it.
- **Built 2026-11-07 (step 4 of `block-textures-spec.md`):** pictures are drawn. The package carries them as raw RGBA by size (a client never decodes an image it is sent); the table gives a face with a picture a palette entry of its own (so a coarse cell, which uses the block's
  own entry, is untextured with no extra code); the renderer keeps a texture array for each size and reads the picture by position in the brick (a slab shows its slice), fading into the flat colour over a band of a quarter of the texture distance. The client's
  `.textures [VOXELS|off|default]` sets that distance (default 64). **The shader that reads pictures is separate from the one that does not**, because merely using the bindings made shadow tests fail on the integrated AMD GPU; a palette with no pictures, or a distance of 0, draws with
  the plain one. The picture shader costs about 10 percent (0.84 ms of 8.1 at 1280 by 720) on that GPU and nothing measurable on the discrete one. A version that works the picture's coordinates out in the fragment shader costs 0.63 and was built and **not kept** (the shader is going to be reworked for shadows, global illumination, MSAA and waving vegetation, which moves vertices and so would make a picture swim); it is in `block-textures-spec.md`, step 4, "Where the cost goes", for an optimisation pass.
- **Built 2026-11-08 (step 5 of `block-textures-spec.md`, the docking port only; owner):** a block with both `engine:front_face` and `engine:axis_face` has its axis go away from the face that was clicked, as a log's does, and its front turned to the player's side across it. The dev docking port has both. The rest of step 5 (grass with an axis, the generator writing orientations) is **skipped until something can show it** (a grass top picture, trees).

## Decided (the content package at the handshake; built in Phase 28)

- **The server says what the world looks like.** `Welcome` carries the game's *package data* (`game_shell::PackageData`): the property schema, every
  block's definition (colour, see-through, the boxes of each state's shape, state colours, the properties it sets), the channels the view reads
  and the ambient light. A client builds its block table from it, matching blocks to its own numbering by identifier, so a client and a server
  that disagree about a block draw it the server's way (the server wins, silently), a block the server has no definition for is drawn as unknown,
  and a recording carries the package, so a replay does not depend on what is installed. Terrain generators and client plugins are still compiled
  in (they cannot travel until generators can be sandboxed code).
- **Inline, not cached.** The package is in every welcome (a few KB for Cave: 1,345 bytes). Caching by hash needs a request message, a cache and a
  way for recordings to embed what was fetched, and saves nothing until packages hold assets; it is built in the phase that adds assets. No hash
  is sent now either: nothing would read it.
- **One spelling.** The byte form is canonical (maps in key order, counts and numbers as varints, colours as `f32` bits), and decoding refuses
  anything the encoder would not write (out-of-order or repeated keys, empty boxes, non-finite colours, a default outside its range), so a
  damaged or hostile package is a connection error naming the cause and never a panic. The codec for definitions lives in `block_models`, the
  package around them in `game_shell`.
- **`GameContent` has one source for this**: `package_data()`. `block_table()` and `ambient_light()` are derived from it for the server side;
  clients no longer call them (they take the table, the channels and the ambient light from the welcome). Server code that reads the table keeps
  using `GameContent::block_table`.
- **A package that does not fit what the client loaded** is a loud error (`NoBlockRegistry`, `NoChannel`, a property the schema lacks), not a
  silent fallback to the client's own definitions.
