# Card 1: Minecraft command permission systems (who may run which command)

> Raw notes of research stream 1 of the permissions study: Minecraft command permissions (LuckPerms, vanilla ops, the Fabric and NeoForge permission APIs, FTB Ranks, Bukkit and Paper). Written by a research agent on 2026-11-23 and kept as it wrote it, except that local file paths were removed and two file names lost their code quotes. The survey that draws on these notes is `permissions-survey.md`; it says which claims were checked again. The tags are the agent's own: [re-read] and [raw] mean the source's own text was read, [read] a summary by the fetch tool, [secondary] a second-hand source, [recalled] memory, [inference] reasoning from cited facts.


Checked 2026-11-23. Desk research only: nothing was installed or run. Every statement about how a system behaves comes from
reading its documentation or source, not from running it. I say so wherever it matters.

Evidence tags: the brief's [re-read] [read] [secondary] [recalled] [inference], plus one local tag:
[raw] = the file's own text, read through the GitHub API (not summarised by a small model), read once.
"doc+code" = a documented rule that I also found implemented in the source; I count that as the second reading.
Budget: about 70 tool calls, 4 web searches (the vanilla rework, wildcard incidents, sign click events, namespaced-command bypass). Two fetches of
raw.githubusercontent.com and of luckperms.net failed or came back empty (the LuckPerms site draws its pages with JavaScript);
I read the wiki's own markdown source repository instead (github.com/LuckPerms/wiki, folder `pages/`).

---------------------------------------------------------------------------------------------------------------------------------

## PART 1. Direct answers to the specific questions

### 1.1 Naming of nodes, and how wildcards behave

- A node is a dot-separated string. LuckPerms (LP) lowercases it before lookup [raw, code]. Convention everywhere:
  `owner.area.thing`, e.g. `essentials.fly`, `minecraft.command.ban`, `luckperms.user.permission.set`.
- Sponge: case-insensitive, characters A-Z a-z 0-9 `_` `-` `.`, shape `<PluginID>.<MainGroup>.<Subgroup>` [raw, docs say
  "written for SpongeAPI 7, likely out of date"]. NeoForge: `modid.name` is only suggested [raw]. Fabric API v1: an
  `Identifier` `namespace:path`, and the path may contain `/` (LP turns `mymod:command/admin` into `mymod.command.admin`) [raw].
  lucko's older Fabric API: any string [raw].
- Wildcards differ by system, and this is the main trap:
  - LP: `a.b.*` and the root `*` are applied by LP's own engine. A check walks from the longest prefix up
    (`a.b.c` -> `a.b.*` -> `a.*` -> `*`) and the first wildcard that exists decides, true or false [raw code]. An exact node beats any
    wildcard, whatever the weights (see 2.1). Extras: regex nodes (`r=`), shorthand `{a,b}` and `{1-4}` [raw].
  - Plain Bukkit/Paper has NO engine wildcard. `essentials.*` works only because the plugin declares it as a parent node with a
    list of children. So `*` historically did not mean everything; admins relied on it anyway [secondary: Nucleus page, read].
    LP adds a real wildcard on top of Bukkit (config `apply-wildcards`) [raw].
  - Sponge: no `*`. A parent node implicitly covers all children unless a child is explicitly removed [raw docs].
  - FTB Ranks: implicit parent lookup (strip the last segment until a rank has a value); `*` inside a command name becomes `all` [raw code].
  - NeoForge: the API says no hierarchy at all; the permission handler decides what `a.b` means for `a.b.c` [raw].
- What the incident record says about `*` (honest summary): I found design warnings, not a dated post-mortem of a destroyed server.
  - Nucleus (Sponge plugin) page "wildcard, and why you should not use it" (site frozen, 2016-2020): most complaints came from
    wildcard grants switching on opt-in features (vanish on login, keep inventory...); undoing it needs negative nodes, which is error-prone [read].
  - LP's own docs: `*` is "not recommended" for SpongeForge [raw FAQ]; the `auto-op` option is recommended "instead of assigning a single *" [raw].
  - A search result [secondary]: a LP guide warns that a wildcard also covers nodes of plugins installed later.
  - The first step of LP's own setup is `lp user <you> permission set luckperms.* true` from the console [raw]; so `luckperms.*` is root.
  - FTB Ranks closed a real hole in version 26.1.2.4: commands that change ranks needed only level 2, so command blocks and signs
    could run them; now level 4 [raw changelog]. That is an escalation by a non-human actor (R2), not a wildcard, but it is the same family.
  - Spigot Jira SPIGOT-8134 "Vanilla Minecraft Permissions No Longer Work" and SPIGOT-6215 "Permission problems" came up in search; I did not read them [secondary].
  - Namespaced-alias bypass (`/minecraft:xp`, `/bukkit:pl`) is a known gap that several "command blocker" plugins exist to close [secondary: search snippets only].

### 1.2 How nodes map onto commands that have subcommands

| System | Unit that gets a node | Example |
|---|---|---|
| Paper/Bukkit, vanilla commands | the top-level command only, after following redirects (`/xp` is an alias, so it uses the node of `experience`, by the code; the Paper docs table says `minecraft.command.xp`, so docs and code disagree) | `minecraft.command.gamemode` covers every `/gamemode ...` [raw] |
| Paper, one exception | `/random` is "yes" without a sequence argument, "no" with one (a second rule inside one command) | Paper docs table [raw] |
| VanillaPermissions mod (Fabric, 26.3/26.2/26.1/1.21) | every Brigadier node (command word or argument word) | `minecraft.command.gamemode` plus `minecraft.command.gamemode.creative`; also selector paths such as `teleport.targets.targets.destination` [raw README] |
| FTB Ranks | every Brigadier node, derived automatically at startup | `command.gamemode.creative` [raw code] |
| LuckPerms' own commands | one node per command path (over 100), plus opt-in extra checks on the arguments | `luckperms.user.parent.add` and `luckperms.user.parent.add.admin` [raw] |
| Sponge advice | one node per capability | `myplugin.commands.teleport.execute`, `.all`, `.worlds.<world>` [raw docs] |
| Fabric API v1 / lucko API | whatever the mod author writes | `mymod:command/main`, `mymod:command/admin` as a sub-node [raw] |

Vanilla itself checks a level mostly on the root word only. Once you pass the root, every sub-word is open [raw README of VanillaPermissions,
consistent with the Paper `/random` exception being unusual].

### 1.3 Tags, categories, whole-plugin grants (T6, T13)

- No tags exist in any system I read. Nothing says "all commands tagged building".
- Substitutes: (a) a parent node you build yourself (Bukkit `children`, Sponge parent node, LP wildcard `pack.*`, FTB implicit prefix);
  (b) a group that holds a list of nodes; (c) Sponge `PermissionDescription` "role templates" (user, staff, admin): the plugin says which
  role it suggests for each node, and this changes nothing by itself [raw docs]; (d) LP `bulkupdate` or web editor to apply a list.
- Whole-plugin grant in one step: LP `lp group g permission set pack.* true`; Sponge: grant `pack`; FTB: grant `command.<root word>` per root word.
- Deny a whole pack in one step: LP `pack.* false` only wins over NODES THAT ARE NOT OTHERWISE SET (exact nodes beat wildcards), so any
  explicit `true` held by the player or by any group still wins [doc+code]. Sponge says explicit removal beats the parent [raw docs].

### 1.4 How the default of a brand new node is decided (T6)

| System | Who decides the default | What an undeclared or unknown node gets |
|---|---|---|
| Bukkit/Paper | the plugin's `plugin.yml` `default:` (`true`, `false`, `op`, `notop`); Paper adds a top-level `default-permission` that itself defaults to `op` [raw docs] | `op` (constant `Permission.DEFAULT_PERMISSION` = OP; `PermissibleBase.hasPermission` falls through to it) [raw code] |
| LuckPerms on top | a set value always wins; an unset node "usually" means false, but Bukkit defaults are applied after LP's own nodes [raw] | false, or the plugin's default; the switch `apply-bukkit-default-permissions: false` makes LP ignore every plugin-declared default (closed world) [raw] |
| NeoForge | the mod, at registration: a default resolver function is mandatory per node; nodes must be registered before use or the call throws [raw] | cannot be asked about: `UnregisteredPermissionException` |
| Fabric API (both) | the mod, at the call site: `check(src, node, default)`, `require(node, level)`; no argument means false [raw] | false |
| Sponge | nobody by default; plugins are advised NOT to grant defaults; with no value the answer is denied [raw docs] | denied |
| FTB Ranks | the command's own original requirement (it is kept as fallback) [raw code] | unchanged vanilla or mod behaviour |
| VanillaPermissions mod | vanilla's level check on the root; sub-nodes are open unless denied [raw] | open under the root |

### 1.5 What an admin actually types (LuckPerms unless stated)

- T2 moderator (Paper, vanilla nodes): `lp creategroup moderator`; `lp group moderator permission set minecraft.command.kick true`;
  same for `minecraft.command.ban`, `minecraft.command.pardon`, a mute node of a plugin (vanilla has no mute); `lp user Alice parent add moderator`.
  About 6 to 8 commands, or one web-editor session (`/lp editor`, add the nodes, Save, paste the generated command). To be safe add
  `... minecraft.command.gamemode false` and `... minecraft.command.teleport false`. Traps: an op bypasses everything; on Paper a vanilla
  command passes if the vanilla level suffices OR the node is true (see 4.1), so a moderator who is also a level-2 op still has `/gamemode`.
  Vanilla alone: impossible, because levels are cumulative and level 2 (gamemode, tp) sits under level 3 (ban, kick) [the cumulative rule is read; which commands sit at level 2 and 3 is recalled, not checked].
- T3 temporary: `lp user Alice permission settemp minecraft.command.tp true 1h` (one command). Expiry is audited every 3 seconds. The default
  `temporary-add-behaviour: deny` makes a second `settemp` on the same node fail; the optional modifiers are `accumulate` and `replace` [raw].
- T4 why refused: one step: `lp verbose command Alice tp Bob` (needs the node `luckperms.verbose.commandother`; it makes Alice run the command and prints
  every node checked with its result). Or three steps: `lp verbose record Alice`, Alice runs it, `lp verbose upload` (web viewer with the origin and
  stack trace). Then `lp user Alice permission check minecraft.command.tp` and `lp user Alice parent info` [raw].
- T7 audit: who can run Y: `lp search minecraft.command.tp` (holders of that exact node; it also takes `~~` for SQL-style patterns, e.g. `lp search ~~ minecraft.command.%`).
  I believe, from the wording, that it does not expand wildcards or inheritance [inference]. What X can do: `lp user X permission info` (own nodes),
  `lp user X parent info`, and `lp tree . X` (a colour-coded tree of every node the server has seen, for an ONLINE player) [raw].

---------------------------------------------------------------------------------------------------------------------------------

## PART 2. The cards

## 2.1 LuckPerms (Bukkit, Paper, Fabric, NeoForge, Sponge, Velocity, Hytale beta), wiki and master source read 2026-11-23
Depth: deep

**Model in five lines [raw]**
1. Subjects are users (keyed by UUID; the name is refreshed at login) and groups. Everything is stored as a node: a permission, a group membership
   (`group.<name>`), a weight (`weight.<n>`), a prefix, a meta value. A node has a value (true/false) and optional expiry and contexts.
2. Grant and deny are both first-class: `permission set <node> true|false`. Unset means "undefined", which usually ends as false.
3. Inheritance: a user or group has parent groups; every user is in `default`. Tracks (ladders such as default -> helper -> mod -> admin) exist only
   for promote/demote and do not affect inheritance.
4. Contexts (`key=value`): `server`, `world`, `gamemode`, `dimension-type`, `proxy` built in; plugins can add more (WorldGuard regions need the ExtraContexts plugin).
   A node applies only if the player satisfies one value for each context key the node names.
5. Defaults: group `default` for everyone; its name is NOT configurable (the wiki calls this the most disliked feature) and the fix offered is to let
   `default` inherit another group, e.g. `member`.

**Granularity [raw]**: per exact node; per wildcard prefix; regex; per context (server, world, gamemode, dimension, custom). Not per tag, not per object, not per region
without an add-on. Per subcommand only if the plugin gives subcommands their own node.

**Groups, roles, teams [raw]**: a user may hold many groups. Priority is the integer `weight` (higher wins; any integer incl. negative; the wiki advises gaps of 100).
Temporary membership: `parent addtemp`. `parent set` replaces, `parent add` adds. Primary group exists only for display and tie-breaks.

**When rules conflict: the exact algorithm [doc+code]**
1. Only nodes whose contexts the player currently satisfies are considered (offline players have no context).
2. The holders are visited in this order: the user first, then parent groups. Default traversal is depth-first pre-order (config
   `inheritance-traversal-algorithm`, other values breadth-first and depth-first post-order). Sibling groups are ordered by weight, highest first.
   On equal weight a user's primary group comes first, otherwise the previous order is kept [raw code]. I read that previous order to be temporary
   memberships first and then name order; this tie-break is not in the wiki pages I read, confidence medium [inference].
3. Inside one holder, nodes are ordered: a node with contexts before one without; one with a server context before one without; then one with a world
   context; then bigger context sets; then special node types before plain permissions; then temporary before permanent; then sooner expiry first;
   then more specific wildcard; then key; then value [raw code NodeComparator and ContextSetComparator].
4. For each exact key the FIRST node met wins (the code uses put-if-absent). So: own nodes beat inherited; server beats global; world beats global;
   temporary beats permanent; sooner temporary beats later temporary. These are exactly the rules the wiki lists in Advanced-Setup [raw].
5. A check on a node then runs through "processors" in order and the first one that answers wins: exact key; (Bukkit: child permissions); regex;
   wildcard (longest prefix first, then `*`); Sponge implicit wildcard; (Bukkit: two map processors for plugin-declared defaults, one of which can let default-negated nodes beat wildcards through the setting apply-default-negations-before-wildcards; I read their order, not their code); (Bukkit: op returns true for anything
   still undefined); on Fabric/NeoForge an integrated-server owner gets true for anything still undefined [raw code]. A true OR false wildcard answers.
6. Consequence that surprises people (derived, not run): weights decide between nodes with the SAME key only. A `* false` or `pack.* false` in a very
   heavy group does NOT beat an exact `pack.x true` in a light group, because the exact key is looked up first [doc+code, inference for the consequence].
7. Each answer carries which node and which processor decided, and an "overridden" earlier answer if one was overridden [raw code and its unit test]. This is what
   the explain tools show.
Count of distinct precedence rules an admin must know: about 8 (exact vs wildcard; wildcard length; own vs inherited; weight order; server/world specificity;
temporary vs permanent; expiry order; Bukkit defaults/op) plus config switches that turn processors on and off (`apply-wildcards`, `apply-regex`,
`apply-shorthand`, `apply-bukkit-*`, `include-global`, `apply-global-groups`).

**Delegation and escalation [raw]**: whoever holds `luckperms.user.permission.set` (or `luckperms.user.parent.add`) may set ANY node or add ANY group to ANY user,
including themselves, unless the opt-in setting `argument-based-command-permissions` is on (it is off until enabled). With it on, LP checks extra nodes:
`<base>.modify.self|others`, `<base>.modify.<group>`, `<base>.usecontext.<key>.<value>` and argument nodes such as `luckperms.user.parent.add.<group>`;
all must pass (a false anywhere denies, else one true is needed). There is NO built-in rule "you cannot grant what you do not have". The wiki's own recipe for
"promote people but not to or past their own rank" needs five nodes and brace shorthand such as `luckperms.user.promote.staff.{admin,owner}` set to false. `lp verbose command`
can run a command AS another user and has its own node, which `luckperms.*` includes. `bulkupdate` is console-only and asks for a confirmation code so nobody can "sudo the console".

**Automation [inference + raw]**: LP knows only players (and, on Hytale, a few non-player entities left to the game). Command blocks are not LP users. In Bukkit a command block
acts like an operator (I infer this from how Bukkit's console-type senders are op); Paper has a setting that makes command blocks obey the vanilla level as well
(see 4.1). So "limited power for a command block" cannot be done in LP.

**Admin interface [raw]**
- Typed commands, all tab-completable. Web editor: `/lp editor [all|groups|users|online|<filter>]` opens a session; edits are made on a website; Save generates ONE command
  that you paste into the game or console; a summary of the change is printed. The server needs no inbound connection.
- Explain: `verbose on|record|off|upload [filter]` (filter language `&`, `|`, `!`, parentheses, matches the start of the node or the user name), `verbose command <user> <cmd>`,
  `permission check`, `tree [scope] [player]`, `search`, `user info`, `parent info`, `permission info [priority|abc sorting]`.
- Audit: `log recent|search|userhistory|grouphistory|trackhistory`, `log notify on|off` (notifications default on).
- Bulk: console `bulkupdate <users|groups|all> <update|delete> ... "permission ~~ essentials.%"` (SQL-like, with confirmation). Backup/move: `export` and `import`
  (gzip JSON; `--replace`; `--without-users`; `--without-groups`; `--upload` for a web code). Temporary: `settemp`, `parent addtemp`.
- Sync across servers needs a database plus a messaging service; changes on one server are otherwise not seen by others [raw FAQ].

**Safety [raw]**: nobody has LP commands after install except the console; ops also may, because `commands-allow-op` is shown as true in the config page (the real default file was not read), and `auto-op` can replace vanilla
ops with a node (`luckperms.autoop`, re-checked only at join and world change). Root wildcard `*` and `luckperms.*` are root. The Fabric FAQ (possibly stale for 26.x) says
LP cannot gate vanilla commands there; see 3.4 and 4.2. Running another permission plugin in parallel is the top cause of "my permissions do not work" [raw FAQ].

**Task table T1..T13 (LuckPerms alone, on Paper unless noted)**

| Task | How, with step count | Score | Tag |
|---|---|---|---|
| T1 one friend builds in one area | Not LP's job: a region/claim plugin owns membership. LP can scope nodes by `world=`; region contexts need the ExtraContexts add-on. 2 plugins, 3+ steps | 2 | [raw] mention of ExtraContexts; WorldGuard side [recalled] |
| T2 moderator | `creategroup` + one `permission set` per command + `parent add`; 6-8 commands or one web editor session; add explicit false for gamemode/tp; op status and Paper's OR rule can still leak | 4 | [raw, doc+code] |
| T3 one command for an hour | `permission settemp <node> true 1h`; 1 command; self-ending | 5 | [raw] |
| T4 why refused | `verbose command <user> <cmd>` (1 step) or record/run/upload (3); `permission check`. Does not explain refusals that are not LP checks (vanilla level, plugin logic) | 4 | [raw, code] |
| T5 strip a griefer and keep out | `user X clear` returns them to `default` rights; below that you need a `jailed` group with explicit false on each default node (a heavy `* false` does not work, see rule 6); keeping out is the server ban (by UUID, survives rename), not LP | 3 | [doc+code, inference] |
| T6 new plugin, 15 commands | default comes from the plugin (undeclared = op); nodes appear in `tree`/`verbose` only after first use; grant `pack.* true` (1 step); deny whole pack is weak (explicit trues win); switch `apply-bukkit-default-permissions false` closes all plugin defaults | 3 | [raw, doc+code] |
| T7 audit | `search <node>` (exact holders), `user X permission info`, `parent info`, `tree . X` (online only, effective, coloured). 3-4 commands, no single "effective rights of X incl. wildcards" list | 3 | [raw], wildcard expansion not seen [inference] |
| T8 delegate a team | enable `argument-based-command-permissions`; give helper `luckperms.user.parent.add`, `.add.<team>`, `.remove`, `.remove.<team>`, `luckperms.modify.user.others`, `luckperms.usecontext.global`; deny `luckperms.modify.user.self`. Works, but about 6-7 nodes, one config switch, and you must know it | 2 | [raw] |
| T9 bootstrap and recovery | first admin: console `lp user <you> permission set luckperms.* true`; ops may too (`commands-allow-op` true); console always recovers; integrated-server owner gets everything not explicitly denied | 4 | [raw, doc+code] |
| T10 team of 3, shared thing | groups hold members (`creategroup`, 3x `parent add`, later `parent add/remove`); but LP has no objects: the plugin must check a node such as `plugin.vehicle.<id>` | 2 | [inference] |
| T11 automated actor, limited power | not possible: no subject for command blocks; "who it runs as" not shown | 1 | [inference] |
| T12 change, undo, audit | effect at once on the server (caches cleared); `log recent/search/userhistory`; no undo command (re-run the inverse, or restore an `export`); multi-server needs messaging | 3 | [raw] |
| T13 everything except one | allow `pack.*`, deny exact `pack.fill`: exact beats wildcard, longer wildcard beats shorter, so it is predictable; no tags (1). Combined | 3 (deny-inside-allow alone 4) | [doc+code] |

**Scores**
- S1 predictability 3: the documented rule list is short and matches the code, but weights only order equal keys (surprise in rule 6), the tie-breaks are undocumented, and
  Bukkit/op layers sit on top [doc+code].
- S2 explainability 4: every result carries its deciding node and processor, verbose shows every check live, a web viewer shows the origin down to the code line, tree shows
  effective rights per player [raw]. Not 5: a refusal made by something other than LP is invisible, and wildcards are not expanded in `search`.
- S3 safety 3: strong console recovery, action log, confirmation on bulk edit; weak: `*` and `luckperms.*` are root, no "cannot grant what you lack" rule, argument checks
  opt-in and awkward, ops bypass by default.
- S4 scale 4: groups + weights + wildcards + regex + shorthand + web editor + `bulkupdate` + export/import keep 100 commands x 20 roles to a few hundred rows [inference from features;
  I did not measure].
- S5 learning curve 3: easy first hour (the wiki's first group takes 4 commands), hard once defaults, op, contexts, children and weights interact; the maintainers say the
  default-group design is disliked [raw].

**Pain points and incidents**: see 1.1. Also: a refused `/gamemode` on Fabric because LP cannot see vanilla checks (FAQ, may be stale) [raw]; a second permission plugin silently takes over [raw FAQ];
"op still works after I negated the node" is structural: the op processor and Bukkit defaults answer for anything not explicitly set [doc+code, inference].

**Lessons for R1..R9**
- R9, copy: return the deciding rule and source with every answer, keep the overridden answer, offer "run this command as user Y and show the checks" (LP's `verbose command`), and a
  viewer. Adapt: LP's verbose must be switched on beforehand; a small always-on buffer of the last refusals per player would answer "why was I refused?" after the fact.
- R4, avoid: letting an exact allow in a light group beat a wildcard deny in a heavy group, without saying so. Prefer ONE stated order: most specific rule first, then deny before allow at equal
  specificity, then weight only to order rule sources. Confidence: medium-high that this is easier to explain; I did not test an alternative.
- R3, copy: users hold many groups; caches are recomputed on change, nothing frozen. Avoid: undocumented tie-breaks; require unique weights or define a documented tie rule.
- R1, adapt: `default` as a fixed built-in group that may itself inherit a custom group is the proven workaround; build `everyone` that way from the start. Store by identity, show the nickname.
- R7, copy: web editor producing one pasteable command with a summary; console-only bulk edit with a confirmation code; export/import file; tab completion.
- R8, copy: console is root; the integrated-server owner is a last-resort "true" that explicit rules can still override.

**Sources**: github.com/LuckPerms/wiki `pages/Usage.md`, `Context.md`, `Verbose.md`, `Weight.md`, `Permissions.md`, `Argument-based-command-permissions.md`, `Permission-Commands.md`, `Parent-Commands.md`,
`Bulk-Editing.md`, `Tracks.md`, `Default-Groups.md`, `Log-Commands.md`, `Web-Editor.md`, `General-Commands.md`, `Configuration.md`, `Advanced-Setup.md`, `FAQ.md`, `Why-LuckPerms.md`, `Hytale.md` [raw].
github.com/LuckPerms/LuckPerms: `common/.../calculator/*`, `node/comparator/NodeComparator.java`, `context/comparator/ContextSetComparator.java`, `inheritance/InheritanceComparator.java`,
`model/PermissionHolder.java`, `bukkit/.../calculator/*`, `common/minecraft/.../calculator/*`, `fabric/.../listeners/*`, `neoforge/.../service/NeoForgePermissionHandler.java`,
unit test `PermissionCalculatorTest.java` [raw]. The luckperms.net/wiki pages themselves came back empty (JavaScript) [read, failed].

---------------------------------------------------------------------------------------------------------------------------------

## 2.2 Vanilla Java Edition (op levels), wiki read 2026-11-23
Depth: deep

**Which version the wiki describes**: the page "Permission level" does not tie its Java text to any version; its footer says last edited 6 Nov 2026 and the sidebar lists Java 26.3 as the latest [read].
The `server.properties` page covers keys up to Java 26.4 (snapshot 9) with defaults "as of 26.3" [read]. Neither page describes the rework in 2.2.5 below.

**Model in five lines [read]**
1. Protected: the server, from players. Subject: a player (by UUID in `ops.json`), the console, command blocks, functions, and entities as sources.
2. Grant only. There is no deny and no per-command grant: you are given one level 0 to 4, and each command needs a level.
3. Levels are cumulative. Page wording: 1 Moderator (bypass spawn protection), 2 Gamemaster (most gameplay commands, command blocks, game mode, difficulty, selectors), 3 Admin (multiplayer
   management), 4 Owner (everything, incl. stop). Level 0 is everyone.
4. `ops.json` entry: `uuid`, `name`, `level` (if absent it takes `op-permission-level`, default 4), `bypassesPlayerLimit`. `/op` gives `op-permission-level`. Singleplayer or LAN owner with cheats = 4.
5. Defaults: a non-op has level 0.

**Other settings [read]**: `function-permission-level` default 2 (range 1-4; added in 1.14.4 pre-release 4); the old `enable-command-block` property was removed in 25w35a and replaced by the game
rule `enableCommandBlocks`; `white-list` became true by default in 26.3 snapshot 2; a "management server" (a JSON remote-control protocol) arrived in 25w37a with a secret sent in a header and TLS
on by default (off until `management-server-enabled`). I did not read what the management protocol lets a client do with operators.

**2.2.5 The 2025-2026 rework (evidence is code, not documentation)**: the server code now has a `PermissionSet` object per source, with entries that are either a command level
(`Permissions.COMMANDS_GAMEMASTER`, `COMMANDS_OWNER` ...) or a named atom with an `Identifier` such as `minecraft:commands/entity_selectors` [raw: LuckPerms Fabric listener wraps `PermissionSet`,
`LevelBasedPermissionSet`, `Permission.Atom`; FTB Ranks calls `player.permissions().hasPermission(Permissions.COMMANDS_GAMEMASTER)`; Paper main (pushed 2026-11-22) has
`hasPermission(net.minecraft.server.permissions.Permission, String)`]. LP only intercepts atoms; level checks fall through to vanilla. A search snippet says the entity-selector atom arrived in 26.2
[secondary, not verified]. Fabric API now has an official `fabric-permission-api-v1` module (date not found), and lucko's API releases simply track game versions: 1.21.9 (2025-10-01), 1.21.11 (2025-12-09), 26.1 (2026-03-25) [raw releases].
I could NOT pin the first version of the rework: the wiki pages for 1.21.11 and 26.2 show nothing about it in their first 100,000 characters [read]. Treat "vanilla now has named permissions
but still decides every command by level" as [inference] with medium confidence.

**Granularity**: per command by level only (root word). Not per subcommand, pack, tag or object.
**Groups**: none; one number. **Conflicts**: none possible (one number). **Delegation**: a level-3 admin can use `/op`; the new op gets `op-permission-level` (default 4), so I believe a level-3 admin
can create level-4 owners [recalled, NOT verified; worth checking before you rely on it].
**Automation**: functions run at `function-permission-level`; command blocks at level 2 [recalled]. The wiki's `/execute` page says nothing about `as` and permissions [read]; Paper's source copies
the source's permission set in `withLocation`, so I infer `/execute as/at` keeps the CALLER's permissions while changing the entity [inference, medium]. Signs and clickable chat text run commands as
the clicker: signs got click events in 14w25a and ran them with level-4 power; non-ops could run op commands from signs (bug MC-201807); a confirmation screen for click events running operator
commands appeared around 25w20a [secondary: search snippets and a mod page].
**Admin interface**: `/op`, `/deop`, `ops.json`, console. No "why refused" (the client simply hides or rejects the command). No audit besides the admin-command log game rule [recalled]. No temporary grant.
**Safety**: default `op-permission-level` 4 gives an op everything including `/stop` and `/op`; the SSP owner is 4.

| Task | How | Score | Tag |
|---|---|---|---|
| T1 | impossible (spawn protection radius and op only) | 1 | [read] |
| T2 | impossible: levels are cumulative, so ban/kick (3) comes with gamemode/tp (2) | 1 | [read] |
| T3 | impossible without a hack (`/op` is permanent) | 1 | [inference] |
| T4 | no tool | 1 | [read] |
| T5 | `/deop` + `/ban` (2 commands, ban is by account) | 4 | [recalled for ban by UUID] |
| T6 | the mod chooses a level in code; admin cannot change | 2 | [inference] |
| T7 | read `ops.json` | 2 | [read] |
| T8 | impossible; possible escalation via `/op` | 1 | [recalled] |
| T9 | console `op <name>`; or edit `ops.json` | 4 | [read] |
| T10 | impossible | 1 | [inference] |
| T11 | `function-permission-level` and command-block level are the only dials | 2 | [read] |
| T12 | `/op` `/deop` immediate; edit-while-running behaviour of `ops.json` is not on the wiki page | 2 | [read] |
| T13 | impossible | 1 | [read] |

Scores: S1 5 (one number) [read]; S2 1 (no reason shown) [inference]; S3 2 (level 4 default, no deny, `/op` hazard); S4 1 (5 levels); S5 5.
**Lessons**: the whole ecosystem below exists because one cumulative number cannot express T2. Copy the levels as DEFAULT BUNDLES only (op = everything), never as the model (R3, R4).
Avoid: sources that borrow the caller's full power (signs, clickable text, functions): in every case the later fixes were "ask first" screens or higher required levels (R2).

**Sources**: minecraft.wiki Permission_level, Server.properties, Ops.json, Commands/execute, Java_Edition_1.21.11, Java_Edition_26.2 [read]; Paper patch of CommandSourceStack [raw]; LuckPerms and FTB Ranks source [raw].

---------------------------------------------------------------------------------------------------------------------------------

## 3. Permission APIs for mod authors

### 3.1 Fabric: lucko's fabric-permissions-api and Fabric API's `fabric-permission-api-v1`
Depth: deep for the API, "n/a" for admin tasks (an API has no admin; a provider such as LP supplies it)

**How a mod declares, and a separate system decides [raw]**
- lucko API (`USAGE.md`): the mod calls `Permissions.check(source or entity, "mymod.node")`, optionally with a fallback `true` or a vanilla level such as 4, or `Permissions.require("node")` inside a
  command's `requires(...)`. Offline players: `check(uuid, node)` returns a future. Nothing is registered up front. A provider listens to `PermissionCheckEvent` and returns TRUE, FALSE or DEFAULT.
  Versions: 0.7.0 for 26.1, 0.6.1 for 1.21.11, 0.5.0 for 1.21.9-10 [raw].
- Fabric API v1 (in the Fabric repo, default branch 26.3): nodes are typed (`boolean`, integer limit, string, or a codec-defined type) and keyed by an `Identifier`; the check goes
  through a `PermissionContext` (entity, command source, offline player, or a custom one that can carry a block position). A provider registers `PermissionEvents.ON_REQUEST`; callbacks are called in order and the
  first non-null answer wins, null means "pass". Handlers must be thread-safe and fast; offline lookups may be slow. The helper `PermissionPredicates.require(node, default|vanilla level)` is meant for commands.
  The package doc gives three example uses: commands for helpers/moderators/admins, dynamic limits (warps, protected area size), and "may this special interaction happen inside this claim" [raw].
- No registry: a node exists only when some code asks about it, so an admin cannot list a pack's nodes in advance. LP shows them after first use (`tree`, `verbose`).
- Several providers: in v1 the first non-null answer wins by registration order [raw]; I did not read how the lucko API orders them; no explicit "choose one" setting (contrast NeoForge).

**Task table**: T6 default is the mod's call-site argument (score 3: safe if the author picks false or a level); T4 no explanation from the API itself (n/a); T13 hierarchy is left to the provider (n/a);
T11 context can carry the actor and a position, so a claim or block-owner module can answer "may this actor do this here" (score 3 for the hook, [inference]); all other tasks n/a.
S1 n/a, S2 n/a (provider), S3 3 (defaults are per call site, no central review), S4 n/a, S5 4 (a mod author writes one line) [raw].

### 3.2 NeoForge (and Forge) PermissionAPI, source of branch 26.3.x
Depth: deep for the API
- Declare: every node must be registered in `PermissionGatherEvent.Nodes` and stored in a static field; asking about an unregistered node throws [raw]. A node has: a name (`modid.path`, suggested),
  a type (boolean, string, integer...), a MANDATORY default resolver (a function of the player, UUID and dynamic context), optional dynamic context keys, and an optional readable name and description meant for
  handler UIs [raw].
- Decide: a separate "handler" is chosen by a server config value at startup; the built-in handler just runs each node's default resolver (so with no permission mod, every node falls to its author's default).
  LuckPerms' handler answers from its data and falls back to the node's default resolver when LP has no answer [raw code].
- Hierarchy: explicitly none in the API ("`modid.name` is not a parent of `modid.name.sub`"); the handler decides [raw].
- Lessons: the registry up front makes "list everything this pack adds and its default and its description" possible (R6, R7), and a mandatory default makes "undeclared" impossible. Cost: one more line of boilerplate
  per node and no dynamic nodes without a dynamic context.
- Task table: T6 score 4 (list + default + description exist before use) [raw]; T4 n/a; T13 n/a (handler); others n/a. S3 4 (mandatory default, unknown nodes rejected), S5 3 [inference].

### 3.3 FTB Ranks (admin layer for Fabric/NeoForge modpacks), main branch pushed 2026-11-16
Depth: deep
**Model in five lines [raw code and changelog]**
1. Protected: modpack servers. Subjects: players. Roles are "ranks" stored in `ranks.json5` (and player membership in `players.json5`); ranks are namespaced `server.` (admin's) or `pack.` (shipped by the
   modpack; read-only for admin commands).
2. A rank has a `power` (integer), a `condition` (when it applies for a player) and a list of nodes with true/false, string or number values. Conditions include always, default (added by hand), op, playtime,
   stat, dimension, spawn area, creative mode, fake player, "rank added", "rank applies", and combinators and/or/xor/not.
3. Ranks active for the player are sorted by power, highest first. For a node, the first rank that defines it wins; if none, the last segment is cut and the search repeats up to the root. So the most specific
   defined node wins across all ranks; power decides between ranks that define the same node.
4. On startup it walks every Brigadier command node (mods included, forks skipped), replaces the node's requirement by a wrapper and names it `command.<path>`. The player's rank value decides; if no rank says
   anything, the ORIGINAL requirement of the command still applies. Non-player sources (command blocks, console) keep the original check.
5. Defaults: nothing changes until an admin writes a node, so a new mod's commands keep the mod's own default.

**Delegation and automation**: in 26.1.2.4, commands that change ranks need level 4 (they used to need 2, so command blocks and signs could run them); viewing commands need level 2, console, or the singleplayer owner; people
joining a published SSP world lose ranks commands; a rank file error stops all ranks loading (fatal at start, kept old config on reload) [raw changelog + code].
**Admin interface**: `/ftbranks` with create, delete, edit, add, remove, list, list_ranks_of, list_players_with, node, condition, show, reload, refresh_readme [raw: registered command names; argument syntax not read].

| Task | How | Score | Tag |
|---|---|---|---|
| T1 | no regions; only dimension/spawn conditions | 1 | [raw] |
| T2 | `create`, `node` for kick/ban..., `add` player: about 5 commands, console or level-4 | 4 | [inference from command names] |
| T3 | no expiry; a `playtime` condition is not a timer | 1 | [raw] |
| T4 | no explain command; `show <rank>` and `list_ranks_of <player>` | 2 | [raw] |
| T5 | `remove` rank + vanilla `/ban` | 3 | [inference] |
| T6 | commands appear automatically as `command.*` nodes; default unchanged (safe); deny a pack by its root word | 4 | [raw code] |
| T7 | `list_players_with`, `list_ranks_of`, `show` | 3 | [raw names] |
| T8 | editing needs level 4: no delegation | 1 | [raw] |
| T9 | console / SSP owner / level 4 | 4 | [raw] |
| T10 | no objects | 1 | [raw] |
| T11 | players only; automation keeps the vanilla check | 1 | [raw] |
| T12 | `reload`; no log or undo seen | 2 | [raw] |
| T13 | most specific node wins, so deny `command.fill` inside a broad allow works; no tags | 3 | [raw code] |

S1 4 (one lookup rule: specific first, then power), S2 2 (no explain tool seen), S3 3 (original default kept; edit rights fixed after the hole; no wildcard needed), S4 3, S5 3.
**Lesson for R6**: `pack.` ranks versus `server.` ranks: a pack ships suggested roles, the admin's roles live apart and the admin cannot corrupt the shipped ones. Copy.

### 3.4 VanillaPermissions (Fabric, DrexHD, pushed 2026-09-19; successor of lucko's retired minecraft-command-permissions-fabric) [raw README]
Gives vanilla commands a node per Brigadier word; sub-words are open when the root is open unless denied (allow `minecraft.command.gamemode`, deny `.creative` and `.spectator`); selectors are checked by RESULT, not by
syntax, with scope nodes (`minecraft.selector.entity|player|self.<selector>`) and metadata limits; command blocks and datapacks bypass selector checks; nodes for bypassing spawn protection, whitelist, player limit,
move/chat speed checks. It states that LP does not support wildcards for metadata. It shows that T13 (deny inside allow) is a first-class use. Version list: 26.3, 26.2, 26.1.x, 1.21.x and older [read, modrinth page].

---------------------------------------------------------------------------------------------------------------------------------

## 4. Bukkit, Spigot, Paper

### 4.1 plugin.yml permissions and how vanilla commands got nodes (Paper docs and source, main branch 2026-11-22)
Depth: deep

**Model [raw]**
1. Protected: the server and other plugins' features. Subjects: `CommandSender` (player, console, command block, minecart command block, remote console).
2. Declared in `plugin.yml`: `permissions:` entries with `description`, `default` (`true`, `false`, `op`, `notop`; aliases such as `admin`), `children` (name: true/false); `default-permission` sets the default for entries that give none (itself `op`);
   `commands:` entries with `permission:` and `permission-message:`. Paper docs: players only see commands they may use (tab-completion hiding).
3. Decided by a separate permission plugin (LP) that attaches explicit values; if none, the declared default for the sender's op status; if the node was never declared, `op` [raw code].
4. Children: a parent node bundles children; a plugin's `x.*` is only a convention. LP applies child values with the sign following the parent [raw code, medium].
5. Server file `permissions.yml` can define extra nodes with description and default; the Paper docs warn it is NOT a replacement for a permission plugin [raw].

**Vanilla commands**: each gets `minecraft.command.<root word>` (parent node `minecraft.command`, "all vanilla commands"); plus Bukkit `bukkit.command.*`; plus `minecraft.admin.command_feedback` (who sees other admins' command feedback) and
`minecraft.command.selector` (use @-selectors) [raw]. Defaults: almost all `op`; `me`, `msg`, `help`, `trigger` true by the registration code, and the docs table lists help, list, me, msg, teammsg, trigger, chase as default-yes.
**The decision rule for a vanilla command on current Paper [raw patch]**: it passes if (the vanilla level suffices) OR (the sender has the node). Exceptions: if the server option "ignore vanilla permissions" is on, only the node counts;
for the special NULL source (no real sender) and, when Paper's `commandBlocks.forceFollowPermLevel` is on, for command blocks, BOTH must pass. Consequence (derived): negating a node cannot take a command away from a high-level op unless that option is on. The patch comment says
the Paper change is "Fix permission levels for command blocks", meaning the default combination was a bug-class surprise.
**Docs versus code**: the Paper docs table says `xp` uses `minecraft.command.xp` with alias `/experience`; the code de-redirects aliases and registers `minecraft.command.experience` [raw, inference that docs are stale].

| Task (with LP behind it) | Score | Note |
|---|---|---|
| T2 | 4 | nodes exist per vanilla command; see 1.5 |
| T6 | 3 | author-declared default; undeclared = op is a good safe default but nothing enforces `default:` honesty; admin has no list before first use unless the plugin declares it |
| T13 | 2 | `children` can bundle, but a deny on a child needs the permission plugin; vanilla subcommands share one node |
| T4/T5/T7/T12 | n/a | provided by the permission plugin |
S1 3 (op, defaults, children, attachments, OR rule), S2 1 on its own, S3 3 (undeclared = op is safe; `default: true` is trusted), S4 n/a, S5 4 for plugin authors.
**Lessons**: R6 copy "undeclared means op/admin only" and the `description` field; avoid per-command-root-only granularity and the OR rule; avoid trusting the plugin's `default: true` without an admin switch (LP's `apply-bukkit-default-permissions false` is that switch).

### 4.2 Fabric note: vanilla commands have no nodes unless a mod adds them
LP's FAQ says Fabric does not add permission checks for vanilla commands; use VanillaPermissions (3.4) [raw]. The FAQ page may predate the PermissionSet rework (2.2.5).

---------------------------------------------------------------------------------------------------------------------------------

## 5. Brief cards (light)

### 5.1 Sponge (Subject and context permission service) [raw docs; text says written for SpongeAPI 7, likely out of date]
Depth: light
- Everything with permissions is a Subject, held in collections: User, Group, System (console), Command Block (stated use: run a command block "only with the permissions of the creator"), Role Template.
- Resolution order (documented): the subject itself (transient before persistent), then its parents, then the collection's defaults, then the service-wide defaults, and at the end DENY. Unset means denied.
- Nodes are hierarchical; a parent grants all children unless a child is removed; there is no `*`; contexts (world, region...) come from `ContextCalculator`s; transient data lasts only for the session (handy for minigames).
- `PermissionDescription` registers a description, owner plugin, and a suggested role (user, staff, admin) per node, with templates like `<World>`; it changes no access.
- LP adds optional implicit-wildcard processing for Sponge. SpongeForge disables the OP system once a permission plugin is installed (LP FAQ).
- Lessons: copy "unset is denied", the Command Block subject idea, role templates for T6, and transient versus persistent data (T3, T11). Avoid: defaults that plugins grant themselves (the docs advise against).

### 5.2 Bedrock Edition [read]
Depth: light
- Player tiers `visitor`, `member`, `operator` in `permissions.json` as `{permission, xuid}` entries; identity is the Xbox account id (XUID), which needs online mode; new players get `default-player-permission-level`
  from `server.properties`; `/permission reload` reloads the list on a running server; the allow list (`allow-list`, formerly `white-list`, renamed in 1.18.10) is a separate file keyed by XUID [read].
- Commands also have levels 0 to 4 on Bedrock (4 for `permission`, `allowlist`, `stop`, `save`...) [read, the wiki text was summarised and muddled; treat the exact split as uncertain].
- Lesson: identity-keyed tier file plus reload command is the simplest "file plus command" model; three tiers is too few for T2 (R4).

### 5.3 PermissionsEx and GroupManager (history) [raw + read]
Depth: light
- PermissionsEx repository is archived (last push 2024-02-03); GroupManager still receives pushes (2026-01-07) [raw API]. LuckPerms' own pitch is that older plugins were abandoned and lack contexts, web editor,
  verbose, network sync, speed [raw, self-interested]. LP ships a migration page with command-equivalence tables and a compatibility bridge that routes old PEX/GM commands to LP [raw].
  I found no independent post-mortem on why they were replaced; the reasons above are LP's account. Both were file-based per-world permission plugins [recalled].
- Lesson: the replacement won on explanation tools and editors, not on the core model (groups + nodes + weights stayed).

---------------------------------------------------------------------------------------------------------------------------------

## PART 3. Lessons for R1 to R9 (my opinion, with confidence)

1. R9 and R4 together (high confidence): LuckPerms is the best model of explainability: every decision knows which rule and which source decided, and a "run as" check lists the rules consulted. Copy that into the decision
   function itself (return a record, not a bool). Make the order of rules short and stated, and put "most specific wins, then deny wins, then priority" in the first paragraph of the docs. LP's hidden surprise (an exact allow
   beats a heavier group's wildcard deny) is what a stated order avoids.
2. R6 (high): the safe default must be decided at declaration time and visible BEFORE first use. NeoForge's mandatory default and registry, Sponge's role templates and "unset is denied", and Bukkit's "undeclared is op" are
   three ways to get there; combine them: each pack declares nodes with description, suggested role and a default no looser than "admin"; a server switch ignores pack-declared defaults. FTB Ranks' `pack.` versus `server.` roles
   keep shipped suggestions apart from the admin's own.
3. R2 (high): non-human actors need their own rules. FTB Ranks had to forbid rank editing from command blocks and signs; vanilla sign and click events ran with level-4 power of whoever clicked; Sponge has a Command Block subject
   "with the permissions of the creator". Your R2 design (owner's power, or anonymous operator) matches the best of these; add (a) permission-administration commands refuse non-human actors by default, (b) "run as another actor"
   is a separate privilege (LP's `verbose command` node), (c) owner-power functions show a confirmation first time.
4. R4 granularity (medium): per-root-word nodes (Bukkit) proved too coarse (T13: VanillaPermissions and FTB Ranks both had to add per-word nodes); no system has tags. If you want tags, they must be a first-class "bundle" with
   a name, members, and a description, because wildcards by prefix cannot express "building".
5. Escalation (high): LP shows what happens without "you cannot grant what you lack": its fix is an opt-in set of extra nodes. Build the subset rule into the engine: a granter may give only what they hold, in the scope
   they hold it. Console and root key stay unrestricted.
6. R1/R8 (medium): everything here keys users by account UUID. Your key per install fits the same slot. Keep the nickname as a display label and a lookup convenience only; show both in every audit line.
7. R7 (high): the web-editor-produces-one-command pattern and the console-only bulk command with a confirmation code are worth copying even before a GUI exists: an "edit a file, then `apply` it with a printed summary" flow.
8. Avoid: root wildcard as the way to make an admin (LP and Hytale's docs both use it, and everyone warns about it). Make "op" a real built-in group with a name instead, so the owner is a group member, not a wildcard.

---------------------------------------------------------------------------------------------------------------------------------

## PART 4. Not read, not checked, uncertain

- Never ran any of these systems: every behaviour above is read from docs or source, none measured.
- Could not read: Mojang's own changelogs and source (minecraft.net unreachable; the wiki pages for 1.21.11 and 26.2 were cut at 100,000 characters); the first version of the 2.2.5 rework is unknown.
- Not read: `/lp user X permission check` real output; the verbose web viewer's actual fields (I read the wiki's text and the check's data type only); the LP config default values (I read examples, not the default file);
  `InheritanceGraph` itself (I read the comparator); FTB Ranks' `NodeCommand` argument syntax; WorldGuard and ExtraContexts (T1 side); the Sponge `Inheritance` section of the newest SpongeAPI; the management-server's operator calls.
- Unverified recalls: command blocks run at level 2; `/op` hands out `op-permission-level` even when typed by level 3; ban-by-UUID; Bedrock `allowlist.json` field names.
- Search-snippet only [secondary]: Spigot Jira SPIGOT-8134 and SPIGOT-6215, the sign-exploit bug MC-201807, the 25w20a click-event warning, the 26.2 entity-selector atom, the namespaced-alias gap.
- Hytale: LP has a beta for Hytale and says Hytale's own permission system is very similar to Bukkit's and is replaced (its `/op` and `/perm` stop working; "OP-like" is `lp user X permission set *`) [raw LP Hytale.md]. Another card should cover Hytale properly.

## PART 5. Source list (all fetched 2026-11-23)

- github.com/LuckPerms/wiki (pages listed in 2.1) [raw]; github.com/LuckPerms/LuckPerms (files in 2.1) [raw]; luckperms.net/wiki [read, empty].
- github.com/lucko/fabric-permissions-api `README.md`, `USAGE.md`, releases [raw]; github.com/FabricMC/fabric `fabric-permission-api-v1/` (branch 26.3) [raw].
- github.com/neoforged/NeoForge `src/main/java/net/neoforged/neoforge/server/permission/` (branch 26.3.x) [raw].
- github.com/FTBTeam/FTB-Ranks `CHANGELOG.md` and `common/src/main/java/dev/ftb/mods/ftbranks/*` [raw].
- github.com/DrexHD/VanillaPermissions `README.md` [raw]; modrinth.com/mod/vanilla-permissions [read]; github.com/lucko/minecraft-command-permissions-fabric (archived) [raw].
- github.com/PaperMC/Paper (`CommandSourceStack.java.patch`, `VanillaCommandWrapper.java`, `CommandPermissions.java`, `PermissionDefault.java`, `Permission.java`, `PermissibleBase.java`) [raw];
  github.com/PaperMC/docs (vanilla-command-permissions.md, `plugin-yml.mdx`, `bukkit-permissions-configuration.mdx`) [raw].
- github.com/SpongePowered/SpongeDocs `source/plugin/permissions.rst`, `source/server/management/permissions.rst` (branch stable) [raw].
- minecraft.wiki: Permission_level, Server.properties, Ops.json, Permissions.json, Commands/execute, Bedrock_Dedicated_Server, Java_Edition_1.21.11, Java_Edition_26.2 [read]. Raw wikitext was blocked by a bot check; I did not try to get round it.
- v2.nucleuspowered.org/docs/nowildcard.html [read]. Search snippets [secondary]: Mojang bug tracker MC-201807, Spigot Jira SPIGOT-8134 and SPIGOT-6215, a Modrinth click-event mod, builtbybit and Spigot forum threads.
