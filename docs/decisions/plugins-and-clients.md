# Plugins, messages and the client: what was decided

The plugin host, property bags on block definitions, declared messages and commands, the client interface, the user interface, the material palette, and asking a player a question. Specs: `plugin-api-spec.md`, `property-bags-spec.md`, `messages-and-commands-spec.md`, `client-interface-spec.md`, `ui-spec.md`. Moved from `design-decisions.md`, which keeps what holds for every game; the stories behind these entries are in `history/decision-diary.md`. Status key as in `design-decisions.md`.

## Decided (the plugin host; built in Phase 23)
- A game's server rules are plugins run by `plugin_host::PluginHost` (the specification and its record are in `plugin-api-spec.md`). What a plugin
  registers, and when: systems in the phases `Decide`, `Controllers`, `Physics`, `Settle` and `Rules`; handlers of events, delivered first in first
  out right after the system or handler that caused them returns; handlers of client messages and commands; saved state.
- **The order of the plugin list never changes the result.** Plugins are registered by dependency then identifier, systems and handlers are ordered
  by the `after`/`before` they declare (ties by plugin identifier, then their own), saved sections are written sorted by identifier, and nothing
  runs in parallel. Each game's golden session (its recorded stream and saved world, hashed and pinned) is run with the plugin list as listed, reversed and
  rotated.
- A system's reads and writes of channels are declared and checked (in debug builds); two systems that use one channel, one writing it, with no order between
  them are refused when the host starts. A `Mind` system cannot write the world, only emit intents. A rule reads and writes one grid at a time, by `GridId`
  and the grid's own coordinates.

## Decided (property bags on block definitions; built in Phase 24)
- A block definition carries namespaced, typed properties; the table keeps one column per declared key, so a read in a hot rule is an index by
  model. Properties are **per state**: a value can be set for every state of a block or overridden for one state (state, else block, else the
  key's default). Kinds are bool and the integers (u8, u16, u32, i32), each range-checked when the table is built; a bad definition is an error
  there, never a surprise in play.
- The owner of a key declares it (dependency direction games to plugins to engine): `base_blocks:emission`, `base_blocks:flammability` and
  `base_blocks:passable` are the base blocks plugin's; the engine reserves `engine:density` and `engine:placeable_on_moving_grids`. A game adds its
  plugins' declarations through `PackageSpec.properties`. Rules take a `PropertyHandle` once and read it by model.
- Saved files are unchanged: properties come from definitions, not from the world.

## Decided (declared messages and commands; built in Phase 25)
- A game or plugin message is a type that implements `protocol::Message` (kind, encode, decode, samples), used by both the sending and the receiving
  end. `Registrar::message::<M>` decodes for the handler (a payload that does not decode is ignored and counted, never disconnects the sender) and
  `declares_message::<M>` lists a kind the plugin only sends. The host tests every declared message's codec on its samples (`check_messages`,
  run when a game's server is built): it round-trips, encodes deterministically, refuses trailing bytes, and a cut-short payload is refused or is
  itself a payload. The engine's own hot-path messages keep their compiled codecs. No code generation and no schema: the review judged it not
  worth it at this size.
- A command is a `CommandSpec` with a name, a summary and typed arguments (word, number, integer, identifier, rest). The host checks a call,
  answers a malformed one (`usage: /tp X Y Z; Z must be a number`), generates the usage line, and provides `/help` itself. No permission level
  until a command needs one.

## Decided (the client interface; built in Phase 26)
- The client half of a plugin is a separate `ClientPlugin` (same identifier as its server plugin) registering `on_message::<M>` (a server
  message, typed), `view` (adjusts what is drawn), `hud_line`, `key`/`keys` and `mouse` (actions) and `command` (a `.`-command, with the same typed
  arguments as server commands). State belongs to the plugin. `GameDefinition` no longer has `ClientState` or the six client hooks.
- Hooks are pure functions of the server's stream and the plugin's state, so a replay is unaffected: `on_message`, `view` and `hud_line` run in a
  replay, and actions and commands only while playing. A hook asks the server for something through its context (`send_command`, `send_message`),
  never by touching the connection.
- **Any number of actions may be bound to one key or mouse button; all run, in order of plugin identifier and then registration** (the owner's
  decision). An action has its own unique identifier; a key or button is its default binding, so that rebinding by client commands (a later
  phase) names actions. `.keys` lists them; `.ambient`, `.help` and `.keys` are the shell's own client commands, registered the same way.
- The engine does not know Cave's light: `world_view` has a neutral `BrickLight` hook (compared by identity) that turns a brick's channels into
  per-voxel brightness; the multiscale light (`ScaledLight`, `LevelFormat`, blending) is in Cave. Measured: the hook costs nothing visible
  (the per-brick time of Cave's 4-scale light, 2.3 to 2.8 ms before and after).
- HUD lines are joined into the window title until the UI toolkit of Phase 30.

## Decided (the user interface; built in Phase 30)

- **One small interface model, three sources, one drawing path.** `ui` is a crate with no graphics: a tree (`Panel`, `Label`, `Button`, `Bar`, `Swatch`, `Space`), its
  limits, a canonical byte form, and a layout in whole pixels at a whole-number scale (so a picture of it is the same on every machine). The renderer draws
  flat rectangles (`set_overlay`); text is rectangles too. The shell's own screens, a client plugin's `overlay`, and the server's forms all end up in one list.
- **The server describes forms in engine messages,** so every game's clients can draw them with no plugin and a recording shows them. A form belongs to
  one player, has a number never reused, is not saved, and is dropped when the player leaves; only a button it has can be pressed, only by the player it was
  shown to, and only a modal form can be put away. The plugin hears `UiResponded` and answers by updating, closing or doing something else.
- **A modal interface has the player's input and the world has none:** the mouse is freed, held keys are forgotten, a half typed command is dropped,
  clicks and keys (Tab, arrows, Enter, numbers, Escape) go to its buttons. A question the server asks (Phase 27h) is shown this way, and `.choose N` still works.
- **Client plugins cannot take the input:** an overlay is always shown as a panel. A choice is the server's form.
- **The title is for the game and the frame statistics;** what the player is doing (position, HUD, the command being typed, the reply) is on the screen.
- **Pictures compared to the pixel are taken once two frames agree** because of a quirk of the development machine's integrated GPU (see `ui-spec.md`).

## Decided (the material palette; built in Phase 31)

- **The game's palette is a storage buffer, indexed by the quad's 24-bit material value,** so a game has 1,048,576 materials (a choice of the renderer, to bound the buffer at
  16 MiB; the format has room for 16 million). Avatar colours are the last 16 values (`0xFFFFF0`..), kept in the frame uniform, so no game value is reserved.
  A value past the palette draws magenta, as a block of a missing plugin does. Nothing outside the renderer changed.

## Decided (asking a player a question; built in Phase 27h)

- Asking is an engine facility, not a game's: any plugin may ask any player a question with a title and 1 to 9 options (`Context::prompt`) and
  hears the answer as the event `PromptAnswered`. The engine owns the numbering and the validity (a question is answered once, by its player, with an
  option it has); the plugin owns what the question means and what to do while it waits (holding an entity, for instance).
- Questions have no time limit and are not saved. A player leaving drops theirs; the plugin asks again if it still needs to. This keeps the
  facility stateless across saves; a saved world never holds a question for a player who may never return.
- Ids are never reused, and the client is told when a question is over (`engine:prompt_closed`), so neither a stale answer nor a stale screen can
  happen. The shell shows the first question on the HUD line and answers with `.choose N`; the toolkit of Phase 30 restyles it without changing
  the messages.
