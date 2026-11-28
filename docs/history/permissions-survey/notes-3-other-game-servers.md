# Card 3: admin and role permission systems of dedicated game servers outside Minecraft

> Raw notes of research stream 3 of the permissions study: admin and role systems of other game servers (Luanti, Vintage Story, Factorio, SourceMod, Garry's Mod CAMI and ULX, Rust Oxide, 7 Days to Die, and in brief TShock, Project Zomboid, Space Engineers, Valheim, ARK, Hytale). Written by a research agent on 2026-11-23 and kept as it wrote it, except that local file paths were removed and two file names lost their code quotes. The survey that draws on these notes is `permissions-survey.md`; it says which claims were checked again. The tags are the agent's own: [re-read] and [raw] mean the source's own text was read, [read] a summary by the fetch tool, [secondary] a second-hand source, [recalled] memory, [inference] reasoning from cited facts.


Checked 2026-11-23. Desk research only (documentation, source code, issue trackers); nothing was installed or run. Written for the
owner of the voxel game; plain words. Research agent 3 of 5.

Deep cards (full T1 to T13 table and S1 to S5), sections 2 to 8: Luanti with the areas mod, Vintage Story, Factorio, SourceMod,
Garry's Mod (CAMI the interoperability standard, plus ULX/ULib as the admin mod that implements it), Rust with Oxide/uMod, and
7 Days to Die. Light cards (section 9): TShock (Terraria), Project Zomboid, Space Engineers, Valheim and ARK, Hytale. Section 1 answers
the specific questions, section 10 draws the lessons for R1 to R9, section 11 lists what I could not check.

## 0. How to read this card

- Tags are the brief's tags. I added a qualifier to `[read]`: "raw" means I read the raw file text (source code or wiki source) in one
  pass; "summary" means a small-model summary from the fetch tool. `[re-read]` means I cross-checked the fact in a second source or a
  second pass (for example the wiki text and the C++ source). `[inference]` is my reasoning from cited facts, and every S1 to S5 score is
  an `[inference]`.
- Raw copies of every file I read are in a temporary scratch folder (not kept)
  (temporary).
- Searches used: 17 of the 20 allowed. The shared allowance did not run out for me.
- There is no usability test behind any "easy" or "confusing" statement here. They come from documentation, source code, hosting-company
  guides and a few forum and issue titles. Where a forum thread could not be opened (Factorio forum returned 403 to both of my fetch
  routes) I say so.
- Quotes are kept to one short phrase per source. Everything else is paraphrased.

## 1. Answers to the specific questions

### 1.1 Per-command overrides AND a way to grant or deny a whole set at once (T6, T13)

| System | An admin sets a rule for ONE command | An admin sets a rule for a WHOLE SET | Deny inside allow | Tag |
|---|---|---|---|---|
| SourceMod | Yes: override by command name (change the required flags globally, or allow/deny for one group) | Yes: command groups written `@Name` (a label the plugin puts on its commands), and flags | Yes through the global file, verified in the registration code: a rule on the command replaces the rule on its command group, so `"@Pack" "b"` plus `"sm_x" "z"` means the pack needs flag `b` except `sm_x`, which needs root. The wiki also documents per-group allow/deny blocks for command groups; in the code I read, that check looks the rule up by the typed command name, so I could not confirm it applies to a group label. Across several groups the first group with any matching rule decides, so the result depends on group order | [re-read] wiki text; [read raw] C++ |
| TShock (light) | Per permission string | Dotted names with wildcards (`tshock.admin.*`) | Yes: a `!` prefix negates; exact name beats wildcard; nearest group beats farther group | [read raw] source |
| Oxide (Rust) | Per permission string that a plugin registers | Wildcard by plugin prefix (`plugin.*`), but it is expanded into the list of permissions that exist AT THAT MOMENT | No deny at all. You grant the wildcard, then revoke the exceptions one at a time. Revoking with a wildcard failed in a forum thread | [re-read] source + docs; [read summary] forum |
| 7 Days to Die | Per command: a number 0 to 1000 | Only by the number ladder (everything with level at or above N) | No | [secondary] (hosting guides quoting the file's own comments) |
| Garry's Mod, ULX | Per access string (one per command or privilege) | No wildcard in the check (exact match) | Only for one player (a user deny beats everything); groups have no deny list | [re-read] source |
| Luanti | Per privilege. The mod author decides which command needs which privilege; an admin cannot rebind | A privilege bundles what its author chose; `/grant` takes a comma list | No deny exists | [re-read] |
| Vintage Story | Per privilege (30 built in plus mods) | A role is a bundle of privileges | No deny | [read raw] |
| Factorio | Per input action (about 300 low-level actions), not per command | A group's checkboxes; "everything except X" is one unticked box | Yes (each box is allow or deny) | [read raw] |
| Hytale (early access) | Per command and per argument node, per the COMMUNITY docs | Wildcards | Yes with a `-` prefix, per community docs | [secondary] only; official docs silent |

Only SourceMod has both per-command rules and rules on named sets of commands as first-class, documented features. Only TShock and
(per community docs) Hytale combine hierarchical names, wildcards and negation. That combination is the closest match to R4.

### 1.2 "Check the effective permission" and "why refused" tools (T4, T7)

| System | What exists | Tag |
|---|---|---|
| Luanti | The refusal text lists the missing privileges. `/privs [name]` lists a player's privileges. `/haspriv <privilege>` lists ONLINE players holding it (needs `basic_privs`). `/help privs` lists every privilege with its description. `/help` shows only the commands the caller may run | [re-read] code |
| Oxide | `oxide.show user <name>`, `oxide.show group <name>`, `oxide.show perm <permission>` (who holds it), `oxide.show groups`, `oxide.show perms` (everything registered) | [re-read] official docs + source |
| SourceMod | `sm_who [name]` lists users and their flags. Nothing explains a refusal | [re-read] wiki |
| CAMI | The answer callback has a (yes/no, reason text) pair, and `CAMI.GetPlayersWithAccess` lists online holders. ULX, the main implementer, answers yes/no only and drops the reason | [re-read] source |
| Vintage Story | `/player <name> privilege` and `stats`, `/role <role>`, `/list role`, `/list privileges` | [read raw] |
| Factorio | `/permissions get-player-group <player>`, then the checkbox window | [read raw] |
| 7 Days to Die | `admin list`, `cp list` (names from search snippets only) | [secondary] |
| Hytale | `/perm user list <uuid>`, `/perm user group list <uuid>` | [secondary] |

No system tells you WHICH RULE decided in a rule system that has deny (SourceMod, TShock). That is a gap R9 can fill. Luanti's refusal
text is the best example of the cheap version: say what was missing.

### 1.3 How an admin avoids locking themselves out (T9)

| System | Mechanism | Tag |
|---|---|---|
| Luanti | The player named in the `name` setting holds the owner's privileges and `/revoke` refuses to strip the flagged ones ("Cannot revoke from admin"). Recovery is a config-file edit | [re-read] |
| SourceMod | The server console is always allowed; on a listen server the host is root. Recovery is a file edit and `sm_reloadadmins` | [re-read] source |
| ULX | Console and listen-server host always pass the check | [re-read] source |
| Oxide | The server console id always passes. Rust's own `ownerid` is set from the console | [re-read] source; [secondary] for ownerid |
| TShock | A one-time setup code is printed at startup; `/setup <code>` gives one command's worth of superadmin so you can create an owner account; then the setup system switches itself off. `/su` gives 10 minutes of superadmin to anyone with that permission | [read raw] source |
| Vintage Story | Fresh server presumably has no admin; use `/op <name>` from the server console | [read raw] + [inference] |
| Factorio | `server-adminlist.json`, or `/promote` from the console. A forum thread (not opened) says deleting the admin permission group dropped its members into Default | [read raw] wiki; [secondary] thread |
| 7 Days to Die | Edit `serveradmin.xml` and restart (level 0 entry); in-game `admin add` | [secondary] |

### 1.4 Temporary grants (T3)

- Only TShock has a real one: `/tempgroup <user> <group> [time]` swaps the player's group for the time given (a timer in memory, lost on
  restart), and `/su` is the same idea for 10 minutes. [read raw] source.
- 7 Days to Die blacklist entries carry an "unban date" (temporary bans) [secondary]. ULX has timed bans [recalled].
- Luanti, Vintage Story, Factorio, SourceMod, ULX groups, Oxide, Hytale: none found. For Luanti and Oxide I read the grant code and it
  has no duration [re-read].

### 1.5 The typical moderator role and the cost of building one (T2)

| System | How | Steps | Score |
|---|---|---|---|
| Luanti | `/grant Mod kick,ban,basic_privs` (mute works by revoking `shout` from the target) | 1 | 5 |
| Vintage Story | A role ships with the game ("Survival Moderator", code `sumod`): kick, ban, announce, build everywhere; no game mode, no teleport | 1 (`/player X role sumod`) | 4 (no mute) |
| SourceMod | Flags `b c d` (generic, kick, ban) plus `j` (chat, which I believe covers mute and gag [recalled]) in a file, then reload | 2, file edit | 3 |
| TShock | New group with parent and three permissions | 3 to 4 | 3 |
| ULX | New group, then one `groupallow` per command | 4 or more | 3 |
| 7 Days to Die | `admin add <name> 1`, plus `cp add` for each command the moderator may use (unlisted commands are admin-only by default) | 1 to 4 | 3 |
| Oxide | Rust's own `moderatorid <id>` (1 command) covers game kicks and bans, but it is Rust's level system, not Oxide's; Oxide groups cover only plugin features | 1 | 3 |
| Factorio | Impossible: kick and ban are admin commands and admin is one switch | none | 1 |
| Project Zomboid | A fixed ladder with a "moderator" step that includes teleport; the one host guide I found describes no per-capability setting | 1 | 2 [secondary] |

### 1.6 What is easiest to learn; what the evidence says is confusing or dangerous

My opinion (confidence: medium, because no usability test exists, only documents):

1. For an admin who types commands, Luanti's model is the easiest to learn: one noun (a privilege), one verb (`/grant`), a refusal that
   names what was missing, and `/help privs` to discover every privilege. Its price is no groups, no deny, and no way to define a role once.
2. Vintage Story's shipped, described roles are the easiest way to get a safe moderator in one step.
3. SourceMod is the most capable of the file-based systems and the hardest: its own wiki calls the system "quite daunting to users".
4. Oxide is easy to start and has the best "show me" commands, but the wildcard and bulk-revoke gaps bite as soon as a server has many plugins.

Dangerous or confusing, with evidence:

- Wildcard grants to the group that everybody is in: Oxide's `default` group is given to ALL connecting players, so `*` there publishes
  everything; hosting guides warn about it [re-read official docs + [secondary]]. I found no source that describes an actual incident.
- Oxide wildcards copy the permissions that exist now. Plugins added later are not covered (safe, but surprising) [re-read] source.
- SourceMod: edits do not apply until a reload; a connected admin keeps the old group data until reconnect or reload; an override with
  empty flags makes a command public; `z` (root) ignores immunity; SourceBans sync can overwrite hand edits [re-read] wiki; [secondary].
- 7 Days to Die and Valheim: ONE entry in the whitelist file switches on whitelist-only mode and locks everyone else out [secondary],
  the same trap in both.
- 7 Days to Die: "lower number is more power" and the file's own wording ("equal to or above") reads backwards for the number [secondary].
- Factorio: the wiki permission page is flagged outdated for 2.0, and several action rows have "?" as their description [read raw].
- Luanti: `/grant X all` includes `privs`, `server` and `password`; the `password` privilege lets its holder reset passwords, which I
  believe includes the owner's account [inference, setpassword code not read].
- CAMI: the ULX implementation replaces a plugin's description text with a fixed sentence and drops the reason string [re-read] source.

## 2. Deep card: Luanti (formerly Minetest) with the areas protection mod

Depth: deep. Version checked: engine master (docs.luanti.org pages and `builtin/` code via the GitHub API, 2026-11-23); areas mod master.

- Model in five lines:
  1. Protected: world edits, chat, every chat command, and anything a mod gates on a privilege.
  2. Subjects: a player NAME (a name plus a password held by the server; no keys).
  3. Grant and deny: privileges are a flat set of names per player; grant adds, revoke removes; there is no deny, and the API docs say
     privileges must only ever add abilities.
  4. Inheritance: none (an issue asks for it, closed 2022, reason not read).
  5. Defaults: new players get the `default_privs` setting (interact, shout); the player named in `name` is the owner; mods choose
     whether their privileges go to the owner and to singleplayer (`give_to_admin`, `give_to_singleplayer`).
  [re-read] docs page, `privileges.lua`, `chat.lua`, lua_api.md.
- Granularity: per privilege (mods choose). A chat command declares the privileges it needs (all of them required); a command that
  declares none is open to everyone. Per region: the areas mod. No tags, no packs, no subcommands (a mod can check more in its own code).
  [re-read].
- Groups, roles, teams: none in core. The areas mod can open an area to a "faction" from a separate faction mod. [read raw] areas README.
- When rules conflict: there are no conflicting rules in core. areas has one setting: by default a position is allowed if ANY area
  containing it is the player's own, shared or open, and a stricter overlapping area does not block; the experimental setting
  `use_smallest_area_precedence` lets the smallest area containing the position decide instead. A position in no area is open to anyone
  who may interact. [re-read] `api.lua` + `settingtypes.txt`.
- Delegation and escalation: `privs` lets the holder grant or revoke ANY registered privilege (`all` means every registered one), and the
  code never checks that the granter holds what they hand out. `basic_privs` lets the holder grant or revoke only the privileges named in
  the owner's `basic_privs` setting (default interact, shout), so a moderator can mute and unmute but cannot escalate, unless the owner
  widens the setting. [re-read] `chat.lua` grant and revoke code.
- Automation: no actor model in core. A mod runs commands as whichever player name it passes. The command-block mod runs as its owner
  [recalled; not checked]. Mod code may call `core.set_player_privs` directly, which bypasses every check.
- Admin interface: chat commands `/grant`, `/revoke`, `/grantme`, `/revokeme`, `/privs`, `/haspriv`, `/help privs`; the config file
  (`name`, `default_privs`, `basic_privs`); the auth database for offline fixes [recalled]. Audit: each grant and revoke writes a log
  line at "action" level naming caller, privileges and target, and mods can hook grant/revoke [re-read]. No undo, no history, no timed grant.
- Safety: strangers get only interact and shout. The owner's flagged privileges cannot be revoked. A new mod's privilege reaches nobody
  except the owner (if the mod flags it so). Hazards: `all`; the `server` and `password` privileges; a mod command declared with no
  privileges; the owner is identified by NAME, so whoever first registers that name on an empty server may become owner
  [inference, join-time code not read]. The docs page says the owner has all privileges; the code gives the owner only privileges flagged
  `give_to_admin`, so `teleport`, `give`, `fly` and others are not automatic, though the owner can grant them to themselves
  [re-read, mismatch noted].

Task table

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 | areas: `/area_pos set` and punch two corners; `/set_owner Alice Home` (or `/protect Home` if self-protection is on); `/select_area <id>`; `/add_owner <id> Bob Home`. About 5 actions. Bob can build only in his sub-area; everyone else is blocked. "Only their own things" has no core answer | 3 | [read raw] |
| T2 | `/grant Mod kick,ban,basic_privs`: 1 command | 5 | [re-read] |
| T3 | Impossible without a mod: no duration anywhere in grant | 1 | [re-read] |
| T4 | Read the refusal (names the missing privileges), then `/privs Bob`: 1 to 2 steps. For areas the message names the owners. No "which rule" question arises | 5 | [re-read] |
| T5 | `/revoke X all` then `/ban X` (docs: bans names and IPs): 2 commands. Survives a rename only through the IP part | 3 | [read raw] + [inference] |
| T6 | Nobody gets the new privileges except the owner; `/help privs` lists them with descriptions. Whole pack in one step only if the mod used one privilege for its 15 commands; otherwise name each in a comma list. No wildcard | 3 | [re-read] |
| T7 | `/haspriv <priv>` for online holders; `/privs X` for one player. Command-to-privilege mapping is by reading the mod; offline holders need the database | 3 | [re-read] |
| T8 | No teams. `basic_privs` hands out only the owner's list, so no escalation, but it is global. Area owners can add sub-owners | 2 | [re-read] |
| T9 | Put the owner's name in `name`; recovery is editing it and restarting (the restart part is my reasoning) | 4 | [re-read] + [inference] |
| T10 | areas plus the faction mod: `/area_faction_open <id> <faction>`, or three sub-owners. 3 to 4 steps | 3 | [read raw] |
| T11 | No actor concept in core | 2 | [recalled] |
| T12 | Takes effect at once (privileges are read at each check); undo is a re-grant; the log has who and when | 3 | [re-read] |
| T13 | No categories, no deny: `/grant X all` then `/revoke X one` (risky) | 2 | [re-read] |

Scores: S1 predictability 5 (a flat set; the only precedence knob is one areas setting). S2 explainability 4 (names the missing
privilege; cannot ask about another player's command). S3 safety 3 (good defaults and owner protection; `all`, public-by-omission
commands, name identity). S4 scale 2 (no roles, so 100 commands by 20 roles means editing each player's set; comma lists help).
S5 learning curve 5. All `[inference]`.

Pain points (issues, titles only; summary): #12130 "let privileges inherit from other privileges" (closed Mar 2022); #11795 make default
privileges revocable (open since Nov 2021); #15582 separate authentication from privileges so one auth database can be shared (open,
Dec 2024); #12116 change default permissions for singleplayer and multiplayer (closed Apr 2022). [read summary]
https://github.com/luanti-org/luanti/issues?q=is%3Aissue+privileges (page 1 of 101 only).

Lessons for R1 to R9:
- Copy (R9): the refusal that names what was missing; `/help` hiding commands the caller cannot run; `/help privs` as the one list of
  every privilege with a description (R6).
- Copy (R8): the owner's set that `/revoke` refuses to strip; the explicit per-privilege "owner gets it by default" flag, which is a
  plugin-declares, server-decides default.
- Copy (R5/T8): `basic_privs`, a NAMED set that the owner chooses and a delegate may hand out. In our design this could be "a helper may
  grant only privileges carrying tag X".
- Avoid (R1): identity by name. The open issue about separating authentication from privileges points the same way as R1.
- Avoid (R4): no groups, no deny. Avoid "commands with no privilege are public"; make "nobody" the default.
- Sources: https://docs.luanti.org/for-players/privileges/ [re-read, direct page text]; builtin/game/privileges.lua, chat.lua,
  builtin/common/chatcommands.lua, doc/lua_api.md in https://github.com/luanti-org/luanti [re-read raw, via `gh api` contents];
  https://github.com/minetest-mods/areas README.md, api.lua, init.lua, interact.lua, settingtypes.txt [read raw].

## 3. Deep card: Vintage Story

Depth: deep. Version: wiki pages written for 1.21.x (land claiming page is stamped 1.21.1; the role/command pages carry older
verification stamps); `vsapi` master source. Unverified: whether every role default on the wiki matches a fresh install of the
current version.

- Model in five lines:
  1. Protected: building, using blocks, chat, commands, land, and server control, through 30 built-in privilege codes plus mod ones.
  2. Subjects: a player (account-bound id, which I believe is the vintagestory.at account [inference]); one ROLE per player, plus extra
     privileges granted to the single player.
  3. Grant and revoke only; no deny.
  4. A role has a number ("privilege level") that orders roles: -1 for visitors and limited players, 0 player, 100 creative player,
     200 survival moderator, 500 creative moderator, 99999 admin; there is no role inheritance.
  5. Default role for a new player is `suplayer`; roles live in the server config file with their own description and land-claim quotas.
  [read raw] wiki Server_Config, commands pages; vsapi source.
- Granularity: per privilege; each command is tied to a privilege in code. Region: player-made land claims with ordered access
  tiers (traverse, use, all) for a player, a player group, or everyone. No tags; mods add privileges.
- Groups, roles, teams: ONE role per player; separate player "groups" (chat and team groups) that players create themselves; claims can
  be granted to a group. Role level is the priority. [read raw].
- When rules conflict: nothing to conflict, since there is no deny; a claim grants the highest tier given to the player, their group, or
  everyone [inference].
- Delegation and escalation: the `grantrevoke` privilege is documented in the source as: you may set only roles of your own level or lower
  and hand out only privileges you hold. `root` means everything. Enforcement code not read; a hosting guide repeats the rule
  [read raw] comment; [secondary].
- Automation: not found (no command blocks, no scripting in the pages read).
- Admin interface: chat commands (`/role`, `/player`, `/list role`, `/list privileges`, `/op`, `/land`, `/group`), the server config file for
  defining roles, and an in-game command handbook. No GUI. No timed grant. No audit trail seen.
- Safety: visitors cannot change anything; players can build only in unclaimed or granted land; the moderator role cannot teleport or
  change game mode; `/op` is the one-step route to admin and works from the console. Per-role quotas bound how much land one player can
  claim (volume, number of areas, smallest size).

Task table

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 | `/land claim new`, `start`, move, `end`, `add`, `save <name>`, `grant Joe all`, `save` again. 8 to 9 commands; the claim's index changes after saving. The wiki says claiming is only possible by typing these commands | 3 | [read raw] |
| T2 | `/player X role sumod` (1 command, stock role). Lacks a mute privilege; a custom role means editing the config file | 4 | [read raw] |
| T3 | None: `/player X privilege grant tp` has no duration | 1 | [read raw] |
| T4 | `/player X privilege`, `/player X role`, `/role <role>`, `/list privileges`: 3 to 4 commands. No refusal text found | 3 | [read raw] |
| T5 | `/player X role suvisitor` plus `/ban X reason`. Ban is by the player's account id, not the nickname | 4 | [read raw] + [inference] |
| T6 | n/a: I did not verify how mod privileges reach roles (the role flag `AutoGrant` exists; purpose not read) | n/a | |
| T7 | `/list role` then `/role <r>` for each, `/player X stats`. 3 to 5 commands | 3 | [read raw] |
| T8 | Team lead: players may create and run their own groups with default player roles, so membership is delegated with no admin power. Role-setting helper: `grantrevoke` limits to own level and held privileges | 4 | [read raw] comment only |
| T9 | Nobody is admin on a fresh server; `/op <name>` from the console. Recovery the same | 4 | [read raw] |
| T10 | Create a group, then `/land claim grantgroup <group> all` and save. About 5 commands; later membership changes use group commands only | 4 | [read raw] |
| T11 | n/a (no automation actors) | n/a | |
| T12 | Role and player commands act at once [inference]; config-file edits need a restart [recalled]; no history | 3 | |
| T13 | No deny or categories: `/role <r> privilege revoke <p>` for one exception, 1 command | 3 | [read raw] |

Scores: S1 5 (additive only; two ordering rules: role level, claim tier). S2 3 (good listing commands; no refusal explanation found).
S3 4 (sane shipped roles, bounded delegation, quotas; root and console unlimited). S4 3 (30 privileges, so 20 roles are 20 short
lists; but commands collapse into few privileges, so granularity is coarse). S5 4 (shipped roles come with descriptions).

Pain points: the claim workflow is a typed sequence and the index changes on save (wiki text); the wiki's tier list (traverse, use,
all) and its command reference (use, all) disagree [read raw]; hosting-panel consoles run as the server console without checks, which
can hide permission problems in testing [secondary]. No forum thread of an incident was found (one search returned only hosting
guides).

Lessons:
- Copy (R5): ORDERED access tiers where each includes the one below (traverse < use < all) map well onto "read, call, write".
- Copy (R5): per-role quotas (area, count, minimum size) in the same object as the privileges.
- Copy (T8): the "may hand out only what you hold, to roles at or below yours" guard.
- Copy (T2/R6): ship described default roles (a moderator that cannot teleport) so the common case is one command.
- Avoid: role creation by file edit only; one role per player (R3 wants sets).
- Sources: https://wiki.vintagestory.at/Server_Config, /List_of_server_commands (+ /role, /player, /land subpages), /Land_claiming
  [read raw, MediaWiki source]; https://github.com/anegostudios/vsapi `Server/Privilege.cs`, `Common/IPlayerPrivilegeGroup.cs`
  [read raw]; hosting guides found by search [secondary].

## 4. Deep card: Factorio

Depth: deep. Version: Lua API "latest" (2.x); the wiki Permissions page is flagged outdated for 2.0 (200 rows) while the current Lua
docs list about 297 input actions.

- Model in five lines:
  1. Protected: what a client may ASK the server to do (an "input action": build, mine, copy a blueprint, open a GUI and about 300 more),
     not commands.
  2. Subjects: players by account name; each is in exactly one permission group; separately a player is or is not an admin.
  3. Both allow and deny: a group is a list of actions each ticked or unticked.
  4. No inheritance. The Default group takes everyone new [secondary: wiki user page snippet].
  5. Commands are admin-only; the multiplayer setting `allow-commands` governs whether commands work at all; scenario scripts (`/c`)
     need admin and permanently disable achievements for the save.
  [read raw] wiki Console, Permissions, Multiplayer; Lua docs.
- Granularity: input action only. A single action called "admin action" gates everything that needs admin, such as `/ban`. No per-command, no
  per-mod permission (mod GUI events fall under shared "Gui click" style actions). No regions. [read raw].
- Groups: one per player; the Lua API lets a script create groups, move players, and tick actions (`create_group`, `add_player`,
  `set_allows_action`); there are events for group changes and for promote and demote; `admin` is readable and writable from script
  [read raw].
- When rules conflict: not applicable (one group, boolean boxes).
- Delegation and escalation: "add, delete, edit permission group" and "import permissions string" are themselves actions in the table, so
  a group that may edit groups can edit its own box list [inference]. Non-admins are refused the editing window [secondary].
- Automation: scripts and mods run with full server power and are not limited by permission groups, because the system covers client
  inputs only (a comment in the wiki source says it is based on input actions sent from clients) [read raw]. `/c` commands are visible
  to all players [read raw].
- Admin interface: a checkbox window (`/permissions`), console `/permissions add-player | create-group | delete-group | edit-group <group>
  <action> <true/false> | get-player-group | remove-player | rename-group | reset`, `/promote`, `/demote`, `/admins`, `/config set
  allow-commands ...`. Groups can be exported and imported as one string. [read raw]. No refusal text, no history, no timed grant.
- Safety: commands admin-only by default; the admin list is a file. A forum thread (title only, 403 to my fetch) reports admins falling
  into Default after their group was deleted; a second thread title reports "headless servers can have no admins". [secondary]

Task table

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 | Impossible: permissions are global per group, not spatial | 1 | [read raw] |
| T2 | Impossible: kick and ban are admin commands, and admin is one switch. A scenario script with a custom command is the only way | 1 | [read raw] |
| T3 | Only by a Lua script with a timer (`/c`, which disables achievements) | 2 | [inference] |
| T4 | `/permissions get-player-group X`, then open the window and read the box. 2 to 3 steps | 3 | [read raw] |
| T5 | `/ban X reason` (1 step, by account name). Stripping all actions needs a group with every box off | 4 | [read raw] |
| T6 | n/a: mod commands are the mod's business (usually an admin check in the mod's own code [recalled]) | 1 | [recalled] |
| T7 | Open the window, read one group at a time; `/admins` for admins | 3 | [read raw] |
| T8 | The edit-group action can be given to non-admins, but then they can change any group including their own | 1 | [inference] |
| T9 | `server-adminlist.json` (a list of names) or `/promote` in the console | 4 | [read raw] |
| T10 | No ownership model for things | 1 | [read raw] |
| T11 | Scripts run unrestricted; no limited actor | 1 | [read raw] |
| T12 | The window and console apply at once [inference]; `reset` undoes everything; export string gives a backup; no who/when except through script events | 3 | [read raw] |
| T13 | "Everything except one": untick one box (1 step). A new game version may add actions that default to allowed [inference] | 4 | [read raw] |

Scores: S1 5. S2 2 (state is visible, no reason). S3 3 (commands admin-only, but the default group allows nearly every input).
S4 2 (about 300 boxes per group; export/import strings help). S5 4 (checkbox window, but 300 rows).

Lessons:
- Avoid (R4/R6): permissions as low-level input events. They cannot express "this pack's command", and the list drifts with every game
  version (the wiki page is already flagged outdated).
- Copy (R7): the typed form `edit-group <group> <action> <true|false>` (one rule per command line, scriptable) and the single-string
  export and import of a whole group (bulk, backup).
- Copy (R2): the clear statement that scripts are not limited by player permissions. Our R2 does better by giving scripts the
  owner's power; this shows the cost of not doing so.
- Sources: https://wiki.factorio.com/Console, /Permissions, /Multiplayer [read raw wikitext]; https://lua-api.factorio.com/latest/
  classes/LuaPermissionGroups.html, LuaPermissionGroup.html, LuaPlayer.html, defines.html [read raw text]; forum threads
  forums.factorio.com viewtopic p=581996, 402420, 274334, 260768 [secondary, from search snippets; both fetch routes returned 403].

## 5. Deep card: SourceMod (Source engine servers)

Depth: deep. Version: current SourceMod wiki text and the master source of `core/logic/AdminCache.cpp` and `core/ConCmdManager.cpp`,
read 2026-11-23.

- Model in five lines:
  1. Protected: admin commands registered by plugins and the engine, and WHO an admin may act on (targeting).
  2. Subjects: admins identified by Steam ID, by IP address, or by name plus a password; anyone without an admin entry has nothing.
  3. A set of single-letter flags (21: a to n, root `z`, and six custom `o` to `t`). A command lists the flags it needs and the admin
     must hold ALL of them. Groups bundle flags, an immunity number and per-command allow or deny overrides.
  4. An admin inherits any number of groups; groups cannot nest; flags add up; immunity takes the highest value.
  5. Defaults: no entry means no flags; a command registered with no flags is open to everyone; the console, and the host of a
     listen server, always pass.
  [re-read] wiki text and C++.
- Granularity: per command, per command group (a label a plugin puts on its commands, written `@Name` in rules), per flag. A plugin may
  also ask about a name that is not a command at all (the wiki's example is a made-up usage name), so a sub-feature can have its own
  rule. No regions. [re-read].
- Groups: many per admin. Group-level overrides depend on the order the admin lists the groups. [re-read].
- When rules conflict (read from the C++): AT REGISTRATION (and again whenever overrides reload) a command's required flags are the
  plugin's default, replaced by the global rule on its command group, replaced in turn by the global rule on the command itself, so the
  command beats its group. AT USE (`CheckAdminCommandAccess`): (1) an admin with root passes. (2) For each of the admin's groups in
  order, look up that group's rules by the name it is handed (a rule of kind command and a rule of kind command group; the command rule
  replaces the other); the first group with any rule decides: allow means yes, deny means no. (3) Otherwise the admin must hold every
  flag the command needs. (4) Otherwise no. The wiki states the order-of-groups consequence in plain words. Doubt, `[inference]`: the
  dispatcher hands that function the typed command name, never the command's group label, so a per-group `"@Pack" "deny"` line may never
  match at dispatch; I read the two files but did not run it, and the wiki documents it as working. Targeting is a separate test: three
  immunity modes plus "immune from this other group" entries, and root ignores them. [re-read] wiki and code for the rest.
- Delegation and escalation: no in-game editing of flags in the base install; changes are file edits or an SQL source (the SourceBans
  web panel writes these [secondary]). The rcon flag lets a holder run server commands, which I believe includes reloading the admin
  list [inference]. "Grant what you don't have" does not arise, since nobody grants in game.
- Automation: plugins act as the server, which always passes. Bots and fake clients are refused. [re-read] source.
- Admin interface: four files (`admins_simple.ini`, `admins.cfg`, `admin_groups.cfg`, `admin_overrides.cfg`) or SQL; `sm_reloadadmins`
  to apply; `sm_who [name]` lists access; an admin cache dump exists in the code (command name `sm_dump_admcache` [recalled]). No
  reason for a refusal beyond a generic line. No timed access.
- Safety: an override with empty flags makes a command public (the wiki example says exactly this). Overrides are not applied until
  reload, and a connected admin keeps the old group data until they reconnect or the cache is reloaded (the wiki calls this an
  optimization). `z` ignores immunity. The quick start is a one-line file entry with immunity 99 and `z` for yourself.

Task table

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 | n/a: a match-based shooter has no land or ownership | n/a | |
| T2 | Entry `"STEAM_0:1:123" "bcdj"` in `admins_simple.ini` (or a group with those flags), then `sm_reloadadmins`: 2 steps, one file edit. Map change and cvar flags are simply not given | 3 | [re-read] |
| T3 | Not built in (no duration anywhere in the admin files) | 1 | [re-read] |
| T4 | The refusal is a generic line ("[SM] You do not have access to this command", from the dispatcher code) with no reason. By hand: `sm_who name` for flags, then the command's required flags and overrides in three files and group order. No tool | 2 | [read raw] |
| T5 | `sm_ban` by Steam ID or IP (1 command), remove the admin entry if any. The Steam ID survives a rename | 5 | [re-read] |
| T6 | The plugin author sets default flags and may name a command group. If a group label exists: one line in `admin_overrides.cfg`, `"@PackName" "z"` to lock the whole pack to root or `"@PackName" "b"` to open it to every generic admin (verified in the registration code). If not: 15 per-command lines. Safe default = the author's flag (open if the author registered a public command). Edit and reload. Pack access for ONE admin group only (`"@PackName" "allow"` in that group) is documented but unconfirmed in the code | 3 | [read raw] + [re-read] |
| T7 | `sm_who` per user. Per command: nothing | 2 | [re-read] |
| T8 | n/a in game; the helper would need file or SQL access | n/a | |
| T9 | Add `"yoursteamid" "99:z"` to `admins_simple.ini`, then `sm_reloadadmins` from the server console (console always passes). 2 steps | 3 | [re-read] |
| T10 | n/a (no shared things) | n/a | |
| T11 | n/a: plugins are not actors with a limited power; the console is root | n/a | |
| T12 | Edit file, `sm_reloadadmins`, no restart; connected admins' group data waits for reload; undo is your file backup; logs of admin actions exist [recalled] | 3 | [re-read] |
| T13 | In `admin_overrides.cfg`: `"@Pack" "b"` plus `"sm_x" "z"` (the command's rule beats its group's rule, verified at registration). Deterministic, but it is a flag ladder, not a per-role allow/deny. The per-group form (`"@Pack" "allow"` plus `"sm_x" "deny"` in one group block) is documented; unconfirmed in the code | 3 | [read raw] |

Scores: S1 3 (root, group rules in group order, flags, global overrides: four layers; the order dependence is documented but easy to
trip on). S2 2. S3 3 (safe by default but the empty-flags trap, root, and rcon). S4 4 (21 flags, groups and command-group labels
compress 100 commands by 20 roles well). S5 2.

Pain points: reload requirement and stale group data (wiki, [re-read]); override order matters and web-panel sync can overwrite hand
edits (forum snippets from a search; threads not opened) [secondary].

Lessons:
- Copy (R4/R6): the command GROUP label. A pack labels its commands once, and one rule covers the pack. This is the nearest thing to
  "per pack" in any system I read.
- Copy (R4): allow and deny written per group (documented; my code read could not confirm the command-group form), and a separate
  "who may act on whom" test (immunity). CAMI's target parameter is the same idea.
- Avoid (R3): the cache that goes stale until reload. R3 requires lookup at every use.
- Avoid (R9): the order-of-inheritance tie-break. Two admins with the same groups in another order get different answers.
- Avoid: single letters as the main vocabulary; an empty value meaning "public".
- Sources: https://wiki.alliedmods.net/Overriding_Command_Access_(SourceMod), /Adding_Groups_(SourceMod), /Adding_Admins_(SourceMod),
  /Admin_Commands_(SourceMod) [re-read, MediaWiki source]; https://github.com/alliedmodders/sourcemod `core/logic/AdminCache.cpp` and
  `core/ConCmdManager.cpp` [read raw]; forum snippets [secondary].

## 6. Deep card: Garry's Mod, CAMI (the standard) with ULX/ULib (an admin mod that implements it)

Depth: deep. Versions: `sh_cami.lua` stamped 20211019; ULib/ULX master (ULib embeds an older CAMI, "20150902.1"). SAM (the other
common admin mod) was NOT read: my search found only hosting guides that show how to make yourself superadmin (a `sam setrank` command
typed in the console), nothing about its permission model [secondary].

- Model in five lines:
  1. CAMI is a thin interface, not an admin mod. Three base groups exist (user, admin, superadmin). Every player has exactly one
     usergroup; a custom group must inherit another group; inheritance is not checked when registered, so load order does not matter.
  2. Third-party ADDONS register privileges: name, a minimum level (user, admin or superadmin), optional description, optional custom
     check function. Admin mods answer "may this player do this privilege, to this target?" and may return a reason string.
  3. If no admin mod answers, CAMI answers from the minimum level and the base groups.
  4. ULX/ULib: groups hold allow lists of access strings and one parent; a user has a group plus personal allow and personal deny lists.
  5. Admin mods must NOT register their own privileges through CAMI; it is for addons only.
  [re-read] README and `sh_cami.lua`; ULib source.
- Granularity: per privilege name (a free string; no namespace, no wildcard); an "extra info" table can carry the fallback level, an
  ignore-immunity flag and the command's arguments; a target player can be passed. ULX adds "access tags" (a string stored with an allow
  entry that restricts arguments or targets). Category labels exist for display only. [re-read].
- Groups: ONE per player. ULX lets a group set `can_target` (who it may act on).
- When rules conflict: the exact ULib check is: the console or a listen-server host passes; a personal deny means no ("deny overrides
  all else", its own comment); a personal allow means yes; then the group and its parents in turn, the first allow means yes; nothing
  found means "no instruction", which callers treat as no. Groups have no deny list. In the CAMI layer, a hook that returns nothing
  hands the question to the next admin mod and finally to CAMI itself; with two admin mods installed, which one answers first depends
  on hook order [inference]. [re-read].
- Delegation and escalation: the ULX commands that edit users and groups default to the top level (superadmin). A lower group that is
  given `userallow` can give itself anything [inference]. `can_target` limits acting on higher groups, but ULX's CAMI answer ignores the
  target argument completely (the function does not read it) [re-read] source.
- Automation: a non-player actor (the console, an invalid player) always has access in both CAMI's fallback ("the server always has
  access") and ULib. This is the "anonymous operator" model of R2. Dangerous commands (running Lua, rcon) exist as ordinary privileges
  at the top level [recalled].
- Admin interface: console and chat commands `ulx adduser`, `adduserid`, `removeuser`, `userallow`, `userdeny`, `addgroup`, `groupallow`,
  `renamegroup`, `setgroupcantarget`, `removegroup`; a graphical editor (XGUI) [recalled]; data files. Changes re-check connected players
  and save at once (the function's own comment says so). `userdeny` takes a `should_use_neutral` option, which I take to mean removing an allow without adding a deny [inference; body not read].
  No reason text, no history. [re-read].
- Safety: the first time ULX sees a CAMI privilege it adds the privilege's name to the allow list of the group named by the minimum
  level (admin, for example) and records the registration, so later edits by the admin stick and later changes by the addon do not
  apply (the code returns early if any group already holds the string). ULX gives every CAMI privilege the fixed description "A
  privilege from CAMI", discarding the addon's own text. In CAMI's own fallback handler, which runs only when no admin mod answers, a
  privilege that was never registered gets "no" when the caller passes no extra-info table, although the comment says the fallback
  defaults to the admin level (the code yields nothing in that case) [re-read, mismatch noted]. With ULX installed, ULib answers
  instead, and it also says "no" for anything it has no instruction about. An offline-player query (`SteamIDHasAccess`) is answered by
  ULX only for players who are connected; CAMI's own fallback answers that no information is available. [re-read].

Task table (ULX as the admin mod, CAMI privileges underneath)

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 | n/a for admin mods; prop-protection addons have their own friend lists (CAMI was modelled on that kind of interface, CPPI) | n/a | [re-read] README |
| T2 | `ulx addgroup moderator user`, then one `ulx groupallow moderator "ulx kick"` per command (kick, ban, mute): 4 or more commands, or ticks in the graphical editor | 3 | [re-read] |
| T3 | None for access (timed bans yes [recalled]) | 1 | [re-read] |
| T4 | No tool; the refusal is a generic message [recalled]. Read the group and user entries by hand | 2 | [re-read] |
| T5 | `ulx ban <name> 0 reason` (by Steam ID, survives a rename) and `ulx removeuser`: 2 commands | 4 | [recalled] + [re-read] |
| T6 | The addon registers each privilege with a minimum level; ULX writes it once into that base group (admins get it); description lost. Whole pack: no wildcard, so one `groupallow` per privilege (15) | 2 | [re-read] |
| T7 | Code can call `CAMI.GetPlayersWithAccess` (online holders). No command for it; the graphical editor shows a group | 2 | [re-read] |
| T8 | Not possible without handing out the editing commands, which allows self-escalation | 2 | [inference] |
| T9 | Dedicated: `ulx adduserid <steamid> superadmin` typed in the server console [recalled]; listen-server host is automatically full | 4 | [re-read] |
| T10 | n/a | n/a | |
| T11 | Console and non-player actors pass everything; no limited actor | 1 | [re-read] |
| T12 | Instant and saved; no history; no who or when | 3 | [re-read] |
| T13 | No wildcard, no group deny: a personal deny for one player is possible; per group you list every allowed command | 2 | [re-read] |

Scores: S1 4 (one short rule list for ULX; less clear when two admin mods share CAMI). S2 2. S3 3. S4 3 (inheritance helps; no
wildcard). S5 3. All `[inference]`.

Pain points (titles only, from the issue pages; summary): CAMI #8 "is the HasAccess check ever called" (closed Dec 2020), #4 how to get
usergroups into CAMI (closed 2019), #2 descriptions per command (closed 2016); the CAMI issue page showed three issues, all closed. ULX #139 "ranks
reset after map change or restart" (closed Jan 2019), #44 "group rights reset automatically" (closed Apr 2016), #140 and #141 a
superadmin lacking rights for staff commands (closed 2018). Reasons not read. [read summary]. The CAMI README itself warns that bugs
appear when CLIENTS do not know they hold a privilege, because the client mirrors the answer for its own UI. [re-read]. A Workshop
comment says custom ranks were not picked up unless entered by hand [secondary].

### 6.1 CAMI as a model for "plugins declare their commands, the server decides"

What CAMI gets right (all `[re-read]`):
- The split of jobs. The addon says "this privilege exists, here is a sane default, here is what it is for", and then only ASKS. It never
  asks "is this player an admin". The admin mod owns the answer.
- A fallback that works when no admin mod is installed (the minimum level against three base groups), so an addon is usable on day one.
- The question carries context: target player, ignore-immunity, and the command's arguments.
- The answer is (yes or no, reason text). The reason slot is the right shape for R9.
- A way to ask "who has this" (online players) and lifecycle events for registering and unregistering.

What is weak:
- One group per player (R3 wants a set), and the default is only three words.
- Flat global names with no namespace, so two addons can collide (R6 wants namespaced ids).
- Optional for admin mods, and the custom check function "may not be called by all admin mods".
- The reason string is unused by the biggest implementation, and the addon's description is replaced by a fixed sentence.
- The default is copied once into the admin mod's own files, so the addon can never update it.
- No wildcard, no deny, no pack concept, no timed grant. Those live in each admin mod differently.

What I would copy or adapt (design opinion; confidence medium to high, since it rests on a standard plus one large implementation):
1. Copy the split and the (answer, reason) shape. Make `why` the same call with a switch.
2. Adapt the default: the pack's proposed default (nobody, op, everyone) is a live bottom layer evaluated at each check, not a copy
   written into a group list. The admin's rules sit above it. Uninstalling the pack removes its layer; an update changes it.
3. Adapt the id: use the project's namespaced ids so collisions cannot happen and "everything in pack X" is a prefix.
4. Adapt the context: keep the target argument, and make sure the answer actually uses it (ULX drops it).
5. Show new declarations to the admin ("these 15 are new; proposed default: nobody") instead of silently stamping them.

## 7. Deep card: Rust with the uMod/Oxide permission system

Depth: deep. Version: `Oxide.Core` develop branch source (`src/Libraries/Permission.cs`) and the official uMod permissions page, read
2026-11-23. Rust's own `ownerid` and `moderatorid` come from hosting guides only [secondary].

- Model in five lines:
  1. Two layers. Rust itself has owner (level 2) and moderator (level 1), set by Steam id in a server file, and these are not Oxide's.
     Oxide permissions are consulted only by plugins.
  2. Subjects: platform ids (Steam id). Oxide stores a "last seen nickname" next to the id as a label only.
  3. A user has their own list of permission strings and a set of groups. A group has a list, a title, a rank number and at most one parent.
  4. The check is: the user's own list OR any of the user's groups OR those groups' parents. No deny exists; revoke deletes an entry.
  5. Defaults: Oxide creates `default` (everyone who connects, automatically) and `admin` (those Rust recognises as admins). Nothing is
     granted until someone grants it. The server console always passes.
  [re-read] source and official page.
- Granularity: per permission string registered by a plugin and meant to start with the plugin's name (a missing prefix only logs a
  warning; a duplicate name is ignored with a warning). Wildcards `*` and `plugin.*` copy the matching permissions that EXIST NOW into the
  list of the user or group. A permission that was never registered cannot be granted. [re-read].
- Groups: many per user; one parent per group with a loop check; the rank number is stored and not read by the check [re-read].
- When rules conflict: none, since only allows exist.
- Delegation: console, RCON, or chat in some games; no scoped delegate. Whoever can run `oxide.grant` can grant anything registered.
- Automation: plugins call the check themselves with the player's id string; the console id passes; hooks fire after a grant, so a
  plugin can audit grants. [re-read] source.
- Admin interface (official page): `oxide.grant user|group <name> <permission>`, `oxide.revoke ...`, `oxide.usergroup add|remove <user>
  <group>`, `oxide.group add|remove|set|parent`, `oxide.show user|group|perm|groups|perms`, with `o.` as a shorthand. Data is stored
  in `oxide.users` and `oxide.groups` files [re-read source]. No timed grant, no history.
- Safety: the `default` group reaches strangers, so any grant there is public. Asking about an unknown id creates an empty stored
  record for it [re-read] source, which grows with every stranger. A forum answer says there is no revoke-all command and suggests
  recreating the group [read summary].

Task table

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 | n/a in Oxide (Rust uses cupboard authorisation, a game rule; not read) | n/a | |
| T2 | `moderatorid <id>` gives Rust's own moderator level (kicks, bans, admin tools; not every console command). For plugin features: `oxide.group add moderator`, then grants per plugin. Rust's moderator cannot be tuned to "no teleport" with Oxide (my reading) | 3 | [secondary] + [re-read] + [inference] |
| T3 | None in the core (no duration in the grant code) | 1 | [re-read] |
| T4 | `oxide.show user X` (groups and permissions), `oxide.show perm P` (holders): 2 commands. Group parents need a further `oxide.show group` | 4 | [re-read] |
| T5 | Revoke each permission and remove from groups (no revoke-all), plus Rust's own `ban <id>`. The Steam id survives a rename | 3 | [read summary] |
| T6 | New plugin: nobody has its permissions. `oxide.show perms` lists them. `oxide.grant group vip pack.*`: 1 command. Deny whole pack: do nothing. A plugin that registers names without its prefix is missed by the wildcard | 4 | [re-read] |
| T7 | `oxide.show perm P` and `oxide.show user X`: 1 command each | 4 | [re-read] |
| T8 | None: any console user can do everything | 1 | [re-read] |
| T9 | `ownerid` from the console puts you in the `admin` group (a `server.writecfg` step is advised by some guides and disputed by one) | 4 | [re-read] + [secondary] |
| T10 | Not an Oxide concept (teams come from plugins) | n/a | |
| T11 | Plugins are the server; no limited actor | 1 | [re-read] |
| T12 | At once; hooks allow plugin-made audit; undo is the opposite command; no built-in who or when | 3 | [re-read] |
| T13 | `oxide.grant group g pack.*` then `oxide.revoke group g pack.a` and `pack.b`: 3 commands; works because the wildcard became concrete entries. Wildcard revoke reportedly fails | 3 | [re-read] + [read summary] |

Scores: S1 5 (union only). S2 3 (good `show` commands; group parents not flattened for you). S3 3. S4 3 (no multi-permission command,
no wildcard revoke). S5 4.

Pain points: uMod forum thread 3746 (relative dates, about 2019 to 2021): no way to grant or revoke several permissions in one command;
wildcard revoke did not work; no revoke-all; the staff answer was to recreate the group [read summary]
https://umod.org/community/general-support/3746-grantrevoke-multiple-permissions-at-once. Hosting guides warn that the group wildcard
is dangerous and disagree on command names and default group names across years [secondary].

Lessons:
- Copy (R6): permission names that must start with the pack name, with a registry that lists everything registered; "nothing is
  granted until you grant it".
- Copy (T7): `show perm <name>` (who holds it) beside `show user <name>` (what they hold).
- Avoid: wildcard copied at grant time. It is safe for new plugins and wrong for "the pack" as a living thing; and it leaves "revoke
  everything of the pack" unsupported.
- Avoid (R1): a record created for every stranger just because the check ran.
- Avoid: no deny and no bulk operations.
- Sources: https://github.com/OxideMod/Oxide.Core `src/Libraries/Permission.cs` [read raw]; https://umod.org/documentation/plugins/permissions
  [re-read, page text]; forum thread above [read summary]; Rust console admin guides via search [secondary].

## 8. Deep card: 7 Days to Die

Depth: deep, with weak evidence. The game's own documentation was not found. The best source is the generated `serveradmin.xml`
template as quoted on a hosting company's wiki (it reproduces the file's comments), plus a second hosting guide dated 2026-07-06.
Both are [secondary]. The in-game commands `admin add` and `cp add|remove|list` appear only in search snippets, not in any page I
opened. Platform-prefixed user entries came with the 1.2 update according to a hosting guide.

- Model in five lines:
  1. One number from 0 to 1000 per user, lower meaning more power.
  2. One number per command; a user may run a command when the user's number is not larger than the command's number.
  3. Subjects: a platform id (Steam or EOS); the entry's name is for display only; Steam GROUPS can also be listed with two levels.
  4. No deny, no sets: the number ladder is the only structure; everything at level N is also allowed to every lower number.
  5. Defaults: a user with no entry is 1000; a command not listed needs level 0. The file's own comments say both that level 0 is the
     default for unlisted commands and that only the server can use them; I read that as "level-0 admins and nobody else". The file
     lists the harmless commands (help, gettime, say) at 1000 so players may use them.
- Granularity: per command name; no subcommand, tag or pack; reserved-slot and admin-slot server settings reuse the same numbers.
- Groups, roles, teams: none; one number per user.
- When rules conflict: a single comparison; a Steam-group entry versus a personal entry: not read.
- Delegation and escalation: a helper with the right to run `admin` could probably set any level; not checked. Community posts show
  the `admin add` command given levels 0 and 1 [secondary].
- Automation: the server console and telnet or web panel tokens; not read.
- Admin interface: the XML file (restart to apply, per the hosting guide) and in-game console commands. Blacklist entries have an unban
  date and a reason, so bans can be timed. The whitelist is switched on by having any entry in it.
- Safety: unlisted commands are admin-only, so a new mod's commands are closed by default (the strongest safe default among the systems
  read). The whitelist trap. The wording "equal to or above" in the file reads opposite to the ladder.

Task table

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 | Not a permission feature; the land-claim block has its own friend list [recalled] | n/a | [recalled] |
| T2 | `admin add Bob 1`, plus `cp add <cmd> 1` for each command (the template already puts kick at 1). Teleport and game mode stay closed because unlisted means 0 | 3 | [secondary] |
| T3 | None (only timed bans) | 1 | [secondary] |
| T4 | `admin list` and `cp list`, then compare the two numbers by hand | 3 | [secondary] |
| T5 | Ban by platform id with a date; remove the admin entry: 2 steps | 4 | [secondary] |
| T6 | Safe default is perfect (admin-only). Granting a whole pack: one `cp add` per command (15) | 3 | [secondary] |
| T7 | `cp list` and `admin list` give everything; "who can run Y" is a hand comparison | 3 | [secondary] |
| T8 | No scoped delegation, no groups | 1 | [inference] |
| T9 | Edit the file with a Steam id at level 0, restart; or `admin add` from the console | 3 | [secondary] |
| T10 | n/a | n/a | |
| T11 | n/a | n/a | |
| T12 | File edits need a restart (hosting guide); console commands probably live, not verified | 2 | [secondary] |
| T13 | Unlisted means 0, so "everything except two" means listing every other command at the lower level (100 or more entries) | 2 | [inference] |

Scores: S1 5. S2 2. S3 4. S4 2 (one line per command, and 20 roles cannot be represented unless they form a chain). S5 3 (the idea is
tiny but the documentation is poor and drifting).

Pain points: file format drift (platform prefix), hosting guides say most published command lists predate the current game, and the
whitelist-by-existence trap [secondary]. Two guides disagree about the section names (`users` versus `admins`, `command` versus
`permissions`); the template quotation shows `admins` and `permissions`.

Lessons: copy "unlisted means admin-only" and "name is a display label only"; copy the self-explaining comments in the generated file.
Avoid a single ladder and one line per command.
Sources: https://www.g-portal.com/wiki/en/?p=6385 [secondary, quotes the file]; https://nodecraft.com/support/games/sevendaystodie/
how-to-change-permission-levels-on-a-7-days-to-die-server [secondary, 2026-07-06]; a third hosting guide [read summary]; search snippets.

## 9. Light cards

### 9.1 TShock (Terraria server mod)
Depth: light, but I read the source: `Group.cs`, `GroupManager.cs`, `Permissions.cs`, `Commands.cs` on `general-development`
[read raw, single pass].
- Groups with a parent chain and dotted permission names (155 constants, such as `tshock.account.login`); a `!` prefix negates;
  `a.b.*` wildcards. Default chain: guest, default, vip, newadmin, admin, trustedadmin, owner, plus a reserved `superadmin`.
- Decision rule from the code: the exact name is looked up in the player's group and then up the parent chain, nearest first; at each group a
  negation is checked before an allow; if nothing mentions the exact name, the wildcard forms are tried from the most specific
  (`a.b.*`) down to `*`, each along the chain. The first group that mentions a form decides. So deny-inside-allow works for exact names,
  even when the allow is a wildcard in a child and the negation sits in a parent.
- Temporary: `/tempgroup <user> <group> [time]` (timer in memory only), `/su` (10 minutes of superadmin), `/sudo <command>` (one
  command as superadmin).
- Bootstrap: on first start a setup code is printed on the server console; `/setup <code>` grants one command's worth of superadmin
  to create an owner account, then the setup system turns itself off and writes a lock file; a later `/setup` is logged as an
  incident. The refusal text points lost admins to the project's support site.
- My rough scores: T2 3 (group plus three permissions), T3 4 (the group swap is not a one-command grant, but it ends by itself), T9 5,
  T13 5 (wildcard plus negation in one group). Not scored in full.
- Lesson: the best bootstrap for a server with no admin yet (R8 variant with no key file), and the best working allow/deny rule among
  the systems whose source I read. The rule has three sub-rules (exact before wildcard, nearest group, negation before allow in a
  group), which is learnable but not obvious.

### 9.2 Project Zomboid
A fixed ladder of six access levels (admin, moderator, overseer, GM, observer, none) set with `/setaccesslevel "name" level` in game or
`SetAccessLevel` in the console. Per-level capabilities come from one hosting company's page (admin: everything; moderator: kick, ban,
teleport; overseer: view and teleport but no kick or ban; GM: event tools; observer: invisible flying watcher). The game company's
pinned forum post with the official differences was not read. [secondary]. Lesson: a fixed ladder cannot express "moderator that
cannot teleport", which is exactly T2.

### 9.3 Space Engineers
Five ranks promoted one step at a time: player, scripter, moderator, space master, admin. Promotion is done by an admin in the player
list or admin screen, or from the dedicated server manager or the remote client, and the target must be online; admins are also listed
in the server configuration file. Hosting guides did not describe what each rank can do. [secondary]. The "scripter" rank exists to
allow player-written scripts as a level of its own (my memory of its purpose, not checked) [recalled]. Lesson for R2: "may run scripts"
deserves its own privilege, separate from moderation.

### 9.4 Valheim and ARK
Valheim: three plain id lists in the server folder: admins (who may use kick, ban, unban, list bans and save), bans, and a permitted
list that works as an allowlist, where a single entry locks everybody else out. After crossplay, ids carry a platform prefix; hosting
guides disagree on whether bare numbers are accepted. Admin status does not unlock cheat commands on a dedicated server. [secondary]
via search. ARK: not researched (no search spent). From memory only: an admin password plus a list of ids allowed to use cheat
commands. [recalled, unchecked]. Lesson: the smallest possible model works for a friend server and has the whitelist-by-existence trap.

### 9.5 Hytale (early access since January 2026)
- What is documented officially (the Hytale Server Manual, page updated 2026-10-12): the server folder has `permissions.json`
  ("permission configuration") next to the ban and whitelist files; configuration files are read at start and written whenever in-game
  actions occur, such as assigning permissions by command; manual edits while the server runs will likely be overwritten; an update
  notice goes "to players with permission". The manual has no section on the permission model. [re-read, page text read twice].
- What hosting companies document [secondary]: `/perm user add <uuid> <permissions>` (comma separated, applies at once, no restart),
  `/perm user list`, `/perm user group add|remove|list`, similar group commands, an OP group, and a Default group with no permissions
  that every new player joins; a web panel with group editing. One guide warns that many online lists of permission node names were not
  verifiable and tells you to trust `/help` run as an operator.
- What community modding docs say (volunteers, not the game company; treat as unverified) [secondary]: nodes are dotted names built
  from the command (`hytale.command.gamemode`, and a node per argument such as `hytale.command.gamemode.creative`); check order is user
  entries, then groups, then "virtual" groups from the game mode, then false; the first definitive match wins; `*` and `plugin.*`
  wildcards; a `-` prefix denies and wins at each level; groups appear when first used; two default groups, OP (`*`) and Default;
  storage is a replaceable "provider" so LuckPerms-style plugins plug in. Plugin commands get nodes by the same pattern.
- Lesson: the shape this design is converging on (dotted names derived from the command, wildcard, negation, groups,
  pluggable backend) is what R4 and R6 ask for. But the only complete description is third-party, there is no `why` tool, and the
  official page already warns that file edits and commands fight each other. Do not copy details without reading the game's own code.

## 10. What this means for R1 to R9

Top findings, each with the evidence's strength:

1. CAMI is the right shape for "plugins declare, the server decides" and should be adapted, not copied (6.1). Adapt: the plugin's
   proposed default is a LIVE bottom layer, ids are namespaced, the answer is (yes or no, reason), the target is used. Evidence: standard
   plus its largest implementation read at source level [re-read]; confidence high on the shape, medium on the adaptation.
2. "Closed unless granted" for anything new, stated where the admin will see it: 7 Days to Die (unlisted commands are admin-only),
   Oxide (nothing granted), Luanti (owner only). None of them shows the admin what just appeared; ULX silently stamps it. Add a
   "new and ungranted" view. Confidence high.
3. Only systems with deny (SourceMod, TShock, and per community docs Hytale) can meet R4, and they differ on precedence. SourceMod's
   result depends on the order of an admin's groups, which R3's SET of groups cannot afford. TShock's is deterministic but needs three
   sub-rules. My recommendation: exact beats wildcard; at equal specificity deny beats allow; no order dependence anywhere. Opinion;
   confidence medium; no usability study of any of these exists.
4. Nobody offers "which rule decided". The best cheap version (Luanti: say what was missing) and the best interface shape (CAMI's
   reason slot) are both thrown away or unused in practice. R9 can lead here. Confidence high that the gap exists in these systems;
   my search for tools was limited to documentation.
5. Snapshots drift: SourceMod caches admin data until reload or reconnect, ULX copies defaults once, Oxide copies wildcards at grant
   time. R3's "looked up every time" is the opposite and matches Luanti, Oxide (checks) and ULib (query). Confidence high (code read).

By requirement:

- R1 (key identity, nickname is a label, strangers join). Every system except Luanti keys on a platform id (Steam, EOS, an account id)
  with the name as a free label: 7 Days to Die says so in the file's comment, Oxide stores a "last seen nickname". Luanti keys on the name, and an open
  issue asks to separate authentication from privileges. Copy the id-plus-label form. Avoid creating a stored record for each stranger
  just because a check ran (Oxide does). Strangers' default is the lowest everywhere (Luanti `default_privs`, 7 Days to Die 1000,
  Oxide `default`, Hytale Default).
- R2 (actors; owner-attributed blocks and scripts). Every system treats the console and non-player actors as unlimited (CAMI fallback,
  ULib, Oxide's console id, SourceMod's client 0, 7 Days to Die's "server"). Nobody gives a limited, attributed, revocable actor (T11
  scored 1 or n/a in all seven). TShock's `/sudo` (run one command with a substituted group, then restore) is the nearest idea.
  Factorio shows the cost of the opposite choice: scripts are not limited by player permissions at all. Space Engineers' "scripter"
  rank and Factorio's admin-only Lua command show "may run code" is always its own gate. R2's design is ahead of anything surveyed.
- R3 (sets of groups, live lookup). Sets of groups exist in SourceMod, Oxide, Hytale; one per player in CAMI/ULX, Vintage Story,
  Factorio, 7 Days to Die. SourceMod's "first group with a rule wins" is the cautionary tale.
- R4 (per command, subcommand, pack, tag; both allow and deny). See 1.1. Closest: SourceMod's command-group labels with global flag
  overrides (its per-group allow/deny form is documented but unconfirmed), and TShock's dotted names with negation. No system has tags
  as a first-class idea; SourceMod's command group is the nearest.
- R5 (ownership, sharing, read/call/write). Luanti areas (owner, sub-owners, open, faction) and Vintage Story claims (three ordered
  tiers for a player, a group, everyone; per-role quotas) are the two examples. Copy VS's ordered tiers.
- R6 (third-party packs; safe default; no manual). Covered in findings 1 and 2. Luanti's `/help privs` and "help hides what you cannot
  run" are the cheapest discoverability aids.
- R7 (typed commands and files; GUI later). Copy Factorio's `edit-group <group> <action> <true|false>` and its one-string export and import;
  allow lists and wildcards in revoke as well as grant (Oxide's complaint thread); keep commands one-line.
- R8 (owner is an operator; server owner is a root key in a file). Luanti's irrevocable owner set, SourceMod's one-line
  `99:z` entry, TShock's setup code. Add: the console is root; `revoke all` must not strip the owner.
- R9 (decisions on the server, explainable). Luanti's message, CAMI's reason, TShock's logged setup incident. See finding 4. CAMI's own
  README says bugs arise when a client does not know what it may do: the server must send the client what its UI needs.

## 11. What I could not read or check

- Factorio forum threads (HTTP 403 to my fetch routes): the lockout, "no admins" and "moderator permission level" threads are known by
  title and search-snippet summary only. I could not check which Factorio permission-group initial states apply to a fresh game.
- SAM (Garry's Mod): no primary documentation found in the searches I made; not assessed.
- ARK: not researched. Valheim, Project Zomboid, Space Engineers: hosting guides only; no per-rank power lists for Space Engineers.
- 7 Days to Die: no official page; the in-game `admin` and `cp` commands appear only in snippets; I did not check whether a command
  change applies without restart.
- Vintage Story: whether `grantrevoke` is enforced as its comment says (code not read); what `AutoGrant` does; refusal messages; how mod
  privileges reach roles; how a fresh install's roles compare with the wiki's copy.
- Luanti: the code that gives the owner privileges at login (`auth.lua`) and the `/setpassword` code were not read, so the owner-name
  squatting risk and the `password` privilege takeover path are inferences. Mod-level timed grants were not searched for.
- SourceMod: I read the access check and the command registration code, not the plugin-side calls. I could not confirm that the wiki's
  per-group `@Group` allow/deny form takes effect at dispatch (the check looks rules up by the typed command name), and I did not
  confirm the exact name and effect of the cache dump command.
- Oxide: the console command code (command names come from the official page); the Rust side of `ownerid` and `moderatorid` is
  hosting-guide only.
- No system was run. Every "works" statement is from reading, not trying.

