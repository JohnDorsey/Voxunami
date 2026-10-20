# Phase 30: a user interface the server can describe

Status: proposed, waiting for the owner's answers to section 6. Nothing is built. Read from the plan (revision 4 and the
client-interface options in `plugin-architecture-review.md`) and from the code after Phase 29; the choices are mine and marked.

## 1. What is wrong today

The renderer cannot draw text. Everything a player reads goes in the **window title**: the HUD lines of the client plugins, the
command being typed, the reply to it, a question the server asks (`.choose N`), the frame rate. A game cannot show a menu, a
count, a health bar, or a choice with buttons. Nothing a *server* wants to show can look like a screen, and a recording shows
none of it. The plan's decision (revision 4, "Client interface"): UI described by the server in the shared value vocabulary and
driven by the stream, so a replay shows exactly what the player saw, and a client needs no plugin installed.

## 2. Design (my proposal)

**One small UI model, three sources, one drawing path.**

- **The model** (`ui` crate, no graphics): a tree of `Panel` (a box with a title, a colour and children laid out in a row or a
  column), `Label` (text, colour), `Button` (text, an identifier), `Bar` (a fraction of full, colour), `Swatch` (the colour of a block,
  with an optional count: an item icon) and `Space`. No scripting, no free positioning: a panel is anchored to a corner, an edge or the
  middle of the screen and its children flow. Sizes are whole pixels at a whole-number scale (so a picture of it is the same on every
  machine and can be pinned). Layout, hit testing and navigation are pure functions of the tree and the screen size, with tests first.
- **Drawing:** the renderer gets one more pass after the world: filled rectangles and glyphs from an embedded 8x8 bitmap font,
  as quads in screen space. Text is ASCII at first (the font's 128 glyphs); anything else shows as a box.
- **Source 1, the shell's own screens:** the HUD (what the title shows now, drawn in a corner), the command line being typed and its
  reply, and the question a server asks (`engine:prompt`), restyled as a modal panel with a button for each option. The messages
  and `.choose N` do not change.
- **Source 2, client plugins:** a sixth client verb, `overlay(|context| Option<Ui>)`, a pure function of the stream and local state like
  `hud_line` (which stays and is now drawn on the screen, not in the title). Cave's hotbar and inventory are the first user.
- **Source 3, the server:** three engine messages: `engine:ui_show { id, modal, anchor, tree }` (replaces the form with that id; a form that is not modal is a HUD panel the server keeps up to date), `engine:ui_close { id }`
  and, from the client, `engine:ui_event { id, element, what }` with `what` being a button press or the form being dismissed. A form is
  for one player; the plugin that showed it hears the event as `UiEvent` (the way it hears `PromptAnswered`) and answers by
  updating (`ui_show` again) or closing. Forms are not saved, are dropped when the player leaves, and a closed form never answers. The
  tree is checked on both ends (a depth and size limit, text length, a button identifier is unique within a form) so a hostile server
  cannot make a client do unbounded work.
- **Input:** a modal form releases the mouse (the window already captures it on a click and releases it on Esc), the player clicks a
  button or presses its number, and Esc dismisses it; the world stops taking mouse and movement keys while a modal is open. A
  replay shows the form and never answers.
- **Replay and tests:** what is drawn is a function of the stream, local settings and the screen size. Tests render pictures headless and pin them,
  as the Cave pictures tests do, and the golden sessions do not move (no golden session shows a form).

## 3. What it does not do (and why later)

Text input fields (chat, signs, anvils), images and textures, scrolling lists, drag and drop, fonts beyond ASCII, mouse hover effects, and
rebinding keys through a menu. Each is an addition to the tree or a verb on top of it; none is needed to deliver the three
sources above. Server-driven *HUD* panels that are not modal (a health bar the server keeps updated) are the same message with a
`modal: false` and an anchor: included.

## 4. Steps

- **30a** The `ui` crate: the tree, its checks, layout at a whole scale, hit testing, keyboard navigation. Tests first.
- **30b** The renderer's overlay pass and the font; pictures pinned (GPU tests, on both systems).
- **30c** Protocol (version 6): the three messages, the session state, `UiEvent` in `plugin_host` with `Context::show_ui/close_ui`, replay, limits.
- **30d** The shell: the HUD, the command line and the replies on the screen instead of the title; prompts as modals; mouse and keyboard.
- **30e** Cave: the hotbar and an inventory panel as an `overlay`.
- **30f** Documentation, gates on both systems.

## 5. Risks

- **The font and the pixel pictures are the least certain part:** exact pixels depend on the GPU's blending and texture sampling. The overlay
  uses nearest-neighbour sampling of an integer-scaled font and flat colours, which should be exact; if a driver differs, the pictures are
  pinned within a small tolerance, as the existing GPU tests are.
- **Modal input** touches how the window captures the mouse; it is where I expect surprises, so 30d is built and tested on its own.

## 6. Questions for the owner (my recommendation first)

1. **The font.** The `font8x8` crate (MIT, 0.3.1, no dependencies, the 128 ASCII glyphs as bitmaps; recommended, medium-high confidence): crisp,
   deterministic, and tiny. The alternative is a TrueType font and a rasteriser crate (`fontdue`, `ab_glyph`) with a bundled font file: prettier at
   large sizes and ready for other scripts, but a font licence to carry, a rasteriser whose output we do not control, and pictures that are harder
   to pin.
2. **Scope of this phase:** text, panels, buttons, bars, swatches, no text input (recommended). Chat and the command line stay one line of
   text drawn at the bottom; typing in a form waits for a phase that needs it.
3. **Server forms now, or only the client toolkit?** Build both now (recommended): the replay and limits questions are the hard part, the prompt is
   already a first user of "the server asks, the player answers", and Phase 32's missing-ship choice wants it. The alternative is to do 30a, b, d and e
   and leave 30c until Phase 32, which saves a protocol version now and costs a second one later.
4. **The prompt becomes a modal panel with buttons** and keeps `.choose N` as the keyboard way (recommended).
5. **The HUD leaves the window title:** the HUD lines, the position, the command line and replies are drawn in corners; the title keeps the game's name
   and the frame statistics (recommended). Or keep the title as it is and add the drawn HUD beside it (more clutter, nothing lost).
6. **Cave's first screens:** a hotbar (the placeable blocks as swatches, the selected one outlined, counts in survival) always shown, and an
   inventory panel on a key (`E`), both client-built (recommended). Or leave Cave's screens for after the toolkit exists.
