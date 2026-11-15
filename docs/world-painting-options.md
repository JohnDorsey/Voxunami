# Painting on a block in the game: options for VirtualAtlas and the game

**A proposal; nothing here is built.** Written 2026-11-15 on the branch `graphics-world-painting`. The owner asked to plan and explore, not to build. Claims are marked: **[read]** I read it in the code or a document, **[believed]** my reasoning, not run, **[not checked]** a question I did not look into.

## The goal, in the owner's terms

Paint on a block's face in the running game, with `plugins/base_blocks/textures/VirtualAtlas.py` (VA) as the painting tool, while the picture used for editing is the game's own rendering, with any pack or shader pack, as the player sees it. Cave must grow as little as possible. The tool is for developers: a player leaves it uninstalled, and it must not open a way to cheat.

## What is true today

- **[read]** The base blocks' pictures are `include_bytes!` in `plugins/base_blocks/src/lib.rs`. They go into the package (raw RGBA, no PNG on the wire) and the client builds one texture array for each picture size (`renderer/src/block_textures.rs`). A face's picture is one word: bit 0 "has a picture", bits 1 and 2 the size class, bits 3 to 10 the layer. That word is already a tile identity.
- **[read]** Texture packs (`block-textures-spec.md`, D9: a client folder that overrides a face's picture, cosmetic, replays unaffected) are specified and **not built**; no code mentions them.
- **[read]** Shader packs change live: `.shader NAME` and `.shadersetting` call `set_shader_pack_with_settings`, which makes the shaders again. Nothing watches files on disk, and there is no `/reload` (`architecture-review.md` A6 says so; no file-watching crate is a dependency).
- **[read]** A client plugin's context has `target()`: a CPU ray hit with the grid, voxel, the voxel before it and the distance. It also has `send_command` and `send_message`, so a client plugin can act for the player.
- **[read]** The renderer can render to an offscreen texture and read it back (`render_to_rgba`, used by `--screenshot`). Features of a shader are compiled in or out (`pictures_on`/`pictures_off`, `relief_*`), so a feature that is off costs a player nothing.
- **[read]** VA is one Python file (5,233 lines) that keeps one PNG for each tile and an `atlas.json`. It has no way to talk to another program.
- **[read]** `architecture-review.md` D.3 already draws the line: a client mod that only changes what this player sees and hears is allowed; resource packs are safe from the start; client code mods come after server mods, in a sandbox with a narrow API. The client regenerates terrain from the seed, so it already knows terrain the player has not seen.

## Four questions, kept apart

The owner's idea (video out, mesh and hovered name out) answers the first two together. They have separate answers, and the best answer to each is different.

### A. How does the painter see the game?

| | What | For | Against |
|---|---|---|---|
| A1 | **The game's own window is the view.** VA is a second window with the tools. In a paint mode the game frees the cursor and sends VA the pointer's tile pixel and the buttons. | No video at all; "exactly as the player sees it" is trivially true; least code. | Two windows. The brush guide has to be drawn by the game (see C). |
| A2 | VA captures the game's window from the screen. | Nothing added to the game for pixels. | The frame and the pointer's answer drift apart (they come by two roads); DPI and a covered window break it. |
| A3 | The owner's: the game writes each frame into shared memory (or a shared GPU texture) and VA shows it. | One window; VA can draw over the 3D view. | A frame is 3.7 MB at 1280 by 720 (about 110 MB/s at 30 per second, **[believed]** fine in shared memory, not measured). A shared GPU texture from Python is a large piece of work **[believed]**. |
| A4 | **VA asks and the game answers**: VA sends a camera, the game renders it and returns the frame with its pick map in one reply (`render_to_rgba` is the seed). | The frame and the answer cannot disagree. VA owns the window. | A request and answer for each frame; the game's window may be hidden or a second renderer. |
| A5 | **One frozen frame.** On a key the game sends VA a snapshot: the frame, the pick map and the table of names. VA paints against the snapshot; the live game shows each stroke at once (C). | No streaming, no sync, no hurry. The first thing to build, **[believed]** and it grows into A3 or A4 if wanted. | Moving the camera means a new snapshot. |

Opinion (**medium confidence**): A1 plus A5's snapshot first. The game's window already shows the true render, so A3 buys one window at a real cost. Choose A3 or A4 later only if the owner wants VA's tools over the 3D view.

### B. How does a pointer become a tile pixel?

- **B1, the CPU does it** (the owner's "shape of the mesh and the name of the texture"): use `target()`, work out the face and the point on it, and turn that into a tile pixel. It is exact only for a flat face with no relief. It must repeat the shader's choices a second time: the block's orientation (24), slabs and stairs taking a slice (D10), ships' own coordinates, and the shift of the prisms or of parallax occlusion. Two implementations of "which pixel is this" will drift apart, and then the pixel painted is not the pixel seen. **Not recommended.**
- **B2, the GPU says it** (**recommended**): the fragment shader writes a second output, the picture word and the pixel position in the tile **after** the relief shift, to a second render target. Only in a variant compiled in for the tool (the pattern of `pictures_on`/`pictures_off`), so a player's shader is byte-identical. The answer is the shader's own, so relief, orientation, slabs and ships are right by construction. Every screen pixel has an answer, so VA can draw a stroke in screen space and map each step to a tile pixel; a stroke crossing a block edge continues into the next block's tile pixels. Cost: 8 bytes a pixel of extra write, in the dev variant only (**not measured**).
- **B3**, the same shader rendered again for a 2 by 2 scissor around the cursor (a scissor, not a smaller viewport, so derivatives are right): less bandwidth but a second draw path, and VA could not draw a stroke in screen space. B2 is simpler.
- **Fallback with no GPU change:** the tool forces `relief flat` (a setting that exists) while painting, so a flat face is exact and B1 is enough. It gives up "with the relief on", so only a stopgap.
- **[not checked]** Whether a shader pack can replace the part of the shader that looks up a picture. I believe a pack hooks lighting and the engine owns the lookup (so every pack gets the pick output), but I have not re-read the interface for it. This decides whether "any shader pack" works.
- Limits **[read]**: pictures are drawn only within the texture distance (64 voxels by default) and levels of detail above 0 carry none; beyond that the answer is "no picture".

### C. How does a change reach the game?

- **C1, files and a watcher:** VA saves the PNG, the game notices and reloads. Today the base pictures are inside the program (`include_bytes!`), and a picture reaches a client as part of the server's package, so a reload is "the integrated server reads again, builds a new package and sends it" (the `/reload` of the architecture review). That is the big piece, and it is semantic (block data can change).
- **C2, a client-side picture override** (**recommended**): the renderer gets `set_picture(word, pixels)`, which writes one layer and its mip chain of the existing array (a 16 by 16 picture has 5 levels). It is cosmetic: not in the package, not in the world, a replay is unaffected, the server never knows. VA pushes its pixels over the bridge as it draws, so unsaved strokes show live; Ctrl+S writes the PNG as it does today. The same override, read from a client folder, **is** the D9 texture pack, so a player gets a feature out of it, not only a developer. It must cover the companions (`_s` specular, `_n` normal with its height in alpha), or relief cannot be painted.
- The brush guide in A1 can be a temporary override too (the brush's pixels drawn at half strength over the tile), so the shader needs nothing for it.
- **Order for "pack reloading" (three kinds, three sizes):** R1 pictures, client only, via C2. R2 shader pack files: watch the pack's folder (poll modified times at about 4 per second, which needs no dependency; or the `notify` crate, which would need a licence check) and call the existing `set_shader_pack_with_settings`. R3 block data and the server's package: a real `/reload` where blocks keep their numbers by name; the big one, last.

### D. Who may run it, and the cheating question

What I think is true (**confidence: high** on the first two):

1. **A signature or a whitelist does not stop a modified client.** The source is open, so anyone can recompile a client. Only the server's own rules (it is authoritative, and rate-limits input) stop cheating. Signing answers a different question: whether a player should trust a mod's author.
2. **A client that regenerates terrain from the seed already knows hidden terrain.** An x-ray cannot be prevented by any rule about mods here, so the tool adds no new loss. What would be new is a mod that **sends** things (input, commands) as a player.
3. So the useful lines are: **not shipped to players, not able to send, not reachable by others.**

Layers, each cheap:

- **L0: a cargo feature (`paint-bridge`), off in a release build.** For a player the code does not exist: no loader, nothing to attack. This is "leave it uninstalled" with no signing at all.
- **L1: off at runtime unless asked** (`--paint-bridge`), and it prints a random token that VA must send.
- **L2: reachable only from this machine:** a named pipe or a Unix socket, or loopback TCP with the token. A web page can reach loopback TCP (DNS rebinding), so the token is not optional there.
- **L3: capability.** The bridge may read the pick map and set pictures; it is **not given** `send_command` or `send_message`. A tool that cannot send cannot automate play.
- **L4: the server's say (the owner's third idea, cheap).** Singleplayer: allowed. On a joined server: only if the welcome carries a flag (`allow_client_tools`), off by default. Replays: allowed (there is no server).
- Signing waits for **loadable** client code (the WASM stage). A compiled-in tool needs none.

## What it adds to Cave

Three pieces, each useful on its own, and a small crate; nothing else of VA's work (brushes, undo, colours, wrap-assist) moves into the game.

1. `renderer`: the pick output, a compiled-in variant. Proof: the player's pictures stay byte-identical (the test used for the relief refactor).
2. `renderer`: the picture override (C2), which is also the texture pack.
3. A bridge crate behind the feature (a client plugin that gets the pointer, the pick map and the names table, and calls the override).
4. In VA, in Python: a socket client, the screen-to-tile mapping and the snapshot view.

A bonus **[believed]**: every voxel of dirt shares one tile, so a stroke at a block's edge shows on the neighbours at once. That is VA's wrap-assist, in the world, for nothing.

## A proposed order (tests first, safest piece first)

1. **Picture override**, with tests that one layer and its mips change and no other, on the four adapters. Then a loose-pictures folder for it (a texture pack, R1).
2. **Pick output**, a variant; tests that it names the right word and pixel on the plateau scene in each of the three reliefs, and that the player's variant is unchanged to the byte.
3. **Bridge**, behind the feature, with a fake VA as the test; L1 to L4.
4. **VA side**, the snapshot view (A5) and drawing through it.
5. **Shader-pack reload** (R2).
6. Later, only if wanted: streaming (A3 or A4), and the server `/reload` (R3).

## Questions for the owner

1. One window for the game and one for VA (A1), or VA owning the window (A3/A4) from the start?
2. Edits live in the client only (C2), so on a joined server only the painter sees them; a change becomes everyone's only when its PNG reaches the pack. Is that right?
3. Do you agree with the layers, in particular that a joined server must opt in (L4), and no signing for now?
4. Should the first version paint the companions (`_n` height, `_s`) too, or only the colour picture?
5. Is R2 (watching shader pack files) wanted before R1, since it is smaller?
