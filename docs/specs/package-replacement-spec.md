# Replacing the package while the world runs: what a recording does

**Accepted by the owner on 2026-11-16 (option A, and all four answers yes) and built: the client half in 38e3cb1, the server half in the phases of `hot-reload-plan.md` (the server sends it: `Server::replace_package`, phase 4).** Written 2026-11-15 on the branch `graphics-world-painting`, as step 4 of `hot-reload-plan.md`; what follows is the proposal as it was written, and `hot-reload-plan.md` says what was built and what was not. Marks: **[read]** read in the code or a document, **[believed]** my reasoning, not run, **[measured]** run.

## The problem

A client's whole view of the world comes from the server's message stream, and a recording is that stream, so *replay equals live* is the property every other part leans on **[read: `protocol-and-replay.md`]**. What the stream carries once, at the start, and never again, is the **package** (how every block looks and behaves) and the **world description** (the tables that number blocks and generators): both are in the `Welcome` **[read: `server_message.rs`, `Welcome`]**. A client that gets a second `Welcome` calls it an error **[read: `client_world.rs`, "a second welcome"]**.

Step 5 would let the server read block data again (`/reload`) while players are in the world. After that the package a client holds is not the package it was welcomed with. A recording that does not say so replays the old look, so replay no longer equals live: **a recording that cannot show a reload is a recording that lies about it.** This document chooses how the stream says it.

Three facts that decide the options:

1. **The server does not know who records.** A recording is made by the client, by wrapping its connection **[read: `protocol-and-replay.md`: "a recording is just the frames a client received"]**. So the option in `hot-reload-plan.md` "refuse a reload while a recording is made" can only be decided by a process that is both server and recorder (singleplayer); a hosted server cannot know. I wrote that option too quickly; it is **not** an option for a server, and I drop it.
2. **What a reload can change is smaller than a whole `Welcome`.** Blocks keep their numbers by name (the rule of step 5, from the hard rule that saved data keeps only namespaced ids): a reload **adds** entries to the block registry (never renumbers, never removes) and **replaces definitions**. Channels, generators and levels do not change **[believed]**. Registries are dense and append-only on both sides (`Registry::register`, `register_missing`) **[read]**; the client's translator keeps, for each registry, how many entries the source lists and a map from the source's ids to the client's, which would have to grow by the same entries (no method for that exists) **[read: `translation.rs`]**.
3. **Rejoining is slow.** Loading a world to a view radius of 12 took 8.8 to 13.8 s in the headless runs of this session's live checks (RTX 3090) **[measured, three runs; those runs mesh synchronously, a window streams, so a rejoin in a window would be less, by an amount I have not measured]**. A reload that costs that every time is a poor reload.

## The options

| | What happens at a reload | Protocol | Replay file | The player's view | Cost to build |
|---|---|---|---|---|---|
| **A. A message that replaces the package** (recommended) | the server sends `PackageReplaced` (the appended registry entries and the new package) first in the tick; every client applies it | **version 12**, one new kind | none: it is a frame like the others | kept; only what the change touches is made again | a client that can swap its block table and registry, and mesh again what changed |
| **B. A second `Welcome` is legal** | the server sends a whole `Welcome` and the client forgets everything and starts again | version 12, a rule changed, no new kind | none | thrown away and rebuilt (the 9 to 14 s above, as a ceiling) | the client's join code made re-entrant on a live session |
| **C. Disconnect and rejoin** | the server closes with a reason; the client joins again; the recorder starts a new file | none | one file per join (the recorder opens the next) | thrown away and rebuilt, and the player logs in again (two ticks) | the recorder and the client taught to rejoin; nothing in the protocol |
| **D. No live reload** | a reload is written to disk and applies at the next start | none | none | none | nothing, and no step 5 |
| **E. The client is not told** | the server swaps its content; clients keep the old package | none | none | **wrong** until they rejoin | nothing, and it breaks replay equals live |

**My recommendation: A**, confidence **medium-high** that a message beats the others, **medium** about its exact contents (they depend on the client's numbering, which I read but did not run). Reasons: C and B throw the view away at every reload, and a reload is something a modder does dozens of times in an hour; C also needs the recorder to learn about segments, work that A does not need, since a replay of A is the stream as it was; E is the lie the document starts from; D is fine as a first release but is not hot reloading. B is A with a bigger hammer: it would be the **fallback** if the client's incremental rebuild turns out to be much harder than I expect, because both put the same frame in the recording and the same rule in the client (a frame that replaces the package at a tick); only what is in the frame and what the client does with it differ.

## The message (option A)

`ServerMessage::PackageReplaced`, server to client, kind 16, protocol version 12:

| Field | Form |
|---|---|
| appended entries | a count of registries, then for each: the registry's kind (identifier), a count, and the identifiers appended to it, in order |
| package | bytes: the new package data, as `Welcome.package` is |

**Rules**

1. **Where.** The first message of a tick's stream for every joined client, before the interest change, so that the bricks and edits of the same tick, which may use a new block, are read with the new tables. The server swaps its own content at the start of the same tick, so what the stream says from this tick on is what the server did. A client that joins later gets the new package in its `Welcome` and is never sent this message.
2. **Same bytes for everyone.** The bytes of the message depend on nothing about the client (the property "streams are independent of which clients are watching what" holds).
3. **Appended only.** A new registry entry takes the next number, as it would on a fresh start with the same files. An identifier the old package had that the new one lacks stays in the registry (its block is drawn as missing content, as a block of a missing plugin is: the hard rule of `CLAUDE.md`). The client refuses a replacement that drops an identifier or lists one the registry does not have.
4. **The client applies it with the tick**, at `TickCompleted`, with the tick's other messages, so the view is never half a tick (the rule of the stream). It builds everything beside the old state first (the table, the translator, the palette) and swaps only if all of it is good.
5. **If it is not good, the client stops.** An unusable replacement (a package that cannot be decoded, a table the renderer cannot draw, a registry that does not match) is a protocol error like a bad `Welcome`: the client disconnects with the reason, and a replay ends the same way. The server checks a reload before it sends it, so this happens only for a bug, and a bug should be loud. (The alternative, keeping the old package and warning, would make a replay depend on what the recording player's program could draw, not on the stream alone.)
6. **What the client makes again.** The table (`BlockTable`) and the palette with its pictures; the meshes of the bricks that hold a block whose shape, opacity, transparency, cut-out or pictures changed, found by comparing the old and new table block by block; the textures that the client's own `--texture-pack` overrides wrote (they are written again over the new pictures, since the textures are made again). Colours alone change the palette and no mesh **[believed: meshes hold material numbers, not colours; to be checked in step 5]**. The first version may make all the meshes again and be measured; the comparison is the second.

## What the replay does

Nothing new. The replay file is frames, the format stays at version 1, and the player stands in for the server and hands the client the frames, so the client does what it did live **[read: `protocol-and-replay.md`, "The replay file"]**. A seek, which does not exist (replay from the start is the only way), would have to keep the package as of the tick it lands on when keyframes come: a note for that day. The version bump makes recordings of version 11 unreadable; the standing rule is that a program reads only its own version before a first release **[read]**.

## The tests, written before the code (on the fake server of `end_to_end_tests`)

1. **The codec:** the layout pinned byte for byte; truncated, trailing, damaged and random frames are errors and never panics (as for every message).
2. **Replay equals live across a replacement:** a recording of a client through a scenario with a replacement at tick N, played back, gives the same events and the same view after **every** tick as the live client, including the block table (colours, shapes, opacity) at each tick.
3. **A tick is whole:** before `TickCompleted` of tick N the client still has the old table; after, the new one; a brick of tick N that uses a new block is read with the new tables.
4. **Appended only:** a replacement that drops an identifier, or lists an entry the registry lacks, or lists one twice, makes the client disconnect with a reason that names it; a replay of that stream ends the same way.
5. **A late joiner:** a client that joins after a replacement gets the new package in its `Welcome` and no `PackageReplaced`; its view equals that of a client that was there for it.
6. **The server's streams stay byte-identical** for identical inputs and independent of what the clients watch, with a replacement in them.
7. **Step 5's own:** the package after a live replacement equals the package of a fresh start with the same files, to the byte; the world saved after it equals the world saved after a fresh start.

## What I did not check

- Whether the client's meshers can drop only the bricks of a changed block (they share the table through `Arc` in six places in `session.rs`, so a swap needs one place to hold it); that is for step 5 to read first.
- How a game's own server rules read block properties (`DENSITY`, flags such as which blocks a player can place) after a swap in the middle of play; the server swaps between ticks, which should be safe, but I have not read every reader.
- Whether `Registry` and `SourceTranslator` can grow without a hole for bricks translated before the growth (the bricks the client holds are in the client's numbering, which only grows, so they should be right; a test of case 3 above is where it would show).

## Questions for the owner

1. **A, or B as the first version?** (A is my recommendation; B first would be smaller and would cost a reload's rebuild time.)
2. **A bad replacement stops the client with a reason** (rule 5), or keeps the old package with a warning (the replay then depends on the player's program)? I recommend stopping.
3. **First version remakes every mesh** and is measured, then the comparison block by block? I recommend that order.
4. **Is a block that disappears from the data kept as missing content** (rule 3, from the hard rule), with the reload saying which? I believe the rule already decides it.
