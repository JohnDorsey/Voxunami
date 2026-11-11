# Shader packs in the block-game genre: how they work, and what to take from them

*A survey, written 2026-11-10 for the owner, who asked how shader packs work in Minecraft (OptiFine and Iris) and in other block games, because many future players will come from those communities, and who put power, flexibility and extensibility ahead of familiarity. It is research, not a decision: section 6 is my opinion and section 8 has the questions only the owner can answer. It describes other people's software as of its date, so it is filed under history.*

**How far to trust it.** Five research agents read primary sources (documentation, source repositories, Mojang's release notes, Modrinth's API); another re-read the pages I lean on most; a seventh went deep on Aperture (2.16). Unmarked statements were read by an agent on the cited page. **[re-read]** means a second agent read the page again to check the claim. **[inferred]** is our reading, not something the source says. **[unverified]** was not read from a page (memory, search snippets, a page that timed out). **[run]** is something I ran on our own renderer. *Not read at all:* the internals of BSL, Chocapic13, Kappa, Sildur's and SEUS PTGI (no public source); Aperture's own source and Java API documentation (not public: it is described from its example pack and from beta packs); Discord, where much of that community talks; Valve's and ZDoom's wikis. Mojang's announcements were read by two agents; my own re-read of them timed out twice, and so did the Aperture agent's (it used a news report).

## Contents

1. The short version
2. How a Minecraft shader pack works (OptiFine, Iris)
3. Other block and voxel games
4. Other engines and tools: the seams they chose
5. Our interface against theirs
6. What I would do
7. Coarse terrain shadows
8. Questions for the owner
9. Sources, and a table of their words and ours

---

## 1. The short version

1. **A Minecraft shader pack replaces the game's lighting, not its geometry.** The game decides what is drawn and in what order (vanilla's order). The pack supplies a GLSL program for each *category* of draw (terrain, entities, water, sky, particles, the held item), named by file (`gbuffers_terrain.fsh`). The programs write into several render targets (`colortex0` to `colortex15`), then the pack's fullscreen passes (`deferred`, `composite`, `final`) do the lighting, effects and tone mapping. Before all of it a `shadow` program draws the world from the sun.
2. **There is no manifest.** How many buffers exist, their formats, whether they are cleared, and the pack's options with their value lists are all *scraped from the shader source*: `const` lines, sampler declarations, and `#define` lines with a trailing comment. It makes a pack easy to begin (an empty folder is a valid pack: everything falls back) and gives ambiguous options, drifting documentation and no typed validation.
3. **The power comes from four things:** the multi-target deferred pipeline with ping-pong buffers; the pack's control of the shadow pass (its own projection, so it can warp the map toward the player); block ids mapped in `block.properties` so a shader can tell foliage, water and lamps apart; and, in Iris, compute shaders with storage buffers and 3D images, with which packs *build their own voxel model of the world* and run flood-fill lighting and ray tracing.
4. **What it cannot do:** read block data directly (blocks are tagged by lists of names only), add scene passes, rely on a stable API, or protect itself from drivers (NVIDIA accepts what AMD refuses). A pack author also rebuilds what vanilla gave for free (alpha test, ambient occlusion from vertex colour).
5. **Iris is the living standard today.** It is a mod on top of Sodium that runs OptiFine-format packs unmodified, by rewriting their old-style GLSL, and adds feature flags, compute, storage buffers, custom images, and programs for Distant Horizons (the LOD mod). OptiFine's own specification is an unlicensed text file in a docs-only repository that the Iris team calls wrong in places. Its own account has said that Iris will be discontinued in favour of Aperture (item 6).
6. **The ground is shifting under both.** Mojang announced (2026-02-18) that Java Edition is moving from OpenGL to Vulkan; Vulkan is the default in the 26.4 snapshots, and Mojang says replacing its shaders from a resource pack is unsupported. Iris does not support Vulkan, and its developer is building a successor, **Aperture**: Vulkan, Slang shaders, a pipeline that the pack *configures in typed code* (TypeScript in its January 2026 example, Java in every beta pack since June), built-in cascaded shadows, optional hardware ray queries; a closed beta since May 2026, with no support for old packs and no public source or Java documentation. The OptiFine format is becoming a legacy format with a replacement taking shape, which makes this a good time to learn from both (2.16).
7. **Other block games mostly do not let players write shader code.** Bedrock replaced its dead shader packs with *data* (lighting and atmosphere as JSON: Vibrant Visuals); Luanti has engine-owned shaders and Lua lighting parameters; Veloren turns modes into `#define`s; Hytale supports no client mods; Teardown and Roblox offer property bags. Vintage Story, Terasology and Canvas do let packs write shaders, and show the costs (file-level overrides collide; a regex-patching library had to be invented).
8. **General engines converge on the seam we built.** A user callback fed by engine-computed light and shadow (Unity's `Lighting<Name>`, Godot's `light()` with `ATTENUATION`, GZDoom's `ProcessLight`), every hook optional, plus parameters declared next to the code with ranges (ReShade, RetroArch, Godot) and passes declared as data (RetroArch, Canvas) or as typed code (Aperture).
9. **Our first interface is on solid ground and narrow.** `pack_shade(Surface)` is the Godot `light()` idea. It has none of what makes Minecraft packs powerful: passes and targets, materials, history, settings, queries. And **our ban on loops is stricter than any system I found**; it would rule out filtered shadows, ambient occlusion and ray marching, which is what packs mostly are.
10. **Distant shadows are a weak spot of that whole world.** One orthographic map warped toward the player; beyond it, fakes (skylight lightmap, screen-space ray marching); the LOD shadow pass of Distant Horizons exists and the biggest packs switch it off; no big pack uses cascades. Our eleven cascades to 59 million voxels go further than they do, **but in Cave they did not work at a distance: the planet's ground was left out of every map coarser than a voxel (found and fixed 2026-11-11, section 7; my first reading in this report said the opposite and was wrong)**.

---

## 2. How a Minecraft shader pack works

### 2.1 The idea

Vanilla Minecraft lights a face with a lookup in a tiny *lightmap* (block light and sky light, 0 to 15 each) and a fixed brightness for each face direction, using its own shaders. A shader pack throws those shaders away and brings its own. The format was invented in 2012 as a mod (daxnitro's GLSL Shaders Mod, then karyonix's Shaders Mod, then merged into OptiFine by its author for 1.8.x), when Minecraft used OpenGL's fixed-function pipeline. That is why packs still read built-ins like `gl_Vertex`, `gl_Color` and `gl_MultiTexCoord1`, call `ftransform()`, and write `#version 330 compatibility`, and why Iris needs a translator to run them on a modern renderer (2.12).

The choice that shapes everything: **the game owns the frame; the pack supplies programs for named stages.** The game has a fixed list of stages in a fixed order. For each category of geometry it draws its own meshes in vanilla's order, and runs the pack's program of that category's name, or a fallback. Between and after those it runs the pack's fullscreen passes. The pack never describes a pipeline; it names files.

A pack lights in one of two places. **Forward:** the terrain program computes the lit colour as it draws (Complementary Reimagined, MakeUp Ultra Fast). **Deferred:** the geometry programs write albedo, normal, light levels and material flags into buffers, and a later fullscreen pass lights every pixel once (Photon). Deferred is easier to extend with effects that need the whole picture, and is what the format's buffer machinery is built for.

### 2.2 On disk

```
.minecraft/shaderpacks/
  MyPack.zip                  (or an unzipped folder MyPack/)
    shaders/
      shaders.properties      options screens, profiles, buffer sizes, blending, uniforms, program switches
      block.properties        block names -> the integers the shader sees (item/entity/dimension .properties too)
      lang/en_us.lang         labels and tooltips for the options
      lib/*.glsl              shared code, pulled in with #include
      gbuffers_terrain.vsh    vertex stage;  .fsh fragment;  .gsh geometry;  .csh compute (Iris, OpenGL 4.3)
      gbuffers_water.fsh ...  one pair per category the pack cares about
      shadow.vsh / shadow.fsh
      composite.fsh, composite1.fsh ... final.fsh
      world-1/  world1/       optional folders that replace the files for the Nether and the End
      textures/noise.png      the pack's own textures
```

The player picks a pack in Options, Video Settings, Shaders. The values of the pack's options are saved beside it as `shaderpacks/<pack>.txt`; Iris keeps the chosen pack and a few switches in `config/iris.properties`. Reload is a key (F3+R in OptiFine; Iris has R, K to toggle shaders, O for the pack screen). A compile error shows as a chat message; the detail is in the log. Iris, with its debug mode on (Ctrl+D), shows an error screen and dumps the patched shader sources to a folder. OptiFine's `-Dshaders.debug.save=true` dumps its final sources.

### 2.3 The frame

```
 setup / begin          (Iris only: compute, once / each frame)
      |
 shadow                 the whole world seen from the sun -> shadowtex0/1, shadowcolor0/1
 shadowcomp 1..99       optional fullscreen or compute passes on the shadow buffers
      |
 prepare 1..99          fullscreen passes before any terrain is drawn
      |
 opaque gbuffers        sky, sun/moon/stars, terrain (solid, cutout), entities, block entities,
      |                 damage overlay, selection box, solid hand: vanilla's order, the pack's programs
 deferred 1..99         fullscreen: the lighting of everything opaque, in many packs
      |
 translucent gbuffers   water and stained glass, particles, clouds, rain/snow, world border, hand
      |
 composite 1..99        fullscreen: fog, reflections, volumetrics, TAA, bloom, depth of field, tone map
      |
 final                  fullscreen, straight to the screen
```

The order of the gbuffer stages is *vanilla's draw order*, so it can move when Mojang reorders rendering **[inferred]**: the OptiFine document lists the order as constants (`renderStage`) without notes per version.

| Program | What the game draws with it | If the pack has none |
|---|---|---|
| `shadow` | everything, seen from the sun (Iris adds `shadow_solid`, `_cutout`, `_water`, `_entities`, `_block`) | none |
| `shadowcomp`, `prepare` (and `1..99`) | fullscreen, after the shadow map / before the terrain | not run |
| `gbuffers_basic` | the leash, the selection box | none |
| `gbuffers_line` | block outline, fishing line | basic |
| `gbuffers_textured`, `gbuffers_textured_lit` | particles; lit particles, world border | basic; textured |
| `gbuffers_skybasic`, `gbuffers_skytextured` | sky, horizon, stars, void; sun and moon | basic; textured |
| `gbuffers_clouds` | clouds | none (the OptiFine document); Iris falls back to textured |
| `gbuffers_terrain` | terrain, solid and cutout (Iris splits `_solid` and `_cutout`) | textured_lit |
| `gbuffers_damagedblock`, `gbuffers_block` | the breaking overlay; block entities (chests, signs) | terrain |
| `gbuffers_entities`, `_entities_glowing`, `_armor_glint`, `_spidereyes` | mobs and players; glow; enchant glint; glowing eyes | textured_lit; entities; textured; textured |
| `gbuffers_hand`, `gbuffers_hand_water` | the held item, opaque and translucent | textured_lit; hand |
| `gbuffers_weather`, `gbuffers_beaconbeam` | rain and snow; the beacon beam | textured_lit; textured |
| `deferred` (`1..99`) | fullscreen, between the opaque and the translucent | not run |
| `gbuffers_water` | translucent terrain: water, stained glass | terrain |
| `composite` (`1..99`) | fullscreen post passes | not run |
| `final` | fullscreen, to the screen | not run |

This is the OptiFine document's table, **[re-read]** for the names and the fallback column; "not run" for the fullscreen rows is **[inferred]** from its "none". The menus and the GUI are drawn with no pack program at all. Iris adds `setup` and `begin`, terrain/entity/particle splits (`gbuffers_particles(_translucent)`, `gbuffers_entities_translucent`, `gbuffers_block_translucent`, `gbuffers_lightning`), tessellation shaders, and `dh_terrain`, `dh_water`, `dh_shadow` for Distant Horizons.

### 2.4 Render targets and textures

- **`colortex0` to `colortex15`** (the old names `gcolor`, `gdepth`, `gnormal`, `composite`, `gaux1` to `gaux4` are 0 to 7). At least 4 exist on every machine, up to 16 where supported (macOS: 8); a program writes at most 8 at once. Iris raised the count to 32 (1.10.5). **[re-read]** for 16 and 8.
- **Which ones a program writes** is a comment in the fragment shader: `/* RENDERTARGETS: 0,1,2 */`.
- **Allocation by declaration.** Declaring `uniform sampler2D colortex4;` anywhere makes five buffers exist. Formats are `const int colortex0Format = RGBA16F;` lines (default RGBA8). Buffers are cleared each frame unless `const bool colortex2Clear = false;`, which is how a pack keeps *history* for temporal anti-aliasing.
- **Ping-pong.** Each buffer is a pair, A and B. Fullscreen passes read one and write the other, then flip; the geometry programs read a few and write the main one. Reading and writing one buffer in the same pass gives artifacts the pack author has to avoid.
- **Depth and shadow textures.** `depthtex0` (everything), `depthtex1` (no translucents), `depthtex2` (also no hand); `shadowtex0` (all casters), `shadowtex1` (opaque only), `shadowcolor0/1`; `noisetex`.
- **Sizes.** `size.buffer.colortex6 = 0.5 0.5` makes a half-size buffer; `scale.<program>` shrinks a pass's viewport.

### 2.5 What the pack is given

| Group | Examples |
|---|---|
| Matrices | `gbufferModelView`, `gbufferProjection`, the previous frame's, `shadowModelView`, `shadowProjection`, all with inverses |
| Time | `worldTime`, `frameCounter`, `frameTimeCounter`, `sunAngle`, `moonPhase` |
| Camera, player | `cameraPosition`, `previousCameraPosition`, `eyeBrightnessSmooth`, `isEyeInWater`, `heldItemId` |
| Sun, sky, weather | `sunPosition`, `moonPosition`, `shadowLightPosition`, `skyColor`, `fogColor`, `rainStrength`, `wetness` |
| Per draw | `entityColor`, `entityId`, `blockEntityId`, `renderStage` |
| Vertex attributes | `mc_Entity` (x is the block's integer from `block.properties`), `mc_midTexCoord`, `at_tangent`, `at_midBlock`; the old `gl_*` names |
| Custom uniforms | `uniform.float.name = expression` in `shaders.properties`, evaluated on the CPU (`smooth`, `if`, `worldTime`, ...) |

Everything about the world itself, beyond what is drawn this frame, is missing. A pack that wants a block's emission or a voxel's occupancy has to rebuild it (2.11).

### 2.6 The property files

`shaders.properties` is a flat list of keys: toggles (`clouds`, `oldLighting`, `separateAo`, `shadow.culling`), the options UI (`screen`, `sliders`, `profile.NAME`), per-program blend and alpha test, buffer sizes and flips, custom textures, `program.<name>.enabled = <option expression>` (a disabled program counts as absent, so fallbacks apply), and `uniform.*` and `variable.*` expressions. `block.properties` gives each integer a list of names, with state filters. An illustration written from the documented syntax (not a real pack's file):

```properties
# shaders.properties
profile.low  = shadowMapResolution=1024 SHADOW_PCF !SSAO !BLOOM
profile.high = shadowMapResolution=2048 SHADOW_PCF SSAO BLOOM
screen = <profile> <empty> [SHADOWS] [POST] *
screen.SHADOWS = SHADOW_PCF shadowMapResolution shadowDistance
sliders = shadowDistance BLOOM_STRENGTH
program.composite2.enabled = SSAO
size.buffer.colortex6 = 0.5 0.5
blend.gbuffers_water = SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA
uniform.float.nightMix = smooth(2, if(worldTime > 13000 && worldTime < 23000, 1, 0), 5)

# block.properties
block.1 = short_grass tall_grass fern
block.2 = oak_leaves spruce_leaves birch_leaves
block.3 = oak_stairs:facing=east,west:half=bottom
layer.translucent = glass_pane
```

The integers are the pack's own: one pack's `10005` is foliage, another's is something else. Numeric vanilla ids ended in 1.13. Modded blocks must be named one by one, so the files grow (MakeUp's is 90 KB for 32 ids; Solas's 117 KB), and the community keeps shared lists and add-on packs (Euphoria Patches, an add-on for Complementary, has about 30 million downloads).

### 2.7 Options

A pack's options are `#define` lines in its shader files:

```glsl
#define SSAO                      // Screen-space ambient occlusion. High cost.
#define SHADOW_DARKNESS 0.10      // Shadow darkness [0.05 0.10 0.20]
```

`#define NAME` with no value is a switch (on; commented out it is off). A bracketed list after `//` gives the values to cycle through, and the rest of the comment is the tooltip. Left click steps forward, right click back, shift click resets. The game scans the files, builds the options screen, writes the chosen values into the source before compiling, and recompiles. `screen.*` groups them into pages, `sliders` marks which show as sliders, `profile.*` sets many at once (the current profile is detected from the values), and `lang/*.lang` translates labels. An option is only recognised if the same file uses it in an `#ifdef` or `#if`.

Real packs have a great many: Photon 382 labelled options on 59 screens, Complementary Reimagined 306 on 50, Rethinking Voxels 435 on 87, Solas 407 on 85 (tallies by the research agent from their repositories). Profiles run Potato to Ultra (Complementary has eight).

### 2.8 What real packs do with it

| | Photon | Complementary Reimagined | MakeUp Ultra Fast |
|---|---|---|---|
| Style | full deferred | forward-lit, thin G-buffer | forward-lit |
| What the geometry programs write | albedo, a material mask, an octahedral flat normal, block and sky light, a detailed normal, packed specular (`colortex1`, `colortex2`) | the already-lit colour (`colortex0`); smoothness, material mask and skylight factor (`colortex6`) | the lit colour; shadows are sampled in the fragment shader |
| Before translucents | sky map, clouds (with upscaling), ambient occlusion, then the shading | sky, aurora, stars, clouds, outlines | ambient occlusion, volumetric clouds |
| Composite passes | volumetric fog, translucent blend, depth of field, temporal upscaling and exposure, a bloom pyramid (many passes), grading and tone map, motion blur, FXAA (about 21 in all) | fog and reflections, volumetric light and refraction, bloom and lens flare, TAA, FXAA | volumetric light, bloom and blur, motion blur, TAA/FXAA |
| `final` | sharpening, sRGB, dither, debug views | on-screen error text | tone map, colour-blind filters, sharpening |

(From each pack's own files; Complementary's and MakeUp's composite roles are inferred from what each file includes.) Conventions that recur: one shared source file per program with tiny per-dimension stubs that `#define` the stage and `#include` it; per-frame values (TAA jitter, time-of-day factors) computed as custom uniforms in `shaders.properties`; a `DEBUG_VIEW` option that shows any buffer on screen (Photon); on-screen error messages from `final` (Complementary).

### 2.9 Shadows, the way packs do them

- **One orthographic map, warped.** The `shadow` program renders the world from the sun into a single map. The map covers `shadowDistance` blocks (defaults: Photon 128, Complementary 192, Solas 128 to 512 by quality) at a resolution of 1024 to 4096 (options up to 16384). To spend the texels near the player, packs **warp the map's coordinates** with a distortion function (`f = length(xy)*b + (1-b)`, divide by `f`, with `b` near 0.85; Photon uses a quartic norm and an explicit inverse). Iris's own tutorial calls this an old technique, says cascaded maps do better, and says the pipeline makes anything fancier hard **[re-read]**.
- **Bias and the gap at the base of a shadow.** Packs push the sampled position along the face's normal in world space, by an amount that grows with distance and with how edge-on the face is to the sun (Complementary: 0.12 + 0.0008 d^1.5), and sample foliage and low-skylight pixels from the block's centre to stop leaks in caves. MakeUp uses only a dithered tiny depth bias. This is the same trade the contact-trace spec measured: any bias leaves a strip.
- **Filtering.** The hardware comparison sampler plus rotated sample patterns that temporal anti-aliasing resolves; Photon does soft shadows with a three-tap blocker search and a Vogel-disc filter of 6 to 12 taps (PCSS).
- **Coloured shadows.** `shadowtex0` holds every caster, `shadowtex1` only the opaque ones; where they differ, the colour is read from `shadowcolor0` (tinted glass and water).
- **Leaves.** The shadow fragment shader discards low-alpha pixels.
- **Cascades.** None in the big packs read. A demo (Null-MC's CSM) packs four cascades into one texture and says it lacks CPU-side access for frustum splitting and culling; Alpha Piscium warps rectilinearly; Aperture has cascades as a built-in (2.16).
- **Beyond `shadowDistance`.** There is no map, so packs fake it. Complementary blends over the last 16 blocks into a term made from the skylight lightmap value; Photon fades into the same skylight term or into a **screen-space ray march** (ten steps, choosing vanilla or LOD depth by distance); Complementary samples a reprojected previous-frame screen-space shadow for Voxy's LODs.
- **Distant Horizons.** Iris has a `dh_shadow` pass that draws LODs into the same map, and Distant Horizons has *no shadow distance of its own* **[re-read]**. In practice the cull is a frustum test over each LOD column. Packs choose: Bliss offers an optional LOD shadow map (off by default); **Photon, Complementary and MakeUp all set `dhShadow.enabled = false`**. One map cannot be both close-up sharp and horizon-wide, and the biggest packs gave up the second.

### 2.10 Materials

- **Block ids** (2.6) are the only per-block data a shader gets. The `block.properties` of Photon has 96 ids, Complementary's 437 (its convention: 5xxx block entities, 1xxxx terrain, odd numbers non-solid for coloured light, 3xxxx translucent, 4xxxx items). Iris 1.7 added `%tag` names; Iris 1.7 also gave each vertex its block's light level in `at_midBlock.w`, so a pack can treat an unlisted lamp mod generically **[inferred]**.
- **labPBR 1.3**, the community's material standard, packs two textures. `_n`: red and green hold the normal's x and y (z is rebuilt), blue is ambient occlusion, alpha is height. `_s`: red is perceptual smoothness, green is F0 (0 to 229) or a hard-coded metal (230 to 237 are iron, gold, aluminium, chrome, copper, lead, platinum, silver; 255 means "use the albedo"), blue is porosity (0 to 64) or subsurface scattering (65 to 255), alpha is emission (0 to 254; 255 means none). The older "seusPBR" layout differs. A resource pack declares `format=lab-pbr/1.3`, and the shader sees `normals` and `specular` samplers.

### 2.11 Voxel-aware techniques

Because no block or voxel data reaches a shader, packs that want the world as a volume *rebuild it from what the shadow pass is sent*:

| Technique | Mechanism | Limits |
|---|---|---|
| Voxelise into a 2D texture (OptiFine era) | a geometry shader in the shadow pass writes block ids into the shadow buffer | cubes only; costs you the shadow map; no geometry shaders on Apple Silicon |
| 3D images and compute flood fill (Photon, Complementary) | the shadow program stores a block id into an 8- or 16-bit 3D image (64 to 1024 blocks across; last write wins); compute passes spread light through it | one value per voxel; partial blocks approximated |
| Storage-buffer triangle scene (Rethinking Voxels) | the shadow geometry shader appends triangles (up to 524,288) with per-cell lists; compute passes build lights and an irradiance cache | needs compute, storage buffers and custom images; about 32 MiB |
| An extension mod (Photonics) | voxel ray tracing as an Iris add-on used by several packs (alpha) | one loader |

The interface support that exists for this: `shadow.culling = reversed` with `voxelDistance` (keep voxelising outside the light's frustum), images and storage buffers (OpenGL 4.2 and 4.3: **none of it on macOS**), and `at_midBlock`. The Iris README lists "unlimited color buffers, direct voxel data access, and fancy debug HUDs" as what the project would like to add. Modrinth tags 287 of its 1,124 shader projects `colored-lighting` and 60 `path-tracing`.

### 2.12 Iris: how it works and what it adds

- **What it is.** A shader-pack loader that sits on Sodium (OptiFine and Sodium cannot run together). Loaders: Fabric, NeoForge, Quilt; the Forge port Oculus is unofficial and stopped at 1.20.1. Goal: existing packs run unmodified.
- **The translator.** A pack's GLSL is preprocessed (`#include`, option defines), then rewritten at the syntax-tree level by a library (glsl-transformer): `gl_Vertex` becomes a call that decodes Sodium's compact vertex, `gl_ModelViewMatrix` and `gl_Color` are renamed, `ftransform()` is injected. Both `core` and `compatibility` packs end up as core-profile code; the docs recommend `compatibility`. Error line numbers refer to the *patched* code.
- **An extended vertex format** carries `mc_Entity`, `mc_midTexCoord`, `at_tangent`, `at_midBlock` for terrain, so a pack's attributes work as in OptiFine.
- **Feature negotiation.** A pack declares `iris.features.required` and `iris.features.optional`; the optional ones define `IRIS_FEATURE_<NAME>`. Flags that must be declared to be used: `CUSTOM_IMAGES`, `HIGHER_SHADOWCOLOR`, `SEPARATE_HARDWARE_SAMPLERS`, `SSBO`, `TESSELLATION_SHADERS`, `FADE_VARIABLE`, `TEXTURE_FILTERING`; query-only: `COMPUTE_SHADERS`, `ENTITY_TRANSLUCENT`, `REVERSED_CULLING`, `CAN_DISABLE_WEATHER`, `PER_BUFFER_BLENDING`, `BLOCK_EMISSION_ATTRIBUTE` **[re-read]**. A flag the device lacks makes Iris raise an error (the page does not say more).
- **Added to the format:** compute shaders (`.csh`, up to 27 per pass, plus `setup` and `begin`), storage buffers (`bufferObject.<n>`, 128 MB guaranteed), images (`image.<name>`, 16), custom textures, 32 colour buffers, `shadowcolor2` to `7`, custom uniforms and variables (as OptiFine), the extra programs of 2.3, `dimension.properties`, Iris-only uniforms (`cameraPositionInt/Fract`, `eyePosition`, player health, `currentDate`, ...), and macros to detect it (`IS_IRIS`, `IRIS_VERSION`, `MC_GL_<extension>`).
- **What it declines.** Connected textures, custom item textures, dynamic lights and zoom (other mods do them). Core-shader replacement by resource packs is ignored while a pack runs. Packs built for the Focal Engine (Continuum 2.1) are listed as never supported.
- **Docs.** `shaders.properties` (a documentation site, source in IrisShaders/docs, last commit 2026-09-15): the programs overview is tagged unfinished, the Voxy page only points to a chat channel, the Aperture pages call themselves outdated. A separate open reference, ShaderDoc, exists because OptiFine's document is all rights reserved.

### 2.13 What the player sees

Packs live in `shaderpacks/` as zips or folders (the `shaders/` directory is found even when nested). Options, Video Settings, Shader Packs lists them, with buttons for the folder and the pack's own settings screen, generated from its `#define`s, with profiles. A pack that fails to load drops to normal rendering and tells the player in chat. Pack authors distribute through Modrinth and CurseForge; per Modrinth's API there are 1,124 shader projects, and the top by downloads are Complementary Reimagined (72.5 million), Complementary Unbound (47.1 million; the same code with other defaults), BSL (31.7), Photon (29.2), Solas (18.7), Bliss (15.7), MakeUp (14.2), Rethinking Voxels (13.7). Iris itself has 194 million downloads. Licences are mostly custom or "all rights reserved"; credit-and-borrow is the norm (Complementary began as a BSL edit, Photon credits Complementary, Solas credits four others).

### 2.14 The ecosystem now (2026-11-10)

- **Minecraft's versions** are now named by year: 26.1, 26.2, 26.3, with 26.4 in snapshots.
- **Iris** (Modrinth API): 26.3 is supported by Iris 1.11.7 (2026-09-30), 26.2 by 1.11.4, 1.21.11 by 1.10.8; one to four builds a month, landing on Minecraft's release day; each build targets particular Sodium builds. **OptiFine**: a 26.2 preview (2026-10-20); the last non-preview is for 1.21.11 (2026-02-05); still incompatible with Sodium.
- **Mojang** (read by two agents; my re-read timed out, and a third agent, working from a news report, could confirm only the announcement and 26.2's experimental Vulkan, not the 26.4 details): on 2026-02-18 an article announced the move from OpenGL to Vulkan, to enable Vibrant Visuals on Java; modders are told it will cost "more effort than the updates you undertake for each of our releases". 26.2 snapshot 1 (2026-04-07) had a Vulkan backend with a graphics API setting and a reversed depth buffer; 26.2 shipped (2026-06-16) with OpenGL the default and Vulkan experimental; 26.3 (2026-09-15) added order-independent transparency, a `/posteffect` command and one ShaderC compile path for both backends (`#include` replaces `#moj_import`); 26.4 snapshot 1 (2026-09-22) made Vulkan the default, and snapshots 7 and 8 pick OpenGL or Vulkan at random each launch as a test. Resource-pack core shaders (`assets/<ns>/shaders/core`) and post-effect JSON (`assets/<ns>/post_effect/*.json`) remain replaceable but, by the wiki, "not an intended or supported feature"; its history table has 14 shader-format rows between 1.21.2 and 26.4.
- **Iris and Vulkan.** Iris 1.11.0's notes say Vulkan is unsupported. The Iris account's own posts (read by one agent, through a mirror) say more: on 2026-02-18 that Iris will be discontinued and a new shader mod has been in work for two years; on 2026-02-19 that it is **Aperture**, in private beta, without support for old packs, a Vulkan rewrite begun the month before; on 2026-05-13 that betas had gone privately to shader developers (Minecraft 26.2, Vulkan, Sodium). Its developer will add no Iris maintainers but accepts pull requests that fix released versions (2026-02-22), and Iris's default branch still received commits until 2026-09-15. A one-person project, **Vitrail** (beta, 2026-10-13), runs OptiFine-format packs on Mojang's Vulkan renderer by adapting Iris code; **Sulkan** is a Vulkan shader mod with its own pack format; Canvas (JSON pipelines) has been dormant since 2024.
- **Aperture** (the Iris developer's successor): a closed-beta Vulkan rewrite in which a pack configures the pipeline in typed code and shades in Slang. It is the most design-relevant thing in this ecosystem, so it has its own section, 2.16.
- **Distant Horizons, Voxy, Bobby.** Distant Horizons (LOD terrain) reaches packs through `dh_terrain`, `dh_water` and `dh_shadow` programs, a material id attribute (`dhMaterialId`, 16 `DH_BLOCK_*` ids), uniforms for planes and projections (`dhNearPlane`, `dhFarPlane`, `dhRenderDistance`, `dhProjection`...), LOD-only depth samplers (`dhDepthTex0/1`), and the `DISTANT_HORIZONS` define; all need the `compatibility` profile **[re-read]**. LOD textures reached packs only in September 2026. A pack with no `dh_*` programs is incompatible with it. Voxy works differently: a pack ships a small patch (`voxy.json` plus `voxy_opaque.glsl`...) that modifies Voxy's own LOD shader. Bobby only lets real chunks load farther. Depth conventions are a live problem (forward versus reversed Z broke LODs for some users in October).

### 2.15 What authors struggle with, and wish for

- **Drivers.** NVIDIA accepts invalid GLSL that AMD rejects (packs fail on AMD); Intel exposes 7 attachments; Apple Silicon crashes on global-scope calls and is unsupported by Iris; vendor macros exist so packs can branch. Compile time is sequential, up to about 20 seconds for a heavy pack (an open request for parallel compilation).
- **GLSL spread.** `#version 120` (MakeUp), 130 (Complementary), 400 (Photon), 430 (Rethinking Voxels), all compatibility profile.
- **Doc defects** in the OptiFine text (`depthtex1` is unit 11 in one table and 12 in another); the format's quirks (buffers from declarations, options only if an `#ifdef` mentions them, conflicting defaults making an option ambiguous and locked).
- **Not expressible:** new scene passes, per-entity-type programs, direct block data, custom layers for opaque cubes.
- **Wish list** (scattered, mostly implicit): more colour buffers (done in Iris), voxel data access, debug tools, block-state data for block-display entities, a renderer-neutral API for LOD mods, add-ons that adjust a pack without redistributing its code, Vulkan support, parallel compilation. Iris maintainers want the per-pack "skylight cave-leak" hack to become unnecessary; it remains each pack's job.

### 2.16 Aperture, the successor in the works

*A seventh agent went deep on this (160 tool calls); the evidence is listed at the end of the section. Aperture's own source, its Java API documentation and its built-in Slang modules are not public, so the Java-generation parts come from reading beta packs and give names, not code. The one page of documentation about it calls itself "extremely outdated".*

**Status.** A successor to Iris by Iris's developer (IMS212 on GitHub), "in work for two years" in the Iris account's words, with no support for old packs. A closed beta for invited shader developers (the README of the FRACTURE pack says only closed-beta members can try it); I found no public build, licence, release date or system requirements. Every 2026 source mentions only Vulkan; whether an OpenGL path remains was not found. The betas' version strings name the Minecraft build they run on: beta.3 on 26.2-pre-4 (2026-06-05) up to beta.9 on 26.3-pre-1. Users ask for an open release and dates (Iris issue 3279) and no maintainer has replied.

**Two generations of pack.** The public example pack (IrisShaders/Aperture-Example-Pack, `slang` branch, last commit 2026-01-08) configures the pipeline in **TypeScript**: a `pack.ts` against a typed `iris.d.ts` of about 1,000 lines. Every beta pack I found that was started in June 2026 or later configures it in **Java** instead, against `dev.irisshaders.aperture.api`: FRACTURE (by the author of Photon), Arc4, Amaryllis and Nifl. Their `pack.json` has an `api` field such as `0.0.1-exp.10-snapshot+mc26.3-local`: a pre-1.0 version, a channel, a counter and the Minecraft build. That Java superseded TypeScript is my inference from dates; I found no statement that TypeScript was removed.

**The shape of a pack (Java generation).** `pack.json`; `src/java/` with a class implementing `ShaderPack` and one implementing `PackSettings`; Slang programs under `src/program/` and libraries under `src/lib/`; a generated `settings.slang`; `lang/en_US.lang`; noise textures. `ShaderPack` has four hooks: `configurePipeline`, `configureRenderer`, `onNewFrame` (once a frame, given the frame's state) and `setBlockId` (given a block state, returns an integer). How the Java is compiled and loaded: not found.

**How a pipeline is declared** (names from the example's TypeScript API; the Java API has equivalents).
- *No default pipeline.* The pack creates every texture it uses, by name (2D, 3D, array, cubemap, from an image or a file), with an explicit format and size; only the depth textures exist by default. A Slang global of the same name is bound to the texture.
- *Object shaders* draw the game's own geometry: one program per `Usage` (37 in the TypeScript API: terrain solid, cutout and translucent, entities, particles, sky, hand, a `SHADOW_*` variant of each, point-light shadows), with `BASIC` the fallback for a usage with no program. Outputs are explicit: a fragment output field is bound to a texture by name (no `RENDERTARGETS` comment), and blending is set in code.
- *Stages and command lists.* Between the game's own draws the pack lists full-screen and compute passes by stage (Java era: `SCREEN_SETUP`, `PRE_RENDER`, `POST_SHADOW`, `PRE_TRANSLUCENT`, `PRE_OVERLAY`, `POST_RENDER`, `POST_UPSCALE`). Order is declaration order. A final combination pass is mandatory. The render size and the window size are separate and first-class, which is how a pack upscales.
- *No hidden ping-pong.* A pack that wants history allocates two textures and swaps them by re-pointing a binding (`overrideObject`); explicit read and write hazards are among the stated fixes.
- *Constants.* A value the pack exports from its code (`exportBool`, `exportInt`, ...) arrives in the shader as a link-time constant (`extern static const`, a Slang feature), so a setting can specialise the compiled shader.

An illustration of the shape, written by me from those names, not copied from a pack and not run:

```ts
export function configurePipeline(pipeline: PipelineConfig): void {
    let color = pipeline.createTexture("colorTex")
            .width(screenWidth).height(screenHeight)
            .format(Format.RGBA16F).build();
    pipeline.createObjectShader("terrain", Usage.TERRAIN_SOLID)
            .location("objects/terrain")        // a Slang module
            .target(0, color)                   // fragment output 0 goes to colorTex
            .exportBool("fancyLeaves", true)    // arrives as a link-time constant
            .compile();
    let post = pipeline.forStage(Stage.POST_RENDER);
    post.createComposite("tonemap")
            .location("post/tonemap", "main")   // module, entry point
            .target(0, color).compile();
    post.end();
    pipeline.createCombinationPass("post/combination").compile();
}
```

**The Slang side.** `import aperture;` is always needed; it provides an `ap` uniform tree (camera, world, timing, celestial, game: the groups seen in use in the packs read, not a complete list) and the `Vertex` and `PixelData` types. A Java-era geometry shader is a struct with an `emitVertex` and a `shadeSurface` function. `Vertex` offers position, uv, colour and light coordinates and, when present, the normal, tangent, vertex ambient occlusion, overlay colour, offset to the block's centre and the block's integer; `PixelData` offers the albedo, normal and specular samplers when present. labPBR atlases are built in. A full-screen pass is a fragment entry point reading `SV_Position`; a compute pass uses `numthreads` with the dispatch size set from code.

**Blocks and materials.** There is no `block.properties`. The engine calls the pack's `setBlockId(state)` and stores the integer in the vertex; the callback can test the namespaced id and the tags. Packs build their own tables: FRACTURE packs a material id and a light-source id into two bytes. The older (TypeScript-era) docs list engine helpers `iris_isFullBlock`, `iris_hasFluid`, `iris_getEmission` and `iris_getLightColor`. I found no engine voxel grid; FRACTURE writes its own voxel face data from the vertex stage.

**Settings.** Typed and declared in code: a bool with a default; an int with min, max, step and default, or with a list of values; a float with min, max, step and default; nested screens; labels and tooltips from `lang` keys. A generated `settings.slang` turns an option into a link-time constant or a field of a constant buffer (which one the third argument of `option` chooses is undocumented; that true means adjustable at runtime is my inference).

**Shadows.** Built in: cascades (cascade 0 the smallest), array textures, a hardware comparison sampler, coloured translucent shadows into a pack-owned array texture. The TypeScript API's `ShadowSettings` has `resolution`, `cascades`, `pssmLambda`, `closestImportantDistance`, `outerMarginBlocks`, `outerMarginPixels`, `entityCascadeCount`, `safeZone`, `distance`, `near`, `far` and `enabled`, with no comments or defaults, and I found no documentation of the middle ones. What packs set: 4 cascades at 160 blocks and 2048 texels (the Java template), 8 at 160 and 1024 (Amaryllis), 1 to 16 at 16 to 1024 blocks (FRACTURE's options), and in two TypeScript packs 4 cascades with 2 for entities. The pack samples the cascades itself (`compare(uv, cascade, depth)`): the engine hands over no visibility. **Point lights** have shadow maps too (a resolution, a cap on how many exist and how many update per frame, the nearest few redrawn each frame with entities, terrain cached, a ranking threshold); their docs are "not ready". **Far terrain (Distant Horizons, Voxy): nothing found** in docs, packs or trackers.

**Compute, ray tracing, history.** Compute passes: yes. Hardware ray queries (a Slang `HWRT` interface over terrain and entities, enabled in code with `enableRT()`) appear in two beta packs (Arc4 and FRACTURE), not in the TypeScript API; that they need ray-tracing hardware is from memory **[unverified]**. History: by hand only, as above. CPU data reaches shaders as typed records written to a mapped buffer each frame (`mappedBuffer`, Java era), as `ap.*` values, or as exports.

**Why it exists, and what is unfinished.** Stated fixes: no fixed default pipeline; unlimited textures, buffers and programs; explicit outputs instead of `RENDERTARGETS`; explicit alpha test; explicit hazards instead of hidden ping-pong; core profile; cascaded shadows without distortion; hooks for vertex formats and transforms; portability beyond OpenGL. Unfinished: the one docs page is outdated and links to a page that does not exist; point-light docs are not ready; the API is at 0.0.x and its names change between betas (a pack's history has "update to new beta declarations"). Controversial: old packs unsupported; closed beta; licence unknown (Iris is LGPL-3.0).

**What I take from it (opinion).**
1. *It chose code for the pipeline and the settings, twice (TypeScript, then Java).* That is evidence that a purely declarative manifest runs out for authors at the frontier: conditional and repeated passes, computed sizes, per-frame data. It weakens my recommendation of JSON in 6.2 item 1 (now medium-low confidence). What it does not change is the cost for us: pack code would run on the player's machine unsandboxed, which is the reason the games that refuse client shaders give (section 3).
2. *Copy:* named resources and explicit outputs; explicit read and write hazards; render size apart from window size (our 60 fps on the integrated GPU is an upscaling question as much as a shading one); settings typed with min, max, step and default; settings that specialise the compiled shader (the WGSL `override` of 6.2 item 8); the block's integer asked of the block once, at mesh time.
3. *Costs it shows:* an API at 0.0.x that ties authors to the Minecraft build; a closed beta with no public documentation, which is a barrier for modders; a build step (`mvn validate`) where Minecraft's format needed none.
4. *Distant shadows are not solved there either:* four to eight cascades over a few hundred blocks, and nothing found for far terrain.

**Evidence.** Read on the page or API response: the example pack (branches, commits, `pack.ts`, the Slang files, all of `iris.d.ts`, issue 2 and PR 1); the docs page `shaders.properties/aperture/migration/` and, in the docs repository, the older overview and `oldInfo`; the Iris account's posts of 2026-02-18, 02-19 and 05-13 (read through a mirror); Iris discussion 3026 and issues 3279, 3416, 3171 and 3025; the READMEs and sources of FRACTURE, Amaryllis, Arc4, Nifl and JavaShader, and the TypeScript-era packs Arc-3, PointShadowSample and Luminista-aperture; two GamingOnLinux articles. Not read: Discord; a DeepWiki page (HTTP 429); a NamuWiki page (HTTP 403; its claims of an Iris feature freeze and an experimental Vulkan Iris dated 2026-10-05 are unverified); Mojang's own article (timeouts). Could not find: official docs or source for the Java API and the `aperture`, `object_shader` and `rt_pipeline` Slang modules; a repository, licence, download or release date; what an `api` mismatch does; how settings persist; Distant Horizons or Voxy support. **Unread lead:** `coderbot16/Steadfast` (GPL-3.0) has Slang files that import `object_shader`, so it may be an openly licensed Aperture pack worth reading.

---

## 3. Other block and voxel games

| Game | How much can players change? | Format | Notes |
|---|---|---|---|
| **Minecraft Bedrock** | data only (Vibrant Visuals); legacy shader packs are dead | JSON files, `texture_set.json`, a manifest capability | no shader code; per-platform binaries for the old materials |
| **Minecraft Java, vanilla** | replace core shaders and post-effects (unsupported) | GLSL plus `post_effect` JSON | format changes most releases; Vulkan coming |
| **Canvas** (Java mod) | full pipeline in data | JSON5 pipelines plus GLSL; options become `#define`s | dormant since 2024 |
| **Vintage Story** | shader files replaceable; a C# API to add programs | GLSL 330 `.vsh/.fsh`, includes | few packs; replacements collide |
| **Luanti** | settings plus what a game sets from Lua | engine GLSL; `set_lighting{...}` | no mod shaders ("would need sandboxing") |
| **Veloren** | quality modes; 38 experimental flags | GLSL to SPIR-V; generated `#define`s | no packs; hot reload |
| **Hytale** | none (server plugins and assets; no client mods) | JSON assets | stated policy |
| **Teardown** | per-voxel materials; a Lua property bag for environment and post | Lua | no shaders |
| **Roblox** | PBR maps; post-effect objects; no custom shaders | properties | "shaders have been used for cheating" (a forum post) |
| **Terasology** | shaders and a render graph are module assets | GLSL plus `.info`/`.mat` JSON; a DAG of nodes | namespaced ids |

**Bedrock.** The old `.material` packs stopped working on Windows and consoles after 1.16.200 and elsewhere after 1.18.30; the engine's compiled `.material.bin` is per platform, and only unofficial loaders exist. The official route is **Vibrant Visuals** (the 2023 "Deferred Technical Preview", shipped 2025-06-17 on consoles, new phones and PC, switched on per device). A pack opts in with the manifest capability `pbr` (engine 1.21.120+). Each surface has a `*.texture_set.json` naming `color`, `normal` or `heightmap`, `metalness_emissive_roughness` (or `..._subsurface`). The look is **JSON files with a `format_version` and an identifier**, each optional with engine defaults: `lighting` (sun and moon in real lux, auto exposure, tone mapping), `local_lighting` (per block: a static light or a point light with specular and dynamic shadows, which "considerably" cost more), `atmospherics` (Rayleigh and Mie scattering), `color_grading` (per tonal range, colour temperature in kelvin, six tone-map operators), `water` (absorption, waves, caustics). Any field can be a **keyframe table over the time of day**, and a pack can ship several files, picked by biome and blended by where the viewer stands. Schemas are versioned per feature and have churned (a rename in 1.21.120; built-in biome lighting overrides the pack's since 1.21.90).

**Java vanilla.** The wiki records churn in every release (1.21.5 removed the per-program JSON and moved programs into code; 1.21.6 made uniforms std140 blocks; 1.21.9 required GLSL 330; 26.2 merged the text shaders; 26.3 replaced the transparency post effect with built-in order-independent transparency). A mistake in a replaced shader disables the whole pack.

**Canvas** shows a declarative renderer: pipelines in `assets/<ns>/pipelines/` define `drawTargets`, `images`, `framebuffers`, `programs`, `options` (default, min, max: they build the settings screen and become `#define`s, so they are compile-time constants), `include`, and named pass lists (`onInit`, `onResize`, `afterRenderHand`, `fabulous`...), with `glslVersion` and `enablePBR`. It is marked early alpha and has not been updated since 2024-06.

**Vintage Story** treats shaders as assets: a mod replaces a built-in file by shipping the same path. That collides (one shader mod replaces `particlesquad.fsh`; users delete it to use another's), and the community answered with **ShaderPatchesLib**: JSON regex patches in grouped files (a failing patch skips its group), targeted at every shader that includes a file, switchable at runtime. The engine feeds a fixed uniform set; mods add programs through a C# `IShaderAPI` at named render stages (`Opaque`, `OIT`, `AfterPostProcessing`, `Ortho`...). Third-party shader packs are few and in beta; the rendering wiki says a tutorial is missing.

**Luanti.** About 20 shader directories ship with the engine; a setting can point to another directory, and the engine writes `#define`s for each enabled feature into the shader before compiling. Mods and texture packs cannot supply shaders; games steer the look with `player:set_lighting{saturation, shadows, exposure, bloom, volumetric_light, ...}` plus sky, sun, moon, stars and clouds calls, most of which "have no effect" on a client that disabled the feature. A developer note says custom shaders are "very long term" and "would need sandboxing"; a pull request adding client grading settings was closed unmerged in 2026-09 because reviewers wanted the modders in control.

**Veloren** (Rust, wgpu) writes its shaders in GLSL, compiled to SPIR-V with shaderc; includes resolve against a closed table of names; per pipeline it generates `#define FLUID_MODE FLUID_MODE_LOW`-style lines. Its settings split into modes that need the pipeline *rebuilt* (cloud, fluid, reflection, lighting algorithm, shadow mode, AA) and modes that do not, and unknown saved values fall back to a default. The nearest thing to packs is 38 experimental flags in `settings.ron` (Cinematic, Outlines, Newspaper, ViewNormals...), each a `#define`, with "do not report rendering bugs".

**Terasology** models its pipeline as a DAG of render nodes that modules subclass, and shaders as module assets with a JSON parameter list; a code comment warns that a missing module name produces ids like `null:fbo.something` that "register happily and then fail to match anything".

---

## 4. Other engines and tools: the seams they chose

For each, *the seam* is what the user writes against what the engine provides.

| System | The seam | Parameters | Passes | Safety, versioning |
|---|---|---|---|---|
| **Unity** (built-in pipeline) | `surf(Input, inout SurfaceOutput)` describes the surface; `Lighting<Name>(s, ...)` shades it; `Input` is a list of what the shader needs (`viewDir`, `worldPos`, ...) so the compiler writes the passes | `Properties { _MainTex ("Texture", 2D) }` | URP/HDRP: render-graph passes declare `UseTexture`; HDRP: seven injection points | surface shaders exist only in the built-in pipeline, now deprecated |
| **Godot** | `fragment()` writes `ALBEDO`, `ROUGHNESS`...; `light()` runs per light per pixel with `LIGHT`, `LIGHT_COLOR`, `ATTENUATION` (distance **and shadow** combined), accumulating into `DIFFUSE_LIGHT`; `render_mode` keeps engine behaviour without code | uniforms with hints (`source_color`, `hint_range`), groups, per-instance and global uniforms | `Compositor` effects (compute), renderer-specific | loops allowed, an infinite one freezes the app; `diffuse_*` versus a custom `light()` unspecified |
| **GZDoom** | engine fills a `Material` struct; the user implements optional hooks (`SetupMaterial(inout Material)`, `ProcessLight`, `ProcessMaterialLight`) | GLDEFS blocks, `uniform` with `cvar` binding | post-process shaders (`PostProcess` blocks) | the engine detects which hooks exist and injects shims for three generations of signature; text patching, brittle |
| **ReShade** | whole entry points inside `technique { pass { ... } }`; the engine supplies the back buffer and the game's depth | uniform annotations (`ui_type`, `ui_min`, `ui_max`, `ui_step`, `ui_items`, `ui_category`); `source=` binds engine values (frametime, random...); presets are INI | ordered techniques of ordered passes | API version handshake for add-ons; loops allowed; "Performance Mode" turns uniforms into **constants** at load |
| **RetroArch slang presets** | one file per pass; the frontend supplies sampled inputs by *name* (`Original`, `Source`, `OriginalHistoryN`, `PassOutputN`, `PassFeedbackN`, `UserN`) and sizes | `#pragma parameter ID "Label" initial min max step`; presets override by id; **delta presets** store only changes | the preset: `shaders=N`, scale type per pass, formats, LUTs; history and feedback are inferred from use | strict: any unrecognised member fails; feature probes (`_HAS_*` macros) |
| **Bevy** | `ExtendedMaterial`: the extension replaces the shaders; the user owns the fragment function and *calls engine library functions* (`pbr_input_from_standard_material`, `apply_pbr_lighting`) | extension data at a fixed binding | prepass/forward split by conditional compilation | WGSL, with WESL (imports, conditionals) |
| **Unreal** | domains times blend modes times a fixed list of shading models (Default Lit, Subsurface, Clear Coat, ...); post-process materials with only an emissive output | node parameters | priority-ordered post-process materials | the closed list of models is being replaced by composable "Substrate"; Epic advises using post-process materials sparingly |
| **glTF** | parameters and textures only (`KHR_materials_clearcoat`, `..._sheen`, `..._variants`) | with defaults | none | `extensionsRequired` normally avoided; the base material is the fallback |
| **Quake 3 / Source** | scripts of stages (blend, rgbGen, tcMod) / shaders compiled into the engine, content sets parameters and the engine picks a fallback shader by hardware level | literals | stages per surface | pure data: safe, portable, an expressiveness ceiling |

**Patterns that recur.** (1) *The engine computes the inputs and owns the loops and the shadows; the user returns a colour* (Unity, Godot, GZDoom): our seam is mainstream. (2) *Describe the surface, then light it, then post-process* as three separate hooks. (3) *Every hook optional, the engine supplies defaults.* (4) *Parameters declared beside the code with a range, a step, a label and a group, the UI generated from them, saved as sparse deltas* (ReShade, RetroArch, Godot). (5) *Passes declared as data, resources named by use* (RetroArch, Canvas, URP). (6) *Tiers:* data only (Bedrock, glTF), hooks (Unity, Godot, GZDoom), raw passes (ReShade, RetroArch). (7) *Version or capability negotiation* (ReShade's handshake, RetroArch's probes, glTF's used-versus-required, Iris's flags). (8) *Bake parameters into the compiled shader as constants* (ReShade Performance Mode; for wgpu the analogue is WGSL `override` constants set at pipeline creation). (9) *Layered presets* (RetroArch: game over directory over core over global).

**Pitfalls seen.** An authoring dialect tied to one pipeline dies with it (Unity's surface shaders). Raw-code escape hatches rot (Unity's Custom Function node, Java's core shaders). A hook that makes the user rebuild data they cannot know (GZDoom's `ProcessMaterial`). Making users configure what the engine knows (ReShade's depth conventions). No validation, or all-or-nothing failure (tModLoader crashes on a missing parameter; Java disables the pack on one error). Per-light and per-pixel hooks multiply cost (Godot `light()`). Closed enumerations grow until replaced (Unreal's shading models). Precedence left undefined (RetroArch). Renderer-specific features without capability tiers (Godot's compositor). **Fairness:** NVIDIA's Freestyle limits custom filters in competitive games, and a pack that replaces lighting can act as a fullbright cheat **[inferred]**. I found no surveyed system that *bans* loops: ReShade and Godot allow them, Godot warns.

---

## 5. Our interface against theirs

| | OptiFine / Iris | Aperture (closed beta, 2.16) | Ours today |
|---|---|---|---|
| **Unit of a pack** | folder of GLSL files named by stage | Slang shaders plus pipeline code: TypeScript in the example, Java in the betas; `pack.json` with an `api` version | a folder with `pack.json` and one WGSL file defining `pack_shade` |
| **Declared structure** | none: scraped from source (`const`, samplers, `#define` comments) | typed code API: named textures, object shaders, stages, settings with min, max, step and default | `pack.json`: id, name, interface number; nothing else |
| **Pipeline** | fixed list of stages; the pack fills them | no default pipeline; the pack lists passes per stage in order, with a mandatory final pass | none: the engine's one pass, then fog |
| **Targets** | `colortex0..15` (32 in Iris), formats by `const`, ping-pong | the pack creates named textures (2D, 3D, array, cubemap) and binds shader outputs to them by name; render size apart from window size | none |
| **Shadows** | the pack runs the `shadow` program, owns the projection and warps it; samples the map itself | built-in cascades (4 to 8 in the packs read, 160 to 1024 blocks), array textures, hardware compare; point-light shadow maps with a per-frame budget | engine-owned: 11 cascades to 59 million voxels, plus the exact contact walk; the pack gets one number |
| **Visibility the pack sees** | none: it computes it | none: the pack samples the cascades itself | `sun_visible`, `sky_visible`, `uncovered`, `above_horizon` |
| **Materials** | block ids from `block.properties`; labPBR textures | a `setBlockId(state)` callback returns an integer per block (id and tags can be tested); labPBR atlases built in; older docs list helpers for emission and light colour | the face's colour only |
| **World data** | none direct; rebuilt by voxelising in the shadow pass | no engine voxel grid found; packs build their own from the vertex stage; hardware ray queries in two betas | the engine has an occupancy volume and an exact ray walk; the pack cannot ask |
| **Temporal** | history by not clearing a buffer; the pack does its own jitter | no built-in history: two textures swapped by hand with `overrideObject` | none |
| **Compute** | Iris (OpenGL 4.3) | yes | none |
| **Settings** | `#define` comments scanned from source | typed options in code (bool; int and float with min, max, step, default; lists), a generated `settings.slang`, labels from `lang` files | none |
| **Versioning** | `version.<mc>`, Iris feature flags | an `api` string in `pack.json` (0.0.x, channel, counter, Minecraft build); what a mismatch does: not found | `interface` number, refused on mismatch |
| **Safety** | none; loops, bindings and textures all allowed; compile errors only | none found: a pack contains Java code; how it is loaded and limited: not found | no loops, bindings or textures; errors at the pack's own line; the card's refusal handled |
| **Portability** | OpenGL, compatibility-profile baggage; Vulkan in doubt | Vulkan (closed beta on Minecraft 26.2 and later) | WGSL through naga: every wgpu backend |

The Aperture column is the Java-era betas where they differ from the TypeScript example; its API is at 0.0.x and its names change between betas. "Not found" means not found in what could be read, not absent.

**What the table says in a sentence:** our seam is the *last stage of a deferred pipeline* (the lighting function with the engine's visibility as input), done more safely than anyone does it; what we have not built is everything before it (materials, extra surface data), everything after it (passes, targets, history), the declarations that would let a pack describe itself, and the ways to ask the world questions.

---

## 6. What I would do

*Opinions, with confidence. The order matters only for how it shapes the design, as the owner said; the reasons are given.*

### 6.1 Keep

- **The engine owns visibility, the pack styles it.** (High confidence.) Every Minecraft pack reimplements bias, warping and filtering, and that duplicated work is the source of their cost differences, their cave leaks and their gap at the base of shadows. Ours has the exact contact walk, which no pack could write, and the engine can improve it for every pack at once.
- **Local and cosmetic.** (High.) It fits the hard rules, and the games that refuse client shaders say why: Luanti (a pack would "need sandboxing") and Roblox (shaders "have been used for cheating"; both quotes are in section 3), while Hytale allows no client mods at all. Remaining risks: a pack that hangs the GPU, and a pack that acts as fullbright in multiplayer (question 2).
- **A numbered interface that is refused on mismatch, and checks with line-accurate errors.** (High.) Java disabling a pack on any error, tModLoader crashing, and GZDoom's three generations of text-patched shims are what its absence costs.
- **One source language through naga.** (High.) Bedrock's compiled, per-platform `.material.bin` is why its old packs died with a platform change; Java's 26.3 moved both of its backends to one compile path (ShaderC), which is the same direction.

### 6.2 What is too narrow, in the order I would settle it

1. **A manifest that declares things.** Typed settings (toggle, choice, range with step, group, default), the interface version, the engine answers the pack needs, the passes (below). Minecraft scrapes this from shader source and pays in ambiguity; ReShade, RetroArch and Canvas declare it as data, Aperture as typed code. It comes first because every other feature hangs on it. We already read JSON packs with generated schemas, so I would extend `pack.json`, and keep it declarative (statically checkable; no code of the pack runs outside the shader) **[opinion]**, with a small safe expression language for computed values and for conditional or repeated passes (OptiFine has `program.<name>.enabled = <expression>` and `uniform.*`; a bloom pyramid is one pass many times). **Aperture is the counter-evidence:** its authors chose code, twice (2.16), which suggests expressions will eventually run out for authors at the frontier; the cost of code for us is a pack running unsandboxed on the player's machine. (High confidence in a manifest; **medium-low in JSON over code**: the most contestable recommendation here, question 7.)
2. **Loops.** Our ban is stricter than every system surveyed and removes the techniques packs consist of: PCF and PCSS filtering, ambient occlusion, ray marching, blur kernels, volumetric steps. I would allow `for` loops whose bound is a compile-time constant (or a setting's declared maximum), no `loop` or `while`, and a budget on the product of nested bounds, all checkable on naga's output. The remaining risk (a long-running shader) is shared with every system surveyed. (Medium-high; it reverses a rule I set in step 1, so it is the owner's call: question 1.)
3. **Requests, not resources.** A pack states which engine answers it wants (occluder distance for soft penumbrae, an ambient occlusion term, a bent normal or sky direction, thickness for leaves, the previous frame), and the engine computes and writes only those. It is Unity's `Input` list and Iris's feature flags; it matters most on our integrated-GPU budget, where an unused extra buffer costs bandwidth. (Medium-high.)
4. **Passes after the colour.** Bloom, depth of field, temporal anti-aliasing, tone mapping, volumetric fog and sharpening are where most of Photon's twenty-one composite passes go. A declarative pass list in the manifest (named targets with format, scale and history; each pass names a WGSL file, its inputs and its output; engine buffers by name: `color`, `depth`, `normal`, `albedo`, `material`, `sun_visibility`, `motion`) is RetroArch's model with Aperture's ambitions; resources named by use let the engine skip what is not read. The owner left post-processing out "for now"; I would still design the manifest to *allow* it. Two things to copy from Aperture (2.16): resources named and sized explicitly, with the render size apart from the window size (a render scale is also how an integrated GPU reaches 60 fps), and explicit read and write hazards in place of hidden ping-pong. (High that it is needed; medium on when.)
5. **Materials from block data.** Rendering properties on the block definitions themselves (emission, roughness, foliage and sway, translucency, subsurface), delivered in `Surface`, so a modded block describes itself and no pack needs a 117 KB list of names. A pack may still override by namespaced id or tag, which gives Minecraft authors their `block.properties` habit. Aperture replaced the property file with a callback that is given each block's state and tags and returns an integer, so the genre is already moving from lists of names to asking the block (2.16). (High.)
6. **A vertex hook** (waving leaves, water), run by the same code in the picture and in the shadow passes. Minecraft packs write the waving in both the terrain and the shadow programs, and a mismatch would show as a wrong shadow **[inferred]**. (Medium; the contact walk uses the lattice, so a displaced mesh differs slightly from it.)
7. **Bounded world queries.** Offer the occupancy volume and exact ray walk we already have as sandboxed calls (`trace(origin, direction, longest)` and the exact sky cover, the "bounded queries" that `shader-pack-spec.md` lists as future hooks), so that nobody needs the voxelisation hack. Cost and the shape of the answers need a design pass; today the windows hold occupancy only, not materials or light. (Medium.)
8. **Settings as pipeline constants** (`override`s), as ReShade's Performance Mode does and as Aperture does with exported link-time constants (2.16), so changing a setting does not recompile text. (Medium; needs checking against how naga treats loop bounds from overrides.)
9. **Authoring tools:** debug views of any buffer (Photon's `DEBUG_VIEW`, Veloren's `ViewNormals`), reload, and errors shown in the game. (High; it pays at once.)
10. **Sky, fog and the sun's look** as data first, hook second. Bedrock's JSON atmospherics and day-time keyframes cover most art direction without code; for a game of planets, an atmosphere definition per body, blended by where the viewer is, is natural. (Medium-high.)

### 6.3 What not to copy

Scraping structure out of source text; the legacy names and unit numbers; each pack numbering blocks for itself; ping-pong buffers whose misuse gives silent artifacts; the GLSL version and profile zoo; closed lists of shading models (parameters and hooks age better: Unreal's Substrate); replacing whole files (Vintage Story's collisions: use named hooks); includes resolved by path (resolve by namespaced id and fail loudly, as Veloren's closed table does).

### 6.4 Familiarity at no cost to the design

Keep the `shaderpacks` folder name and zip-or-folder packaging; a settings screen generated from the manifest with profiles (Low, Medium, High) and groups, because players expect them; a reload command; translations by `lang` files; and a **porting guide with a table of their words and ours** (section 9; the vocabulary the next generation of authors will know is Aperture's, object shader, composite, compute, stage, combination pass, so the table has both). I would not try to *load* OptiFine packs: Iris needed an AST translator and an extended vertex format to run them on a renderer built for them, Mojang's change is breaking even that, and our shading model differs. Familiar concepts and player experience, not familiar code.

### 6.5 The default pack should be a pack

The strongest check that an interface is real is that the default is written through it. Today the default's lighting already is `pack_shade` (in `lighting_default.wgsl`); the sun's visibility, the fog, the sky and the pictures are engine code that a pack cannot reach. Each hook added above should be used by the default first, so that nothing the default does is unavailable to another pack.

---

## 7. Coarse terrain shadows

**What the genre teaches.** Nobody there has solved distance well. Packs use one map warped toward the player; past it they fade into the skylight lightmap or a screen-space march; Distant Horizons' LOD shadow pass exists, adds an unbounded caster cull, and the three biggest packs turn it off; the cascades in the big packs are none. Aperture has built-in cascades, which supports the direction we took, but what its packs set is 4 to 8 cascades over a few hundred blocks (up to 1,024 in FRACTURE's options), and I found nothing for far terrain (2.16): our 59 million voxels are far past what that world attempts. **Screen-space marching** (what Photon does far away) only sees what is on screen and ghosts under temporal upscaling; for a pilot looking down on terrain most of it is on screen, so it is a plausible cheap far-field tool, but it cannot cast from behind the camera.

**What we have.** The shadow spec's design (its section 2, read) reaches 59 million voxels, 59,000 km at a metre a voxel, with coarse nodes as casters and receivers; Cave's planet is 150 km in radius (from the comment in `games/voxunami_cave/src/game.rs`), so a view of it from orbit is well inside that reach; I did not check the farthest bodies of Cave's space level. **[run]** I took two pictures from the pole of Cave's planet (1280 by 720, view radius 12, `--camera 16,63950,16` at 17:30 with pitch -6 degrees, and `16,64300,16` at 18:30 with pitch -10), each with the shadows on and with `--shadows off`. I did not measure the camera's height over the ground: the walking body settles, so read them as views from a walker's place. With the sky masked out (a check: not one sky pixel differs between the pair), with the shadows off **57% (17:30) and 47% (18:30) of the terrain pixels in the 40 rows under the horizon change** by more than 6 of 255 in some channel, and 79% to 100% of those in the middle distance (rows 260 to 560 of 720). (An earlier figure of mine, 21% to 64%, counted sky pixels as terrain; it was wrong, and these replace it.)

**Correction, 2026-11-11, and I was wrong a second time: what I read from those numbers, "the shadows reach the horizon", is false.** The owner looked at the game and said he saw no shadows on the coarse terrain, and he was right. A face turned from the sun changes when the shadows are turned off too (it loses the shadowed share of the sky), whatever does or does not cast onto it, so the change between *on* and *off* does not show that distant terrain casts. The test that does is to take the cause out and see whether the picture changes. **[run]** With the nodes of the coarse levels left out of the casters, six of the eleven pictures I made (15:00, 18:30, and 16:30 at four headings) did not change by more than 0.02% of the pixels in any band of rows, and the other five (17:30) changed only in rows 180 to 440, the middle distance, by up to 18% of a band; a count inside the renderer then showed the cascades of 2,048, 16,384 and 131,072 voxels half width planned, drawn, and **given no caster at all**. The cause is in section 7.1. What my first pictures did show, and still show, is that the shade is dim and flat: in the 18:30 picture the far ridge that faces the sun is bright and the hills and floors that the near hills shade are an even dark green with no relief inside them, while with the shadows off the whole view is one even green in which relief reads only from the voxel edges. *Shape reads where light meets dark, and not inside the shade.*

### 7.1 What was wrong, and the fix (2026-11-11)

**[run]** Cave's planet is a grid (a ship with terrain), and the renderer counted every grid as a thing that moves. The maps that are kept for a while, those of a texel wider than a voxel (every cascade past 512 voxels of half width), hold only terrain, because a kept map would hold a ship's shadow where the ship was. So the planet's ground was in no sun map past 445 voxels from the camera (counted), and, *read from the code and seen in the pictures, not counted*, in none of the sky's cover maps (they never hold things that move): a count in a picture at the pole had 6,547 of 6,549 things "that move", and no cascade past 512 had a caster. The tests of 2026-10-28 that showed shadows at every scale used the terrain of a level (bricks and nodes held as terrain), never a planet's grid, so they passed. It is a bug and not a decision: the spec said "things that move cast shadows only into the fine maps" to mean ships.

The fix: a grid can be told it is **ground** (`VoxelRenderer::set_grid_is_ground`; the session says it once for a grid with terrain, `MeshUpdate::GridIsGround`). Ground is held like terrain: in every map, the sky's too, and a change of its bricks, nodes or pose makes the kept maps out of date as a change of terrain does. A ship that is not ground is as it was. Tests, written first: the block and roof scene of the shadow spec held as a planet's grid, at four scales, on four adapters (it failed at level 9, the camera 10,000 voxels from the floor, with the floor "1.000 of the light" where the block shades it, and passes now); a ship's node far from the camera is still in no kept map; ground that moves is followed by the kept maps (killed by a mutant that leaves the version unchanged); the session says it once. The workspace tests pass (3,239; 5 ignored). **Pictures** (seven views, before and after; I looked at two pairs, 15:00 and 17:30 at heading 0, and measured all seven: up to 34% of the pixels of a band of rows changed): the far hills now have shade where the near ones shade them, and the middle distance shows shadow shapes it did not. **Cost** (`cave --benchmark`, 1280 by 720, 17:30, dev profile, one run each, the spread on this machine is about 0.4 ms on the RTX and 0.5 on the integrated GPU): RTX 3090 3.31 to 3.38 ms, integrated AMD 13.87 to 14.01 ms. *Not measured:* a planet that moves on a rail (the window carries a grid's pose on between ticks, so its pose changes in every frame, and each change makes the kept maps out of date; they are drawn again at the budget of 1.5 million quads a map, the same as for edited terrain, but I have not seen what that costs).

**One change in the pictures that was not asked for:** the sky's cover maps hold the ground now too, so the planet's overhangs and caves darken as the shadow spec's section 8 says they should (Cave 0: dark by day): the mean brightness of the 15:00 picture falls by 3 of 765 and the steps under ledges are darker. It was designed and never worked on a planet. The owner may want to look at it.

**What is left of the question below.** The first thing missing was the casting itself, and that is fixed. Whether the shade is then legible enough for judging relief is still open, and now it can be looked at.

**So I do not yet know what else is missing**, and I would rather ask than guess. Candidates, from the spec's own limits list and from what packs spend effort on:

1. **Readability inside shadow.** *Read from the spec and `games/voxunami_cave/src/game.rs`:* a place the sun does not reach gets about a quarter of the sky's light (`sky_light_in_shadow` 70 of 255), and the sky's light is itself 110 of 255, so an upward face in shade is about 12% of full brightness before its colour (my arithmetic, from `lighting_default.wgsl`). That share is the same for a valley floor and for the lee of a hill. The only thing that varies it is the sky's cover (shadow-spec sections 7 and 8): a heightfield, thirteen taps 3 to 8 voxels across, from maps that reach 11, 39, 94 and 205 voxels from the camera, with "farther the sky is open". So beyond 205 voxels there is no ambient occlusion at all, and nearer it only darkens what is under a roof or a hill. Minecraft packs spend most of their passes on exactly this (ambient occlusion, sky-hemisphere ambient, bent normals; sections 2.8 and 2.9). My opinion, medium confidence: for judging relief, this matters more than the shadow edge does.
2. **Softness and size of the edge at distance.** The texel of a cascade doubles over the first five (a sixteenth of a voxel to one voxel), then grows fourfold, then eightfold (shadow-spec section 2); the spec's own figure for the blur is a texel over the sine of the sun's height, "seven times as long at 8 degrees up".
3. **Casters dropped** (shadow-spec section 7, read): a caster smaller than two texels of a cascade is not drawn into it, so the trees and bricks round the camera do not shade far terrain; things that move cast only into the maps of a texel of a voxel or less, out to about 500 voxels, so a ship's shadow on far ground is not drawn; a far map of edited terrain follows within a second or two.
4. **Contact at coarse levels.** The exact walk covers the near windows. The contact-trace spec (section 4.3, read) sends a place whose cascade is beyond them to its phase 5, "not designed further", and reasons that the strip there is about a cell wide and cells follow pixels, so about a pixel on the screen. *Believed from the spec, not measured at coarse levels.*
5. **Past 59 million voxels** there are no shadows (spec): a view of a 150 km planet is far inside that, so I list it for completeness.

If the aim is "pilots judge terrain from a distance", I would measure *legibility* and not only correctness: a set of fixed views (a mountain range at 06:45 and 17:30 from altitude, a valley, a canyon, an orbit approach), with the terrain's relief recoverable from the picture, checked by eye and by a number (edge contrast between lit and shaded slopes). **My opinion, medium confidence:** the survey and the pictures together say the distant shadows exist and the next gain is not another cascade but what the shade looks like and what the pack can do with it (candidate 1, and the pack's access to the sky's openness, section 6.2 items 3 and 7), so "coarse terrain shadows" may be better named "legible shade at distance". I may be wrong about what you saw; question 5.

---

## 8. Questions for the owner

1. **Loops.** Allow `for` loops with constant bounds and a budget (my recommendation), or keep the ban? It decides whether a pack can filter shadows, occlude ambient light or ray march at all.
2. **Fullbright.** Should a pack be *able* to ignore darkness (show a cave as lit)? Today it can: it receives the answers and may disregard them. It matters only in multiplayer, and is cosmetic for now; I would decide before the first release.
3. **Post-processing.** You left it out "for now". The survey says it is most of what packs are made of. Should the manifest be designed to allow passes from the start, with the passes built later?
4. **Materials in block data.** May rendering properties (emission, roughness, foliage and sway) become part of block definitions, so that modded blocks describe themselves?
5. **Coarse terrain shadows.** *Answered 2026-11-11 by the owner's look: there were none, and it was a bug (section 7.1, fixed).* What remains is whether the shade is legible now: which of the candidates in section 7, if any, is still missing once the shadows are there?
6. **OptiFine and Iris packs.** Confirm that we do *not* try to load them, and give a porting guide instead.
7. **Declarative or code.** *Answered 2026-11-11 by the owner: pipelines are set up in a programming language, not yet chosen; the game does not read the comments of a shader (see "The owner's answers" below).* The recommendation I gave (declarative JSON, medium-low confidence) is overruled; the cost it named, that a pack can no longer be checked statically or shared without trust, is now a design problem for the choice of language (sandboxing, limits on time and memory).

**The owner's answers and wishes, 2026-11-11** (said with the report in hand; each is also in `future-goals.md`):

- The game **does not read a shader's comments.** Settings and options are not scraped from `#define ... // [values]` lines as Minecraft does (section 2.7); a pack declares them in its own setup.
- **Pipelines are set up in a programming language** (as Aperture's packs are, section 2.16), which has not been chosen.
- A pack can give the player **options menus as nested screens**, as in Iris. In the far future: **any combination of nested screens and vertical tabs.**
- **Shadow cascades are to be well supported.**
- **LabPBR** is to be supported eventually, and **our own PBR format** that holds all channels of a texture in **one JPEG XL file**.
- Someday, long after the game is playable and has had its first release, it should be possible to **switch from our graphics system to one that loads Aperture shaders.** *My reading, not the owner's words:* that is a reason to keep the pack interface's concepts (named textures, objects shaded by kind, stages, typed settings, constants fixed when a pack is built) close to Aperture's, and to keep the graphics system behind a seam.

---

## 9. Sources, and their words and ours

**Their words and what each is, with ours today.**

| Their word | What it is | Ours today |
|---|---|---|
| `gbuffers_*` | the pack's program for a category of geometry | one shader for every face; `pack_shade` |
| `colortex0..15` | the pack's render targets | none |
| `deferred`, `composite`, `final` | fullscreen passes before translucents, after, and to the screen | none (the engine adds fog) |
| `prepare`, `shadowcomp`, `setup`, `begin` | passes before terrain, on the shadow buffers, compute | none |
| `shadowtex0/1`, `shadowcolor0/1` | the shadow map and coloured shadow buffers | engine-owned cascades; not visible to packs |
| `mc_Entity`, `block.properties` | block id as the shader sees it, and the list that makes it | none |
| `at_midBlock` | offset to the block centre, plus its light level (Iris) | none |
| `#define X // [a b c]` | an option, and its values | none (`pack.json` has no settings) |
| `profile.*`, `screen.*`, `sliders` | option presets, pages, sliders | none |
| `uniform.float.x = expr` | a uniform computed on the CPU | the frame's fixed uniform |
| `iris.features.required` | capability negotiation | `interface` number |
| `dh_terrain`, `dh_shadow` | programs for LOD terrain | LOD drawn by the same shader |
| `Usage.TERRAIN_SOLID`, `createObjectShader` (Aperture) | a program for one category of geometry, in the successor | one shader for every face |
| `Stage.POST_RENDER`, `createComposite`, `createCombinationPass` (Aperture) | lists of full-screen and compute passes by stage, and a mandatory last pass | none |
| `exportBool`, `extern static const` (Aperture) | a value from the pack's code that arrives in the shader as a link-time constant | none (WGSL `override` is the analogue) |
| `setBlockId`, `Vertex.blockID()` (Aperture) | the block's integer, asked of the block by a callback | none |
| `ap.camera`, `ap.world`, `ap.celestial` (Aperture) | the uniform tree a shader reads | `frame.*` |
| labPBR | a material texture standard (`_n`, `_s`) | none |
| lightmap | block light and sky light, 0 to 15 | `lamp`, `side`, `sky_visible` |

**Primary sources** (most of the facts above): OptiFine shader specification, `OptiFineDoc/doc/shaders.txt` and `shaders.properties` in github.com/sp614x/optifine; Iris documentation, shaders.properties (the site), and its repository IrisShaders/Iris and ShaderDoc; Iris's Distant Horizons page (`shaders.properties/current/reference/mod-support/distant_horizons/`), flags page (`.../shadersproperties/flags/`) and shadow tutorial (`.../guides/your-first-shaderpack/4_shadows/`); Mojang's articles on minecraft.net (the Vibrant Visuals for Java announcement of 2026-02-18, the 26.2 and 26.3 release notes, 26.4 snapshots 1, 7 and 8); the Minecraft Wiki's Shader page; Microsoft Learn's Vibrant Visuals pages and the Bedrock Wiki's shaders page; the repositories of Photon (sixthsurge), Complementary Reimagined, MakeUp Ultra Fast, Rethinking Voxels, Solas, Bliss; Modrinth's API (versions and downloads, queried 2026-11-10); labPBR specification (shaderlabs.org wiki); Canvas (vram-guild), Luanti, Veloren, Terasology and Vintage Story (API docs and repositories); Teardown, Roblox and Hytale documentation; ReShade (`REFERENCE.md`, `ReShade.fxh`), RetroArch (libretro slang shader docs and source), Unity, Godot, Unreal and glTF documentation; GZDoom source (`main.fp`, `hw_postprocess.cpp`); Aperture: the example pack (IrisShaders/Aperture-Example-Pack: `pack.ts`, `iris.d.ts`, the Slang files), the docs page `shaders.properties/aperture/migration/`, the Iris account's posts (read through a mirror), Iris discussion 3026 and issues 3279 and 3416, the sources of the beta packs FRACTURE, Amaryllis, Arc4 and Nifl, and two GamingOnLinux articles (2.16 lists them).

**Checked a second time [re-read]:** the OptiFine program table, fallbacks and colour-buffer counts; Iris's feature flags (the page says a missing required feature makes Iris "throw an error", not that the pack fails to load: I corrected that wording); the Distant Horizons page (it does not say `dh_water` falls back to terrain, which I dropped; it does say LODs have no shadow distance of their own); the shadow tutorial's three statements.

**Not read, or only from snippets:** BSL, Chocapic13, Kappa, Sildur's and SEUS PTGI internals; Aperture's source and Java API documentation (not public), Discord, and two wiki pages about Iris and Aperture (HTTP errors); the Valve and ZDoom wikis; Mojang's two announcement pages in my own re-read and in the Aperture agent's (timeouts; it used a news report). 2.16 has the details and an unread lead.
