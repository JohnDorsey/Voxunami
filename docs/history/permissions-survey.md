# Permissions on game servers: what works, how usable it is, and what to copy

*A survey written on 2026-11-23 for the owner of Voxunami, who asked what other games and Minecraft mods do to manage permissions on servers, and for a usability study of them, before roles, teams and grants per command, per pack and per tag are designed for the command line and scripting (branch `scripting-integration`). It is research, not a design: the recommendations are opinions with a confidence stated, and the questions at the end are the owner's. It describes other people's software as of its date. Nothing was run and no person was watched.*

**How far to trust it.** Five research streams read documentation, source code and papers (their full notes are in `docs/history/permissions-survey/`, one file per stream, with the sources of the claims below). Each claim carries a tag. **[re-read]** the source's own text was read (a raw file, source code, or a paper converted to text), and for the claims a recommendation leans on, read a second way. **[read]** read through a fetch tool that summarises each page with a small model, so numbers and wording may be the summary's. **[secondary]** a second-hand source: a search snippet, a hosting guide, a forum. **[recalled]** memory, unchecked. **[inference]** reasoning from cited facts. **[checked]** I looked at the primary text myself after the streams had finished; there are two such checks and they are named where they matter (the precedence study's numbers, and the order in which LuckPerms tries a wildcard).

Five limits matter. (1) **Nothing was run.** "How it behaves" means how its documentation and source say it behaves. (2) **The scores are judgements.** Four agents scored 19 systems from 1 to 5 against one task list; a difference of one point is noise, and the pattern of 1s and 5s is what to look at. (3) **No study of game servers, or of administering by typed commands, was found.** Every human-factors study below used file permissions, firewalls or phones with students or office workers, so each step from there to "an admin typing commands in a game" is my judgement, and I say so. (4) **Some pages could not be read:** Mojang's own site, Discord's and Second Life's support pages, the Factorio forum, OpenSim's wiki; and nobody could date vanilla Minecraft's new permission objects. (5) **The search allowance was shared** (56 searches in all, 20 allowed to each stream), so a statement that something does not exist, such as "no game lets an admin grant by tag", is weak evidence.

## What the design has to do

The study was done against these needs and tasks. They come from the owner's decisions of 2026-11-22 and 2026-11-23 and were the brief every agent read.

- **R1** A player is a public key made per install: no accounts, no passwords. A nickname is an unverified label, unique per server. Strangers may join public servers.
- **R2** Things that act: players; the server console; command blocks (owned by the player who claimed them and run with that owner's privileges, or ownerless and run as an anonymous operator); and player-written scripts and functions (a shared function runs with the caller's power unless its owner marks it, and then with the owner's).
- **R3** An actor holds a set of groups; `op` is a built-in group and `everyone` the default. Privileges are looked up at every run, never frozen.
- **R4** Wanted later: grant or deny per command, subcommand, pack and tag; role-based and team-based.
- **R5** Things have owners (blocks, ships, scripts, the variables they keep); sharing is by grants to named players or groups: read, call, write.
- **R6** Third parties add packs of commands, and an admin must be able to decide who may use them without reading a manual.
- **R7** Administration is by typed commands and files (a one-line command box; no text fields yet).
- **R8** A singleplayer owner is operator of their world; a dedicated server's owner is a root key in a server file; no central service.
- **R9** Every decision is made on the server and can be explained.

| Task | What an admin tries to do |
|---|---|
| T1 | Let one friend build in one area (or only their own things), nobody else |
| T2 | Make a moderator who may kick, mute and ban, but not change game mode or teleport anyone |
| T3 | Give one player one command for an hour, ending by itself |
| T4 | Find out why player X is refused command Y, and which rule decided |
| T5 | Strip a griefer of everything and keep them out, through a rename and a reconnect |
| T6 | Install a pack of 15 commands: what is the safe default, can one step grant or deny the whole pack |
| T7 | Audit: who can run command Y; everything player X can do |
| T8 | Let a helper manage one team's members without making admins or touching anything else |
| T9 | Bootstrap a fresh server; recover when the only admin is lost |
| T10 | A team of three shares one thing (vehicle, block, script data) that outsiders cannot use; change the members later |
| T11 | An automated thing (command block, scheduled script, bot) runs with limited power; see who it runs as; revoke it |
| T12 | A change takes effect at once without restart; undo a mistake; see who changed what and when |
| T13 | "Everything in a category except one": all commands tagged "building" except `/fill`; a whole pack except two commands |

## The short version

1. **Nothing surveyed meets all nine needs, but every piece has a precedent.** The nearest pieces: LuckPerms (a decision that can tell you why; grants that end by themselves; a web editor that produces one pasteable command), Garry's Mod's CAMI standard (an addon declares a command and its proposed default; the admin system decides), GriefPrevention (a four-step trust chain that is easy to predict), Roblox and Discord (a default for a whole pack, with per-command exceptions; named capability tags for scripts), TShock (a working allow-and-deny rule and a one-time setup code), and Towny and Vintage Story (a helper may hand out only what they hold).
2. **The rule for conflicts is a usability lever, and one controlled study measured it.** In a 54-person study the conflict rule alone, with the interface unchanged, moved the share of correct answers by 22 to 78 points; five of six differences were statistically significant **[checked]**. "The narrower rule wins" did best on four tasks. "Deny always wins" did best where a group-wide ban had to beat one person's own allow (50% against 88%). The large systems (AWS, Cedar, Google Cloud, Azure) make deny the strongest rule and independent of order **[read]**; the ones whose rules depend on order (sudo's last match, file ACLs, SourceMod's group order) are the ones documented as traps **[re-read]**. A two-layer rule follows from both, in section "What this means for the design".
3. **Show the result, not only the rules.** Two studies at Carnegie Mellon (24 and 36 people) found that a view of who ends up with what lifted correct answers from 25% to 100% on one task and from 0% to 89% on a conflict task **[re-read]**. No system we read has all three questions an admin asks: *why was this refused, what can X do, who can do Y*. LuckPerms answers the first well, Oxide and Kubernetes the second, CC:Tweaked's `commands.list()` is the nicest "what may this actor run", and nobody has a built-in "who can" over wildcards and groups.
4. **Explainability is the weakest point of the whole field.** Of the 19 systems scored, none scored 5 for it; only LuckPerms, GriefPrevention and Luanti scored 4; vanilla Minecraft, command blocks and Luanti's trusted mods scored 1 **[inference from the notes' scores]**. The cheap models are Luanti's refusal that names the missing privilege, Roblox's that names the missing capability, and CAMI's (yes or no, reason) answer, which its largest implementation, ULX, throws away **[re-read]**. Make the decision function return a record (who decided, which rule, which layer), not a yes or no.
5. **New packs: closed by default, and the admin must be shown what appeared.** Luanti, Oxide and 7 Days to Die refuse by silence **[re-read; secondary for 7 Days]**, and none of them shows the admin what just arrived; ULX stamps a pack's proposed default into a group once, silently, so the pack can never update it **[re-read]**. CAMI's shape is right with two changes: the proposed default is a live bottom layer, and the admin is shown "15 new commands, proposed default: nobody".
6. **Nobody has tags; per-root-word nodes are too coarse.** No game permission system we read lets an admin grant by tag **[weak: shared searches]**. The nearest: SourceMod's command groups written `@Name`, Roblox's 41 capability tags on engine calls, and AWS's access levels (List, Read, Write, Tagging, Permissions management), outside games. The mods VanillaPermissions and FTB Ranks both give every word of the command tree its own node **[re-read]**, which suggests that one node per command was too coarse for them **[inference]**: this supports the decision to put subcommands into `CommandSpec`.
7. **Nobody attributes automation to a person; your R2 is ahead of everything surveyed.** A vanilla command block has no owner, runs at a fixed level 2, and whoever may edit it gets everything it can do; on Paper all command blocks share one operator object **[read, re-read]**. Closest precedents: FTB Chunks (a machine acts under its placer's id; unknown machines are refused by default), Space Engineers (a programmable block recomputes its reach from its owner at every run), and GriefPrevention and WorldGuard (a machine acts as the owner or member of the area it sits in) **[re-read]**.
8. **Write access to code that runs with someone's power is that power.** Space Engineers' check on replacing a program, in the one 2022 source file read, tests the sender's scripter rank and not ownership of the block **[single raw read]**; Roblox free-model backdoors give a model's author the game's authority; Luanti's trusted mods hold the whole machine and had two advisories (one dated 2026-04-16) **[re-read; read]**; Paper's `minecraft.commandblock` node lets a non-op in creative mode write commands that run at level 2 **[re-read]**. The design should never make "may edit a block or a marked function" weaker than "may do what it does".
9. **Delegation needs a rule built into the engine.** Kubernetes lets you grant only what you hold, with named escape verbs; Vintage Story and Discord restrict you to roles at or below yours; Towny makes the right to hand out each rank a node of its own; Luanti's `basic_privs` names the few privileges a moderator may hand out **[read, re-read]**. LuckPerms has no such rule: its opt-in version needs about six nodes and a config switch, and scored 2 for delegation. FTB Ranks closed a real hole in 26.1.2.4: the commands that change ranks needed only level 2, so command blocks and signs could run them **[re-read]**.
10. **Temporary grants are almost missing: the cheapest win in the field.** Of the game systems, only LuckPerms (`settemp`) and TShock (`/tempgroup`, `/su`) have them. T3 was the worst task: 11 of 17 scored 1. One consequence for Voxunami: expiry must count game ticks, not clock time, under the hard rule against wall-clock time in simulation **[inference]**.
11. **A fresh key costs a griefer nothing, so bans are speed bumps.** Almost every system keys grants on a stable id and shows the name as a label (Luanti keys on the name, and an open issue asks to separate authentication from privileges; Kubernetes warns that a reused name inherits rights), and none has an answer to ban evasion with free identities: the ones with bans lean on accounts held by a central service. Safety has to come from a weak `everyone` group, and a ban from a deny in a layer that a personal allow cannot beat. Lockout is best handled by a root key in a file, a console that is always root, a `revoke all` that cannot strip the owner, and (TShock) a one-time setup code printed on a fresh server **[re-read]**.
12. **In a one-line command box, copy these patterns:** LuckPerms's web editor, whose Save produces one command to paste, with a printed summary; Factorio's export and import of a whole permission group as one string; Luanti's `/help`, which lists only what you may run, and `/help privs`; per-command defaults printed before first use. Avoid role-by-power matrices: EVE Online's corporation roles took 19 screens and were redesigned to 5 **[read]**.

## What was looked at

More than fifty systems and models; the first block is covered in depth with all thirteen tasks, the rest in brief or as models. "Depth" is the research notes' own word.

| System | Model in one line | The idea worth taking | The weak point | Depth, evidence |
|---|---|---|---|---|
| Minecraft Java, vanilla ops | One level 0 to 4 per player, one level asked by each command; cumulative; no deny | Simple to explain | A moderator who kicks but cannot teleport is impossible; no reason shown; named permission objects appeared in code in 2025-26 and no page describes them | deep; [read], code [re-read] |
| Bukkit and Paper `plugin.yml` | A plugin declares nodes with default true, false, op or not-op | Defaults are declared with the node | An undeclared node counts as op; vanilla level OR node, so a deny cannot take power from an op | deep; [re-read] |
| LuckPerms | Groups with weights, nodes, allow and deny, contexts, temporary grants | Every answer carries the node and the rule that decided; `verbose command` runs a command as another user and lists each check | An exact node beats any wildcard whatever the weights **[checked]**; no "cannot grant what you lack" rule; a refusal made by something else is invisible | deep; [re-read] |
| Fabric Permissions API, NeoForge PermissionAPI, FTB Ranks, VanillaPermissions | A mod declares a node and a default; a separate system decides | NeoForge makes the default mandatory, and unregistered nodes throw | FTB Ranks's command-block hole (above) | deep; [re-read] |
| GriefPrevention | Land claims; trust chain access, containers, build, manage | The most predictable model; the refusal names the owner and the level | Admin bypass is not logged; a manager can open the claim to everyone | deep; [re-read] |
| WorldGuard | Regions with flags; priority number; deny beats allow at equal priority; children inherit what they do not set | The clearest allow-plus-deny rule found | At least seven interacting rules, with documented traps | deep; [re-read] |
| Towny | Rank by action matrix per town and plot; per-rank grant nodes | An assistant may give "helper" but not "assistant" | No single conflict text found | deep; [read] |
| Vanilla and Paper command blocks | One stored command; ownerless; level 2; one global dial | Vanilla's `/trigger`: a narrow verb a player may use that a block reacts to **[recalled]** | Edit power equals run power; no per-block limit | deep; [read, re-read] |
| Luanti with the areas mod | A flat set of named privileges per player name; `/grant`; owners of areas | The refusal lists what was missing; `basic_privs` limits a moderator's grants | No groups, no deny, no timed grant; keyed by name | deep; [re-read] |
| Vintage Story | One role per player; 30 privileges; ordered claim tiers | Shipped, described roles (a safe moderator in one command) | No deny; no timed grant; enforcement of "hand out only what you hold" not read in code | deep; [read] |
| Factorio | Permission groups over about 300 input actions, each ticked or not | Export and import of a group as one string | Admin is one switch; scripts are not limited by it | deep; [read] |
| SourceMod | Flags, immunity, per-command overrides, command groups `@Name` | Rules on commands and on named sets of commands | The first group with a rule decides, so the order of an admin's groups matters | deep; [re-read] |
| Garry's Mod: CAMI and ULX | A standard through which addons register privileges with a proposed default | The (yes or no, reason) answer and the split of jobs | One group per player; flat names; ULX copies the default once and drops the reason | deep; [re-read] |
| Rust with Oxide | Plugin-named permission strings; groups with one parent; no deny | Nothing granted until someone grants it; `oxide.show` queries | A wildcard copies only what exists now; no bulk revoke | deep; [re-read] |
| 7 Days to Die | A number per command, 0 to 1000, lower is more power | Unlisted commands are admin-only | Evidence is hosting guides only | deep, thin; [secondary] |
| Second Life | Object permission bits; group roles and abilities; a script asks a person for named powers | Power over a person needs that person's yes | The debit prompt (money) became a scam route; a group with no owners left cannot be recovered | deep; [read] |
| Space Engineers blocks | Owner, faction or all, per block; reach recomputed per run | Recomputed from the owner every time | Program replacement checked against rank, not ownership (one 2022 file) | deep; [re-read] |
| Roblox | Group roles in rank order; 41 capability tags for sandboxed scripts | A nested sandbox gets the common part; an error names the missing tag | Sandboxing is opt-in; the default is full authority | deep; [re-read] |
| Discord | 41 permission flags; roles; channel and per-command overrides; 8-step published algorithm | Per-pack default with per-command overrides and a visible "Not Synced" state | Allow beats deny between roles; renaming a command deletes its grants | deep; [re-read] |
| Luanti trusted mods | The operator's list decides which mods get the whole machine | Default empty, one line to audit | Two security advisories on how the asking mod is identified | deep; [re-read, read] |
| In brief | TShock, Project Zomboid, Space Engineers ranks, Valheim and ARK, Hytale (official page names only a file; the model is described by hosting companies and community docs), Sponge, Bedrock, PermissionsEx and GroupManager (history), LWC and Lockette, FTB Teams and Chunks, CC:Tweaked command computers, WoW guild ranks, EVE corporation roles, Google Drive and Notion sharing, OpenComputers | | | light |
| As models | AWS IAM, Cedar, Google Cloud and Azure deny rules, Kubernetes RBAC, Zanzibar, Apache Shiro strings, Windows ACLs, sudo, Deno, the confused deputy (Hardy 1988) | See sections below | | models; [read], papers [re-read] |

## The usability study

### Method

Each deep system was walked through the thirteen tasks by a research agent working from documentation and source: how the task is done, how many commands or clicks, and a score from 1 to 5 with an evidence tag. 5 means one obvious, discoverable, safe step; 3 means possible but you must know the trick or edit files; 1 means impossible, or dangerous, or needs a restart. This is a desk walk-through in the style of a cognitive walkthrough, not a test with people; the notes (`notes-5-models-and-method.md`, section 7) contain a ready-to-run protocol for the real test, which is the way to turn these scores into evidence.

### The scoreboard

A dot means the task does not apply to that kind of system, or nothing was found. The average is for orientation only: the rows do not solve the same job (land systems skip T2 and T9; command systems skip T1 and T10) and four different agents scored them.

| System | T1 | T2 | T3 | T4 | T5 | T6 | T7 | T8 | T9 | T10 | T11 | T12 | T13 | avg (n) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| LuckPerms (Paper) | 2 | 4 | 5 | 4 | 3 | 3 | 3 | 2 | 4 | 2 | 1 | 3 | 3 | 3.0 (13) |
| Vanilla Java ops | 1 | 1 | 1 | 1 | 4 | 2 | 2 | 1 | 4 | 1 | 2 | 2 | 1 | 1.8 (13) |
| FTB Ranks | 1 | 4 | 1 | 2 | 3 | 4 | 3 | 1 | 4 | 1 | 1 | 2 | 3 | 2.3 (13) |
| GriefPrevention | 5 | · | 1 | 4 | 3 | 4 | 2 | 4 | · | 5 | 3 | 3 | 3 | 3.4 (11) |
| WorldGuard | 5 | · | 1 | 4 | 3 | · | 4 | 4 | 4 | 5 | 3 | 3 | 4 | 3.6 (11) |
| Towny | 4 | 4 | · | 3 | 3 | 4 | 3 | 4 | 5 | 5 | 3 | 3 | 4 | 3.8 (12) |
| Command blocks (vanilla, Paper) | · | · | 3 | 2 | · | 4 | 1 | 1 | · | · | 2 | 3 | 2 | 2.2 (8) |
| Luanti + areas | 3 | 5 | 1 | 5 | 3 | 3 | 3 | 2 | 4 | 3 | 2 | 3 | 2 | 3.0 (13) |
| Vintage Story | 3 | 4 | 1 | 3 | 4 | · | 3 | 4 | 4 | 4 | · | 3 | 3 | 3.3 (11) |
| Factorio | 1 | 1 | 2 | 3 | 4 | 1 | 3 | 1 | 4 | 1 | 1 | 3 | 4 | 2.2 (13) |
| SourceMod | · | 3 | 1 | 2 | 5 | 3 | 2 | · | 3 | · | · | 3 | 3 | 2.8 (9) |
| Garry's Mod (CAMI, ULX) | · | 3 | 1 | 2 | 4 | 2 | 2 | 2 | 4 | · | 1 | 3 | 2 | 2.4 (11) |
| Rust (Oxide) | · | 3 | 1 | 4 | 3 | 4 | 4 | 1 | 4 | · | 1 | 3 | 3 | 2.8 (11) |
| 7 Days to Die | · | 3 | 1 | 3 | 4 | 3 | 3 | 1 | 3 | · | · | 2 | 2 | 2.5 (10) |
| Second Life | 4 | 4 | 2 | · | 4 | · | · | 4 | 1 | 3 | 3 | · | 2 | 3.0 (9) |
| Space Engineers (blocks) | · | 3 | 1 | 2 | 3 | · | · | · | 4 | 3 | 3 | · | · | 2.7 (7) |
| Roblox | 2 | 5 | 2 | 4 | 4 | 3 | 3 | 5 | 1 | 4 | 4 | 3 | 2 | 3.2 (13) |
| Discord | 4 | 5 | 2 | 2 | 4 | 4 | 3 | 3 | 3 | 5 | 4 | 4 | 3 | 3.5 (13) |
| Luanti trusted mods | · | · | · | 1 | · | 5 | 5 | · | · | · | · | 1 | · | 3.0 (4) |

Where a cell has two readings in the notes, the first is shown: GriefPrevention T1 is 5 for an area and 2 for "only their own things"; Roblox T4 is 4 for scripts and T9 is 1 for recovery; LuckPerms T13 is 3 (4 for deny-inside-allow alone, 1 for tags, which it lacks). Space Engineers and Second Life are ownership systems, so many tasks do not apply.

### What the scoreboard says, task by task

| Task | Mean (n) | What the best did | What most do | What to take |
|---|---|---|---|---|
| T1 friend builds in one area | 2.9 (12) | GriefPrevention `/trust Friend` in the claim; WorldGuard `/rg addmember`; Towny `/plot trust add` (5, 5, 4) | Command-permission systems do not do areas at all; LuckPerms needs a region plugin (2) | Voxunami has no land claims yet, so T1 depends on a feature that does not exist; the trust chain (access, containers, build, manage) is the model for sharing verbs |
| T2 moderator | 3.5 (15) | Luanti: one command `/grant Mod kick,ban,basic_privs` (5); Vintage Story: a shipped role, one command (4); Roblox and Discord 5 | LuckPerms takes 6 to 8 commands or one web-editor session (4); impossible in vanilla (cumulative levels) and Factorio (admin is one switch) | A moderator must be a list of things allowed, not a rung of a ladder; ship described starter roles |
| T3 one command for an hour | **1.6 (17)** | LuckPerms `settemp`, one command (5) | 11 of 17 systems scored 1: nothing exists | The cheapest win; store expiry in game ticks |
| T4 why refused | 2.8 (18) | Luanti's refusal lists the missing privileges (5); LuckPerms `verbose command` (4); Oxide `show` (4) | Nobody says which rule decided in a system that has deny (SourceMod, TShock) | Return a record; make the refusal say what is missing |
| T5 strip a griefer | 3.6 (17) | SourceMod (5) | A ban verb exists everywhere (no 1s) | Easy to do, weak in effect with free keys (finding 11) |
| T6 new pack of 15 | 3.3 (15) | Oxide: nothing granted until granted (4); NeoForge: mandatory default; Paper's docs list a default for every command (4) | No system shows the admin what just arrived | Closed by default plus a "new and ungranted" view |
| T7 audit | 2.9 (17) | Oxide's `show` queries and WorldGuard (4); CC:Tweaked `commands.list()` | "Everything X can do, through wildcards and groups" is rarely one command; LuckPerms `search` finds exact holders only | Three typed queries: why, what, who |
| T8 delegate | **2.5 (16)** | Roblox (5): manage only roles below yours, grant only what you hold; Towny, Vintage Story, WorldGuard 4 | Six systems scored 1; LuckPerms 2 | Build the rule into the engine |
| T9 bootstrap, recovery | 3.5 (16) | Towny 5; TShock's setup code (5, light card) | Console always root is the common answer; Second Life groups and Roblox 1 (no recovery) | Root key in a file, console root, setup code |
| T10 team shares a thing | 3.2 (13) | Towny, WorldGuard, GriefPrevention, Discord (5) | Command-permission systems have no objects, so 1 to 2 | Teams are groups; sharing is a grant on an object |
| T11 limited-power automation | **2.3 (15)** | Roblox capability containers and Discord integration permissions (4); no 5 anywhere | Five scored 1; command blocks 2 | R2 is ahead of everything; see finding 7 and 8 |
| T12 change now, undo, audit | 2.8 (17) | Discord (4: audit log) | Most 3: effect is at once and a log exists, but **no undo command was found in any system** (re-run the inverse or restore an export) | Change log from day one; undo later |
| T13 all except one | 2.7 (17) | WorldGuard, Towny, Factorio (4); TShock (5, light card): wildcard plus negation in one group | The documented hazard: sudo says subtracting from ALL is trivially bypassed; AWS warns NotAction can grant more than meant | Attach a denial to the action that finally runs, not the typed name |

Three tasks form the bottom of the table: **T3** (temporary), **T11** (limited-power automation) and **T8** (delegation). They are also the three the owner's design leans on most (scripts, blocks, helpers), so they are where borrowing least helps and designing carefully matters most.

### Explainability

Scored apart from the tasks: LuckPerms, GriefPrevention and Luanti 4; WorldGuard, Towny, Vintage Story, Oxide and Roblox 3; FTB Ranks, Factorio, SourceMod, ULX, 7 Days to Die, Space Engineers and Discord 2; vanilla ops, command blocks and Luanti trusted mods 1; Second Life not applicable **[inference from the notes' own S2 scores]**. The best cheap answers: Luanti's refusal text, Roblox's error naming the first missing capability, LuckPerms's tools, AWS's denial message and Cedar's list of every deciding policy (outside games). The shared flaw: even the good ones name one reason; AWS names only one policy type when several deny, and Discord's "view server as role" shows what a role sees but not why **[read; secondary]**.

### What studies of people say

All studies below used file permissions, firewalls or phones. Their size and tasks are stated so that the carry-over can be judged.

| Study | What it found |
|---|---|
| Reeder and others, CHI 2011, "More than Skin Deep"; 54 students who had never administered, three conditions of 18 | Same interface, only the conflict rule changed: accuracy differed by 50, 38, 55, 22, 78 and 50 points over six tasks, five statistically significant. Narrower-wins beat Windows' deny-wins on four tasks (for example 89% against 11% when two groups disagree). Deny-wins beat it where a group-level deny had to defeat a person's own allow (88% against 50%, p = 0.014). The abstract: small changes to a system's rules can change its usability "beyond the power of user interfaces to correct" **[checked: abstract and table read in the paper]** |
| Maxion and Reeder 2005; 24 technical students | An effective-permissions dialog against the Windows XP one: success 83%, 100%, 100% against 58%, 25%, 75%; the commonest failure was not knowing what to do, because the needed information was not shown **[re-read by the notes' author]** |
| Reeder and others, CHI 2008, "Expandable Grids"; 36 students | A matrix view of who can do what: on a conflict task 89% against 0% (small policy) and 100% against 6% (large); 14 of 27 tests significant, all for the matrix; it slowed more than the dialog as policies grew **[re-read]** |
| Wool, firewall rule sets (IEEE Computer 2004; revisit 2009 and 2010) | Configuration errors rise with the size and tangle of the rule set; a revisit found no improvement in newer versions **[secondary; abstract read]** |
| Felt and others 2012; 308 survey and 25 lab participants | At install time 17% paid attention to the permission list and 3% could answer three questions about it; a list shown once is not read **[re-read]** |
| Wijesekera and others 2015; 36 participants, 27 million logged uses | At least 80% would have blocked at least one request they saw logged: per-use prompts inform but people mostly click yes (a later six-week study found 84% of prompts accepted **[secondary]**) |
| Mazurek and others, CHI 2010; 33 people in 15 households | Wanted policies are social and reactive, not written in advance: people want to adjust in response to requests **[re-read]** |

What none did: test a game server, test typed commands, test experienced admins, or test recall of one's own configuration months later. Role engineering (how many roles an organisation ends up with) has claims but no controlled study **[secondary]**.

## What this means for the design

Each decision below gives the evidence, then what I would do, with a confidence. These are opinions; the owner's questions are collected at the end.

**D1. Which rule wins in a conflict (R4).** Evidence: the precedence study; the large systems' order-free deny; LuckPerms's exact-beats-wildcard surprise; SourceMod's group order; Paper's OR between two layers.
*I would:* two layers. **Everyday grants** (groups, teams, personal) where the narrower rule wins (a personal rule beats a group's; a command beats its pack), so that "Alice is the exception" is one local addition, and at equal rank deny wins. **A short guardrail layer** written only by root, where deny always wins and nothing local can undo it: bans, quarantine, "nobody may". The study's weak spot, the group-wide ban that a personal allow defeated, is why safety denies live in the second layer. Print the rule in two sentences in `help`, and let every explanation quote the sentence that applied. No weights and no order of groups anywhere. Confidence: medium (one study; novices; file permissions; the study did not test two layers or games).

**D2. What a permission is attached to (R4, R6).** Evidence: vanilla and Bukkit nodes are per root word and proved too coarse; VanillaPermissions and FTB Ranks add a node per word of the command tree; LuckPerms's own commands have one node per command path, with opt-in argument checks; AWS classifies every action (List, Read, Write, Tagging, Permissions management) so one grant covers a class; Discord gives a pack a default and lets each command differ, showing "Not Synced"; Roblox lists capability tags per engine call. Shiro and AWS warn that text globs also cover later additions and that an empty part can silently mean "all".
*I would:* give every subcommand a namespaced id of the form `pack:command.subcommand` (the subcommands you decided to put in `CommandSpec` make this free), and let each carry a **class** from a short fixed list (for example read, change, permission, automation) and optional **tags** declared by its pack. A grant may name an exact id, a subtree, a pack, a class or a tag. A wildcard means a subtree only, never a text pattern, and an empty part is an error. Confidence: medium for ids and classes; **low for tags**, for which there is nothing to copy (see the open problem below).

**D3. Third-party packs (R6).** Evidence: closed-by-silence in Luanti, Oxide and 7 Days to Die, with no view of what arrived; CAMI's declare, propose, decide split; ULX's copy-once flaw; NeoForge's mandatory default per node; Sponge's "unset is denied" and role templates; Bukkit's "undeclared means op"; LuckPerms's switch that ignores every plugin default.
*I would:* each pack declares a proposed default for each command (nobody, op or everyone); the proposal is a live bottom layer evaluated at each check, not a copy written into a group, so an update or an uninstall takes effect; the admin's rules sit above it. A server switch ignores pack-proposed defaults. At install the admin is shown "15 new commands: 9 read, 4 change, 2 permission; proposed default: nobody; grant with one command". Your three op-only commands (`gamemode`, `light`, `tp`) are the same mechanism applied to the game's own pack. Confidence: high on closed-and-visible; medium on the details.

**D4. Groups and roles (R3).** Evidence: union of roles in Second Life, Discord and Roblox; rank used only to decide who may manage whom (Discord, Roblox, WoW); one group per player in CAMI, ULX, Vintage Story and Factorio; SourceMod's first-group-wins; LuckPerms's weights, which only order nodes with the same key; Oxide's one parent per group.
*I would:* a set of groups; `op` and `everyone` built in and `everyone` editable like any group; permissions are a union; a group's **rank** number only says who may manage whom; no weights. To avoid role explosion (a role per team per area), grant to a (group, scope) pair rather than making a role for each combination. Confidence: medium-high.

**D5. Delegation and escalation (R3, T8).** Evidence: Kubernetes's rule with named escape verbs; Vintage Story's "roles at or below yours, privileges you hold"; Towny's per-rank grant nodes; WorldGuard's `addmember` apart from `addowner`; Luanti's `basic_privs`; AWS's boundary and its documented hole (the delegate can create users who reach what the delegate cannot).
*I would:* one engine rule: you may grant only what you hold, in the scope you hold it; plus **management grants** per group ("may add and remove members of group red"), which is Towny's idea. Root and console are unrestricted and always logged. Confidence: medium-high.

**D6. Actors that are not a person at a keyboard (R2, T11).** Evidence in findings 7 and 8, and: Sponge has a "Command Block" subject that runs "with the permissions of the creator"; Hardy's 1988 confused deputy (a compiler with its own authority overwrote a billing file named by a caller); Kubernetes treats "may create a workload" as holding the rights of every account the workload can name; Deno equates running a subprocess with everything; Paper's "both layers must pass" rule is the right idea for blocks; vanilla's `/trigger` (a level-0 verb that a level-2 block reacts to **[recalled]**) is the pattern for a marked function.
*I would:*
- **Name every actor in the log:** a player; the console; `block at x,y,z owned by K` or `server block` (the stand-in for an ownerless one); `function f of K on behalf of C`.
- **Permission-administration commands refuse non-human actors by default**, with an op able to allow it on purpose. This is the FTB Ranks lesson, and confidence is high.
- **A marked function** runs with the owner's *current* power limited to a ceiling its owner declared, and the call is recorded "on behalf of" the caller. If anyone but the owner edits it, the mark is cleared, or it runs with the common part of the owner's and the editor's power (Roblox's nested rule). When it takes a target from its caller (a player, a place, a variable name), it must check what the **caller** may do to that target.
- **Write access to a block or marked function must never be weaker than the power it runs with.**
- **Creating automation is itself a privilege** (the class `automation`): it is the thing Kubernetes gates when it gates workload creation.
Confidence: medium; high for the second and fourth points.

**D7. Explaining a decision (R9, T4, T7).** Evidence: LuckPerms's record and `verbose command`; Luanti, Roblox and GriefPrevention's refusal text; CAMI's reason slot; Cedar's list of all deciding policies against AWS's one-layer message; the studies showing that an effective view lifts correct answers from 25% to 100%.
*I would:* the decision function returns a record (allowed or not; the layer; the rule; the group or grant it came from; the actor chain including "on behalf of"; any rule it overrode). On it build three typed queries: **why** (player, action), **what** (player) and **who** (action), each naming every deciding rule, not one. The refused player is told what is missing in words; an op gets the whole record. A small buffer of recent refusals per player lets an admin ask "why was Alice refused a minute ago" without having switched tracing on first. Confidence: high.

**D8. Temporary grants and history (T3, T12).** Evidence: only LuckPerms and TShock have temporary grants, TShock's timer lives in memory and is lost on restart; LuckPerms keeps a log and has no undo; Luanti logs each grant; Roblox keeps an audit log; Saltzer and Schroeder: recording a breach can stand in for preventing it.
*I would:* every grant may carry an expiry, written as a duration (`for 1h`) and stored as a game tick, checked at lookup; an append-only change log (who, what, when, from which actor) from the first release; undo later; export and import of the whole rule set as one string for backup (Factorio). Confidence: medium-high; the tick rule is an inference from the hard rule on wall-clock time.

**D9. Identity, bans and lockout (R1, R8, T5, T9).** Evidence in finding 11, and: Oxide creates an empty stored record for every unknown id it is asked about, so its files grow with every stranger; Luanti keys on the name and an open issue asks to separate authentication from privileges; Second Life groups with no owners left cannot be recovered; WoW has a bounded takeover after 90 days; OpenComputers opens a computer to everyone when its list is emptied.
*I would:* store grants against the key; show the nickname with a short key fingerprint in every log line; a new key starts in the weak `everyone` group; "empty list" always means nobody; a ban is a deny in the guardrail layer on the key; the root key is listed in a server file and the console is always root; `revoke all` cannot strip the root key; a fresh dedicated server prints a one-time setup code that binds the first root key (TShock), so that nobody has to copy a public key into a file by hand. Confidence: medium.

**D10. The admin interface in a command box (R7).** Evidence in finding 12. *I would:* one grammar for every change, for example `perm grant <who> <what> [for <time>]`, `perm deny ...`, `perm revoke ...`; `help` hides what you cannot run; bulk forms (`grant pack:x to group:y`); a printed summary after every change; a text dump that prints the matrix of groups and classes rather than a screen of checkboxes. Confidence: medium; typed-only administration was not tested by any study.

**D11. Sharing data and functions (R5).** Evidence: no game has per-variable or per-function grants for player scripts (weak: shared searches); the nearest are Roblox asset grants (subject, action, object; `Revoked` is a state of its own; a flag extends a grant to what the asset depends on), Discord's per-command overrides for up to 100 users, roles or channels, OpenSim's per-function allow lists **[secondary]**, Google Drive roles with expiry and a separate right to re-share, Notion's "can create" (write without read), and Zanzibar's tuples with teams as sets, answered both by Check and by Expand ("who has access").
*I would:* a grant is (subject, verb, object, optional expiry, optional "as owner" flag). Verbs kept apart: `read`, `call` (with the caller's power), `call as owner` (its own verb, matching your marked functions), `write`, and `share`. A grant says whether `call` also reaches what the function touches. A revoked grant leaves a tombstone ("taken away by K on tick N") so that "why not?" has an answer. Teams are groups, so a grant to a group covers its members live. Confidence: medium; no precedent to test it against.

## Where this touches what you have already decided

Nothing here asks to change a decision; these are the places where the evidence bears on one.

- **An ownerless block runs as an anonymous operator and only ops edit it.** This equals vanilla's rule (the block has operator power, and only operators may edit), and it is safe for the same reason: the right to edit is no wider than the power. The research's warning is about the cases that break that equality. Space Engineers' ownerless actor is the weakest actor in its game, the opposite choice; the safeguards that make yours sound are the named stand-in, a command that lists every ownerless block (T7 scored 1 for command blocks), the refusal of permission-administration commands (D6), and a plain sign in the block's interface that it runs with operator power.
- **Only the placer can claim, and claiming moves the block to that player's privileges.** Claiming can only lower a block's power, and it matches GriefPrevention's owner right that cannot be granted and FTB Chunks's machines acting under their placer's id. Nothing to change.
- **Ownership can be cleared only by breaking the block.** No system we read handles a banned or vanished owner by itself (GriefPrevention leaves bans alone on purpose, for appeals); each gives admins explicit verbs: delete all of a player's claims, transfer to an admin, convert to an admin claim. Breaking is your "delete". What the rule needs to be safe is that a banned owner's blocks lose their power at once, which holds only if the guardrail-layer ban is part of the same live lookup that gives a block its owner's privileges (R3). Worth a test when it is built.
- **A function marked as owner runs with the owner's power.** Add the ceiling, the "on behalf of" record, the cleared mark on a non-owner's edit, and the caller-target check (D6). These refine the decision rather than reverse it.
- **`gamemode`, `light` and `tp` are op-only.** This is the pack-proposed default of D3. `help` should list only what the asker may run.

## Open problems the research turned up

1. **Tags break "the narrower rule wins".** The study's hardest tasks were the two-dimensional conflicts, where one rule is narrower in who and the other in what (the Windows dialog scored 0% on one of them). A command may carry several tags, and two tags need not nest. Options: tags sit at the same rank as packs, with deny winning between equals; or tags appear only in role definitions and in the guardrail layer. I lean to the first; confidence low, and it is the part most worth a playtest.
2. **A nickname spliced into a command is a confused deputy.** A nickname is chosen by a stranger. If a block with operator power builds a command from `{>nearest.nickname}` and the value goes into a `Rest` argument as text, spaces or other words in the nickname become extra arguments. Shiro's documented pitfall is the same shape (a separator in a user-supplied id changes the meaning of a grant). The cheap defence is to allow only letters, digits and a few marks in nicknames, with no spaces. **[inference]**
3. **Expiry must count game ticks**, not clock time (D8), or a recording or a second machine would decide differently.
4. **Never join two permission layers with OR.** Paper decides a vanilla command by "level passes OR node passes", so a deny can never take power from an op; for command blocks it added a setting, on by default, that makes them need both **[re-read]**.
5. **A rename must not destroy grants.** Discord deletes a command's permissions when it is renamed or removed **[re-read]**; key grants by `pack:name` ids and report grants that point at nothing.
6. **The first dedicated-server start** (R8) has no pattern in the keyed world except TShock's setup code.
7. **Per-variable grants have no game precedent.** Zanzibar and Drive are the models, and neither is a game.

## Questions only the owner can answer

Each has my recommendation.

- **P1. Conflict rule.** Two layers as in D1 (everyday narrower-wins with deny winning between equals; a root-only guardrail layer where deny always wins)? *Yes.*
- **P2. What a permission is attached to.** Namespaced subcommand ids with a class from a short fixed list and optional tags (D2)? Which classes? *Read, change, permission, automation.*
- **P3. Packs.** Closed by default, the pack's proposal as a live bottom layer, the admin shown what arrived, a switch to ignore proposals (D3)? *Yes.*
- **P4. Delegation.** You grant only what you hold; plus per-group management grants; root and console unrestricted (D5)? *Yes.*
- **P5. Automation.** The three additions to marked functions and the refusal of permission-administration commands by non-human actors, and a named stand-in with a listing command for ownerless blocks (D6)? *Yes to all.*
- **P6. First release scope.** Expiring grants (in ticks) and the change log in the first release, undo later (D8)? *Yes.*
- **P7. Explaining.** The decision record and the three queries *why*, *what* and *who* in the first release; what the refused player sees is in words, an op sees the whole record (D7)? *Yes.*
- **P8. Nicknames.** Restrict to letters, digits and a few marks, no spaces, unique per server, with the key's short fingerprint beside it in logs? *Yes.*
- **P9. Fresh server.** A one-time setup code printed on the console that binds the first root key, as well as the key file? *Yes.*
- **P10. Playtest.** Run the protocol (five testers, three rounds, success criteria written first) as soon as the first commands exist, and no later than the first prototype of the two layers? *Yes; tasks T3, T8, T11 and T13 first.*

## The playtest protocol

The fifth stream wrote a ready-to-run protocol (`notes-5-models-and-method.md`, section 7): 60-minute sessions, one facilitator and one note-taker; thirteen task cards in plain words (for example "Dana will help moderate. She can kick, mute and ban troublemakers. She must not be able to change anyone's game mode or teleport anyone"), each with a starting state, an oracle (a script that probes what each test player can and cannot do) and, in half of them, a seeded trap; scoring by the final state against the goal, with extra changes counted as wrong (the way the Carnegie Mellon studies scored) and a flag for any unsafe result; six prediction items (read a short rule file and say what a named player may do and which rule decides, half with the explain tools and half without); a ladder of hints that is recorded each time; rules for not leading the participant; and a table of known weaknesses. Five testers per group and up to three rounds, with fixes between them, beat one round of fifteen **[read: Nielsen]**; the thresholds for success are the notes' suggestion and must be fixed before round one. This survey scores systems on paper; this protocol is what would tell you whether the design that comes out of it is usable.

## Sources and notes

The raw notes, one file per research stream, with every source and its tag:

- `notes-1-minecraft-commands.md`: vanilla ops, Bukkit and Paper, LuckPerms, the Fabric and NeoForge permission APIs, FTB Ranks and VanillaPermissions, and Sponge, Bedrock, PermissionsEx and GroupManager in brief; direct answers on node naming, wildcards, subcommands, tags and defaults; the exact commands for T2, T3, T4 and T7.
- `notes-2-minecraft-land-teams-blocks.md`: GriefPrevention, WorldGuard, Towny, command blocks and automation, and LWC, FTB Teams and Chunks and CC:Tweaked in brief; ownership transfer and banned owners; how players see what they may do where.
- `notes-3-other-game-servers.md`: Luanti, Vintage Story, Factorio, SourceMod, Garry's Mod with CAMI and ULX, Rust with Oxide, 7 Days to Die, and TShock, Project Zomboid, Space Engineers, Valheim, ARK and Hytale in brief; CAMI as a model.
- `notes-4-ownership-sharing-scripts.md`: Second Life, Space Engineers, Roblox, Discord and Luanti trusted mods in depth; WoW, EVE, Google Drive, Notion and OpenComputers in brief; how a script acts for an owner; prompts against advance grants; what per-variable grants look like where something is close.
- `notes-5-models-and-method.md`: the studies of people with numbers; AWS IAM, Cedar, Kubernetes RBAC, Zanzibar, Shiro, Google Cloud, Azure, sudo, Deno; the confused deputy; Saltzer and Schroeder applied; evaluation methods; the playtest protocol; a design-space table of the choices real systems made; combinations shown or reported to confuse people; where the literature gives a clear answer.

Primary sources that carry the most weight here:

- [More than Skin Deep: Measuring Effects of the Underlying Model on Access-Control System Usability](https://users.ece.cmu.edu/~lbauer/papers/2011/chi2011-semantics.pdf) (Reeder, Bauer, Cranor, Reiter, Vaniea; CHI 2011) **[checked]**.
- [Reeder's thesis, Carnegie Mellon CMU-CS-08-143](https://www.csd.cmu.edu/sites/default/files/phd-thesis/CMU-CS-08-143.pdf), which holds the Expandable Grids and Salmon studies **[re-read]**.
- LuckPerms: the wiki's source repository at github.com/LuckPerms/wiki (the pages folder) and the code at github.com/LuckPerms/LuckPerms (the calculator and its processors) **[re-read; wildcard order checked]**.
- CAMI: github.com/glua/CAMI and ULX's implementation **[re-read]**.
- Paper's permission reference at docs.papermc.io (`/paper/reference/permissions` and `/paper/reference/world-configuration`), and its source patches for `CommandSourceStack` **[re-read]**.
- WorldGuard's flag and priority pages at worldguard.enginehub.org **[re-read]**.
- The Discord permissions page (Discord's API documentation repository, `discord-api-docs`) and Roblox's `creator-docs` repository, pages on capabilities and groups **[re-read]**.
- Kubernetes RBAC at kubernetes.io (the reference page `access-authn-authz/rbac`); AWS IAM's policy-evaluation page; the Cedar documentation at docs.cedarpolicy.com **[read]**.
- Norm Hardy, [The Confused Deputy](http://cap-lore.com/CapTheory/ConfusedDeputy.html) (1988) **[re-read]**; Saltzer and Schroeder, "The Protection of Information in Computer Systems" (1975) **[read]**; the Zanzibar paper at usenix.org (ATC 2019) **[re-read]**.
- Minecraft Wiki, "Permission level" and "Command Block" **[read]**; Space Engineers' published source (Keen Software House, last commit 2022-01-06) **[re-read, single pass]**.

Not read, so not relied on: Mojang's own changelogs and notes on the new permission objects; Discord's and Second Life's support pages; the Factorio forum threads on lockout; OpenSim's configuration; SAM for Garry's Mod; ARK; the rest of "How Towny Works" (mayor departure and inactivity); the exact attack in Luanti's 2026 advisory.
