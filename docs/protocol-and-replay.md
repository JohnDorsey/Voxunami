# The message stream and replays (protocol version 2)

Implemented by `engine/protocol` (messages), `engine/transport` (carrying frames), `engine/server`, `engine/client`
and `engine/replay`; scenarios that run them together are in `engine/end_to_end_tests`.

The rule everything follows: **a client's view of the world comes only from the server's message stream.** Singleplayer
runs an integrated server behind a loopback connection, so it is the same code path as multiplayer. It also makes a
recording of the stream a complete replay: `replay` writes what a client received to a file, and plays a file back by
standing in for the server, so the unchanged client code rebuilds the views tick by tick.

All integers are little-endian unless stated; "varint" is unsigned LEB128, "zigzag" a signed varint, "bytes" a varint
length then that many bytes, a "string" is bytes of UTF-8, an "identifier" is a string holding `namespace:name`
(see `docs/world-file-format.md` for the same primitives).

## Frames and connections

A *frame* is one encoded message. A `Connection` moves frames, in order and without loss, between two ends, never
blocks, and reports a closed peer only after delivering every frame the peer sent. Two exist: an in-process
`loopback`, and `tcp`. Frames are always encoded bytes, even in loopback, so singleplayer exercises the real codec.

**TCP framing:** each frame is its length (`u32`, little-endian) then its bytes; nothing else. A length over 16 MiB is a
protocol violation and closes the connection. There is no encryption, no authentication and no handshake of its own:
use it on networks you trust. The protocol above it refuses what is malformed, drops connections that never say hello
(counted in ticks), and limits how much of the world one client may watch; it cannot tell *who* is connecting.

Connections never block: `send` queues bytes and sends what the network takes, `receive` collects what has arrived, and
both happen inside those calls, so a program that polls once per tick keeps them moving. A peer that falls so far
behind that 64 MiB are queued for it is treated as gone, so one slow client cannot use unbounded memory or stall the
rest. Reading always comes before writing: writing to a peer that has already gone makes the network stack reset the
connection, which can throw away what that peer sent and we had not yet read. Closing gives queued bytes (a farewell
message) a moment to leave and waits briefly for the peer to finish too, for the same reason.

**Playing over a network:** `voxunami GAME --host [ADDRESS]` runs the server in the game's process and lets others
join, `voxunami serve GAME` runs a dedicated server (saving every `--autosave` seconds and on Ctrl-C), and
`voxunami GAME --join HOST[:PORT]` plays on someone else's. The default port is 24816. Recording works on any of them,
since a recording is just the frames a client received.

## The conversation

1. The client sends `Hello` (protocol version, game id). The server refuses another version or game with
   `Disconnected` and closes the connection. A client that never says hello is dropped after a number of *ticks*
   (`ServerConfig::handshake_timeout_ticks`); the server never reads a clock.
2. The server answers `Welcome`: protocol version, game id, the id it gave the client, ticks per second, the last tick
   completed before the client joined, and a *world description*: a world file (`WorldData::describe`) with seed,
   registry tables, channel layout and, for each level, its name and generator setup, but no assignments, bricks or
   plugin sections. Levels are numbered by their order in it. The client loads
   it like a save (placeholders for content it lacks) and keeps the translator for everything that follows.
3. The client sends `SetInterest` with a level and a box of bricks. The server answers, at the end of its next tick, with
   `InterestChanged` then `BrickContents` for each *edited* brick in the newly watched part. A client watches one box of
   one level at a time; asking for the same box of another level makes all of it new, and the client forgets what it held
   of the first. Unedited terrain is never
   sent: the client generates it from the seed, using the generator the assignments name.
4. Every tick, each joined client is sent, in this order: the interest change and bricks gained (if it asked), the edits
   inside what it already watched, the game messages meant for it, and `TickCompleted`. The client applies a tick's
   messages together, when its `TickCompleted` arrives, so its view is never half a tick.
5. `Goodbye` (or just closing) ends it. Ticks are numbered consecutively in each stream; the client treats a gap as an
   error.

## Messages

Client to server (first byte is the kind):

| Kind | Message | Body |
|---|---|---|
| 1 | `Hello` | protocol version (`u32`), game (identifier) |
| 2 | `SetInterest` | level (varint), box: min x, y, z then max x, y, z (zigzag) |
| 3 | `GameMessage` | kind (identifier), payload (bytes) |
| 4 | `Goodbye` | none |
| 5 | `Command` | text (string, at most 1024 bytes): a line the player typed, such as `/level go cellar` |

Server to client:

| Kind | Message | Body |
|---|---|---|
| 1 | `Welcome` | protocol version (`u32`), game (identifier), client id (`u32`), ticks per second (`u32`), tick (`u64`), world description (bytes) |
| 2 | `InterestChanged` | level (varint), box, then count and per assignment: cube origin x, y, z (zigzag), cube size level (`u8`), generator file id plus one (varint, 0 = none) |
| 3 | `BrickContents` | level (varint), brick x, y, z (zigzag), the brick in its stored form (bytes; see the world file format) |
| 4 | `VoxelEdits` | level (varint), brick x, y, z, digest of the result (`u64`), count and per change: voxel index, channel number, value (varints) |
| 5 | `GameMessage` | kind (identifier), payload (bytes) |
| 6 | `TickCompleted` | tick (`u64`) |
| 7 | `Disconnected` | reason (string) |
| 8 | `CommandReply` | text (string, at most 1024 bytes): the answer to a command this client sent |

Brick contents, channel numbers, content ids and generator ids use the *server's* numbering, which the world description's
tables define. The client translates (`SourceTranslator`): enabling a plugin that shifts every id on one side changes
nothing about what the stream means. **Game messages should name content by identifier text**, not by number, for the
same reason.

`GameMessage` is how games and plugins add their own messages in either direction (entities, chat, inventory): an
identifier saying whose message it is, and bytes only that game reads. The engine never interprets them, and a client
that lacks the plugin that sends one ignores it.

## What the server sends when

- **Newly watched part of a region** (the box minus the previously watched box, found without visiting the bricks both
  share): which generator made each assignment cube touching it, and the contents of every brick there that has ever
  been edited.
- **A brick the client already holds** and that changes this tick: `VoxelEdits` if the brick was already edited before
  this tick; the whole `BrickContents` if this tick is its first edit (the client's copy is what it generated itself,
  and a change list can only be applied to what the server has). Bricks sent in full this tick are not sent edits too.
- **Digest check:** `VoxelEdits` carries the digest of the brick after the changes. A client whose numbering matches the
  server's (`SourceTranslator::is_identity`) checks it and treats a mismatch as being out of sync. This is what notices a
  generator that changed under a world, or any bug in the pipeline, instead of showing a wrong picture. A client with
  different numbering cannot compare digests and does not.
- **Limits:** a client may watch at most `ServerConfig::maximum_watched_bricks`; asking for more disconnects it. So does
  asking to watch a level the world does not have.
- **Commands:** a command reaches the game's `GameLogic::command_received` with the client's other messages, in the order
  sent. The game answers with `ServerWorld::reply`; replies go only to the sender, in the tick's stream after the game
  messages and before `TickCompleted`. A command longer than 1024 bytes is a protocol violation.

## What the tests establish

- Every message survives encoding; the layouts above are pinned by byte-exact tests; truncated, trailing, damaged and
  random frames are errors, never panics.
- The server's streams are byte-identical for identical inputs, and independent of which clients are watching what.
- A client of any plugin set sees, in the region it watches, exactly what the server has: edited bricks, generator
  assignments, and (checked at the end of each run) all generated terrain. Checked after every tick of random play with
  three players, one with a differently numbered game.
- **Replay equals live:** a recording of a client's stream, played back, gives the same events and the same view after
  every tick as the live client had, and the same picture as the server. A replay with fewer plugins keeps edited
  bricks and shows missing terrain as missing. Recordings of identical sessions are byte-identical, and the recording
  of a standard 40-tick, two-player session has a pinned length and checksum that Windows and Linux both reproduce.
- A recording cut short plays its complete ticks and reports it was interrupted; a damaged one plays up to the damage.

## The replay file

```
magic "VOXREPLY" (8 bytes) | format version (u32, currently 1)
then until the end of the file, per frame:
    frame length (varint) | CRC-32 of the frame (u32) | the frame
```

A frame is one server message exactly as the client received it. The first is the `Welcome`, which names the game (so a
launcher can start the right game to play the file) and describes the world. Recording starts when the connection does
(wrap it before the handshake), is flushed at the end of every tick, and never rewrites anything, so a crash loses at
most the tick in progress. If writing fails the game carries on and the error is kept. A recording is valid while the
terrain generators it names keep producing what they produced when it was made, which the generator-version rule
guarantees.

## Known limits and future work

- Servers do not tell clients how much of the world they may watch (`maximum_watched_bricks`), so a client with a larger
  view radius than its server expects is refused with a reason rather than adjusted. Passwords, encryption and
  per-client bandwidth limits do not exist.
- Seeking inside a replay means replaying from the start; periodic keyframes (a snapshot of the view) would fix that.
- A client that finds itself out of sync can only reconnect; a request-and-resend for single bricks would repair it.
- Finding who an edit concerns scans all clients each tick; a spatial index over watched regions takes this from dozens
  of clients to thousands.
- Only full-detail bricks are sent. Distant terrain (LOD) will need interest regions with a level of detail, and a new
  protocol version.
- A recorder that wants to see the whole action (not one player's view) connects as a spectator with a large region.
- Entities, players and structures are not engine concepts yet: games replicate them with game messages until a second
  game shows what is common.
