# Writing a shader pack

A shader pack changes **how a face is lit and coloured**, for you alone: nothing is sent to a server, and a recording shows what it showed when it was made. The engine still works out what is lit (the sun's shadow, the exact contact at the foot of a wall, the sky over a place, the horizon of a planet, the lamps); the pack turns those answers into a colour. `docs/specs/shader-pack-spec.md` has the design and why; this page is for writing one.

## A pack is a folder

```
shaderpacks/
  cel/
    pack.json
    lighting.wgsl
```

`pack.json`:

```json
{
  "id": "someone:cel",
  "name": "Cel shading",
  "interface": 1,
  "description": "The sun's light in three steps."
}
```

- `id` (required): a namespaced id, `author:name`. Two packs with one id are one: the first is used.
- `name` (required): what a person reads. You choose a pack by its id, the part of the id after the colon, its name (any case) or the beginning of any of them.
- `interface`: the version of the pack interface the pack is written for. The engine's is **1**; a pack for another is refused, so that a change of the interface is never a quiet break. May be left out.
- `format`: 1 (may be left out). `description`: free text.
- `settings`: settings of the pack's own, which its lighting reads and the player changes (see "Settings" below).
- `engine_settings`: the defaults the pack sets for the engine's settings, by name (`{ "scene_lift": 1.0, "relief": "flat" }`); each must be in the setting's range, or one of the words of a choice. How the relief of block pictures is drawn is the engine setting `relief`, so a pack that wants it flat says `"engine_settings": { "relief": "flat" }` (there is no `relief` key of the file any more). See `engine/renderer/example_shader_packs/flat`.
- Any other key is an error.

## `lighting.wgsl`

One file of WGSL ([the language](https://www.w3.org/TR/WGSL/)) that defines

```wgsl
fn pack_shade(s: Surface) -> vec3<f32>
```

and may define constants and functions of its own. It returns the **linear** colour of the face at this pixel; the engine puts the fog over it, and then **lifts the whole scene** (the sky too): each channel to the power of one over 1.2, which makes the darks brighter (0.1 comes out 0.147, 0.5 comes out 0.561) and leaves black black and full brightness full. A pack cannot change the lift (the lifting code is the engine's: `lifted` in `voxel.wgsl`, and `lifted_color` in `voxel_renderer.rs` for the sky where the picture is cleared to it); the player can change how much, with the setting `scene_lift` (see "Settings" below). What a pack returns as a data channel is lifted too. Faces of blocks, ships, avatars and far levels of detail are all drawn with it. `Surface`:

| field | what it is |
|---|---|
| `albedo: vec3<f32>` | the colour of the face at this pixel, linear: its picture where it has one |
| `normal: vec3<f32>` | the face's normal, in the world's axes; far over a planet it is turned toward the way out of the planet, which is what the sun's cosine is of |
| `relative: vec3<f32>` | where the point is, relative to the camera, in voxels |
| `distance: f32` | how far from the camera (interpolated between the corners of a quad: use `relative` for an exact one) |
| `lamp: f32` | the light of the lamps and the ambient light, 0 to 1 |
| `side: f32` | how bright the side of the face makes it, 0.4 to 1 |
| `above_horizon: f32` | the share of the sun above the horizon of the planet the place is on, not hidden by a planet or a moon, 0 to 1 |
| `sun_visible: f32` | the share of the sun's light that reaches the place: the above, and the terrain's shadow on top of it, 0 to 1 |
| `sky_visible: f32` | the share of the sky's light that reaches the place past the terrain's shadow, 0 to 1 |
| `uncovered: f32` | the share of the sky's light that reaches the place past what is over it (a roof, a hill over a cave), 0 to 1 |
| `smoothness: f32`, `reflectance: f32`, `metal: f32` | The red and green bytes of the picture's specular companion (`_s`, LabPBR), 0 to 1, faded out with the picture, 0 where it has none, and `metal`, 1 where the green byte is 230 or more (the pixel throws back its own colour: `mix(vec3(reflectance), albedo, metal)` is the light it throws back straight on). The default pack adds the sun's glint from them (`sun_glint`); a pack that does not look has none. Added without a new interface number, as `emission` was |
| `emission: f32` | how much of the pixel shines by itself, 0 to 1: the alpha of the picture's specular companion (`_s`, LabPBR), 0 where it has none. The default pack shows the colour by this share whatever the light: `mix(lit, albedo, emission)`. Added without a new interface number: a pack only reads `Surface`, so an added field breaks none |
| `shading_normal: vec3<f32>` | `normal` tilted by the picture's normal companion (`_n`, LabPBR: the bumps), a unit vector, the same as `normal` where the picture has none or none is drawn. The default pack takes the sun's light as the cosine of this one; the shadow, the sky's light and the lamps' light are not made of it (they have no direction to be bumped by). A pack that keeps `s.normal` has no bumps. Added without a new interface number, as `emission` was |
| `ambient_occlusion: f32` | The share, 0 to 1, of the ambient light that reaches the pixel past the picture's occlusion (`_n` blue, LabPBR: 255 is all of it), faded in with the picture; 1 where the picture has none or none is drawn. The default pack multiplies the lamps' light, the ambient light and the sky's by it, and not the sun's. Added without a new interface number, as `emission` was |
| `subsurface: f32` | How much of the sun's light the pixel lets through from the far side of its block, 0 to 1: the blue byte of the picture's specular companion (`_s`, LabPBR) from 65, `(byte - 64) / 191` (up to 64 is porosity, which is not read), faded in with the picture; 0 where it has none or none is drawn. Added without a new interface number, as `emission` was |
| `sun_behind: f32` | For a face that looks away from the sun on a pixel whose `subsurface` is above 0: the share, 0 to 1, of the sun's light that reaches the far side of the block (where the sun's ray leaves a slab one voxel thick along the face's normal), past the horizon and the terrain's shadow, looked up at that place; 0 for any other face. The default pack lights the face by `sun_light.x * subsurface * sun_behind * max(-dot(normal, sun_direction), 0)`, in the face's colour, added to the sun's own term (which is 0 on such a face). Added without a new interface number, as `emission` was |
| `relief_sun: f32` | The share, 0 to 1, of the sun's light that gets past the relief of the picture itself: the alpha of the picture's normal companion is a height, and a pixel that a higher one toward the sun stands over, in the sun's ray, is in the shade of it; 1 where the picture has no normal companion, none is drawn, or the pixel is at the top. Apart from the terrain's shadow (`sun_visible`). The default pack multiplies the sun's term and its glint by it. Added without a new interface number, as `sun_behind` was |

The strengths the atmosphere sets are in the frame's uniform, which the pack may read: `frame.sun_direction.xyz` and `frame.side_direction.xyz` (unit vectors), `frame.sun_light` (`x` how bright the sun lights a face that looks at it, `y` the sky's light, 0 to 1), `frame.heavens.w` (the light of the night sky, 0 to 1), `frame.fog_color`, `frame.fog_range`. Other members of `Frame` are the engine's and may change without a new interface number.

**The default pack is the equation the engine has always used**, in `engine/renderer/src/lighting_default.wgsl` (and as `renderer::default_lighting()`): start from a copy of it.

## What a pack may not do

The game checks these when it reads the pack (and `voxunami check-shader-pack` says them with the line and column in your file):

- **no loop** (`loop`, `while`, `for`): a loop that does not end hangs the graphics card, so a pack writes its steps out;
- **no binding** (`@group`, `@binding`) and **no variable outside a function** (`var<...>`): a pack is given a `Surface` and the frame's uniform and nothing else;
- **none of the engine's names** for what it keeps to itself (the shadow maps, the palette, the voxels' tables, the pictures and their sampler) and **no texture function** (any word that begins `texture`): a pack cannot ask the map where the shadow is, which is how it cannot bring back the lit strip at the foot of a wall. A word of your own that is one of these is refused too: rename it;
- a pack must define `pack_shade` with the signature above, and must not define a name the engine has (it is told which).

A pack **may** ignore what it is given: a pack with no shadows is a style (`shadowless` below).

## Settings

The engine has numbers and words of taste that a player changes by eye. Each has a name, a range (or a list of words) and a default, and is written into the shaders as a constant of the same name in upper case, so that it costs nothing in a frame:

| setting | default | takes | what it is |
|---|---|---|---|
| `relief` | `prisms` | one of `flat`, `prisms`, `parallax_occlusion` | how the relief of block pictures is drawn. **`flat`** draws the face flat: the tilts of a picture's normal companion (LabPBR `_n`, the "normal map") bump it, so that the sun's light is the cosine of the tilted normal, and the height shades the picture's own relief. **`prisms`** makes each pixel of a picture whose normal companion has a height in its alpha a square column that deep, with its sides lit by the sun; a prism is flat on top, so near the camera the tilts are left out where prisms are drawn. **`parallax_occlusion`** (parallax occlusion mapping, the usual smooth parallax, and the lumpy look of the side-by-sides in `docs/history/parallax-spike.md`) takes the heights as a smooth surface (the four pixels round a place mixed as a bilinear filter would), marches the eye's ray down through it in 12 equal steps and reads the picture where the ray meets it: the pixel grid bends, there are no walls and no flat tops, and the normal picture's tilts still light what is seen (so it keeps the "normal map" that prisms leave out). It suits mud and stone better than pixel art, and past about 0.15 `relief_depth` at a low eye the equal steps show as streaks. Each word is a different shader, so the ones not chosen are not in it at all: flat has no relief code, which saves about 1.2 to 1.5 ms of a 3 ms frame of nothing but dirt on an integrated GPU (nothing on a picture with no relief); parallax occlusion costs about +0.8 to +0.9 ms there at a 25 degree eye (measured 2026-11-15, prisms +1.1 to +1.3 in the same run) |
| `relief_depth` | 0.1 | 0 to 0.2 | how deep, in voxels, the relief of a block's picture is (the lowest pixel of its height field below the top); past about 0.15 the shader's limit of steps shows |
| `wall_darkness` | 0.2 | 0 to 1 | how much darker the ambient light is at the foot of the wall of the deepest prism than at its top |
| `scene_lift` | 1.2 | 0.5 to 3 | the light of the whole scene is raised to the power of 1 over this, so 1 is no lift; black stays black and full brightness stays full |

While playing, `.shadersetting` lists them, `.shadersetting NAME` says what one is for and what it takes, `.shadersetting NAME VALUE` changes it (the beginning of a name will do; a word of a choice is typed in full, in any case, as in `.shadersetting relief flat`) and `.shadersetting NAME default` goes back. A change makes the shaders again, which is a pause of a fifth to half a second. A pack reads them as it reads the engine's other constants (`RELIEF`, `RELIEF_DEPTH`, `WALL_DARKNESS`, `SCENE_LIFT`), and may not define a name of the same spelling. A choice's constant is the place of the word in its list from 0 (`relief` is `RELIEF`), and each word has a constant of its own, so that a shader says what it means: `RELIEF == RELIEF_FLAT` (the engine's words are `RELIEF_FLAT`, `RELIEF_PRISMS` and `RELIEF_PARALLAX_OCCLUSION`).

**Choices are kept in a file next to each pack.** What a player chooses of the settings of a shader is its own, the engine's settings and the pack's, and is written when it changes to a file in the same folder of packs as the pack's folder, called after the folder and `.settings.json`: the pack in `shaderpacks/cel` keeps its choices in `shaderpacks/cel.settings.json`. The game's own shader, which has no folder, keeps them in `shaderpacks/default.settings.json` in the game data folder (so a pack's folder may not be called `default`). The file is one JSON object, a name and the value chosen, a number, `true` or `false`, or the text of a word:

```json
{ "relief": "flat", "scene_lift": 1.4 }
```

Only what is chosen is in it, and a shader with nothing chosen has no file; the game reads it when it starts and writes it whenever a value changes. A name that is not a setting of the shader, or a value that is not one of its setting, is a warning on the error stream and is left out. A file that is not a JSON object is a warning, and is not written over until it is mended or removed. A graphics card that refuses a pack with the choices made for it takes the pack off and forgets those choices (and so its file). `--screenshot` and the other headless runs read the files and never write them.

**A pack's own settings** are declared in `pack.json`, never scraped from the shader's text:

```json
"settings": {
  "steps": {
    "type": "integer",
    "description": "how many steps the sun's light has",
    "default": 3,
    "minimum": 2,
    "maximum": 8
  },
  "outline": { "type": "switch", "default": true }
},
"engine_settings": { "scene_lift": 1.0 }
```

- The key is the name: lower case letters, digits and underscores, beginning with a letter, and not the name of an engine setting.
- `type` is `float` (a number), `integer` (a whole number), `switch` (true or false) or `choice` (one of some words). A number has a `minimum`, a `maximum` and a `default` between them; a switch has a `default`; a choice has `options`, a list of at least two words (each lower case letters, digits and underscores, beginning with a letter, none twice) and a `default` that is one of them. `description` is free text that `.shadersetting NAME` shows. Any other key, or one for another type (`options` of a number), is an error.
- The lighting reads it as the constant `SETTING_` and the name in upper case: `steps` is `SETTING_STEPS`, an `i32` (a float is an `f32`, a switch a `bool`). A choice is an `i32` too, the place of its word from 0, and each word is a constant of its own, `SETTING_` and the name and the word in upper case: `"quality": { "type": "choice", "options": ["low", "high"], "default": "low" }` makes `SETTING_QUALITY`, `SETTING_QUALITY_LOW` (0) and `SETTING_QUALITY_HIGH` (1), and `SETTING_QUALITY == SETTING_QUALITY_HIGH` reads as it says. Two settings that make one constant (a setting `quality_high` beside that one) are refused. A pack that reads a constant it did not declare is refused with the line.
- `.shadersetting` lists the engine's settings, with the pack's defaults, and then the pack's own, while the pack is in use. What a player chooses is for the shader in use alone, engine settings too: the pack has its own choices (and its file, above), and the game's own shader has its own. A change makes the shaders again, as for the engine's.
- `engine_settings` sets what a setting of the engine's is until the player says: `.shadersetting NAME default` goes back to the pack's.

See `engine/renderer/example_shader_packs/cel`, which has `steps`.

## Checking and using a pack

```
voxunami check-shader-pack shaderpacks/cel      # one pack
voxunami check-shader-pack shaderpacks          # every pack in a folder
```

Packs are read from the `shaderpacks` folder of the game data folder (next to your identity: `%APPDATA%\voxunami` on Windows, `~/Library/Application Support/voxunami` on macOS, `$XDG_DATA_HOME/voxunami` or `~/.local/share/voxunami` elsewhere) and from the folder `--shader-packs DIRECTORY` names. A pack that is no good is a warning on the error stream and the others are kept.

```
voxunami cave --shader-packs my_packs --shader-pack cel
```

While playing, `.shader` says which pack is in use and what there is, `.shader NAME` chooses one and `.shader default` goes back to the game's own. A pack the graphics card refuses is taken off again and you are told.

## Examples

`engine/renderer/example_shader_packs` has three (they are checked by tests, and drawn on every graphics card the tests run on):

- `cel`: the sun's light in steps (three, or what the pack's setting `steps` is) instead of a smooth fall-off:

  ```wgsl
  fn pack_shade(s: Surface) -> vec3<f32> {
      let sky = frame.sun_light.y * s.sky_visible + frame.heavens.w * (1.0 - s.above_horizon) * s.uncovered;
      let shaded = s.side * (s.lamp + sky * (1.0 - s.lamp));
      let cosine = max(dot(s.normal, frame.sun_direction.xyz), 0.0) * s.sun_visible;
      let steps = f32(SETTING_STEPS);
      let stepped = floor(cosine * steps + 0.5) / steps;
      let sun = frame.sun_light.x * stepped;
      return s.albedo * (shaded + sun * (1.0 - shaded));
  }
  ```

- `flat`: the default's lighting (a copy, checked against it by a test) with `"engine_settings": { "relief": "flat" }`: the one example that changes only a setting. (The game's own shader is the same with `.shadersetting relief flat`.)
- `shadowless`: a sun that no terrain hides (only the horizon of the planet does): it does not look at `sun_visible`, `sky_visible` or `uncovered`.

## Not yet

A pack cannot change the sky, the fog or the stars, move vertices (waving leaves), know the time, or draw a pass after the world: those are hooks to add when a pack needs them (`docs/specs/shader-pack-spec.md`, section 6).
