# Shader packs: swapping how a face is lit

**Status (2026-11-10): both steps are built: step 1, the interface in the renderer, and step 2, the player's side (folders, `--shader-pack`, `.shader`, `check-shader-pack`, two example packs). The guide for writing a pack is `docs/reference/shader-pack-format.md`.** The owner's wish (`future-goals.md`, Shaders): players can swap the shader, as with Iris for Minecraft, without post-processing passes for now, and the default shader is robust and free of the gap at the base of a shadow. `contact-trace-spec.md` section 8 gave the principle: **the engine owns what is lit, a pack owns how it looks.**

## 1. The seam

The fragment shader of a face (`fragment_main` in `voxel.wgsl`) used to work out, in one body, which of the sun's light reaches the place (the horizon of the planet, the balls that hide the sun, the shadow map and the exact walk of the ray through the voxels), how much of the sky's light, how much of the sky is over the place, the lamps and the side of the face, and then to combine them into a colour. It is now two parts:

- **The engine's:** the picture's colour and holes, the visibilities above, the fog, the sky, the stars, the debug views. Everything that needs the map, the voxels, the pictures or a binding.
- **The pack's:** one function, `pack_shade`, from a `Surface` (the engine's answers) to a linear colour. The engine takes the fog over it.

```wgsl
struct Surface {
    albedo: vec3<f32>,      // the face's colour at this pixel, linear: its picture where it has one
    normal: vec3<f32>,      // in the world's axes; far over a planet it is turned toward the way out of the planet, as the sun's cosine is of
    relative: vec3<f32>,    // where the point is, relative to the camera, in voxels
    distance: f32,          // how far from the camera (interpolated between the vertices of a quad: use `relative` for an exact one)
    lamp: f32,              // the light of the lamps and the ambient light, 0 to 1
    side: f32,              // how bright the side of the face makes it, 0.4 to 1
    above_horizon: f32,     // the share of the sun above the horizon of the planet / not hidden by a ball, 0 to 1
    sun_visible: f32,       // the share of the sun's light that reaches the place: the above, and the terrain's shadow (the map and the exact walk) on top, 0 to 1
    sky_visible: f32,       // the share of the sky's light that reaches the place past the terrain's shadow, 0 to 1
    uncovered: f32,         // the share of the sky's light that reaches the place past what is over it, 0 to 1
    emission: f32,          // how much of the pixel shines by itself, 0 to 1 (the alpha of the picture's specular companion; 0 where it has none)
    smoothness: f32,        // how smooth the pixel is, 0 to 1 (the red byte of the picture's specular companion; 0 where it has none)
    reflectance: f32,       // how much light it throws back straight on, 0 to 1 (the green byte)
    metal: f32,             // 1 where the green byte is 230 or more (it throws back its own colour), else 0
    shading_normal: vec3<f32>, // `normal` tilted by the picture's normal companion (a unit vector; the same as `normal` where there is none): for the sun's light alone
    ambient_occlusion: f32,    // the share of the ambient light (lamps, ambient, sky) that reaches the pixel past the normal picture's blue (0 to 1; 1 where there is none)
}
fn pack_shade(s: Surface) -> vec3<f32>
```

A pack file may also define constants and functions of its own, and read the frame's uniform (`frame.sun_direction`, `frame.sun_light`, `frame.heavens`, `frame.fog_range`, ...: `Frame` in `voxel.wgsl`), which is how it knows the strengths the atmosphere sets. **The default pack is `lighting_default.wgsl`**: the equation that was in `fragment_main`, moved and not changed.

**Why it is this seam and not more.** The engine's answers are the hard part (the strip, the caves' leaks, the planets' twilight, the cost), and a pack that could ask the map itself could bring the strip back with a bias of its own, which is what packs of other games do each for themselves. A pack given the answers can ignore them (a pack with no shadows is a style) and cannot get them wrong. What it cannot do in this step is own the sky, the fog or a pass of its own: those are hooks for later (section 6).

## 2. What a pack may not do (checked by `ShaderPack::new`)

- **No loop** (`loop`, `while`, `for`): a loop that does not end hangs the graphics card, and the engine's own queries are bounded. A pack writes its steps out.
- **No binding** (`@group`, `@binding`) and **no variable outside a function** (`var<...>`): a pack has no resources of its own; it is given a `Surface` and the frame's uniform.
- **None of the engine's names** for what it keeps to itself: the shadow maps and their sampler, the palette, the placements, the voxels' tables and pools, the pictures and their sampler; and **no texture function** (any word that begins `texture`). (A comment may say them.) A name of the pack's own that is one of these is refused too: rename it.
- **A pack must define `fn pack_shade(s: Surface) -> vec3<f32>`**, and its text, put after the engine's, must be accepted by the shader language (naga: parse and validate) for the plainest kind of the engine's shader and for the heaviest, with the problems reported at **the line and column of the pack's own file** (the engine's text comes first, so the lines of the whole are the pack's plus a constant). A pack that redefines a name the engine has, or whose `pack_shade` has another signature, is told so.
- The lexical problems are all reported at once; the shader language says only the first thing it finds in a text that passes them.

What the checks cannot know is what the graphics card says: `VoxelRenderer::set_shader_pack` makes the pack's shaders at once (the plain one, the one with pictures if pictures are in use, the heaviest with the walk if the renderer has one) in an error scope and **returns the card's refusal as an error, leaving the renderer drawing as it did.** (Not tested: no pack that passes the checks and fails the card has been found to try it with.)

## 3. How it is built

- `voxel_source(with_pictures, walk, lighting)` assembles the text of a voxel shader: `voxel.wgsl`, the pictures, the walk's parts, then **the lighting last**. `voxel_shader` makes the module of it. The kinds (16 with the walk) are made lazily as before, now from the text the renderer holds (`VoxelRenderer::lighting`, `ContactGpu::lighting`); changing the pack forgets the walk's shaders and makes the plain ones again.
- `ShaderPack { id, name, lighting }`, `ShaderPackError { problems }`, `ShaderPackProblem { line, column, message }`, `SHADER_PACK_INTERFACE` (1: a pack's `pack.json` names the interface it was written for and a version of the engine that has changed it refuses the pack), `default_lighting()` (the default's text, to start a pack from).
- The shadow maps and the sky are drawn by the engine's own shader as before: a pack does not change them.
- An avatar, an entity's box, a ship and a coarse node are drawn by the same fragment shader, so a pack lights them too.

## 4. Verified (2026-11-10)

- **The default is what it was, pixel for pixel:** 5 views of real Cave (noon, 09:00, 18:30, midnight, 06:45; pitches and yaws differing), 640 by 360, on the RTX 3090 and the integrated AMD GPU, with the executable of the commit before and the one after: **0 differing pixels in all 10**. (The same executable twice also gives 0: the pictures are repeatable.) And all the existing GPU tests pass.
- **The cost is unchanged:** real Cave on the integrated GPU, 1280 by 720, three rounds each, alternating: 18.68, 18.60, 18.69 ms before; 18.62, 18.65, 18.74 after (the machine's drift, which is 1.5 ms higher than the 17.1 of the day before, is in both).
- **Tests** (`shader_pack/tests.rs`, `gpu_rendering/shader_packs.rs`; the GPU ones on all four adapters): the checks (valid packs, a missing `pack_shade`, errors at the right line of the pack, a wrong signature, a name the engine has, a name only the heavier kinds have, loops, bindings, the engine's names and a comment that says them, all the problems at once, comments blanked in place); what a pack is given (the visibilities as the engine works them out: none of the sun in the block's shadow and the sky's light as the atmosphere says, all of both in the light; the horizon with the ground flat and the sun under it; the sky hidden under a roof; the lamp, the side and the place; the colour and the normal), that a pack colours every kind of shader (plain, pictures, the heaviest walk with a second grid's window and an avatar) and leaves the sky and the fog to the engine, and that giving a pack and taking it away leaves the pictures as they were and a pack of the default's text draws as the default.
- **Mutation experiments:** 26 mutants of the interface, the renderer and the checks, with the default's equation: 23 killed at the first run; 3 survived, all closed by stronger tests: the normal (`surface.normal`: the default pack's old tests kill it, my pack tests did not), a pack not given to the shaders made before it came (the test drew once with the walk first), and a missing `pack_shade` (said by the shader language's error and not by the check's own words: the test now asks for the words).

## 4b. Step 2: the player's side (built, 2026-11-10)

- **A pack is a folder** with `pack.json` (`id` namespaced, `name`, optional `interface` (1), `format` (1), `description`; any other key is an error) and `lighting.wgsl` (`game_shell::shader_packs::read_pack`). **Every problem of a folder is reported at once**, each naming the folder and file (and, for the shader, the line and column of the pack's own file). `read_directory` keeps the good packs of a folder whatever the others are, sorted by id, and a second pack with an id is a problem and is left out; files in the folder are ignored.
- **Where they are:** the `shaderpacks` folder of the game data folder (beside the identity), if it is there, and the folder `--shader-packs DIRECTORY` names (which must be). `--shader-pack NAME` starts with one; **`.shader [NAME|default]`** says which is in use and what there is, chooses one, or goes back. A name is the id, its part after the colon (what a player types), the pack's name in any case, or the beginning of any of these if only one pack begins so (`pick`).
- **A pack the graphics card refuses** (`set_shader_pack` returns the error) is taken off the settings by the window, which says so on the status line and the error stream; a screenshot run fails with the message.
- **`voxunami check-shader-pack DIR`** (the launcher; `game_shell::run_check_shader_packs`) takes a pack's folder or a folder of packs, says `ok` for each good one and lists every problem of the rest.
- **`ClientSettings` is not `Copy` any more** (it holds the names of the packs and the choice); nothing copied it.
- **Two example packs** (`engine/renderer/example_shader_packs`: `cel`, the sun's light in three steps, and `shadowless`, which ignores what the engine worked out of the terrain's shadow) are read by a test of the shell, and drawn by a test of the renderer on all four adapters (the cel pack shades the block's shadow still, the shadowless one makes the shadow as bright as the light).
- **Real Cave, 640 by 360, 09:00, on both adapters** with the packs given by `--shader-packs`/`--shader-pack`: the cel pack differs from the default in 90 to 91 thousand of 230,400 pixels (the hills' light is in bands), the shadowless one in 5,291 to 5,292 (the shadows are gone); without `--shader-pack` the picture is the default's.
- **Tests** (`shader_packs/tests.rs`, `client_commands.rs`, `options.rs`): the pack folder (good, every kind of problem at once, the lighting's line), the folder of packs (order, a problem, a duplicate, files ignored, a missing folder), the player's shelf (data folder, asked-for folder, first kept, a missing default folder is none and a missing asked-for one an error), `pick`, `.shader` (says, chooses by name, local name, default, unknown, two), the options, the check command, the examples. **Mutation experiments: 16 mutants of the reader, the shelf, `pick`, the command and the options: 13 killed at the first run; 2 survived and were closed by tests (a beginning of a name that is not a beginning of an id; the id in another case, which `pick` now accepts); 1 survives and is equivalent here (the folders are not sorted before they are read: Windows lists a folder by name, so only which of two packs with one id is kept would show the difference, on a system that lists in another order).**
- **Not unit-tested:** `Session::offer_shader_packs`/`reject_shader_pack` and `apply_shader_pack` (the pictures above show them working; a test of them needs a session and a graphics card in the shell's tests), and the window's way of telling the player of a refused pack.

## 5. Decisions of mine, for the owner

1. **Packs are the client's, like texture packs; nothing travels.** A recording shows what the recorder saw (the hard rule: the client's view comes from the stream; a pack is how it is drawn), and a server is never told which pack a player has.
2. **One function, the colour of a face**, to start. Smaller than Iris's, which is passes; the sky, the fog and a pass are hooks to add by need.
3. **No loops, no resources.** Safe by construction; limits what a pack can look like (no blur, no sampling). The way out for a pack that needs them is a bounded query the engine offers, not a binding.
4. **The engine owns the fog** so that a pack cannot forget it; a hook for it is easy to add.
5. **The interface is numbered** (`SHADER_PACK_INTERFACE`), a pack names it, and a change of the fields of `Surface` is a new number, not a quiet break. (*Read so, 2026-11-12, to be confirmed by the owner:* the number is for a change that can break a pack, a field taken away or its meaning changed; `emission` and `shading_normal` were **added** without one, since a pack only reads `Surface`; a pack that does not look at `shading_normal` has no bumps.)
6. **Words, not the language, are checked for the engine's names** (a local variable called `pool` is refused): simple and safe; the author renames.

## 6. Not built

- Hooks for **the sky** (`sky_fragment`), **the fog**, **waving vegetation** (a vertex hook: it moves vertices, which the shadow maps must follow), **a time** for packs that move (it must come from the game's tick and a seed, not the wall clock), **bounded queries** (`trace(origin, direction, longest)`, the exact sky cover), **settings of a pack** the player can turn.
- **Post-processing** (the owner left it out).
- A pack that passes the language's checks and fails the graphics card, to see the error come out of `set_shader_pack`.
- What OptiFine, Iris and other systems offer, which of it this interface lacks and in what order to add it (a manifest that declares settings and passes, loops with constant bounds, materials in block data) is in `shader-systems-survey.md` (history, 2026-11-10), with the questions it leaves to the owner; the no-loops rule of decision 3 above is the first of them.
- **The owner's answers of 2026-11-11** (in `future-goals.md`): the game does not read comments, so a pack's settings are declared in its setup and not scraped from the shader's text; pipelines are set up in a programming language that is not yet chosen (this spec's `pack.json` is the first version of that setup, for one function); menus are nested screens, later with vertical tabs; cascades are to be well supported (they are the engine's, section 1); LabPBR and a one-file JPEG XL format of our own come later; a switch to Aperture shaders is a distant wish.
