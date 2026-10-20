# Phase 28: the content package at the handshake

Status: built (28a to 28d). The owner answered section 5: inline now, the server wins, no support for old recordings (none exist and nothing is
released, so the old-stream code was deleted instead of kept), blocks only. One change from the design below: the package has no hash, since nothing
would read it (see `design-decisions.md`).

## 1. What is wrong today

A client learns the *numbering* of a world from `Welcome.world_description`, but it learns what a block *looks like* from the game
compiled into itself: `GameContent::block_table()`, `view_channels()` and `ambient_light()` (read in `Session::connect`). Two consequences:

- A client and a server that disagree about a block (a mod changed its colour, a plugin added a property) draw it the client's way, silently.
- A recording cannot be replayed by a build that has different definitions: the pictures depend on the player's installed content, not on
  what the server said. The plan (decision 3, `plan.md`) wants the stream to be self-describing.

What stays compiled in, because it cannot travel yet: terrain generators (the client regenerates terrain from the seed; bit-identical on
both ends), client plugins (code), and the registries' *kinds* (to load a description). Phase 28 moves only *data*.

## 2. Design

**What the package holds** (a single canonical byte string, section 3):
1. The property keys the blocks use (the schema: identifier, value kind, range, default, client-only flag).
2. Every block definition, in the world description's block order, *by block identifier* (colour, see-through, shapes, state colours, set
   properties by key). A block whose plugin the server lacks is not in it: the client already shows such blocks as unknown.
3. The identifiers of the channels the view reads (block, state, light), and the ambient light.

It holds no items, entity appearances or generators in this phase (question 5).

**Where it travels.** Protocol version 5: `Welcome` gains `package: Vec<u8>`, the package bytes, right after `world_description`. A recording
is the frames a client received, so it carries the package with no further work, and a replay needs no installed definitions.

**The client.** `Session::connect` decodes the package after loading the description and builds its `BlockTable` from it (looking blocks up
by identifier, so the translation to the server's numbering that the description already gives is reused). `view_channels` and
`ambient_light` come from it too. A damaged or inconsistent package is a connection error naming the problem, never a panic. The server's
package always wins over local definitions. `GameContent::block_table`, `view_channels` and `ambient_light` stay as the *server's* source of
the package, and leave the client's path (a client no longer calls them).

**Old recordings.** None are read: a program plays only its own protocol version.

**Hash.** The package has a SHA-256 over its bytes (`sha2` is already in the tree through `ed25519-dalek`), computed by both ends and
carried in `Welcome` so that caching by hash can be added later without another version bump. Caching is *not* built now (question 1).

## 3. The byte form

Canonical, so equal packages have equal bytes and one hash: varint counts, identifiers as length-prefixed text, definitions in block order,
properties of a definition in key order (the property-bag spec already makes this canonical), floats (colours) as `f32` little-endian bits.
The codec lives next to the types it encodes (`block_models` for definitions and the schema; `game_shell` for the package around them), and
every piece gets the generated round-trip check and a damaged-input test (truncation, trailing bytes, an unknown property key, a value out
of range, a duplicate identifier).

## 4. Steps

- **28a** The codecs and the package type, with tests first: canonical round trip for definitions, schema and package; stable hash (a pinned
  example); every damaged input is an error. No behaviour change.
- **28b** Protocol version 5 and the server sending it in `Welcome`; recordings carry it; a replay of a version 4 recording still works.
  Goldens repinned once (every `Welcome` grows by the package; the byte delta is checked against the package size).
- **28c** The client builds from the package. The proof: a client whose compiled-in content has *no* block definitions (or different
  ones) still draws exactly what the server describes, live and from a recording. `GameContent`'s three methods leave the client path.
- **28d** Documentation (`plan.md`, `design-decisions.md`, `protocol-and-replay.md`, `plugin-api-spec.md`, build log), gates on Windows and Linux.

## 5. Questions for the owner (my recommendation first)

1. **Inline now, cache later?** Inline in `Welcome` (recommended: a few KB for Cave's blocks, recordings carry it for free, no extra
   round trip or join tick). The alternative, naming it by hash and sending only on request, saves bandwidth only once packages hold assets,
   and needs a new request message, a cache file and a way for recordings to embed what was fetched. I would build it in the phase that adds
   assets. Confidence: high.
2. **A server package that disagrees with the client's compiled-in definitions: the server wins, silently?** (recommended; the point of the
   phase.) Alternative: refuse to join on a mismatch. That would break the "no plugin installed" goal for every modded server.
3. **Old recordings fall back to local content?** (recommended: yes, as 27f did for `Players`.) Alternative: refuse them, which strands
   every recording made so far.
4. **Scope: blocks, properties, view channels, ambient light only** (recommended). Items (identifier, stack size) and entity appearances stay
   in the game for now because nothing but Cave's client plugin reads them; Phase 30's UI will want item names and then they join the package.
5. **Is a generator still compiled into the client?** Yes, until WASM generators exist (a later phase); this phase does not change that.
