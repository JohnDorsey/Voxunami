# Phase 27 specification: entities, items and inventories

**Status: accepted 2026-10-18 with the default answers to all five questions (pause after 27d; players become entities now with a persistent player key; Cave creative by default with `/gamemode survival` per world; no mob; orientation a quaternion). Being built (27a to 27e done: the keypair identity).** It makes concrete requirement 8, 15 and 16 of `plan.md` ("Requirements pulled
forward") and the "Entities and items (Phase 27)" improvement, after option B of `plugin-architecture-review.md` W7 (server-owned entities,
typed components from the property machinery, no ECS). What I say about today's code was read from the code (verified); the design is
my proposal (believed). It is the biggest phase so far, so section 4 splits it at a point where you can stop and look.

## 1. The problem, as it is in the code

- **There are no entities.** Players are poses in a `PoseBook` and announced to everyone in one `Players` game message; 2048's moving blocks are
  state inside its plugin, shown to clients as voxel edits; Eels' eels are voxel patterns; Cave's walkers are bodies inside its movement
  plugin. Nothing has an id the engine knows, and `ServerMessage` has no entity messages.
- **Players do not persist.** A `ClientId` is a number the server gives a connection, and nothing about a player is saved (the `PoseBook` is
  not a saved section). So nothing a player owns, an inventory first of all, can outlive a connection.
- **There is no saved counter for ids.** `GridId` has a reserved range for allocated ids (`FIRST_ALLOCATED`) but nothing allocates or saves
  them yet (that is Phase 32). Entity ids need one now.
- **Items do not exist.** Cave's placeable blocks are a hard-coded array of six identifiers (`PLACEABLE_BLOCKS`) chosen by number keys; breaking a
  block gives nothing; placing costs nothing.
- The renderer draws every other player as the same two-voxel figure in one of 16 colours (`set_avatars`); it has no way to draw anything else.

## 2. Design

### 2.1 An entity

`Entity { id, kind, pose, components }`, owned and simulated by the server only.

- **`EntityId`**: a `u64` from a counter saved with the world, never reused (a replay and a reload agree on ids).
- **`kind`**: a content identifier (`voxunami_cave:item`) in a new registry kind `engine:entity_kind`, so a saved file holds the string.
- **`pose`**: `{ grid: GridId, position: WorldPosition, orientation }`. The frame is a `GridId` (a level's root grid is `GridId::root(level)`), so a
  pose aboard a ship is already expressible; until Phase 32 only root grids exist, and a spawn into any other grid is refused. Orientation is a
  unit quaternion (four `f32`s, compared by bits): yaw and pitch for walkers, full rotation for ships and the octopus later.
- **`components`**: a bag of typed values by component identifier. A component is **a `Message` type** (Phase 25): it already has an identifier,
  an encoding, samples and the generated round-trip check, so there is one value vocabulary and not a fourth. A declaration says whether it is
  *replicated* (sent to clients) or server-only (an AI's memory).
- A kind declares which components its entities have and their defaults; setting one the kind lacks is an error naming both.
- Iteration is always by `EntityId`, never by hash order, so the plugin order and the machine never change a result.

### 2.2 What a plugin does

New registrar verbs (the size budget counts verbs, not kinds): `entity_kind(spec)` and `item(spec)` (content into registries). New context
calls: `spawn(kind, pose, components) -> EntityId`, `despawn(id)`, `entity(id)`, `get::<C>(id)`, `set::<C>(id, value)`, `set_pose`, and
`entities_near(grid, position, radius)` (by region, never a scan of everything: requirement 11). Engine events `EntitySpawned` and
`EntityDespawned`. A system declares the components it reads and writes (`reads_components`, `writes_components`), checked in debug builds
and by the start-up ambiguity check exactly as channels are.

### 2.3 Replication (protocol version 3)

New `ServerMessage`s, before `TickCompleted`: `EntitiesSpawned`, `EntitiesChanged` (pose, and the changed replicated components, each as
its encoded bytes), `EntitiesDespawned`. A client is told of the entities in the level it watches that are inside its watched region, and of
one leaving that region as a despawn. The server keeps, per client, the set it has told of and the last bytes sent, and diffs per tick, in id
order. Because they are stream messages, a recording holds them and a replay shows them with no server (the hard rule). The client keeps a
store of what it was told; client plugins read it and say how to draw each kind (section 2.6). Version 2 recordings should still decode (they have
no entity messages; I will prove it with a stored recording in 27c).

### 2.4 Saving

A new section **`engine:entities`**, version 1: the counter, then every entity sorted by id: kind (a file id into the `engine:entity_kind`
table), grid id, pose, **the world-space fallback pose if the grid is not a root grid** (requirement 16, in the format now, empty until Phase 32),
and its components as `(identifier, bytes)` pairs. The hard rule applies: an entity of a kind from a missing plugin, and a component a kind
does not know, are kept byte for byte and are neither simulated nor replicated. A world without the section loads with no entities.

### 2.5 Items and inventories

- **Items are registry content** (`engine:item`): an identifier, a maximum stack, and properties from the property machinery (Phase 24; for
  example `base_blocks:places_block`, the block an item puts down).
- **`ItemStack { item, count, components }`**: stacks carry their own components (a tool's wear) as `(identifier, bytes)` like entities do.
- **`Inventory`** is a component: a fixed number of slots, each empty or a stack. Operations (`insert`, `take`, `move`) are pure functions with
  property tests (counts are conserved; nothing exceeds a stack).
- Items on the ground are entities of a kind with an `ItemStack` component.

### 2.6 The first users (Cave), and how entities are drawn

- Cave declares an item for each placeable block (the six keys stay as the hotbar), a dropped-item kind, and a plugin that: spawns a drop where a
  block is broken, lets it fall (whole-voxel steps, so no floating-point functions at all), merges stacks that touch, and moves it into a player's
  inventory when they come within reach.
- A client plugin gets one more verb, `entity_appearance(kind, |entity| Appearance)`, and an `Appearance` is a coloured box (size and a block's
  colour), drawn through the renderer's avatar path generalised from "a figure per player" to "a box per entity". Real models come with the
  toolkit (Phase 30) and the octopus (Phase 35).

### 2.7 A player's identity: a keypair per install, no accounts (the owner's decision after review of 27d)

Each install of the game has an **Ed25519 keypair**, made the first time it is needed and kept in a file on that machine (`identity` in the
game's data directory; `--identity FILE` chooses another); the public key is the player's identity. There are no accounts and no names. To
connect, the client proves it holds the private key, so that someone who sees the public key, or a recording of a connection, cannot be that
player:

1. the client sends `Hello` (now with its public key, and so protocol version 4);
2. the server answers `Challenge` with 32 bytes it has not used before;
3. the client sends `Prove`: its signature of those bytes (with the game's identifier and its own key, so that the signature is good for one
   login to one game and nothing else);
4. the server verifies it, then sends `Welcome` as before. A wrong proof is refused with a reason. A key that is already connected is refused
   (one player, one connection), so a copy of a key file cannot take over a session in progress.

This makes a join take two server ticks instead of one. The server tells the game each client's key (`ServerWorld::player_key(client)`). In tests
the challenges come from a counter, so streams stay byte-identical; a real server draws them from the operating system's randomness.
Moving a player to another machine is copying the key file; losing it loses the player (the entity stays in the world, unclaimed). The file
is not encrypted: it is as safe as the rest of the player's home directory. Nothing is encrypted on the wire either; this is proof of who,
not secrecy.

### 2.7b Players become entities

A player is an entity of kind `engine:player` whose components are the key, the inventory, the last pose and which level they are in. The server
finds the saved player entity with the connecting key, or makes one; when the connection ends, the entity is kept in the save and stops being
replicated (it is kept out of interest until the key connects again). `PoseBook` and the `Players` message are replaced by the entity store; the
`PoseReport` the client sends stays (it is input). This touches all three games' presence, Cave's movement, and `session.rs`, and changes every
golden stream (they are repinned once, for that reason, as in Phase 25).

### 2.8 The prompt facility (requirement 15)

Engine game messages `engine:prompt` (a title and options, to one client) and `engine:prompt_answer`; `Context::prompt(client, title,
options) -> PromptId` and an event `PromptAnswered { client, prompt, choice }`; holding the entity meanwhile is the asking plugin's job. The
client shell shows the options as the HUD line and answers with the built-in `.choose N`. First user: the missing-ship case of Phase 32; in
this phase it is tested with a fake plugin. Phase 30 restyles it.

### 2.9 Not in this phase (still planned, and why later)

*Control targets* (a player's inputs going to an entity whose kind supplies the controller), the *`Brain` slot*, and *block data* (a chest's
contents in the grid). Each is first needed by the thing that uses it (the ship's command chair, Phase 32; the octopus, Phase 35; a chest) and
each is a small addition on top of this layer, so building them now would be designing for no user. They stay in the plan.

## 3. Risks

- **Bandwidth and cost.** One diff per client per tick over the entities in its interest. Measured, not guessed: a benchmark with 1000 item
  entities and 4 clients, before declaring it done (the bytes per tick and the milliseconds).
- **Determinism.** Ids from a saved counter, iteration by id, simulation without wall-clock time; the golden sessions grow an entity script
  (drops, a fall, a pickup) so Windows and Linux are compared.
- **Players-as-entities is the riskiest piece** (it replaces presence in every game). It comes after the engine layer has stood on its own.

## 4. Steps

- **27a** Tests first, then the model: the `entities` crate (ids and the counter, pose, orientation, the component bag, kinds, item stacks and
  inventory operations) with property tests, samples and the generated codec check; mutations.
- **27b** The registrar verbs, the context calls, the events, declared component access, and the ambiguity check.
- **27c** Protocol version 3 and the replication: server diffing, the client store, a replay test (live equals replay), and the benchmark.
- **27d** `engine:entities`: the section, the counter, dormant preservation, and tests that a world with entities, saved and loaded, is the
  same world, and that a missing plugin's entities survive byte for byte. **Pause for review here.**
- **27e** A keypair per install: the `identity` crate (generate, keep in a file, sign, verify), `Hello` with the public key, `Challenge` and `Prove`,
  protocol version 4, the server's refusal of a wrong proof or a key already connected, `ServerWorld::player_key`; the tests that join by hand
  take two steps; goldens repinned once if the stream changes (it does: the challenge is in it).
- **27f** Players become entities (the entity store replacing `PoseBook` and `Players`) in all three games; goldens repinned once.
- **27g** Items, stacks, inventories and Cave's drops, pickup and hotbar; the appearance verb and the box drawing.
- **27h** The prompt facility.
- **27i** Documentation (`plugin-api-spec.md`, `design-decisions.md`, `protocol-and-replay.md`, build log), gates on Windows and Linux.

## 5. Questions for the owner, answered

All five defaults: pause for review after 27d; players become entities now, with an identity that outlives a connection (the owner then chose a keypair per install and no accounts, section 2.7); Cave creative by default, survival per
world by `/gamemode`; no mob in this phase; orientation is a quaternion.
