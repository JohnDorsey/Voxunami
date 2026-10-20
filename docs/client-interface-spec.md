# Phase 26 specification: the client interface and the light hook

**Status: accepted 2026-10-18 with the default answers to questions 1, 2 and 4, and with question 3 decided differently by the owner (any number of actions per key or button, section 3). Built (26a to 26d).** It makes concrete the "client interface (Phase 26)" item of `plan.md` and
requirement 7 of "Requirements pulled forward". What I say about today's code was read from the code (verified); the design is my proposal
(believed). Two independent parts: A closes a rule violation (the engine knows Cave's light), B gives plugins the client hooks.

## 1. The problem, as it is in the code

- **The engine knows a plugin's concept.** `world_view` exports `ScaledLight`, `LevelFormat`, `MAXIMUM_LIGHT_SCALES`, `blend_brightness` and
  `combined_light` (about 130 lines of `mesher.rs`), and `ViewChannels` carries an array of `ScaledLight`. Only Cave's multiscale light uses them
  (Cave's `game.rs`, `multiscale_light.rs`, `plugins/`; `engine/world_view/tests/scaled_light.rs` tests them). It was a spike's shortcut
  (Phases 19 to 22) against the rule that the engine never knows a plugin.
- **The client side is the game's, not the plugins'.** `GameDefinition` carries `ClientState`, `handle_game_message(state, kind, bytes)`,
  `view_channels(content, state)`, `primary_action`, `secondary_action`, `key_pressed` and `status_text`. A plugin (or later a mod) cannot add a key,
  a mouse action, a HUD line or a client message handler; and `.`-commands are two hard-coded ones in `client_commands.rs` (`.ambient`, `.help`).
  Tests read the state directly (`player.state.progress` about 15 times in 2048, `.standings` 8 in Eels, `.light_scheme` once in Cave).
- The window has no text drawing (the renderer has none), so the HUD is the window title: `status_text` is appended to it.

## 2. Part A: the bulk light hook (safe, independent, first)

`world_view` keeps one neutral idea: a game may supply how a brick's channels become per-voxel brightness.

```rust
pub trait BrickLight: Send + Sync {
    /// The brightness (0 to 255) of every voxel of this brick, or None when the plain light channel is drawn as it is.
    fn brightness(&self, brick: &Brick, coordinate: BrickCoordinate) -> Option<ChannelStorage>;
}
// ViewChannels { blocks, states, light, bulk_light: Option<Arc<dyn BrickLight>> }   (Clone, no longer Copy; 32 mentions)
```

- Equality of `ViewChannels` (which `set_channels` uses to decide to re-mesh) compares the hook by pointer, so Cave builds the hook once when the
  scheme changes and keeps it, instead of rebuilding it each time the view is looked at.
- `ScaledLight`, `LevelFormat`, `MAXIMUM_LIGHT_SCALES`, `blend_brightness`, `combined_light` and their tests move to Cave (`multiscale_light.rs`),
  unchanged; Cave implements `BrickLight` over them. The mesher calls the hook where it calls `combined_light` now.
- **Performance risk, to measure:** one virtual call per brick job (negligible against meshing a brick), but the hook is shared across worker
  threads, so it must not lock. Measured before and after with the existing multiscale mesh timings; I will report the numbers.
- Proof it moved nothing: the existing `pictures` and `multiscale` tests and the `scaled_light` tests (moved) pass with the same expectations.

## 3. Part B: client plugins

A client half is a separate trait, paired with a server plugin by sharing its identifier (so a mod can later ship both):

```rust
pub trait ClientPlugin: Send {
    fn id(&self) -> NamespacedId;
    fn register(&self, registrar: &mut ClientRegistrar<'_>);
}
```

Verbs of `ClientRegistrar` (the budget: five, not a toolkit):

| Verb | What | Runs when |
|---|---|---|
| `on_message::<M>(\|ctx, message\| ...)` | a server message (the `Message` trait of Phase 25) | in play and in replay (it is the stream) |
| `view(\|channels\| channels)` | adjusts what the view draws from (Cave's bulk light hook) | whenever the view is looked at |
| `hud_line(\|ctx\| Option<String>)` | a line of status text | every frame, in play and replay |
| `entity_appearance(kind, \|entity\| Option<Appearance>)` | how entities of a kind are drawn: a box (size in eighths of a voxel, a block's number for its colour, and an offset of its middle from the entity), until there are models (Phase 30, 35) | every frame, in play and replay |
| `key(KeyCode, ActionSpec, \|ctx\| ...)` | a key action with an id and a summary | in play only |
| `mouse(Button, ActionSpec, \|ctx\| ...)` | the primary or secondary mouse action | in play only |
| `command(CommandSpec, \|ctx, arguments\| -> String)` | a `.`-command | in play only |

- The **context** is a narrow, non-generic view of the session: `send_command(text)`, `send_message(&M)`, the voxel under the crosshair and the
  block there (`target()`), the camera, the tick, the block table, and the entities the client has been told of (`entities()`, with their
  replicated components: a plugin can show the player's own inventory). No access to the connection or the mesher. This keeps hooks pure functions
  of the stream plus local state, so replay is unaffected (hooks that send are never run in replay).
- **A question from the server** (Phase 27h): the session keeps the questions the stream has asked and not closed. The shell puts the oldest first
  on the HUD line and registers `.choose N`, which sends the answer (a client plugin can read `prompt()` and `prompts_waiting()` to draw it its own
  way). Hooks that send are still not run in replay, so a replay shows the question and its closing but nobody answers it.
- **Commands reuse Phase 25's `CommandSpec` and `Arguments`**, with the usage prefix `.` instead of `/`, and `.help` is generated. `.ambient` becomes
  a built-in command registered by the shell by the same verb, so there is one command mechanism, not two. A name that two plugins register is a
  start-up error, like a duplicate server command.
- **State is the plugin's own** (an `Arc<Mutex<..>>` shared by its hooks, the idiom of the server plugins). `GameDefinition::ClientState` and
  the six client hooks above leave `GameDefinition`; it keeps only what only a game says (names, controls, camera, tick rate, join and movement
  messages). Tests that read `player.state.x` read the plugin's state through the handle the game's client plugin exposes.
- **Any number of actions may be bound to one key or mouse button** (owner's decision): they all run, in order of plugin identifier and then
  registration. A key or mouse registration is a *default binding* of an action that has its own identifier (unique: a duplicate action
  identifier is a start-up error); rebinding by client commands comes in a later phase and will be keyed by the action identifier.
- HUD lines are joined into the window title as `status_text` is now (the renderer cannot draw text; that is Phase 30's toolkit). The shape of
  the hook is what Phase 26 settles.

## 4. Steps

- **26a** (Part A) Tests first: a timing measure of meshing with multiscale light, and pin that the pictures do not change. Then the hook and the
  move; verify the same tests pass unmodified (the `scaled_light` tests move with the code, assertions unchanged).
- **26b** The client registrar, `ClientPlugin`, the context and the dispatch in `game_shell`, with tests on a fake session (duplicate key, order of
  HUD lines by plugin id, replay runs only the stream hooks, typed `.`-command arguments, generated `.help`). Mutations of each.
- **26c** Convert 2048, then Eels, then Cave to client plugins; `GameDefinition` loses the hooks; tests adapted. The golden sessions must pass
  unmodified (the server stream does not change).
- **26d** Documentation (`plugin-api-spec.md` verbs, `design-decisions.md`, build log), gates on Windows and Linux, commit, push.

## 5. Questions for the owner, answered

1. Client state is the plugin's own (default). 2. A separate `ClientPlugin` trait (default). 3. Any number of actions per key or mouse button; rebinding
by client commands later (the owner's). 4. HUD lines stay in the window title until Phase 30 (default).
