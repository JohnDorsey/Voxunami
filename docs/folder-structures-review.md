# Folder structures and watched folders: a review

Asked by the owner on 2026-11-18 (the item "Review the folder structures and the watched folders" of `future-goals.md`, Unscheduled) and made the next day, with the question
"could servers and clients have the exact same asset pack format, and how close is that?". **A survey and a proposal; nothing in it is built.** Labels: **[read]** I read the
code or document that says it; **[measured]** a run; **[believed]** reasoning I did not run.

## 1. Every place the program reads or writes a folder or file

| Kind | How it is named | Layout | Read by | Watched? | Reloaded by | Reaches others? |
|---|---|---|---|---|---|---|
| **Built-in content** (`plugins/NAME/data`, `plugins/NAME/textures`) | the source tree, at build time | the canonical archive `build.rs` makes of the folders (since 2026-11-22: `data` whole, every `.png` of `textures`), `include_bytes!`d and read by `data_packs::read_builtin`; only the order of the blocks is a list, `BLOCK_ORDER` | the program | no | a build | yes: it is the package the server sends |
| **Data packs** (server) | `--packs DIR` for `play` and `serve`; no default | one folder per pack: `pack.json` (`id` equal to the folder's name, `depends_on`, `version`, `format`), `blocks/`, `patches/`, `bodies/`, `textures/` (`textures/NAME.png` named by the file, or `textures/NAMESPACE/NAME.png` for a picture of that namespace; `NAME_s.png` and `NAME_n.png` are companions) | `data_packs::read_directory` | **no** | the server command `/reload` (`packs::reload_from_directory`, between two ticks) | yes: in the package; recorded |
| **Client texture folder** | `--texture-pack DIR`; **default `plugins` when it is there** (since 2026-11-18), `--no-texture-pack` | `NAMESPACE/textures/NAME.png` (+ `_s`, `_n`); no manifest | `texture_pack::read_pictures` | with `--watch` | `.reload pictures`; written again over the pictures when the server replaces the package | **no**: this client's drawing only, not recorded |
| **Shader packs** (client) | the game data folder's `shaderpacks` (need not exist) and `--shader-packs DIR` (must exist) | one folder per pack: `pack.json` (`id` namespaced as `author:name`, `name`, `interface`, `format`, `settings`, `engine_settings`), `lighting.wgsl`; the player's choices in `NAME.settings.json` and `default.settings.json` beside the packs | `shader_packs::read_directory` | with `--watch` | `.reload shaders` | no |
| **Engine shaders** (developer) | `--engine-shaders DIR` | the source's `engine/renderer/src` (the shader files only) | `renderer::engine_shaders` | with `--watch` | `.reload engine-shaders` | no |
| **Game data folder** | `%APPDATA%\voxunami` (Windows), `~/Library/Application Support/voxunami`, `$XDG_DATA_HOME/voxunami` or `~/.local/share/voxunami` | `identity` (the key), `shaderpacks/` | the `identity` crate computes the place | no | no | no |
| **The world** | `--world FILE` | one file, written whole to a temporary file and renamed; the ships' files in a folder beside it, `ID-WRITE.voxship` (a half-written `.saving` is left alone) | `files::open_world`, `world_storage` | no | no | no (but it keeps the ids of content that is missing) |
| **Recordings, screenshots** | `--record FILE`, `replay FILE`, `--screenshot FILE` | one file each | `replay`, `headless` | no | no | no |
| **VirtualAtlas** | the folder it is started on | `NAME.png` tiles and `atlas.json` (the grid, the names, the `pbr` setting, per-tile channel text) | VirtualAtlas | by itself | its own save | the game never reads `atlas.json`; the bridge puts pictures in the running client's memory only, never on disk |
| **Pack schemas** | `pack-schemas DIR` | writes `block.schema.json` and the others | n/a | n/a | n/a | n/a |

All of that is **[read]**, from `options.rs`, `packs.rs`, `windowed.rs` (`what_can_be_reloaded`), `shader_packs.rs`, `texture_pack.rs`, `identity/src/lib.rs`, `world_storage`, `host.rs`
and `data_packs/src/directory.rs`. The watching is one mechanism, `reload.rs` (`Watcher`, `DiskSource`: modified time and size, then settled, then a different digest); what it is
pointed at is chosen in `what_can_be_reloaded`: **shader packs, pictures and engine shaders, and nothing else**.

## 2. What does not fit together

1. **Two formats called `pack.json`.** A data pack's names its namespace (`my_pack`, equal to its folder) and what loads before it; a shader pack's names an `author:name`
   id, an interface and settings. A folder that is both is not a thing; each reader rejects the other's keys as unknown. **[read]**
2. **The two layouts of a picture are mirror images**: a data pack has `textures/NAMESPACE/NAME.png`, the client folder `NAMESPACE/textures/NAME.png` (the shape of
   `plugins/`). The names of files, the companions and the pixel rules are the same. **[read]**
3. **The data folder is the identity's.** `shader_packs::default_directory` is `Identity::default_path()` with `shaderpacks` after it, so `--identity FILE` does not move it, and
   nothing says "the data folder" in one place or lets a player choose one. **[read]** (A second player on one machine, or a portable install, cannot have its own.)
4. **A default that depends on the working directory.** Only the texture folder has one (`plugins`); every other default is the game data folder. **[read]** A game started from a
   folder that has an unrelated `plugins` folder would read it (the stop-gap of 2026-11-18).
5. **(Fixed 2026-11-22, step 3 of `specs/pack-archive-spec.md`: `build.rs` takes every `.png` of the folder.) `TEXTURE_FILES` was a hand-kept list**, so a new picture or companion in `plugins/base_blocks/textures` does not ship until someone adds it (found 2026-11-18: five of the
   owner's companions were not in the package). A test keeps each listed entry equal to its file; nothing notices a file that is not listed. **[read]**
6. **Data packs are not watched**, and the server reads them again only on the command `/reload`. In a singleplayer game that is one more thing to type after each save, and
   it is the one kind of content a painter or a modder changes that `--watch` does not cover. **[read]**
7. **The client folder wins over everything.** A picture in the client folder replaces the world's (a server pack's included) on this client, by place. That is as designed (D9 of
   `block-textures-spec.md`: the server's pictures are one movable entry in an ordered list, at the bottom by default) but the ordered list is not built. **[read]**
8. **VirtualAtlas has one folder and the game has several.** The tool edits `plugins/base_blocks/textures`; a pack's pictures live in `PACK/textures/NAMESPACE/`. Pointing the tool
   at that folder should work, since it only needs `NAME.png` files, but I have not tried it. **[believed]**

## 3. Could servers and clients have the exact same pack format?

**Yes for pictures and for the manifest; no for blocks, and that is on purpose.** The authority line is the real boundary: what a server's pack holds changes the world and goes in the
package and the recording; what a client's folder holds is how this one client draws. So a client may read *the same folder* a server reads, and use less of it.

**Proposal** (my opinion; confidence medium-high that it is the right direction, medium on the order of the steps):

1. **One layout for pictures**: `textures/NAMESPACE/NAME.png` (and the plain `textures/NAME.png`, which the data pack reader takes too; which namespace it goes to I did not check), the data pack's. The client folder reads that layout, and **keeps
   reading the old one** (`NAMESPACE/textures/NAME.png`) so `plugins/` works unchanged. A folder that has `pack.json` is read as a pack; one that has none as it is read now.
2. **One manifest for a data pack and a client pack**: `pack.json` with `id`, `format`, `depends_on`. A client reads only the textures and ignores `blocks/`, `patches/`, `bodies/`
   (and says so once), so *the same folder can be given to `--packs` and to `--texture-pack`*. Block files in a client pack never take effect: that stays the server's.
3. **Shader packs keep their own kind**, told apart by what is in them, not by the manifest's name: rename nothing yet, but make each reader say *"this is a shader pack"* or
   *"this is a data pack"* when it meets the other's `pack.json` (today it lists keys as wrong). Whether to rename the shader manifest (`shader.json`) is the owner's question 1.
4. **One data folder**, named in one place, with `--data-folder DIR` to move it; `identity` and `shaderpacks` both under it; `--identity FILE` stays as an override of the key alone.
5. **Ship the folder automatically**: a `build.rs` in `base_blocks` that embeds every `NAME.png`, `NAME_s.png` and `NAME_n.png` of its `textures` that a block (or a companion of one)
   uses, or, smaller, a test that fails when a texture file is not in the list. Either removes the silent case of item 5 above.
6. **Watch what a person edits**: add the `--packs` folder to the watched kinds (as "packs": read again, validated beside, then swapped as `/reload` does), at least for a
   singleplayer game. This is the biggest piece; the server half exists (`package_reload::reload_packs`), the missing piece is a `Source` and the trigger.
7. **Do the ordered list** of texture sources of D9 last: it is where the client folder, the server's pictures and any packs are given an order the player can change.

### How close is that to what we have?

Steps 1 and 2 are small: the data pack's picture reader (`data_packs/src/directory.rs`, `pictures.rs`) and the client's (`texture_pack.rs`) already share the naming, the companion
rule, the size rule and the way a picture finds its place; what differs is the folder walk and the manifest check. I would estimate a day each, with tests first. **[believed]**
Step 4 and 5 are a day or less. Step 6 is the first piece with a risk (a half-written pack on a running server; the settle and the swap-or-keep rules of `reload.rs` are made for
exactly that, so I expect it to fit **[believed]**). Step 7 is a design of its own (the spec says "a list the player orders, saved") and not small.

## 4. Questions for the owner

The owner's answer to the first, and a larger idea in place of steps 1 and 2 (one canonical zip archive for a pack, the same on a server and a client), are in `specs/pack-archive-spec.md`; that proposal supersedes steps 1, 2, 5 and 6 above in how they would be done, and keeps their goals.

1. **Answered 2026-11-20 and built: renamed to `shaderpack.json`**, and each reader names the other kind when it meets the other's manifest (`decisions/packs-and-content.md`). Asked as: rename the shader pack's `pack.json`, or keep both and make each reader name the other kind? (I lean to keeping both for now: the second is a tool of one engine and the name of a
   file is cheap to say twice; confidence low-medium.)
2. Should the `plugins` default of 2026-11-18 stay as it is until step 1 and 4 are done, or become an explicit developer switch (`--dev`, or the folder named in a file in the source
   tree) at once? It is the one default that depends on where the program is started.
3. Is a client pack that carries blocks to be refused with a message (my proposal: ignored, said once), or an error?
4. Which comes first for you: watching `--packs` (step 6), or the automatic shipping of new pictures (step 5)?
