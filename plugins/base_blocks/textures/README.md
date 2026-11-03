# Block textures

Texture images for the blocks of the `base_blocks` plugin. Each tile is its own PNG file in this folder.

Blocks that exist today (ids are `base_blocks:<name>`): stone, dirt, grass, lamp, wood, fire, great_river_water.

The file naming, size, face layout and how the engine loads these are not decided yet; this folder only holds the assets.

## VirtualAtlas.py

An editor that shows all the tile files as one atlas, although no atlas image exists. It is one file and needs
`pip install pygame pillow numpy`. Run it with `python VirtualAtlas.py` from this folder. It is being built in
phases; what works so far:

- The left half is the atlas: click a tile to select it, Ctrl+click to add or remove one, drag to rearrange.
  Dragging a tile onto another swaps them; dragging several selected tiles moves them all by the same offset.
  Mouse wheel scrolls, Ctrl+wheel zooms, Shift+wheel scrolls sideways. Hover a tile to see its file name.
- Below the atlas are the tile files that are not in the atlas yet. Drag one into an empty place to add it.
  Drag a tile from the atlas back down to take it out (only a tile that is exactly its file can leave).
- The arrangement is stored in `atlas.json` and is saved at once after every change:
  `tile_size`, `coordinates_to_names` (`"(column, row)": "file.png"`), and `coordinates_to_data`, which holds the
  pixels (base64 of the RGBA bytes) of a tile that has no PNG file yet. The program refuses to start if one file
  is listed at two places or two files at one place.
- Only PNG files whose size is the tile size are used. A listed file that is missing or has another size shows
  an error symbol; an unlisted file of another size is ignored.
- With no `atlas.json`, the empty atlas asks for the tile size (16 until you say otherwise) and creates the file.

- The right half edits the selected tiles. With several selected they appear as one canvas in their atlas
  arrangement (empty places are hatched and cannot be drawn on), and a stroke can cross from tile to tile.
  Mouse wheel zooms, middle-drag pans, a grid shows when zoomed in. The left button draws with the primary color,
  the right button with the secondary color.
- Tools: pencil, eraser, line, horizontal line, vertical line, horizontal and vertical parallelogram, noise
  brush, air brush, select, eye dropper, fill bucket (exact color match, side by side pixels only). `[` and `]`
  resize: both axes together, Shift for the width, Alt for the height; the lines and parallelograms have one
  width, and the noise brush and air brush have one diameter.
- Noise brush: every pixel of the round or square brush area becomes the color with the chance set by the Density
  slider, rolled again for every pixel the pointer moves over; Opacity below 100% blends each hit over what is
  there, so pixels hit several times in one stroke build up. Air brush: a soft round brush; Hardness is how far
  from the center it stays at full strength, Opacity its strongest coverage; passing over a pixel again within
  one stroke adds nothing.
- Select: drag a rectangle (a click without dragging deselects). Ctrl+A selects the whole canvas, Ctrl+C
  copies, Ctrl+X cuts, Delete clears to transparent. Ctrl+V pastes the picture on the system clipboard (or, if
  that has none, what was last copied here) as a floating picture at the top left of the selection, or of the
  visible canvas. Drag it to move it, Enter places it (over what is there, alpha-over), Esc throws it away;
  choosing another tool or clicking elsewhere also places it. Copying with nothing selected copies nothing. The
  system clipboard is supported on Windows only (PNG, with transparency, and a bitmap); elsewhere copy and paste
  work inside the program.
- Colors: a picker with hue, saturation, value and alpha, a hex field (`#RRGGBB` or `#RRGGBBAA`), the two
  drawing colors, and the 16 most recently used colors (click one to use it; right-click for the secondary).
- Ctrl+Z undoes, Ctrl+Y or Ctrl+Shift+Z redoes (the history is per tile and kept while you switch tiles).
  Ctrl+S saves the shown tiles that changed, Ctrl+Shift+S saves every changed tile. A tile with unsaved changes
  has an orange dot in the atlas, and `*` after its name. Until the quit dialog exists, the window will not close
  while a tile that has a file has unsaved changes. A tile that has no file yet keeps its pixels in `atlas.json`
  after every edit.

Not built yet: renaming, the quit dialog, and importing a picture.

Tests for the logic: `python -m unittest discover -s tools/virtual_atlas` from the repository root.
