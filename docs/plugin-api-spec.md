# Phase 23 specification: plugins, grids, phases and events

**Status: proposed 2026-10-18 for the owner's review. Nothing in this document is built.** It turns the "Plugin shape revisions" and
"Requirements pulled forward" in `plan.md` into a concrete API for Phase 23 and says how each game is converted. The Rust below is a
*sketch* to show shape and size, not final signatures. Where the owner's answer changes something, the section says so.

Read with `plan.md` (the proposals), `plugin-architecture-review.md` (the reasons) and `design-decisions.md` (the rules that hold, above
all the hard rules in `CLAUDE.md`: determinism, string ids in saves, the client's view only from the stream).

## 1. What a plugin author sees, before and after

| | Today | After Phase 23 |
|---|---|---|
| Adding a rule to Cave | edit `CaveLogic` in about five places: the struct, `new`, `tick`, the calls in `set_block`/`light_changed`, `save_sections` | write a plugin; register a system, the events it listens to, its save section |
| What a rule is given | the whole `ServerWorld` and `(LevelIndex, VoxelCoordinate)` | a `GridView`: one grid, its own coordinates |
| How rules hear of edits | `set_block` calls each rule by hand | the engine delivers `BlocksChanged` to whoever subscribed |
| Order of rules in a tick | the order of lines in `CaveLogic::tick` | declared (`after`/`before`), checked at start-up |
| Commands | a `match` on the name in `command_received` | registered; `/help` can list them |
| A system's determinism | a rule of the project | a field: `Deterministic` or `Mind` |

## 2. Vocabulary

- **Plugin:** a value with an id (`voxunami_cave:fire`), the plugins it depends on, and one function, `register`, that is handed a
  `Registrar`. In this phase plugins are in-tree Rust; the same shape is what a loader will fill later.
- **Registrar:** what a plugin registers into. Its methods are the **verbs** of the API (section 3).
- **Phase:** a named stage of the tick (section 4). Systems run in a phase.
- **System:** a function run once per tick in a phase, with an id, ordering edges, a determinism class and declared channels.
- **Handler:** a function run when an event is delivered.
- **Event:** a named, typed message between plugins, delivered synchronously (section 5).
- **Grid, `GridId`, `GridView`:** section 6.

## 3. The registrar: nine verbs

```rust
pub trait Plugin: Send + Sync {
    fn id(&self) -> NamespacedId;                       // "voxunami_cave:fire"
    fn depends_on(&self) -> &[NamespacedId] { &[] }     // registered before this one
    fn register(&self, registrar: &mut Registrar);
}
```

| Verb | Registers | Phase 23 | Later |
|---|---|---|---|
| `content` | an entry in a registry of any kind (blocks; event kinds) | blocks, event kinds | items, entity kinds, brain models (27, 37) |
| `channel` | a per-voxel channel | as `PackageSpec.channels` today | |
| `property` | a typed property key for block definitions | reserved (the verb exists, one test) | Phase 24 |
| `command` | a server command: name, usage, handler | name and handler; `/help` generated | typed arguments (25) |
| `message` | the handler for a client-to-server message kind | as `message_received` today | declared fields (25) |
| `system` | a tick function | here | |
| `on` | a handler for an event kind | here | |
| `save_section` | a plugin's saved state | as `save_sections` today | per-grid scope (32) |
| `client` | a HUD line, a key action, a client command | not in this phase | Phase 26 |

A new *kind of content* is not a new verb. An event kind is content: engine kinds have ids like `engine:blocks_changed`, plugin kinds
`voxunami_cave:block_edit_requested`.

A plugin cannot read another plugin's private state; they share only channels, properties, events and messages.

## 4. The tick, in phases

Today (`engine/server/src/server.rs`, `Server::step`): accept connections; receive each client's messages in client-id order (joins call
`client_joined`, game messages `message_received`, commands `command_received`); one call to `logic.tick`; then send updates (voxel
changes, game messages, replies, `TickCompleted`). `sessions` is a `BTreeMap`, so the order is already deterministic.

After Phase 23 the engine still receives and sends exactly as now. What was one call, `logic.tick`, is a list of phases:

| Phase | Who runs | Phase 23 contents (Cave) |
|---|---|---|
| `Messages` | engine dispatches to registered message and command handlers, in client-id then arrival order | block editing, `/fly`, `/tp`, `/light`, `/level`, movement input |
| `Decide` | systems, class `Mind` allowed | empty (reserved for brains, Phase 37) |
| `Controllers` | systems that turn input into forces | empty now |
| `Physics` | systems; the engine will own this one | movement (it steps Rapier inside itself until structures need an engine-owned physics world, Phase 32) |
| `Settle` | systems | players' poses announced (`PoseBook::tell_everyone`) |
| `Rules` | systems | fire, then light |
| `Send` | engine | unchanged |

Order inside a phase is section 5.3. `Messages` and `Send` are the engine's and take no plugin systems.

## 5. Events, order and determinism

### 5.1 Delivery

The events a system, message handler or command handler causes (the events it sends, and one `ChannelsChanged` for each watched
channel it wrote) are delivered **synchronously, right after it returns**, before the next system runs: each event to all of its
handlers in the global order (5.3). What a handler itself sends or writes joins the back of a first-in first-out queue, so it is
delivered after the other events already waiting, once every handler of the event being delivered has run; and so on until the queue
is empty. (Breadth first: simpler to reason about than a handler's effects running in the middle of the handlers of its own event.) This
reproduces today's behaviour exactly: fire's writes reach the light rule (and movement) *before* the light system runs in the same
tick. A cascade longer than a fixed limit (proposed: 4 096 events or depth 64 in one tick) stops delivery for that tick and is reported;
it is deterministic and a bug in a plugin.

### 5.2 Which events

Engine events (ids `engine:`): `blocks_changed`, `channels_changed`, `player_joined`, `player_left`, `world_loaded`, `world_saving`.
Plugin events are defined by the plugin that owns the meaning. Phase 23 defines none beyond what Cave needs.

- **Changes.** `ChannelsChanged { grid, channel, voxels: Vec<VoxelCoordinate> }` is delivered per grid and channel written. A
  subscriber names the channels it wants (`on_channels(&[block, state], ...)`), and the engine only records changes for channels
  somebody subscribed to, so the 20 000 light writes of a tick cost nothing unless a plugin asks for the light channel.
  `BlocksChanged` is sugar for the block and state channels. (Today the light rule is woken by block edits and not by its own light
  writes; the same holds.)
- **Ask-first events.** Block editing is Cave's, so its events are Cave's: `voxunami_cave:block_edit_requested` and its handlers return
  `Allow` or `Refuse(reason)`; the first refusal in order wins and is answered to the player as a command reply. Whether one is
  promoted to the engine waits for a second game that edits blocks (the project's rule: new code starts in the game that needs it).
  `BlockUsed` (plan.md requirement 4) is the same kind of thing: defined by the plugin that handles use requests.
- Batching: no event is per voxel. This is what keeps the later WASM boundary bulk-only.

### 5.3 Order

Within a phase, systems run in one **sequential** order, and handlers for one event likewise:

1. a topological order of the declared edges (`after`, `before`), and
2. ties broken by plugin id, then by the system's own id. Never by hash order, registration order, thread timing or the order of the
   plugin list.

A cycle or an edge to an unknown id is a start-up error. Nothing runs in parallel. A test shuffles the plugin list and requires
identical digests (section 10).

### 5.4 The determinism class

```rust
pub enum DeterminismClass { Deterministic, Mind }
```

`Deterministic` (the default) is covered by the golden sessions and the partition-invariance test. A `Mind` system gets no way to write
the world: its context holds only an `IntentSink`, and what it emits goes through the same validated path as a player's input. Nothing
in Phase 23 is `Mind`; the type exists so that Phase 37 does not change the API.

### 5.5 Declared channels are checked, not scheduled

A system declares the channels it reads and writes. In tests and debug builds the engine (a) fails a system that touches a channel it
did not declare, and (b) refuses, at start-up, two systems in the same phase that both write a channel (or one reads and one writes
it) with no path of `after`/`before` between them. Handlers are not covered: they can write anywhere, and the check says so rather than
pretending. Nothing is scheduled from the declarations.

## 6. Grids

Only root grids exist in this phase, but every rule is written against the shape structures will need (plan.md requirements 1, 10,
11):

```rust
pub struct GridId(u64);                  // stable within a world; counter saved with the world; today GridId::root(level)

pub trait GridView {
    fn id(&self) -> GridId;
    fn block_and_state(&mut self, at: VoxelCoordinate) -> Option<(u32, u32)>;   // `at` is in the grid's own coordinates
    fn get(&mut self, channel: ChannelIndex, at: VoxelCoordinate) -> Option<u32>;
    fn set(&mut self, channel: ChannelIndex, at: VoxelCoordinate, value: u32) -> Result<(), AccessError>;
    fn read_region(&mut self, region: VoxelBox) -> RegionSnapshot;              // bulk; the only way to read many voxels
    fn gravity(&self) -> LocalDirection;           // the world's gravity in this grid's axes; today always -y
    fn world_position(&self, at: VoxelCoordinate) -> WorldPosition;
}
```

- There is **no** call that returns a whole grid or scans one: access is by voxel, by region, by an active set, or by neighbourhood.
  A grid the size of a moon needs no change to a rule.
- `LevelIndex` stays (levels are real, and portals will cross them); a level's terrain is `GridId::root(level)`.
- `ActiveVoxels` is keyed by `(GridId, VoxelCoordinate)` where it is keyed by `(LevelIndex, VoxelCoordinate)` now. **The saved bytes do
  not change in this phase**: a root grid is written exactly as its level is today (by level name), so the golden saved-world hash
  holds. Non-root grids will be written by id (Phase 32).
- `sky_exposure` is not in the sketch: there is no sky light yet. It joins `GridView` when sky light does.
- The rule traits that exist (`LightWorld`, `FireWorld`, `ScaledLightWorld`) already abstract the world; they take a grid instead of a
  level, which is a type change, not a rewrite. (`Movement` reads the `ServerWorld` directly and gains the same view.)

## 7. Save sections

`save_section(id, scope, write, read)`: as `save_sections` today (a plugin returns `Section`s, reads its own back when built). A new
field, `scope: Global | PerGrid { movable: bool }`, says whether the state is keyed by voxels (so a merge or split can move it) or is a
cache that can be rebuilt (requirement 14). It is recorded and checked for consistency in this phase and used in Phase 32. Unknown
plugin sections are preserved byte for byte, as now.

## 8. How each game is converted

**Cave** (`CaveLogic`, about 500 lines, becomes five small plugins):

| Plugin | Takes from `CaveLogic` | Systems and handlers |
|---|---|---|
| `player_presence:players` (the shared crate) | `PoseBook`: poses, levels, `/level`, pose announcements | `Settle` system: announce poses; message handler; `/level` command; `client_left` |
| `voxunami_cave:block_editing` | `set_block`: reach, state, terrain checks; the set-block message | message handler; emits `block_edit_requested`, then writes |
| `voxunami_cave:movement` | `Movement`: bodies, `/fly`, `/tp`, spawn and input messages | `Physics` system; handler on `blocks_changed` (collider rebuild); `client_left` |
| `voxunami_cave:fire` | `Fire`: the rule, its clock, `ignite` | `Rules` system; handler on `blocks_changed` that sees a fire block placed; save section |
| `voxunami_cave:light` | classic `Lighting`, `MultiScaleLighting`, `/light`, the scheme message, model switching | `Rules` system `after: fire`; handler on `blocks_changed`; command; `player_joined` sends the scheme; save sections |

The order today is movement, poses, fire, light; so `players` after `movement` in `Settle`, `fire` before `light`, `light` `after: fire`.
`set_block` no longer calls fire, light and movement: it writes, and the engine delivers `blocks_changed`.

**2048** (`logic.rs`): the players plugin, and one plugin with the push message, the motion system and its progress save section.
**Eels**: the players plugin, and one plugin for the round (steer message, round system, standings message). Both are small; they go first.

## 9. What does not change in this phase

The protocol, the world file format (the saved bytes are identical), the client, the renderer, `GameDefinition`'s client half, block
definitions (properties are Phase 24), entities (Phase 27), the content package (Phase 28). `Server` keeps its API: the registrar is
behind a `GameLogic` implementation (`PluginHost`) so nothing outside `game_shell` and the games notices.

## 10. Tests

Already in place and required to pass **unmodified** after every sub-step: the three golden sessions (`games/*/tests/golden_session.rs`:
the recorded stream and the saved world of a whole scripted session, hashed and pinned; identical on Windows and Linux; they fail
when fire stops telling the light, or when the light budget changes by one voxel), the pinned generator digests, every existing test.

New in this phase:
- registration errors: a cycle, an edge to an unknown id, a duplicate message kind or command, an unknown dependency;
- order independence: shuffling the plugin list gives identical golden digests;
- event delivery: fire's writes reach the light and movement before the light system in the same tick; the cascade limit stops a
  loop; a refusal wins and is answered;
- the declared-channel checks, with a system that cheats;
- a `Mind` system cannot write (a compile-fail style test, or a type-level assertion);
- `ActiveVoxels` keyed by grid round-trips, and its saved bytes equal the level-name form;
- translation invariance of the rules through `GridView` (the existing partition-invariance test, run through the new API).

The frame-invariance and merge-invariance tests of requirements 6 and 14 need moving grids and arrive with Phase 32; their harness is
designed here and not built.

## 11. Steps, each ending with passing tests, clippy, rustfmt, the goldens on Windows and Linux, and a commit

- **23a** (done) Grid addressing: `GridId`, `GridView`, `ActiveVoxels` by grid; the rule traits take a grid. Cave is still one `CaveLogic`.
  As built: `voxel_space::GridId` (a root grid's id is its level's index; allocated ids start at 2^32); `server::GridView` with `id`, `get`
  and `set`, and `ServerWorld::grid(GridId)` (`NoSuchGrid` for an unknown or non-root grid); `lattice::ActiveVoxels` and `voxel_random`
  keyed by `GridId` (the pinned random numbers and the saved bytes are unchanged, by tests); `LightWorld`, `FireWorld` and
  `ScaledLightWorld` take a `GridId`, and Cave's adapters read and write through `ServerWorld::grid`. `read_region`, `gravity` and
  `world_position` are not in the trait yet: each joins when its first user does. `Movement` and 2048's motion still use levels
  (steps 23c and 23d).
- **23b** (done) The registrar, phases, ordering, events, the declared-channel checks, `PluginHost`; all the new tests above that do not need a
  converted game. As built, in the new engine crate `plugin_host` (depends on `server`; 25 tests, seven mutations of the host all caught):
  - the verbs that exist: `event`, `system` and `mind_system`, `on`, `on_channels`, `on_ask`, `message`, `command`, `save_section`. The
    content-side verbs (`content`, `channel`, `property`) belong to assembling a `PackageSpec` and arrive with Phases 24 and 28; `client` with
    Phase 26.
  - events built: `ChannelsChanged` (per grid and channel; handlers name the channels they want with `on_channels`), `PlayerJoined`,
    `PlayerLeft`. Not built: `BlocksChanged` as sugar (a plugin names the block and state channels, which it knows), `WorldLoaded` (a plugin is
    handed the world when it registers) and `WorldSaving` (`GameLogic::save_sections` takes `&self`).
  - `server::ServerWorld::watch_channels` and `take_changes` are the log behind `ChannelsChanged`: only watched channels cost anything.
  - delivery is first in, first out (5.1 above): a handler's own events and writes wait behind the events already waiting.
  - a `Mind` system gets a `MindContext` with the tick and `push_intent`, nothing else (a `compile_fail` doctest says so); its intents are
    delivered to the message handler of their kind, as if a client had sent them.
  - `Context::world()` is the whole server world, a transitional way to reach what is not a grid (sending, replying, voxels by level); rules use
    `Context::grid()`, whose reads and writes are checked against the system's declarations in debug builds. 23d narrows `world()` where
    it can.
  - an unknown command is answered `unknown command; try /a, /b X` listing the registered usages by command name; 23d decides what Cave's
    golden session needs (its text and order differ today).
  - the order of plugins in the list does not matter (dependencies first, otherwise by identifier), and a test shuffles three busy plugins.
- **23c** (done) Convert 2048, then Eels. As built:
  - `player_presence::PlayersPlugin` (id `player_presence:players`) owns the shared `PoseBook` (`SharedPoseBook`, an `Arc<Mutex<..>>` the game takes
    from the plugin with `book()` and hands to the plugins that use it; they declare `depends_on` it). `PlayersPlugin::new(accept_reported_poses)`
    registers the pose message only for a game that believes what players say; always the `/level` command (usage `/level`), a handler for
    `PlayerLeft`, and a `Settle` system that announces poses. Sharing between plugins is by handles the game creates, not a lookup in the registrar.
  - 2048: `Plugin2048` (`voxunami_2048:rules`): the push message, a `PlayerJoined` handler (the score), the `motion` system in `Rules` (after the
    announcement, as before), and the saved state. `Logic2048` is now only the state they share. Eels: `PluginEels` (`voxunami_eels:rules`): handlers for
    joining and leaving, the steer message, `swim` in `Physics` (the round ticks and each player's pose follows it) so that the announcement in
    `Settle` comes after it, and `standings` in `Rules` after the announcement, as before.
  - Behaviour held: both golden sessions and every existing test pass unmodified, on Windows and Linux. One change: Eels now answers `/level`, as
    every game with the players plugin does (it had no commands; it has one level).
  - `GameDefinition::new_logic` still returns a boxed `GameLogic`; each game builds a `PluginHost` there. 23e replaces it with a plugin list.
- **23d** (done) Convert Cave: players, block editing, movement, fire, light (`games/voxunami_cave/src/plugins/`; `CaveLogic` is gone and `logic.rs` holds
  only the shared numbers). As built:
  - ids `voxunami_cave:block_editing`, `:movement`, `:fire`, `:light`, with `player_presence:players`; the game builds them with `cave_plugins(content)`.
    The fire, light and movement plugins hear of changed blocks as `ChannelsChanged` on the block and state channels; `set_block` no longer calls them.
    Fire notices any block that becomes fire, whoever wrote it; a test plugin that lights a fire in the air is noticed by fire and light.
  - `voxunami_cave:block_edit_requested` (an ask event) lets a plugin the game does not know refuse an edit; the player is told why. A test pins it.
  - the fire and light systems read and write through `Context::grid`, so what they declare is checked in debug builds, and the light system is
    `after` the fire system (leaving that out is refused at start-up).
  - behaviour held: the stream of the golden session is **bit-identical** to the one before the conversion (proved by temporarily restoring the old
    unknown-command text, which gave the old stream hash); every existing test passes unmodified except for two deliberate changes below.
  - deliberate changes: (1) the unknown-command reply is generated from the registered commands, by name: `unknown command; try /fly, /level, /light,
    /tp X Y Z` (it was `/level, /fly, /tp X Y Z, /light`); (2) the host returns saved sections sorted by identifier, so the bytes of a saved world do
    not depend on the order plugins were registered in (before, the light section came first and fire second; each section's bytes were compared and
    are identical). The golden pins were re-taken for these two.
  - one nuance: setting a voxel to the value it has is not a change, so a block edit that changes nothing no longer wakes the light, fire and bodies
    (before, it did, uselessly).
- **23e** Shuffle test, mutation experiments (`PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`), documentation, `GameDefinition::new_logic`
  replaced by a plugin list.

## 12. Questions for the owner

1. **Event cascade limit:** stop delivery for the tick and report (my proposal, deterministic), or treat it as a crash?
2. **Physics:** leave Rapier stepping inside the movement plugin until structures need an engine-owned physics world (my proposal), or
   make the engine own the physics world now? Doing it now is more work and nothing in Phase 23 needs it.
3. **Channel-filtered change events:** is it acceptable that the engine records only the changes somebody subscribed to? It means a
   plugin added later does not see changes made before it subscribed (which is also true of any event).
4. **Where plugins live:** a game's own plugins in the game's crate (the project's rule), shared ones in `plugins/` or the shared
   crate they belong to (`player_presence` for the players plugin). Is that right, or do you want one plugin crate per plugin?
