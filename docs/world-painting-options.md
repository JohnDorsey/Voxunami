# Painting on a block in the game: options for VirtualAtlas and the game

**A proposal; nothing here is built.** Written 2026-11-15 on the branch `graphics-world-painting`. The owner asked to plan and explore, not to build. Claims are marked: **[read]** I read it in the code or a document, **[believed]** my reasoning, not run, **[not checked]** a question I did not look into.

## The goal, in the owner's terms

Paint on a block's face in the running game, with `plugins/base_blocks/textures/VirtualAtlas.py` (VA) as the painting tool, while the picture used for editing is the game's own rendering, with any pack or shader pack, as the player sees it. Cave must grow as little as possible. The tool is for developers: a player leaves it uninstalled. (The owner first asked that it open no way to cheat and then, 2026-11-15, dropped that; section D.)

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

**What a frame through the CPU costs [measured, 2026-11-15, throwaway programs not kept].**

- *GPU to CPU, in the game's process:* reading a 1280 by 720 texture back (a copy to a buffer, mapping it, then copying the bytes out) takes **0.64 to 0.73 ms for the colour frame (3.7 MB)** and **1.13 to 1.22 ms for an 8-byte-a-pixel pick map (7.4 MB)**, on the RTX 3090 and the AMD integrated GPU, each on Vulkan and DX12. Mapping alone is 0.2 to 0.35 ms. Caveat: the texture was never drawn into, so this is the transfer and not waiting for a frame to finish; the frame's own render time is on top of it (which `--benchmark` already reports).
- *Shared memory to pygame, in the Python process* (Python 3.11, pygame 2.6.1, 1280 by 720, the dummy video driver, so the window's own present is not in it): copying the colour frame out of shared memory 0.46 ms; the pick map 0.92 ms; a lookup of one pixel of the pick map is free; a sequence-number check around a copy costs nothing measurable. **Blitting is where a trap is: an RGBA surface costs 8.0 ms because pygame blends the alpha; the same bytes as an `RGBX` surface (alpha ignored) cost 0.27 ms.** So the road from the game through shared memory to the screen is about 2 to 3 ms a frame in all, well inside 16 ms, with no GPU sharing at all.

Opinion (**medium confidence**): A1 plus A5's snapshot first. The game's window already shows the true render, so A3 buys one window at a real cost. Choose A3 or A4 later only if the owner wants VA's tools over the 3D view.

### B. How does a pointer become a tile pixel?

- **B1, the CPU does it** (the owner's "shape of the mesh and the name of the texture"): use `target()`, work out the face and the point on it, and turn that into a tile pixel. It is exact only for a flat face with no relief. It must repeat the shader's choices a second time: the block's orientation (24), slabs and stairs taking a slice (D10), ships' own coordinates, and the shift of the prisms or of parallax occlusion. Two implementations of "which pixel is this" will drift apart, and then the pixel painted is not the pixel seen. **Not recommended.**
- **B2, the GPU says it** (**recommended**): the fragment shader writes a second output, the picture word and the pixel position in the tile **after** the relief shift, to a second render target. Only in a variant compiled in for the tool (the pattern of `pictures_on`/`pictures_off`), so a player's shader is byte-identical. The answer is the shader's own, so relief, orientation, slabs and ships are right by construction. Every screen pixel has an answer, so VA can draw a stroke in screen space and map each step to a tile pixel; a stroke crossing a block edge continues into the next block's tile pixels. Cost: 8 bytes a pixel of extra write, in the dev variant only (**not measured**).
- **B3**, the same shader rendered again for a 2 by 2 scissor around the cursor (a scissor, not a smaller viewport, so derivatives are right): less bandwidth but a second draw path, and VA could not draw a stroke in screen space. B2 is simpler.
- **Fallback with no GPU change:** the tool forces `relief flat` (a setting that exists) while painting, so a flat face is exact and B1 is enough. It gives up "with the relief on", so only a stopgap.
- **Does B2 work with any shader pack, even one built for nothing like this? Yes, with the interface as it is [read, `shader-pack-format.md`].** A pack defines one function, `pack_shade(Surface) -> colour`, and may bind nothing, declare no variable outside a function, and call no texture function. The engine's own shader owns the vertex stage, the geometry, the picture lookup and the relief, and it would write the pick output itself after `pack_shade` returns. So however far a pack's look is from the default's, it decides a colour and nothing it does can reach the pick output. (A pack that threw a pixel away would take its pick with it, which is right: you pick what is drawn. **[Not checked:]** whether `discard` is allowed in a pack; the page does not list it.) The sharing of one fragment shader for colour and pick is the point: the pick is the shader's own answer, not a copy of its logic.
- **Where B2 would stop working, and the rule that prevents it.** If a later pack interface lets a pack run its own passes, move vertices or warp the finished picture (the Aperture-style interface in `shader-systems-survey.md`), a pack could change where a pixel is without the pick output knowing. The rule to write into the shader pack spec now: **the pick target belongs to the engine's geometry pass and never to a pack; a pack interface that moves pixels must move the pick map by the same remap.** A shader pack written for another game that draws everything its own way is outside this interface today, and B1 (the CPU, below) is the only method that does not depend on the shader at all.
- Limits **[read]**: pictures are drawn only within the texture distance (64 voxels by default) and levels of detail above 0 carry none; beyond that the answer is "no picture".

### C. How does a change reach the game?

- **C1, files and a watcher:** VA saves the PNG, the game notices and reloads. Today the base pictures are inside the program (`include_bytes!`), and a picture reaches a client as part of the server's package, so a reload is "the integrated server reads again, builds a new package and sends it" (the `/reload` of the architecture review). That is the big piece, and it is semantic (block data can change).
- **C2, a client-side picture override** (**recommended**): the renderer gets `set_picture(word, pixels)`, which writes one layer and its mip chain of the existing array (a 16 by 16 picture has 5 levels). It is cosmetic: not in the package, not in the world, a replay is unaffected, the server never knows. VA pushes its pixels over the bridge as it draws, so unsaved strokes show live; Ctrl+S writes the PNG as it does today. The same override, read from a client folder, **is** the D9 texture pack, so a player gets a feature out of it, not only a developer. It must cover the companions (`_s` specular, `_n` normal with its height in alpha), or relief cannot be painted.
- The brush guide in A1 can be a temporary override too (the brush's pixels drawn at half strength over the tile), so the shader needs nothing for it.
- **Pack reloading** is its own plan, in `hot-reload-plan.md` (every kind of reload and the order to build them in). The picture override is its step 3, so this tool's first piece is built there, on the way.

### D. Who may run it

**Decided by the owner, 2026-11-15: mod cheating safety is not a concern for this tool.** The layers an earlier draft of this page listed (a cargo feature, a token, loopback only, no sending, a server's opt-in) are dropped, not built. One consequence stays, for engineering and not for safety: the tool's code is a client plugin and a dev variant of the shader, so it is easy to leave out of a build, and the pick output costs nothing in the variant a player's game uses.

### E. The CPU's side: a surface hit, and what a tractor beam needs

The owner's question: is the CPU ray-and-mesh code what a later tractor beam would need? **Mostly yes, but it is a different thing from the painting's pick, and the two should not be mixed.**

- **Today's ray finds a voxel, not a surface [read, `world_view/src/ray.rs`, `session.rs` `target()`].** `cast_ray` steps through voxels and stops at the first one `is_solid` calls solid; `RayHit` has the grid, the voxel, the voxel before it and how far along the ray it enters the voxel's cell. For a full cube that entry is the point on the face. For a slab, a stair or any partial shape (boxes in eighths of a voxel) the ray enters a cell it may not touch, and the point is wrong. There is no code that intersects a ray with a block's boxes (`block_models::Shape::boxes()` is there to do it with).
- **What a tractor beam wants is a point on the surface of the simulation's geometry, on the server, deterministically.** The hard rule (terrain and gameplay bit-identical on Windows and Linux, nothing from a GPU, the server draws nothing) means it cannot use the pick; it needs a Rust function: ray against a voxel's boxes, giving the grid, voxel, face, the point (in the voxel's own coordinates, so that it stays exact far from the origin) and the normal. A ship's grid is the same call in its own coordinates. For a beam that pushes a rigid body at a point, Rapier's own ray query against the bodies' colliders is another way to get a point and normal on a ship **[believed]**; nothing in the code uses it today. I did not check which fits better; it is the first question when the beam is designed.
- **The painter's pick answers another question: which picture pixel is drawn here.** With relief on, the drawn surface is an illusion above the true one, so the two differ by design: the beam should hit the true surface, the brush should paint the pixel the eye sees. One is exact for physics, the other for sight.
- **They can still check each other.** With `relief flat`, the CPU surface hit's picture pixel (face, point, orientation) must equal the pick map's pixel for every ray in a test. That locks the shader's mapping and the CPU's mapping together, and it is the CPU method that stays correct if a pack ever comes from outside the interface (the case where B2 stops working above).
- **A proposal:** do not build the CPU hit for the painter. Build it when the first game feature needs it (the beam), with the return type above, and add the cross-check test then.

## What it adds to Cave

Three pieces, each useful on its own, and a small crate; nothing else of VA's work (brushes, undo, colours, wrap-assist) moves into the game.

1. `renderer`: the pick output, a compiled-in variant. Proof: the player's pictures stay byte-identical (the test used for the relief refactor).
2. `renderer`: the picture override (C2), which is also the texture pack.
3. A bridge (a client plugin that gets the pointer, the pick map and the names table, and calls the override).
4. In VA, in Python: a socket client, the screen-to-tile mapping and the snapshot view.

A bonus **[believed]**: every voxel of dirt shares one tile, so a stroke at a block's edge shows on the neighbours at once. That is VA's wrap-assist, in the world, for nothing.

## A proposed order (tests first, safest piece first)

The owner does not need this soon (2026-11-15). The painting tool's own steps are these, and **its first piece is step 3 of `hot-reload-plan.md`** (the picture override), which is built on the way:

1. **Picture override** (hot-reload step 3).
2. **Pick output**, a variant; tests that it names the right word and pixel on the plateau scene in each of the three reliefs, and that the player's variant is unchanged to the byte.
3. **Bridge**, a client plugin, with a fake VA as the test.
4. **VA side**, the snapshot view (A5) and drawing through it.
5. Later, only if wanted: streaming (A3 or A4), and the CPU surface hit (E) when a game feature needs it.

## Questions for the owner

1. One window for the game and one for VA (A1), or VA owning the window (A3/A4) from the start?
2. Edits live in the client only (C2), so on a joined server only the painter sees them; a change becomes everyone's only when its PNG reaches the pack. Is that right?
3. Should the first version paint the companions (`_n` height, `_s`) too, or only the colour picture?
