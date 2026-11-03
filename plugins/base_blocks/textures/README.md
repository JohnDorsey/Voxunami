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

Not built yet: the editing canvas and tools, renaming, saving tile images, the quit dialog, and importing a picture.

Tests for the logic: `python -m unittest discover -s tools/virtual_atlas` from the repository root.
