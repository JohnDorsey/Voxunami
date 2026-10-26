# Phase 29: data packs (JSON blocks and patches), the generated schema, `check-pack`

Status: built (29a to 29e). *Phase 40c (2026-10-26) added a fourth kind of file, bodies (planets): `rails-and-recipes-spec.md` section 3.3 and `pack-format.md`; they read, load, are checked and have a schema as the rest do.* The owner answered section 6: serde_json (and a JSON Schema validator for tests only), a patch to a missing
property is an error with `"optional": true` to skip it *and the skip is logged*, one block per file named by the file, a pack adds blocks only in its
own namespace, and the placeable property is in. For writing a pack see `pack-format.md`. What changed from the design below while building:
- Reading goes through `serde_json::Value` with a path-tracking walk of our own, not serde derive, so every problem in a file is reported (derive
  stops at the first) and each names its JSON pointer. Only `serde_json` is a dependency of `data_packs`; `jsonschema` is a dev-dependency.
- `add` takes `"shape"` (not `"state"`, which is the number of a state for property operations). A top-level `"$schema"` key is allowed in every file.
  Whole numbers may be written `7.0` (JSON Schema's `integer` takes both).
- `pack-schema` writes three files (`pack`, `block`, `patch`) into a directory. The commands take the game first, like `serve`:
  `voxunami check-pack GAME DIR`, `voxunami pack-schema GAME DIR`.
- A patch may touch only namespaces its pack names in `depends_on`, directly or through the packs it depends on (the rule that keeps load order
  from mattering); a failed `test` is an error even in an optional patch; a pack's folder must be named as its id.
- The materials limit (1,048,576 since Phase 31; it was 240) is checked where packs load and again where a client takes a package, because the renderer would panic.
- `voxunami_cave:placeable` is a bool property; Cave sets it on its six blocks, derives its items and number keys (nine of them) from it, and a client
  reads the list from the server's package.

## 1. What this is for

Today every block is Rust (`BaseBlocks::new`). A *pack* is a folder of JSON that adds blocks and changes other blocks, with no code and
no sandbox: the first thing a mod author can do. It builds on three things that now exist: properties (Phase 24: a block's rule-facing
facts are keys in a schema), the package data (Phase 28: a client draws what the server says, so a client needs no pack installed), and the
registry's placeholders (a world that holds a block of a pack that is missing loads, keeps it byte for byte and draws it as unknown).

Only the server loads packs. A client learns the result through the welcome, and a recording carries it.

## 2. The format (my proposal)

A pack is a directory:

```
my_pack/
  pack.json            { "id": "my_pack", "depends_on": ["base_blocks"] }
  blocks/marble.json   one block, named `my_pack:marble` (the namespace is the pack's id, the name is the file's)
  blocks/glass.json
  patches/dim_lamps.json
```

A block file (every key but `color` optional; unknown keys are errors):

```json
{
  "color": [0.9, 0.9, 0.95],
  "see_through": false,
  "states": [ "full", "half:negative_y", { "boxes": [[0,0,0, 8,4,8]] } ],
  "state_colors": [[0.9,0.9,0.95], [0.8,0.8,0.9]],
  "properties": { "base_blocks:emission": 3, "base_blocks:passable": false },
  "state_properties": { "1": { "base_blocks:emission": 0 } }
}
```

`states` are named shapes (`full`, `empty`, `half:FACE`, `stair:FACE`, the same constructors the engine has) or explicit boxes in eighths of a
voxel; one state if omitted. A property value is checked against the schema's declared kind and range when the file is read.

A patch file changes blocks that already exist (this pack's, a dependency's, or the code's):

```json
{ "target": "base_blocks:lamp", "optional": false, "operations": [
    { "op": "test", "property": "base_blocks:emission", "value": 14 },
    { "op": "set",  "property": "base_blocks:emission", "value": 9 },
    { "op": "set",  "path": "color", "value": [1.0, 0.8, 0.5] } ] }
```

Operations are `set` and `remove` on a property or on a top-level field (`color`, `see_through`, `state_colors`), `add` for a state (appends a shape),
and `test` (the patch is refused unless the value is as stated: how a mod says "I was written against this"). Tags (bool properties) merge by
union: two packs that both set a tag to true agree, and `remove` is how one takes it away.

## 3. Order, and what fails

- Packs apply in dependency order, ties by pack id; blocks are added to the registry after the code's own, in that order, so numbering is the
  same on every machine with the same packs (and saved worlds keep their own tables anyway).
- **Every problem is an error that names the file and the path inside it** (`my_pack/blocks/marble.json: /properties/base_blocks:emission: 300 is
  outside 0 to 255`). A file that does not parse, an unknown key, a block that already exists, a dependency that is missing or cyclic, a `test`
  that fails, a patch whose target does not exist. Loading collects all of them rather than stopping at the first, and a server does not start
  on any.
- A patch to a property whose plugin is not loaded is an error, unless the patch says `"optional": true`, in which case it is skipped (the
  question the plan left open; my recommendation, section 6).

## 4. The generated schema, `check-pack`

- `voxunami pack-schema GAME` prints a JSON Schema for pack files, **generated from the registered property keys and kinds and the shape
  names**, never written by hand, so it cannot drift: a key a plugin declares appears with its type, range, default and doc line. Editors that
  read `$schema` complete and check files as they are typed.
- `voxunami check-pack DIR [--game GAME]` loads the pack against that game's schema and code blocks and prints every error as above; the exit
  status is non-zero if there is any. It is the same loader the server runs, so passing it means the server will start.
- A differential test keeps the two honest: a corpus of valid and invalid pack files is run through the loader and through a JSON Schema
  validator (a dev-dependency only) against the generated schema; they must agree on every file. Where a rule cannot be said in JSON Schema (a
  dependency cycle, a `test` operation) the corpus marks the file as outside the schema's reach, and the loader alone judges it.

## 5. Where it plugs in

- `GameContent` gains `apply_packs(&mut self, packs: &LoadedPacks) -> Result<(), PackErrors>`; `ContentPackage` implements it by adding to its
  block registry (and so to `package_data()`, which is what clients receive). The three games get it for free. The shell's `play`, `--host` and
  `serve` take `--packs DIR` (the directory holding pack folders; default none). Clients take nothing.
- A new crate, `data_packs` (engine level: it knows block definitions and the schema, not any game), holds the model, the reader, the loader,
  the patch applier and the schema generator. The `launcher` gets the two subcommands.
- Limits that exist today and that `check-pack` states instead of failing late: the renderer draws at most 1,048,576 materials (blocks plus state
  colours; Phase 31 widened it from 240), and a block has at most as many states as the state channel holds.

## 6. Questions for the owner (my recommendation first)

1. **JSON library.** Add `serde` and `serde_json` as dependencies of `data_packs` (recommended, high confidence: it is the standard, gives
   parsing with error positions, and the schema generator builds `serde_json::Value`s). Writing a JSON reader ourselves would be correct but
   is a lot of code for no gain. A JSON Schema validator crate (`jsonschema`) as a **dev**-dependency only, for the differential test
   (medium confidence on the choice of crate; I will check its size and maintenance before adding it).
2. **Patch to a property whose plugin is missing:** an error, with `"optional": true` to opt out per patch (recommended). The alternative
   (always ignore with a warning) makes a typo in a property name look like a working pack.
3. **Layout:** a directory with one block per file, named by the file (recommended: diffs and merges are per block, the identifier cannot
   disagree with the content). The alternative is one JSON file per pack holding a map of blocks.
4. **Namespacing:** a pack may only *add* blocks in its own namespace (its id); it may *patch* anything. (Recommended; it is what keeps two packs
   from fighting over a name.)
5. **Can a pack's blocks be played?** Today Cave's six placeable blocks and their items are in Rust, so a data block exists but cannot be
   placed. I propose a bool property `voxunami_cave:placeable` (default false, true on those six) from which Cave's items and hotbar are derived,
   as step 29d, so a pack can add a placeable block. Without it, Phase 29 delivers blocks that terrain and patches can use but the player cannot
   pick up. Recommended: include it (medium-high confidence; it makes the phase end with something playable).

## 7. Steps

- **29a** Dependencies (question 1), the data model and strict reader with errors that carry file and JSON path; tests first (every kind of
  malformed file, one assertion on the exact message each).
- **29b** The loader: dependency order, adding blocks, patch operations, tag union, `optional`, all errors collected; property tests that
  applying packs is independent of the order of the directory listing, and that a failed patch changes nothing.
- **29c** The schema generator, `check-pack` and `pack-schema`, and the differential test.
- **29d** Wiring: `GameContent::apply_packs`, `--packs`, an end-to-end test (a pack adds a block; a client with no pack sees and draws it;
  a recording of the session replays it; a world saved with the pack's block loads without the pack and keeps it), and `placeable` (question 5).
- **29e** Documentation for mod authors (`docs/pack-format.md`), `design-decisions.md`, `plan.md`, build log; gates on both systems.
