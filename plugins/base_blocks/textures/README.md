# Block textures

Texture images for the blocks of the `base_blocks` plugin. Each tile is its own PNG file in this folder.

Blocks that exist today (ids are `base_blocks:<name>`): stone, dirt, grass, lamp, wood, fire, great_river_water.

How the game uses them is decided in `docs/specs/block-textures-spec.md`: the files are `name_top.png`, `name_side.png`, `name_bottom.png` or `name.png`
(a block names `name` in its file, and which faces differ follows from the files that exist), 16 by 16 here but any power of two from 8 to 256. Grass, dirt, stone,
still water, the log (`log_top`, `log_side`), and the dev docking port (`_top`, `_side`, `_bottom`) are used by blocks and embedded in the program; the others are not yet.
The editor below is for making them: the game reads the loose PNG files and ignores `atlas.json`.

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
- A thin sidebar of buttons is right of the canvas. Its checkbox button "W" turns wrap-assist on (the whole button is
  blue) or off (empty). Wrap-assist shows the canvas as a repeating grid of itself, with nothing drawn at the edges, and
  drawing is as if on every copy at once: the brush guide appears once per copy, and a stroke that leaves one side of
  the tile comes back in on the other (every stroke tool, the fill bucket, which fills around the edges, and the eye
  dropper, which picks from any copy). With several tiles shown, the whole canvas is what repeats. The select tool and a
  pasted picture work on the real canvas, counted from the copy you press in. Turning it on or off zooms to fit; it is
  not saved with the atlas.
- Tools (icons in one row; rest the pointer on one to see its name): pencil, eraser, line, horizontal line, vertical line, horizontal and vertical parallelogram, noise
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
  has an orange dot in the atlas, and `*` after its name. A tile that has no file yet keeps its pixels in
  `atlas.json` after every edit.
- Renaming: with one tile shown, click its name above the canvas and type. The `.png` stays beside the field, out
  of the way (a `.png` you type yourself is not doubled). Enter renames at once, on disk (the PNG file and
  `atlas.json`); Esc gives up; clicking elsewhere applies a name that is allowed and drops one that is not (the
  status line says why). A name is refused if it is not a valid file name or if any PNG file in the folder already
  has it, even one the program ignores; a file is never replaced. Renaming is not part of Ctrl+Z.
- New tile: right-click an empty place in the atlas and choose "New tile" (the place is outlined). It makes a fully
  transparent tile there named `new_tile.png` (numbered if that name is taken), selects it, and keeps its pixels in
  `atlas.json`: it has no PNG file until you save it (Ctrl+S).
- Copying: Ctrl+drag a tile, or the selected tiles, to an empty place. Each copy is named `<name> (copy).png`
  (`(copy 2)`, `(copy 3)` if that is taken), is saved as a PNG file at once with the pixels its tile shows now, and is
  selected afterwards; the originals stay. A copy cannot land on a tile (the places the dragged tiles are on count), at
  a negative place or in the unplaced section, and a group with an error tile cannot be copied. A tile you Ctrl+press
  that was not selected joins the group, as with Ctrl+click.
- Purging: right-click a tile in the atlas and choose "Purge tile..." - or "Purge N tiles..." when the tile is one of
  the selected ones, which purges all of them (the tiles are outlined in red). After a short warning, Purge deletes the
  PNG files and the entries in `atlas.json` (places, names and any pixels stored there), including unsaved changes;
  it cannot be undone and is not part of Ctrl+Z. It works on error tiles too (a missing file is fine), and the tiles
  do not become unplaced ones. Enter does not confirm; Esc or Cancel gives up. If a file cannot be moved or
  `atlas.json` cannot be written, every file comes back and nothing changes.
- Closing the window with unsaved tiles (including tiles that have no file yet) asks about each one: Save, Don't
  save or Cancel (Esc), with the current picture next to the one on disk. The box "Do this for all" gives the
  same answer to the tiles still to come. Enter saves. A tile with no file that is not saved stays in `atlas.json`.

- Importing: an empty atlas has the button "Import image as atlas". Choose the tile size first (the field next to
  "Create atlas.json"), then the button opens the system file dialog; a dialog shows the picture with the tile
  grid on it. If the picture does not divide evenly into tiles you choose between padding the partial tiles at the
  right and bottom edge with transparency (the start, nothing is lost) and cropping them away. Import makes the
  tiles at their places in the picture, named `tile_<row>_<column>.png` (a number is added if a PNG file in the
  folder already has the name). They are new tiles: their pixels are in `atlas.json`, and they become files when
  you save them (Ctrl+Shift+S, or when the program asks while closing). At most 2048 tiles can be made at once.

All of the planned features are built.

Tests for the logic: `python -m unittest discover -s tools/virtual_atlas` from the repository root.
