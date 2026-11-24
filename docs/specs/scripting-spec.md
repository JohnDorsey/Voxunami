# Scripting and the command line: a specification

**Status: a draft written on 2026-11-23; the spikes of section 12 were run on 2026-11-24 and their findings are in sections 4.2, 4.4, 9.1 and 9.3 (the programs are in `docs/history/spikes/scripting/monty_line_spikes/`, the numbers in `docs/history/spikes/scripting/monty_line_spikes_results_2026-11-24.txt`); the owner answered S1 to S9 with yes on 2026-11-24; phase 1 (chat and nicknames, section 5) and phase 2 (the line language, after 4.7) are built.** It follows the owner's rule, "let's not build anything until we have a good design and plan". It makes concrete the decisions the owner gave in four rounds of questions (2026-11-22 and 2026-11-23) and in the answers to the ten questions at the end of `permissions-survey.md` (2026-11-23). Branch `scripting-integration`.

**What it is.** Monty (the owner's fork of a sandboxed Python-like interpreter, see `monty-step-budget-plan.md`) comes into the game by way of the chat and the command line: a line may hold *holes*, `{/slash_text}`, `{.dot_text}` and `{>monty_text}`, that nest in any structure, and later command blocks run the same lines. Players keep functions, scripts and shared variables that survive sign-off and restart. Under all of it sits a permission system, because a public server with strangers is expected.

**How to read the tags.** **[decided]** the owner chose it, with the date. **[read]** I read it in the code on 2026-11-22 or 2026-11-23; nothing was run. **[proposed]** my design, an opinion with a confidence where it matters. **[open]** nobody has decided and the section says what is needed. A claim about Monty's behaviour that I did not run is marked **[not run]**.

## 1. What this has to do

The owner's needs, in their words as far as they were given:

- A chat first (plain text to other players), then the holes. `{/tp 50 65 {>server.players.by_nickname("Alex").z + 5}}` is the model line: a Monty value inside a server command, inside a client command if you like. **[decided]**
- Later the same lines run in **command blocks**. **[decided]**
- Players **define functions and scripts that persist** across sign-off and server restart, and **share variables** between ships and blocks. **[decided]**
- **Privacy, collaboration and server automation** at once. **[decided]**
- It should **feel Pythonic**: `{>1+1}` prints `2` in the chat. **[decided]**
- Strangers on public servers someday, so **roles, teams and grants per command, per pack and (later) per tag**. **[decided]** Tags are **not in the first version** (P2).

The research behind the permission half is `permissions-survey.md`, with its five notes in `docs/history/permissions-survey/`. This spec does not repeat it; it cites decisions by their D and P numbers.

## 2. Words used

| Word | Meaning |
|---|---|
| **Line** | What one player types and sends: at most 1,024 bytes, control characters stripped **[read]**. |
| **Sigil** | The first character that says which language follows: `/` a server command, `.` a client command, `>` Monty code. |
| **Hole** | `{` sigil text `}` inside a line. Its value replaces it. |
| **Slot** | An argument of a command that has a kind (Word, Number, Integer, Identifier, Rest). A hole that is a whole argument fills a slot with a typed value. |
| **Actor** | Whoever the server says is doing something: a player, the console, a command block, a function. |
| **Group** | A named set of actors that permissions are given to. `op`, `everyone` are built in. |
| **Root** | The keys listed as owners of a dedicated server, and the console. They may do anything, and nothing may take that from them. |
| **Permission id** | The name of one thing that can be allowed or denied, `pack:command.subcommand`. |
| **Marked function** | A function whose owner marked it to run with the owner's power. |

## 3. What the code has today

All **[read]** on 2026-11-22 and 2026-11-23 unless said:

- **No chat.** The input is a command line that opens only on `/` or `.` (`command_entry.rs`). One line, at most 1,024 bytes (`MAXIMUM_COMMAND_LENGTH`; more is a protocol violation), 8x8 ASCII font, no text-input fields (`ui-spec.md` defers them).
- **No names.** A player is an Ed25519 public key (`PlayerKey`) made once per install and kept in an identity file of 32 secret bytes (`engine/identity`); there are "no names, passwords or servers that know players" (`players-and-ships.md`).
- **No permissions.** `CommandSpec` has none by decision (Phase 25, `messages-and-commands-spec.md`); `/gamemode` lets any player change the whole world's mode. There is no operator concept anywhere. The server's commands are `help`, `level`, `gamemode`, `light`, `fly`, `thrusters`, `tp`, `ship`; none is in `plugins/*`.
- **Commands return nothing.** A handler gets `(context, client, arguments)` and writes text lines with `reply`. `Context.client` is `Option<ClientId>`, so a handler can already run without a client, but it has no idea who the actor is.
- **`Rest` is a list of words.** `Arguments::rest` gives `&[&str]`, split on whitespace; `/tp` takes a `where` Word and a `numbers` Rest, and `/ship`, `/level`, `/gamemode`, `/light` also take a `Rest` and split it in the handler. `CommandLine::parse` splits on whitespace and strips an optional leading `/`.
- **Nothing about a player is saved.** `SaveScope` is `Global` or `PerGrid`. A per-player scope is new. Block data (a chest's contents) is deferred (`entities-spec.md` 2.9), and command blocks need it.
- **Positions** are a brick plus an f64 offset (128-bit voxel coordinates), so a Python float is lossy far out.
- **Monty.** Deterministic step budget and memory limit **[read, built 2026-11-16]**; `MontyRepl` with `feed_run`, `feed_start`, `call_function`; `feed_run` returns the last expression's value (Monty's own test asserts `1 + 1` gives 2; **[not run]** by me). Host objects (lazy attributes, method calls) exist, but the instance store does not travel with a dump: a restored session holds dead proxies. `call_function` errors on a host call (use `feed_start`). Every built-in module is loaded unconditionally by `load_module` (`bytecode/vm/mod.rs:2184`), so there is no host switch. `re.sub` on large input cannot be interrupted (4 MB takes 40 to 70 s, `FORK.md`).

## 4. The line language

### 4.1 Which language a line is in

- A line whose first character is `/` is a server command, `.` a client command, `>` Monty code. **[decided]** (top-level lines need no braces.)
- Any other line is **chat** (section 5). **[decided]**
- Inside a line, a hole is `{` immediately followed by a sigil. A `{` followed by anything else is plain text, so ordinary braces in chat need no escape. **[proposed]**
- `{*` and `{**` after a brace are reserved for unpacking (`{*>...}`, `{**>...}`), later; today they are an error. The reason: `**` maps by argument *name*, so argument names must be valid Python identifiers (`on|off` in the usage of `.shadows` is not one; the spec names must be clean when subcommands arrive). **[decided]**

### 4.2 The scanner

Each sigil picks its own scanner for where the hole ends. **[decided]**

- **`/` and `.` text:** the hole ends at the first unescaped `}` that does not belong to a nested hole. Only sigil-started holes nest; a bare `{hi` inside the text is plain, so a following `}` ends the hole. To say `{hi}` there, write `\{hi\}`. **[proposed]**
- **`>` code:** the hole ends at the `}` that closes it as Python's lexical rules see it: brackets nest, and strings, f-strings (with their own fields and format specs), triple quotes and escapes hide their braces, so dictionary and set literals work (`{>{"a": 1}["a"]}`). **[decided: the owner approved "sigil picks the sub-language and its scanner"]** **Measured (spike S2, 2026-11-24):** a hand-written scanner of about 100 lines and a scanner driven by ruff's token stream (ruff's lexer is the tokenizer Monty itself uses) agree on all 4,000 generated expressions full of braces, quotes and backslashes, with random tails after the hole; Monty compiled all 3,000 generated expressions it was given; four deliberate breakages of the hand-written scanner (a backslash skipping one byte, no `{{` literal, no triple quotes, an f-string field not entered) were each caught; a brace counter was wrong on 1,560 of 2,000. On 60,000 random strings of brace-and-quote soup the two agree on 57,989 and differ only on `#` and on newlines. **Proposed:** use the hand-written scanner (the game need not link a parser to find an end), and keep the differential test against ruff's tokens in the game's own tests, so that a change in Monty's lexer is a failing test, not a silent drift. **`#` is an ordinary character in a hole** (ruff would read it as a comment that hides the closing brace; a line holds no newline), and **blanks right after `{>` are stripped** before Monty sees the code, because Monty reads a leading blank as indentation (`SyntaxError: Unexpected indentation`, checked).
- **`{.}` holes are allowed inside `>` code; `{/` and `{>` are not, yet** **[decided 2026-11-24 for `{.}`; the rest proposed]**. Inside `>` code, outside string literals, a `{.` followed by a letter starts a client-command hole, and anything else after `{.` is Python: `{.5}` and `{...}` are valid sets (found by spike S2), so a set whose first item starts with a dot is written as it is, or `\{.5\}`, or `set([.5])`. Because the client runs a `{.}` hole before the line is sent, **its result enters the code as a literal, never as code**: a string becomes a Python string literal (quotes and backslashes escaped), a number a number. To use it as a number write `float({.x})`. `{/` and `{>` followed by anything inside code (never valid Python: no valid snippet in 19,729 random ones had them) stay reserved and are an error; scripts call server commands by typed calls (S6). **[proposed]**
- **Escapes inside `>` code** are decoded only outside string literals, where a backslash is never valid Python; inside a string literal a backslash is Python's own, so `"a\\b"` means what Python says. **[proposed]**
- **Depth** is limited (eight levels, **[proposed]**, a number to measure, not a principle) and a deeper line is an error.
- A hole may not stand in the name of a command or subcommand. Holes are for arguments only. **[proposed]**

### 4.3 Escapes

Only three, and only these. **[decided 2026-11-23]**

| Written | Means |
|---|---|
| `\\` | one backslash |
| `\{` | a literal `{` |
| `\}` | a literal `}` |

- Any other backslash stays as it is: `\n` is a backslash and an `n`, there are no newlines, no `\xNN`, no quotes and no backslash-space. No quoting is added.
- **Decoded once, after scanning, per layer.** The scanner works on the raw text so that `\}` never closes a hole. Each language then decodes its own layer (for `>`, Monty's own string escapes are Monty's business and run inside Monty).
- **Results are never rescanned and never decoded again.** A value that comes out of a hole and holds `{` or `\` is plain text where it lands. This is what stops one player's text from becoming code in a command block.
- **A result from a client hole (`{.}`) spliced into a `/` line is escaped** (`\`, `{`, `}` each get a backslash), because the client's text was produced where the server cannot see it. **[decided]**

### 4.4 Evaluating a line

Innermost first, left to right; **any failure aborts the whole line** and the error names the hole (its number and where it starts in the line). Side effects of holes already evaluated are not undone, and the error says so. **[decided; the last sentence is mine]**

1. The scanner builds a tree. The outermost language and the command it names are known before anything runs, so the kind of each argument a hole lands in is known.
2. Each hole is evaluated and gives a value (4.5).
3. The outer command is built from its text and its values and checked against its `CommandSpec`, then permission-checked (section 7), then run.

**Cost of a line.** One step budget and one memory limit cover the whole line, so nesting cannot multiply the cost. Each hole result is limited to 1,024 bytes of text and the whole expanded line to 1,024 bytes. **[proposed]** **Measured (spike S4, 2026-11-24, release build, Windows, in process):** a line of `1 + 1` costs 4 steps and 3 us; `f(1)` 8 steps, 3 us; a host call 1.4 us (4.2 us for a whole line that is one call, name look-up included); 100 calls in one line 143 us. A step is 4 to 16 ns on ordinary bytecode, so 100,000 steps are about 0.5 to 1.5 ms: `fib(20)` is 252,000 steps and 1.9 ms, `fib(25)` 2.8 million steps and 45 ms. A runaway `while True` stopped at 10,240, 100,096 and 1,000,192 steps for limits of 10,000, 100,000 and 1,000,000, at the same loop count in each of three runs (the same cut-off). **Proposed defaults, a setting each:** 200,000 steps for a line typed in chat (about 1 to 3 ms), more for a registered script (to be set in phase 5 from a measurement of real scripts). Steps bound bytecode, not one huge native operation (the 2026-11-16 audit found single calls that run for seconds), so the memory limit and the removal of `re` stay.

### 4.5 Values, and what a hole is worth where

This is the "typed slots" decision. **[decided]**

- **A hole that is a whole argument of a typed slot** fills it with a typed value: `Number` takes an int or float (never a bool, NaN or infinity), `Integer` an int, `Identifier` a string that parses as a namespaced id, `Word` a string with no whitespace or control character. A value of the wrong kind is an error naming the slot; nothing is converted by guessing.
- **A hole inside a `Rest` argument, or in chat, is spliced as text:** `str()` of the value. A spliced value that holds spaces becomes more words, which is what pasted text does, and it is the *only* place text is spliced. Because `Rest` is a list of words, this is also the one place a stranger's text could change a command's meaning; section 6.2 closes it for nicknames.
- **What a hole gives** (Pythonic, **[decided]**): `{>...}` gives the value of its last expression; `{/cmd}` gives what the command returns, **`None` unless the command says otherwise**; `{.cmd}` the same for client commands (a string today).
- **A bare top-level line prints like the Python REPL:** `print()` output first, then `repr` of a value that is not `None`. So `>1+1` prints `2`, `>"a"` prints `'a'`, `>x = 3` prints nothing. A `/` line prints the command's replies as it does today and does not print its return value.

### 4.6 What a command returns

A handler may return a small `CommandValue` (None, bool, integer, number, text, list of values, and a plugin-defined record later) besides what it `reply`s. The default is `None`, so no command has to change. **[proposed]** `/tp` and the like keep working unchanged.

### 4.7 Examples

| Line | Meaning |
|---|---|
| `>1+1` | prints `2` |
| `/tp 50 65 {>server.players.by_nickname("Alex").z + 5}` | the hole is inside `/tp`'s `Rest` today, so its value is pasted as text; the owner's model line, with the name look-up the owner chose (section 8.3) |
| `>{"a": 1}["a"]` | a top-level `>` line takes the rest as code; no hole, so braces are just Monty's |
| `hello {>1+1} there` | chat; the hole runs as the sender and the chat says `hello 2 there` (see S3 for whether chat may hold holes at all) |
| `/cmd \{>1+1\}` | the text `{>1+1}` goes to the command as plain text |

### As built (phase 2, 2026-11-24)

The line language is the new crate `engine/line_language`, which knows no command and no interpreter: `scan` finds the holes and decodes the escapes, `evaluate` runs them and hands each command to a `Runner` as a `Call` (a name and the words after it), and `resolve_client_holes` is what the window does to a line before it sends it. The server's `Runner` is in `plugin_host` (`command_received` is `scan` then `evaluate`), the window's in `game_shell` (`.` lines are evaluated, `/` and `>` lines go through `ClientHost::line_for_the_server`, which `Session::send_command_line` calls). What differs from the text above, or was not said there:

- **Numbers.** Depth 8 and 1,024 bytes are as proposed; a line may hold at most **64 holes** (new, a number to measure: each hole runs a command, so a line must not hold a crowd of them). A hole's result is at most 1,024 bytes and the line it makes at most 1,024 (the wire's limit, checked in the window with the sigil counted, so the window never sends a line the server would call a protocol violation).
- **Typed slots** are as in 4.5, in `CommandSpec::parse_items`: a number takes a whole number or a finite one with a fraction and never a truth value, a whole number only a whole number (not `2.0`), a word and an identifier only text, and **the text `5` does not fill a number**. A word is made of text and a hole (`a{/x}`) only in the argument that takes the rest. A result with blanks in it is more words in a `Rest` and one value anywhere else (a `Word` refuses it).
- **A command returns by `Context::give(value)`**, not by its return value, so no handler had to change; `Context::fail(message)` says it failed, and the line stops. **No existing command calls `fail` yet**: `/tp`, `/level` and the others reply their own errors as they always did, so a line goes on after them (S12).
- **Errors** read `hole 2 (at character 9): usage: /go X`, the character counted with the sigil as the first, and say when holes before it had already run (`1 hole before it already ran and is not undone`). The line that fails in its own command, after its holes ran, says the same without naming a hole.
- **Replies of a command inside a hole are shown** to the player in the order they happen, before the outer command's (S13).
- **A `{.}` hole is run by the window before the line is sent** and replaced by its answer: in command text as text with `\`, `{` and `}` escaped (a result is never read as a hole, tested with arbitrary text), with control characters dropped; in code as a Python literal (`'Alex'`, `7`, `0.5`; a quote or a backslash in a string is escaped). A `{.}` hole with a `{/` or `{>` hole in it is refused: the window cannot wait for the server in the middle of a line (S11). The rest of the line goes as it was typed, byte for byte.
- **A line that starts with `/` and has a hole that is never closed** is refused by the window (nothing is sent); the server refuses the same line from a client that sent it anyway.
- **`>`**: the scanner knows code (the end of a `{>}` hole, the strings, the escapes outside strings, `{.` and a letter as a window hole, `{/` and `{>` refused) and the window turns `{.}` holes inside code into literals, but **nothing runs code**: a `{>}` hole on the server answers `code cannot be run yet: scripts are not built`, and a line that starts with `>` is answered the same by the window.
- **Chat holds no holes yet**: a chat line is plain text, braces and all (S3 is answered by phase 5, which brings the permission to run them).
- **Floats are written as Python writes them** (`2.0`, `1e+16`, `1.5e-05`; checked against the output of Python 3 on 30 numbers) from Rust's own shortest digits, which use no `libm`. Text in a list or a code literal is quoted as Python quotes it, except that a character Python cannot print but is not a control character is left as it is.

## 5. Chat

The owner asked for **a real chat first** (Q1). **[decided]**

- Plain text from one player to the others, with the sender's nickname. The command line stays.
- **Proposed:** a key (Enter or T) opens the box; a line with no sigil is chat; `/`, `.`, `>` are commands as today. A new server-to-client message `chat` (sender key, nickname, text, tick) is in the stream, so a replay shows it. Client-side rate limit and a server-side one. The text is plain: no holes run unless S3 says so; control characters are stripped; the length stays 1,024 bytes.
- Chat is itself a permission, `engine:chat.say`, class `change` (anyone speaks by default). Muting is a **guardrail deny** (7.4), not a special case.
- The box today has no multi-line and no text fields. The font is ASCII 8x8, so nicknames and chat are ASCII until the font changes. **[read]**

### As built (phase 1, 2026-11-24)

Chat and nicknames are built as 5 and 6.1 say, with these details: the protocol is version 14 (`Hello` carries the name asked for, `Welcome` the nickname made, `Chat` goes each way); the server makes the nickname when the proof is accepted, from the nicknames of the players joined at that moment (so two players who join in one tick get `Alex` and `Alex_2` in client-id order); chat is limited to a burst of 5 lines and then one line every 10 ticks (`ServerConfig::chat_burst`, `chat_ticks_between_lines`), and a player over it is told once a tick; `T` opens the chat line, `/` and `.` open it for a command, and a sent line is classified by its first character alone, so a line that used to be run as a server command because the slash was optional is now chat; `>` answers that scripts are not built. The key is `T`, not Enter, because Enter is what a player presses to confirm. Not built: holes (phase 2), the permission `engine:chat.say` and muting (phase 4), and the nickname in `/players`-style listings (there are none). Results: `docs/history/build-log.md` "Phase S1".

## 6. Who is acting

### 6.1 Identity and nicknames

- The key is who a player is. A nickname is an unverified label, sent at join, unique per server, and never used for authority. **[decided]**
- **Nicknames allow only A-Z, a-z, 0-9 and the underscore, at most 32 characters.** **[decided 2026-11-23, P8]** At least one. The server enforces it; a client name with other characters is not refused but made safe: other characters become `_`, an empty name becomes `player`. **[proposed]**
- **Uniqueness:** compared without regard to case (so `alex` cannot pass for `Alex`); a second player asking for a taken name gets a suffix `_2`, `_3`, kept within 32 characters. **[proposed, S5]** A nickname belongs to whoever holds it while connected; a grant is never made against a nickname.
- The server prints the first eight characters of the key's hexadecimal form beside the nickname in logs, so that two players who were both `Alex` on different days can be told apart (P8). **[proposed]**
- The join message carries the nickname, which changes the protocol and bumps its version; the recorded sessions are repinned for it with the cause checked, not assumed, as the identity change in 27e was.

### 6.2 Why the character set is also a safety rule

A stranger chooses their nickname. If a command block with operator power builds a command from `{>nearest.nickname}` and the value lands in a `Rest` argument as text, a space or another word in the nickname becomes extra arguments, which is a *confused deputy* (Shiro documents the same shape: a separator in a user-supplied id changes the meaning of a grant). With the character set above there is nothing in a nickname to split or to start a hole. **[inference, `permissions-survey.md` open problem 2]**

### 6.3 Actors

Every action has an actor, written in the log in full **[proposed, D6]**:

| Actor | Looks like in the log | Its power |
|---|---|---|
| A player | `Alex (a1b2c3d4)` | their groups, looked up at each use |
| The console | `console` | root |
| An owned command block | `block at x,y,z owned by a1b2c3d4` | the owner's groups, looked up **at each run** (a banned owner's blocks lose power at once) |
| An ownerless command block | `server block` | the `op` group, no identity **[decided]** |
| An unmarked function | the caller | the caller's power, the owner's data **[decided]** |
| A marked function | `function f of a1b2c3d4 on behalf of Alex` | the owner's power within the ceiling (9.4) |

**Permission-administration commands refuse every actor that is not a person or the console**, unless an op has allowed it on purpose. **[decided 2026-11-23, P5]** (A real mod, FTB Ranks, had this hole in 26.1.2.4.)

## 7. Permissions

Decisions P1 to P7 and P9 are **[decided 2026-11-23]**; the parts the owner did not rule on are **[proposed]**.

### 7.1 What a permission is

- Every command and subcommand has an id `pack:command.subcommand` (the subcommands being in `CommandSpec`, 8.1) and a **class** from a short fixed list: **read, change, permission, automation**. **[decided, P2]**
- A grant names an exact id, a subtree (`pack:ship.*`, meaning everything below `ship`, never a text pattern), a pack (`pack:*`) or a class. An empty part is an error. **[proposed]**
- **No tags in the first version.** **[decided, P2]** The design must not shut them out: a tag would be a fifth kind of target, and the open problem (7.3) is written down for then.
- A rename must not lose grants: grants are against ids, and a grant that points at no id is listed by `/perm` as dangling, not deleted. **[proposed]**
- Who may run `>` (Monty code) is a permission, `engine:script.run`, class `automation`, so that creating automation is itself a privilege. Default: ops only on a server, everyone in singleplayer. **[proposed, S2]**

### 7.2 Groups and actors

- An actor holds a **set of groups**; permissions are the **union**. `op` and `everyone` are built in, and `everyone` is editable like any other group. **[decided, R3]**
- A group's **rank** is a number that only says who may manage whom. There are no weights and no order of groups. **[proposed, D4]**
- A new key starts in `everyone`, which has the weakest set. Safety rests on that, because a fresh key costs a griefer nothing.
- **Op-only by default:** `gamemode`, `light`, `tp`. Everything else in the game's own pack starts open. **[decided]** It is the same mechanism as a pack's proposed default (7.5).
- **Singleplayer:** the local player is op, and root since there is no one else. **[proposed, S8]**

### 7.3 Which rule wins

The owner approved two layers (P1). **[decided]**

- **Layer 1, everyday rules** (groups, teams, personal grants): the **narrower rule wins**; at equal narrowness **deny wins**. "Alice is the exception" is one added rule.
- **Layer 2, guardrails**, written only by root: **a deny here always wins** and nothing local undoes it. Bans, quarantine, "nobody may". The weak spot of the one study (a group-wide ban a personal allow defeated) is why safety denies live here.
- **Narrower** needs a definition, because a rule is narrow in two ways, *who* and *what*. **[proposed, S1; medium confidence, flagged as the part most worth a playtest]**
  1. Compare **what** first: an exact id beats a subtree beats a pack beats a class.
  2. If the same, compare **who**: a personal rule beats a group's, a group's beats `everyone`.
  3. If still the same, deny wins.
  An admin who names one command meant that command, so what-first is the reading of "the exception is the more specific statement". The playtest fixtures (section 12) include the two-dimension conflict on purpose.
- **Never join two layers with OR.** A decision is the guardrail answer first, then the everyday answer. (Paper's "level passes OR node passes" can never take power from an op.) **[decided in effect, `permissions-survey.md` open problem 4]**
- Print the rule in two sentences in `help`, and make every explanation quote the sentence that applied.
- The `op` group is an allow on the whole tree at the lowest everyday rank, so that an explicit deny on one command can still apply to ops; guardrail denies bind ops too. Root and the console are not bound by the everyday layer, and a root key cannot be denied (7.7). **[proposed]**

### 7.4 What a decision looks like

The check returns a **record**, never a bare yes or no. **[decided, P7]**

```
Decision { allowed, layer, rule, source (group or grant), actor chain (incl. "on behalf of"), overridden rules }
```

Three typed queries are built on it in the first release, each naming **every** deciding rule: **`why`** (player, action), **`what`** (player: all they may do), **`who`** (action: all who may). The refused player sees what is missing in words; an op sees the whole record. A small buffer of recent refusals per player answers "why was Alice refused a minute ago" without tracing having been switched on. **[proposed, D7]**

### 7.5 Packs

- A pack declares a **proposed default** for each of its commands: nobody, op or everyone. The proposal is a **live bottom layer** evaluated at each check, not a copy written into a group, so an update or uninstall takes effect. The admin's rules sit above it. A server switch ignores pack proposals. **[decided, P3]**
- Packs start **closed**: a pack that proposes nothing is denied. At install the admin sees "15 new commands: 9 read, 4 change, 2 permission; proposed default: nobody", and `/perm new` lists what arrived and has no grant. **[decided, P3]**

### 7.6 Delegation

- One engine rule: **you may grant only what you hold, in the scope you hold it.** **[decided, P4]**
- Plus **management grants** per group ("may add and remove members of group `red`"). Root and the console are unrestricted and always logged.

### 7.7 Time, history, lockout

- **A grant may carry an expiry**, typed as a duration (`for 1h`), converted at the server's fixed tick rate and **stored as a game tick**, checked at each lookup. It must never be wall-clock time (hard rule). **[decided, P6]** (The conversion rate and its rounding are a number the code owns, not this spec.)
- **An append-only change log** (who, what, when as a tick, from which actor) from the first release; **undo later**. **[decided, P6]**
- **A ban** is a guardrail deny on the key. `revoke all` cannot strip a root key. The root list is a server file; the console is always root; an empty list always means **nobody**, never everybody. **[proposed, D9]**

### 7.8 The first start of a dedicated server (P9)

The owner agreed "yes for now" and asked what the setup code and the key file are. **[decided in principle; the detail is mine]**

- **The player's key file** is the identity file of 32 secret bytes (`identity` in the game's data folder, or `--identity FILE`). The public key is worked out from it and is who the player is. It is the thing a player must keep.
- **The server's root list** is a *different* file on the server: the public keys that are root. A dedicated server needs one, and a new server has none.
- **The setup code:** if the server finds no root key on start, it prints a one-time code (at least 64 bits from the operating system's randomness) **only on its console**. The first player to run `/perm become_root CODE` in the game becomes root, the server writes their public key into the root list, and the code is destroyed. A new code is made at each start for as long as no root key exists. Wrong attempts are counted and slowed, and the code is never in a log a player can read.
- The old route stays: an admin may write a public key into the root list by hand (`--root KEY` or the file), and a client command shows a player's own public key to copy.
- Why: a stranger can walk up to a new server, and nobody should have to copy a 64-character public key into a file by hand. The same trick is TShock's. **[`permissions-survey.md` D9]**

### 7.9 The commands an admin types

One grammar for every change **[proposed, D10]**, each printing a summary of what changed, and `help` listing only what the asker may run:

| Command | Does |
|---|---|
| `/perm grant WHO WHAT [for TIME]` | allow (a player, a group; an id, subtree, pack or class) |
| `/perm deny WHO WHAT [for TIME]` | deny; `/perm deny` in the guardrail layer is root-only (`/perm ban`) |
| `/perm revoke WHO WHAT` | remove a rule |
| `/perm group ...` | create, delete, add, remove, rank |
| `/perm why PLAYER ACTION`, `/perm what PLAYER`, `/perm who ACTION` | the three queries (7.4) |
| `/perm new` | commands that arrived and have no grant yet |
| `/perm log [N]` | the change log |
| `/perm export`, `/perm import` | the whole rule set as one string, for backup (later) |
| `/perm become_root CODE` | on a server with no root key yet, the first player to give the console's one-time code becomes root (7.8) |

## 8. Commands become richer

### 8.1 Subcommands in `CommandSpec`

**[decided]** Subcommands go into the spec (today `/level go NAME` and `/light multiscale` are a `Word` plus a `Rest` split by hand).

- A node of the command tree has **either** arguments **or** child subcommands, not both, and is named by a clean lowercase identifier (so `**` unpacking by name works, 4.1). **[proposed]**
- Each node carries its **class** and its pack's proposed default (7.5). The usage line, `help`, the permission id and the permission check all come from the same tree, so a command and its permission cannot drift apart. A self-check that every node has an id and a class, in the style of the message check, is part of the phase.
- The old whole-line commands keep working: nothing breaks that parses the same text.

### 8.2 The actor in a handler

A handler receives the actor next to the client (`Context` today has `client: Option<ClientId>`). Existing handlers that act on "the caller" keep working for a player actor; a block or function actor whose "me" is a stored key needs that player online for commands that act on them (**[decided]**: an offline owner means "me" does not resolve and the command errors). **[proposed in detail]**

### 8.3 What Monty can see

**Nothing is exposed until the owner has reviewed the list**: "we need to do a careful review of what data items will be exposed in monty and what names they will have there" **[decided]**. So this spec fixes only the mechanism and two names:

- The mechanism: **host objects** whose attributes are read live from the server's state, and whose methods are host calls **[read]**.
- `server.players.by_nickname("Alex")` gives a player object with a visible `.nickname` and `.key`. A nickname is never a key, so the by-nickname spelling keeps `Alex` from looking like something one can index by. **[decided 2026-11-23]**
- **Everything else** (positions, ships, blocks, the world, settings, and what is writeable) is in a separate document, the *data review*, written before phase 6 (section 12). Until then a script sees `server.players` and the caller's own data. **[open]**

## 9. Scripts

### 9.1 Where they run

- `>` and every `{>...}` run **only on the server**. **[decided]**
- **No parallelism and no snapshots:** scripts run on the server's thread at a fixed point in the tick, reading live state. The owner will "build without parallelism and without snapshots and see how it performs". **[decided]** Process or thread per player is **not** built; if scripts run in step-counted slices while the server waits, a worker would buy fault isolation only, no speed (**[my position, unmeasured]**).
- **Determinism.** The Monty fork has a deterministic step budget and memory limit, so a script is cut off at the same instruction on every machine **[read]**. A script's effect on the world is a command, and commands are in the stream, so a recording does not need to run scripts again. **[proposed]** The simulation's hard rules hold: no wall-clock time in a script (a script's time is the game tick), no std transcendental float functions (the fork's patched maths, owner 2026-11-15). **Audited (spike S3, 2026-11-24):** 28 built-in modules import (`sys typing asyncio pathlib os os.path math json re datetime unicodedata itertools dataclasses collections functools copy csv decimal fnmatch hashlib random statistics time tomllib base64 binascii bisect annotationlib`) and 43 other common names do not (`uuid secrets threading subprocess socket io struct enum string` and more). 146 probes were run in two separate processes under each policy. **Under `OsPolicy::default()`, 12 differ between processes: every clock call (`time.time`, `time.monotonic`, `time.perf_counter`, `time.localtime`, `datetime.now`) and every `random` draw. Under the pinned policy none differ.** So the host must set the policy, and set it again before every feed from the game tick: `datetime` is `Fixed` at the instant of the tick, `sleep` is `Zero`, `random_start` is `Seed` (derived from the world seed and the owner's key), process time stays `Zero` (its default). Nothing else in the audit was nondeterministic: `hash()` of strings and numbers, the iteration order of sets and dicts, `id()`, float formatting, sorting and `math` (the fork's `libm`) gave the same results in every process. `os.getenv`, `os.urandom` and `open()` are refused by the standard execution (the host answers no OS call), and `os`, `pathlib`, `asyncio`, `csv` and `tomllib` import but cannot reach the machine; whether they stay in a player's reach is a question for the data review. Two things to know: a `random` generator's state is part of a dump, so a session rebuilt from source starts its stream again; and `frozenset` hashes to 0 (harmless, noticed in the results). **[observed in two processes on Windows, and the pinned output was byte-identical to the same program's on Linux (WSL2, x86_64, 146 lines); one architecture only]**
- **Monty's `re` is disabled** in the fork by a deny list (the owner approved it on 2026-11-23): `re.sub` on large text is uninterruptible. The patch is made when building starts, not before.

### 9.2 What the box can type

One line, 1,024 bytes, and no Python indentation: indentation is only possible as spaces after a continuation prompt (Monty can tell when a line needs more, `detect_repl_continuation_mode`). Multi-line `def` entry is therefore **[open, S4]**: until text fields exist, a one-liner (`>def f(x): return x + 1`) works; a longer definition needs either a continuation prompt (the box stays open across lines until a blank one) or a script editor in a later interface. **I recommend the continuation prompt and no editor yet.**

### 9.3 Persistence

**[decided]** Per-player scripts are saved **on the server**, with no export to a file yet.

- **What is saved is source and plain values**, not the interpreter's state: the text of each `def` and `class`, and a store of plain values (None, bool, int, float, str, lists, dicts of those; a size limit). A restart replays the source and restores the store. The interpreter's `dump` is **not needed at first**: host objects die on dump and restore **[read]**, step counts move with the compiler when the fork changes, and the **measurement says the gain is small** (spike S4): replaying the source of a realistic session (27 functions and classes, a dictionary of 200, 1,000 numbers, 100 names; 34 snippets, 3.5 KB) takes 0.55 ms; loading its dump takes 0.13 ms, and the dump is 35 KB (9.9 times the source); the restored session gave the same answers as the live one. A session costs 39 KB empty and 133 KB with that content, so 1,000 sessions are 133 MB: sessions load on demand and are dropped when idle (counted in ticks, not time).
- A script's owner's session loads **on demand** (a command block runs while its owner is offline).
- A new save scope, `PerPlayer`, beside `Global` and `PerGrid`, keyed by the public key, in a part of the world file the world file format records; data of a missing plugin is preserved byte for byte like all other data. **[proposed]** The exact layout belongs to `world-file-format.md` when built.

### 9.4 Functions and who they run as

- An **unmarked** function runs with the **caller's power** and the owner's data. A **marked** function runs with **the owner's power, and only the owner can mark it**. **[decided]**
- Four additions, which the owner accepted ("I agree", P5): **[decided]**
  1. The owner **declares a ceiling** at marking time (a set of permissions); the owner's current power is cut to it.
  2. Every call is logged **on behalf of** the caller.
  3. **Anyone but the owner editing the function clears the mark.**
  4. If it takes a **target from its caller** (a player, a place, a variable name), it checks what the **caller** may do to that target.
- **Rule underneath:** write access to a block or a marked function must never be weaker than the power it runs with.
- Marked functions are the pattern of vanilla's `/trigger` (a level-0 verb a level-2 block reacts to); that precedent is **[recalled]**.

### 9.5 Shared variables

The owner **wants real shared access to the same variables** (Q11). **[decided]** The design below is **[proposed, medium confidence, S7]**, because the owner has not answered the sharing decision in the survey (D11).

- **Variables live in the server's store, never in a Monty heap**; many scripts, ships and blocks read and write the same ones.
- A variable has an **owner** and **grants**: (subject, verb, object, optional expiry). The verbs are kept apart: `read`, `write`, `share`, `call` (a function with the caller's power) and `call as owner` (the marked kind). A revoked grant leaves a tombstone ("taken away by K on tick N"), so "why not?" has an answer. A group grant covers its members live, so a team is a group.
- There is **no precedent in a game** for per-variable grants **[weak: shared searches]**; Roblox asset grants, Discord's per-command overrides and Google Drive roles are the nearest, and none of them is a game's scripting store.

## 10. Command blocks

Built **last**, since block data is deferred. **[decided]** The rules, from the owner's third and fourth rounds:

- A block **remembers who placed it**. It may or may not have an **owner**. Only the placer can **claim** it, with a button in its interface; others see the owner but no button.
- **Owned:** only the owner edits it, and it **runs with the owner's privileges**, looked up at each run. **Ownerless:** any opped player edits it, and it runs as **`server block`, with op power and no identity**. Ownership is cleared **only by breaking the block**.
- **`{.}` is not allowed in a block** (a client command has no client there).
- A block's "me" is a stored key; commands acting on "me" need that player online.
- Safeguards from the survey: the named stand-in actor, one command listing every ownerless block (T7 scored 1 for command blocks elsewhere), and a plain sign in the interface that says "runs with operator power". A banned owner's blocks lose their power at once because the guardrail ban is part of the same live lookup that gives a block its owner's groups (**to be tested when built**).
- Creating a block that runs `>` is the `automation` class, so it needs `engine:script.run`.

## 11. Compatibility with the project's rules

| Rule | How this meets it |
|---|---|
| Simulation bit-identical on Windows and Linux | Scripts' reach is commands in the stream; step budget and memory limit are deterministic; time is the game tick; expiry is a tick; no new transcendental functions. A module audit is spike S3. |
| Saved files hold only namespaced ids | Permission ids are `pack:name`; per-player data is keyed by key and its content is preserved when a plugin is missing. |
| The client's view comes only from the server's stream | Chat, nicknames and results travel as messages; a replay shows them. |
| Terrain generator versions | Untouched. |
| Dependency direction (games to plugins to engine) | The line scanner, the permission check and the Monty host live in the engine; a pack declares its commands and defaults; the engine does not know a pack. |

## 12. Phases

Tests first in each phase, the safest piece next, verify, continue (the project's phase rule). Each ends with `cargo test --workspace`, clippy and fmt passing and one commit. The order is the owner's: chat, nested parser, subcommands, ops, scripts, persistence, command blocks. **[decided]**

| Phase | Builds | Tests that lock it in |
|---|---|---|
| **S0 spikes** | **Run 2026-11-24** (`docs/history/spikes/scripting/monty_line_spikes/`): S1 worker latency per host call **not run**, since no worker is chosen (scripts run in process); S2 brace scanner against Monty's tokenizer, done; S3 audit of the fork's built-in modules for non-determinism, done; S4 step cost of lines, host calls, session size, dump and replay, done | the results in `docs/history/spikes/scripting/monty_line_spikes_results_2026-11-24.txt` |
| **1 chat** | nicknames (charset, uniqueness, protocol bump), the `chat` message, rate limit, box opens on a non-sigil line | wire round-trip; charset property test; two players with the same wish get distinct names; replay shows chat |
| **2 line language** *(built 2026-11-24, see "As built" after 4.7)* | scanner and evaluator for `/` and `.` holes (no Monty yet), escapes, typed slots, return values, errors that name the hole | scanner property tests; golden table of section 4.7; "results are never rescanned" test; client `{.}` escaping test |
| **3 subcommands** | the tree in `CommandSpec`, ids, classes, usage and `help` from the tree, the self-check | every existing command parses the same text as before; usage lines unchanged except the new ones |
| **4a actors and ops** | `Actor`, the `op` group, op-only defaults, `/perm become_root`, the root list, console as root | `/gamemode`, `light`, `tp` refused for a non-op with words; the first-start flow; empty list means nobody |
| **4b groups and grants** | groups, grants, the two layers, delegation, expiry in ticks, change log, `why/what/who`, `/perm new`, packs' proposed defaults | decision-record golden tests; the two-dimension conflict fixtures; a ban in the guardrail layer beats a personal allow; expiry is by tick (a clock-free test); a rename keeps grants |
| **P playtest** | the protocol in `docs/history/permissions-survey/notes-5-models-and-method.md` (five testers, three rounds, success criteria first) on tasks T3, T8, T11, T13 first | **no later than the first prototype of 4b's two layers** (P10) |
| **data review** | a document: what Monty sees, and its Python names, for the owner's yes | owner's review; no code |
| **5 scripts** | the Monty host, `>` and `{>}`, budget and limits, `server.players.by_nickname`, `re` denied | budget cut-off at the same step every run; `{>1+1}` prints `2`; a runaway loop is stopped; a script cannot reach what the actor may not |
| **6 persistence** | `PerPlayer` scope, source plus store, restart and sign-off survival, shared variables and grants | restart round trip; data of a missing plugin kept byte for byte; a shared variable two scripts both see |
| **7 marked functions** | marks, ceilings, "on behalf of", clearing on edit, the caller-target check | the confused-deputy tests (a function with a caller's target) |
| **8 command blocks** | after block data exists: ownership, claim, ownerless stand-in, ownerless listing | banned owner's block loses power at once; `{.}` refused in a block |

## 13. Questions this draft raised

**The owner answered yes to S1 to S9 on 2026-11-24** (and renamed the setup command, 7.8). S10 came from the spike and was answered the same day (4.2). None blocks phase 1 or 2.

- **S1. Which narrower?** What first, then who (7.3). *Recommend yes; confidence medium; playtest it.*
- **S2. Who may type `>` on a server?** Ops only by default, a permission any admin may grant. *Recommend yes.*
- **S3. Do chat lines hold holes?** `hello {>1+1}` run as the sender, so a player can write their position into a message. *Recommend yes, same permission as `>`; without it a hole in chat is plain text.*
- **S4. Multi-line code?** A continuation prompt, no editor yet (9.2). *Recommend yes.*
- **S5. Nickname clashes?** Case-insensitive, a `_2` suffix (6.1). *Recommend yes.*
- **S6. How does Monty code call a command?** Only through `{/...}` holes today. A way from code (`server.run("/tp 1 2 3")`, or `server.commands.tp(...)` with typed arguments) belongs in the data review. *Recommend typed calls, the same checks as a typed line.*
- **S7. Shared variable grants** as in 9.5. *Recommend yes; medium confidence.*
- **S8. Singleplayer:** the local player is op **and** root. *Recommend yes.*
- **S9. Setup code details** as in 7.8 (a new code each start while no root exists, rate-limited attempts). *Recommend yes.*
- **S10. Holes inside `>` code?** *Answered 2026-11-24:* `{.}` holes are allowed inside `>` code (4.2); `{/` and `{>` stay reserved for now (proposal, not yet put to the owner).
- **S11. A `{.}` hole with a server hole inside it** (`{.say {/name}}`)? Built as an error, because the window would have to wait for the server's answer in the middle of a line. *Recommend leaving it an error;* the other order (a server hole with a window hole inside) works. Raised by phase 2.
- **S12. Commands that reply an error but do not `fail`.** `/tp 1 2 x` replies its usage and a line goes on after it. Phase 3 rewrites every command's arguments into the tree in `CommandSpec`; it can make the checks that a command makes itself into failures. *Recommend yes, in phase 3.* Raised by phase 2.
- **S13. What a command inside a hole says.** Its replies are shown to the player as they happen, like a `print` inside a nested call in Python. The other choice is to keep them out (the hole is worth only its value). *Recommend showing them:* a player who types `{/fly}` in the middle of a line wants to know that it ran. Raised by phase 2.

## 14. What nobody has measured

Everything here is a design on paper. **Not run, and to be measured before the first build:** the brace scanner against Monty's tokenizer (S2); the cost of a hole (Monty's earlier measurement was 4.5 microseconds a host call in process, 2026-11-14, and a worker would add its latency); the dump and restore time of a realistic session; the limits in 4.4; the module audit (S3). The permission rule is one study of novices on file permissions, which did not test two layers or games (`permissions-survey.md` D1), so the playtest is the part of this plan that is evidence, not opinion.

## 15. Where each decision came from

| Decision | When | Where |
|---|---|---|
| Holes, sigils, typed slots, innermost first, any failure aborts the line | 2026-11-23 round 2 | owner's answers to Q4, Q5 |
| Escapes only `\\ \{ \}`, no quoting, no `{{` | 2026-11-23 rounds 4 and 5 | the owner's replies |
| `re` removed from the fork; op-only `gamemode`, `light`, `tp` | 2026-11-23 | the owner's replies |
| Pythonic results; no parallelism, no snapshots; by-nickname look-up; subcommands in the spec | 2026-11-23 round 3 | |
| Marked functions, command-block ownership, ownership cleared only by breaking | 2026-11-23 rounds 3 and 4 | |
| P1 to P10 | 2026-11-23 | answers to the survey's ten questions; P2 without tags, P8 with `[A-Za-z0-9_]` up to 32 |
| Real chat first; per-player scripts on the server, no export; shared variables wanted; no `server.players` until a data review | 2026-11-23 round 1 and 2 | Q1, Q10, Q11 |
