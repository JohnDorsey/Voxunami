# Phase 25 specification: declared messages and a command registry

**Status: proposed 2026-10-18, awaiting the owner's review.** It makes concrete revision 4 of `plan.md` ("Plugin shape revisions") and the
recommendation of `plugin-architecture-review.md` W6: the command registry is worth it; for messages, a trait and a generated test, no code
generation and no schema. Everything below about today's code was read from the code (verified); the design is my proposal (believed).

## 1. The problem, as it is in the code

- **Messages.** Game message kinds in the code (counted from the registrations and the `messages.rs` files; the list may miss a server-only one): client to server: `push`, `set_block`, `spawn`, `move_input`, `steer`, `pose`; server to client:
  `progress`, `standings`, `light_scheme`, `level_change`, and the pose announcements). Each has a hand-written `encode_x`/`decode_x` pair in
  its crate's `messages.rs`, a `NamespacedId` kind function, and a handler that calls `decode_x` and silently drops a malformed payload (the
  same `if let Ok(...)` is repeated in every handler). Nothing ties a kind to its codec, nothing proves a codec round-trips except tests
  someone remembered to write, and the host cannot list which kinds exist.
- **Commands.** `CommandSpec { name, usage, summary }` plus a handler that gets `&[&str]` and parses it by hand (`/tp` and `/level go` each do
  their own argument checking and write their own error text). The unknown-command reply is built in `host.rs` from every `usage`, and there is
  no `/help`.

## 2. Messages

A message type implements one trait, in `protocol` (shared by client and server, so both ends come from one definition):

```rust
pub trait Message: Sized + PartialEq + Debug + 'static {
    /// The kind carried in `GameMessage`; unique across the game's plugins.
    fn kind() -> NamespacedId;
    fn encode(&self) -> Vec<u8>;
    fn decode(payload: &[u8]) -> Result<Self, DecodeError>;
    /// Values that exercise every field, including the extremes; at least one.
    fn samples() -> Vec<Self>;
}
```

- `Registrar::message::<M>(|context, client, message: M| ...)` replaces the byte-slice form: the host decodes, and a payload that does not decode
  is ignored in one place (and counted, for a later diagnostics hook). `Registrar::declares_message::<M>()` is for kinds the plugin only
  sends (server to client), so they are listed too.
- **The generated check.** Each registration stores a type-erased checker. `PluginHost::check_messages()` (called by one test in each game) runs,
  for every declared message and sample: `decode(encode(m)) == m`; `encode` is deterministic; a truncated payload (every prefix) decodes to an
  error or to something that re-encodes to exactly that prefix, and never panics; trailing bytes are an error. It fails when a declared
  message has no samples or when two plugins declare the same kind. This is the gate: it runs for every message anyone ever declares.
- The existing `encode_x`/`decode_x` functions become the trait impls, unchanged bodies, so the wire format does not change. The engine's own
  hot-path messages (brick edits, the stream) are not touched; they keep their compiled codecs in `protocol`.
- Not doing (as the review advised): a field-list macro or code generation, a schema export, versioned message evolution. A `Message` can grow
  a `fn version()` later without breaking implementors if it needs to.

## 3. Commands

```rust
CommandSpec::new("tp", "puts your eyes at a position, in voxels")
    .argument(Argument::integer("x")).argument(Argument::integer("y")).argument(Argument::integer("z"))
```

- An `Argument` has a name and a kind: `Word`, `Integer`, `Identifier` (a `NamespacedId`), or `Rest` (the remaining text, last only), and may
  be optional. The usage line is generated (`/tp X Y Z`), and a call with the wrong count or an argument that does not parse is answered by the
  host with `usage: /tp X Y Z` (and, for a bad value, which argument), so handlers never parse strings. The handler receives `Arguments` with
  typed getters (`integer(0)`, `word(1)`).
- Subcommands (`/level go NAME`, `/light multiscale ...`) stay inside the handler as a `Word` followed by `Rest` for now; a real subcommand
  tree is not worth building for two commands. Their summaries keep listing them.
- **`/help`** is registered by the host: it lists every command's usage and summary, or one command in detail (`/help tp`). The unknown-command
  reply stays "unknown command; try ..." from the same list, now including `/help`.
- Client-side `.`-commands belong to Phase 26 (the client interface), which can reuse `CommandSpec`. Not here.

## 4. Steps

- **25a** Tests first: pin today's replies for each command (wrong arity, bad number, unknown) and today's bytes for each message sample, so the
  conversion is proved not to move anything. Then the `Message` trait and `check_messages`, with mutations (a codec that drops a field, one
  that is not deterministic, one that accepts trailing bytes) each caught.
- **25b** Convert every kind and their handlers; the golden sessions must pass unmodified (messages are byte-identical).
- **25c** The command registry and `/help`; convert `fly`, `tp`, `light`, `level`. The golden sessions' unknown-command reply gains `/help`, so
  the three pinned hashes change once, for that reason only; I will show that by comparing the recorded replies before and after.
- **25d** Documentation (`plugin-api-spec.md` verbs, `design-decisions.md`, build log), gates on Windows and Linux, commit, push.

## 5. Questions for the owner

1. **Permission levels.** The plan item mentions a permission level on commands. Nothing in the game has operators or permissions yet, so a level
   would be a field no code reads. My recommendation (confidence: high): leave it out and add `permission` when the first command needs to
   be restricted, which is a small, source-compatible change. Do you want it now anyway?
2. **`/help` in the host or as a plugin.** I propose the host registers it (it needs the registry). Agree?
3. **Malformed payloads.** Today they are dropped silently; I propose still dropping them, but counting them in the host so a later admin
   command can show them. Or should a client that sends garbage be disconnected? (I would not, yet: a version skew is not an attack.)
