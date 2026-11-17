# The VirtualAtlas bridge

How a program that shows tiles (a game, a viewer, a level editor) lets VirtualAtlas paint on them while they are on its screen. **Nothing here is about one program**: a
program that speaks this is a host, and VirtualAtlas works with it without being changed. The version is **1**.

The idea: the host's window is the view and VirtualAtlas is the tool. The host says which tile its pointer is over and at which pixel, and which mouse buttons are down;
VirtualAtlas paints there with the tool, colours and sizes chosen in its own window, as if the pointer were over its own canvas, and sends back each tile's pixels as they
change so that the host can show them at once. Saving (Ctrl+S) is VirtualAtlas's own, to the PNG files, as always.

## Starting

```
python VirtualAtlas.py --bridge [PORT] [--group NAME]
```

VirtualAtlas listens on `127.0.0.1` (this machine only) at PORT (47625 if none is given) and serves one host at a time; a second connection is closed at once. `--group` makes it
use only tiles that the host says are of that group. The host connects when its user asks (a command, a menu).

## The line

TCP, UTF-8, **one JSON object a line** (`\n` ends it; blank lines are ignored). Every object has a text `"type"`. A line of more than 4 MiB is thrown away. A message of a type this
version does not know is ignored, so a newer side can talk to an older one. A line that is no message is answered by `{"type":"error","text":...}` and nothing else changes.

## What the host says

**`hello`** (first): `{"type":"hello","protocol":1,"host":"Name of the program"}`. Another protocol is refused with an `error`; nothing but `hello` is heard before it.

**`tiles`**: the tiles the host has, said again whenever they change (it ends the stroke in progress):

```json
{"type":"tiles","tiles":[
  {"id":"0:3","group":"base_blocks","files":["stone_top","stone"],"size":16}
]}
```

- `id`: the host's own name for the tile, opaque to VirtualAtlas, used in every other message.
- `files`: the names (without `.png`) of the files in VirtualAtlas's folder that this tile may be, in order; VirtualAtlas takes **the first that exists** and can be drawn on. This is
  the only thing a host must know of VirtualAtlas's folder, and the host's own naming stays the host's: a host may list several names for one tile.
- `size`: the tile's width and height in pixels. A tile whose size is not the atlas's tile size is not used.
- `group` (optional): what the host groups it with (a namespace, a pack); with `--group` only that group is used.

**`pointer`**, as often as the pointer changes (and once when it starts):

```json
{"type":"pointer","tile":"0:3","x":22,"y":9,"left":true,"right":false,"segment":4}
```

- `tile`: the id of the tile under the pointer, or `null` if it is over nothing that can be painted (the sky, a face with a flat colour). An id VirtualAtlas has no file for (it was in `missing`) is the same as `null`.
- `x`, `y`: the pointer's pixel on the tile **repeated in every direction**: the pixel of the tile is `x` and `y` modulo its size, and either may be negative or past the size.
  So a stroke that runs off one side of the tile and on, over the join onto the next face that has the same tile, goes on in the numbers (16, 17, ... after 15) and VirtualAtlas
  draws it with wrap-assist (the stroke comes back in on the other side). The numbers only go on within one `segment`.
- `left`, `right`: whether the buttons are down. Left paints with the primary colour, right with the secondary.
- `segment`: a number that changes whenever `x` and `y` stop going on: the surface turned, another face, another plane, another tile or another turn of it. A new segment while a
  button is down ends the stroke (where it last was) and begins another at the new place. The first segment may be any number.

A stroke is: a pointer with a button down; more pointers with the same tile, segment and button; and one without the button (or with another tile, segment or `null`). Each stroke is
one step of VirtualAtlas's undo history. Pointers with no button down only make VirtualAtlas show that tile.

## What VirtualAtlas says

**`welcome`** (answer to `hello`): `{"type":"welcome","protocol":1,"tool":"VirtualAtlas","tile_size":16}`.

**`known`** (answer to `tiles`): `{"type":"known","ids":["0:3"],"missing":["1:0"]}`: which of the host's tiles have a file in the atlas, and which do not (no file, another size, another
group, a file that is not in the atlas yet, a file that cannot be read). A host may show the player what is missing.

**`tile`**: `{"type":"tile","id":"0:3","size":16,"pixels":"<base64>"}`: the tile's pixels as they are now, `size` by `size` pixels of four bytes each (red, green, blue, alpha), row by
row from the top, base64 (RFC 4648, with padding). Sent **when a tile of the host's changed** since it was last sent (or since `known`, which counts as sent): while a stroke is
going on (each time the pointer has moved, at most once a frame) and when it ends; after an undo or redo, or any edit made in VirtualAtlas's own window. A tile that has not been
painted on is never sent, so the host's pictures are not replaced by the files until something is painted. The host writes the pixels where the tile is drawn, on its own screen.

**`error`**: `{"type":"error","text":"..."}`: a message was refused. The connection stays.

## What a host is expected to do

- Connect when asked, say `hello`, then `tiles`, and wait for `welcome` and `known` (they come back within a frame).
- Say `pointer` when the pointer changes; keep `segment` as above; say `null` when the pointer leaves everything paintable or the host stops (then close).
- Show the pixels of every `tile` it hears; it may keep other data of its own for the tile (a shading map, say), which are not VirtualAtlas's.
- Never wait for VirtualAtlas: it is another process and may be slow, busy with a dialog, or gone. A closed connection is the end of the session; VirtualAtlas ends a stroke it was
  in the middle of, and keeps it.

VirtualAtlas tells its user what is happening in its status line (waiting, connected, gone). With a dialog open it holds the pointers back.

## Not in version 1

Companion pictures (a host's normal or specular maps as tiles of their own, to be painted: they are separate tiles with their own ids and files, and nothing prevents a host from
listing them today); a brush guide drawn by the host; streaming the host's view into VirtualAtlas's window; several hosts; a host asking for a tile's pixels (VirtualAtlas sends them
when they change, never on request); a stroke that goes from one tile onto another tile (it ends at the edge of the first).

## Tests

`tools/virtual_atlas/test_virtual_atlas.py` tests VirtualAtlas's side (the editor's outside pointer, the line, the controller, the socket and the window). A host tests its own side
against a few lines of socket; the Voxunami game's tests are `engine/game_shell/src/paint_bridge/tests.rs`, `games/voxunami_cave/tests/painting.rs` and, with the real program as a
process, `games/voxunami_cave/tests/painting_with_virtual_atlas.rs`.
