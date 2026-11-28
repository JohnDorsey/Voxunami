# Card 2: Minecraft land, trust, team and automation permissions

> Raw notes of research stream 2 of the permissions study: Minecraft land, trust, teams and command blocks (GriefPrevention, WorldGuard, Towny, command blocks, LWC, FTB Teams and Chunks, CC:Tweaked). Written by a research agent on 2026-11-23 and kept as it wrote it, except that local file paths were removed and two file names lost their code quotes. The survey that draws on these notes is `permissions-survey.md`; it says which claims were checked again. The tags are the agent's own: [re-read] and [raw] mean the source's own text was read, [read] a summary by the fetch tool, [secondary] a second-hand source, [recalled] memory, [inference] reasoning from cited facts.


Notes for the owner's permissions study. All pages and files below were read on 2026-11-23.
Desk research only: nothing was installed or run, no repository was changed.

## How to read this card

- Evidence tags are the brief's tags, plus one I add:
  - [raw] = the file's or page's own text, read once by me with no summarising model in between (a GitHub file read with `gh api`,
    or an HTML page turned into plain text by a small script). I use [re-read] only where two independent places agree
    (for example a doc page and the code) or I looked twice.
  - [read] = a fetch tool that summarises pages, so numbers and wording may be the summary's.
  - [secondary], [recalled], [inference] as in the brief.
- Nothing was run. "Verified" below means "read in the source or the docs". What current Paper really does when a command block
  runs `/execute as @p run ...` is NOT tested here.
- Budget: I went over. About 90 tool calls (limit about 70) and 11 web searches (limit 20). I stopped when the card was usable.
- Scores are my judgement from the rubric in the brief; the tag says how well the facts under the score are supported.
- Plain-words glossary: a "claim" is a piece of land a player or the admin marks as protected; a "region" is WorldGuard's word for
  the same; a "flag" is a named on/off or allow/deny switch on a region; a "node" is one permission name such as
  `griefprevention.claims`; "op" is the all-powerful operator status of vanilla Minecraft.

## The five findings that matter most for R1 to R9

1. Nobody attributes automation to a person. A vanilla command block has no owner, runs at a fixed operator level (2), and whoever
   may edit the block gets everything the block may do. On Paper all command blocks share ONE operator permission object, and the
   only fixes are global dials: one level for all blocks, one switch for all blocks, and (Paper, in a later patch) "both
   layers must say yes".
   The closest precedents for "the block runs as its owner" are FTB Chunks (a machine acts with its placer's id, an allow-list
   by name, and a default of "no" for unknown machines) and GriefPrevention/WorldGuard (a machine acts as the owner of, or a
   member of, the area it sits in, and crossing into another owner's area is refused). R2 as written is a real improvement
   over vanilla; its weak spot is the ownerless block, which is exactly vanilla's dangerous case.
2. For land, a grants-only chain is easy to predict. GriefPrevention: four levels in one chain (access, containers, build,
   manage), the owner's right cannot be granted, one entry per player per claim, one switch for inheritance into sub-areas. The
   refusal text names the owner and the level you lack. An admin's bypass is an explicit mode, not an implicit side effect of
   being op, but GriefPrevention does not log what is done in that mode (its own docs say so). Copy the mode, add the log.
3. When you need both allow and deny, WorldGuard shows the minimum complete rule set (priority number, deny beats allow at equal
   priority, child inherits what it does not set, group scopes relative to where the flag sits) and its price: at least seven
   interacting rules and documented traps (a parent with higher priority silently overrides its children; denying
   `block-break` also stops pistons). Its "region with no shape that only carries flags" is a neat policy object for per-pack or
   per-tag defaults.
4. Delegation: the best anti-escalation pattern found is Towny's. The right to hand out a rank is itself a node per rank, so an
   assistant may give "helper" but not "assistant". WorldGuard splits `addmember` from `addowner` and even lets you grant "may set
   flag X to value Y on regions I own". GriefPrevention's manager can make more managers and open the claim to everybody
   (inside that one claim). Paper's rule "vanilla level OR plugin node" means a deny can never take power away from an op; avoid
   two systems joined by OR.
5. Owner gone or banned: none of these handles it by itself, and GriefPrevention does so on purpose (bans get appealed). Admins
   need explicit verbs: delete all of a player's claims, hand a claim to a player (admin-only there), turn it into an
   admin-owned claim that never expires, remove one protected block (LWC's `lwc.admin.remove`), set a new mayor (Towny).
   For R1 (a new key costs nothing) the lesson is that strangers must hold zero rights by default and a ban must suspend the
   owner's authority through the live lookup of R3.

---

## 1. GriefPrevention (Paper/Spigot plugin; master branch on GitHub and its admin docs, read 2026-11-23)

Depth: deep

Version checked: `GriefPrevention/GriefPrevention` master; its `plugin.yml` says api-version 26.2 (the new year-based Minecraft
numbering). The admin guide on dev.bukkit.org is undated, so some default numbers below may be older than the code.

### Model in five lines
1. Protects land that players claim from other players. A claim is a rectangle of columns that always runs the whole height (a
   code comment says the upper limit is ignored). Claims can have sub-areas ("subdivisions"); from memory the nesting is one
   level deep [recalled], and the code says sub-areas cannot be transferred, only top-level claims.
2. Subjects: the owner (stored by account id), other players (by id), the special word `public` (everybody), and admins through
   ordinary Bukkit permission nodes. There are no teams or groups of players.
3. Grants only, no denies. Four levels in one chain: access (buttons, levers, beds) < containers (chests, furnaces, crafting
   blocks, crops, animals) < build < manage (may change the trust list). A fifth level, "edit", belongs to the owner alone and
   can never be granted (the code throws if you try). Each player has ONE level per claim; a higher level includes all lower
   ones (the order of the enum is the comparison, with a code comment warning that reordering it breaks the check).
4. Inheritance: a sub-area asks its parent when it has no answer, unless it was made "restricted" with `/restrictsubclaim`.
   Granting inside a sub-area changes only that sub-area; revoking cascades down into children.
5. Defaults: on unclaimed land in a normal "Survival" world anyone may build (except a few abuse limits: lava and water near
   the surface, sky trees, creeper craters). In "Creative" rules worlds and in the mode `SurvivalRequiringClaims` nobody may
   build outside a claim (the refusal says to claim some land first). Inside a claim strangers have almost nothing; wooden
   doors stay usable unless `LockAllDoors` is on (only iron doors are lockable by default). [raw] (code, plugin.yml, admin docs)

### Granularity
- Per claim (and per sub-area) times four levels. Not per command: about 38 commands. The player commands sit behind a few
  nodes that default to true (`griefprevention.claims` covers most; also `createclaims`, `abandonallclaims`, `trapped`,
  `unlockdrops`, `ignore`); the admin ones sit behind nodes that default to op, 23 of which are children of one parent,
  `griefprevention.admin.*`. No per-tag. Dropped items are also protected for the dying player (`/unlockdrops`). [raw]
- Claim size is limited by "claim blocks" that accrue with play time (docs: 100 at start, 100 per hour, cap 80,000, minimum side
  10; automatic first claim of radius 4 when a newcomer places a chest). [raw] (docs; defaults may be older than the code)

### Groups, roles, teams
- None for players. `/trust public` gives everybody a level on that claim. I looked for "trust a permission group" in the
  current source and did not find it (the old docs and the Sponge port mention permission-node trust); `resolvePlayerByName`
  accepts only a player name or a raw account id string. [raw]
- Several actors cannot be combined: one entry per player per claim, so no stacking.

### When rules conflict
No conflicts are possible among grants because there are no denies. The decision in `Claim.getDefaultDenial` runs in this order
(read in the code, I paraphrase the order): admin claim and you hold `adminclaims` -> allowed; owner-only "edit" and you hold
`deleteclaims` -> allowed; you are the owner, or you are in ignore-claims mode and hold the bypass node -> allowed; an explicit
entry for you at or above the needed level -> allowed; a `public` entry at or above the level -> allowed; (build only) crops may
be placed with container level; then ask the parent claim unless restricted; otherwise refuse with a message. A plugin event
(`ClaimPermissionCheckEvent`) can change the verdict and the reason afterwards. [raw] (cross-checked with the enum comments and
plugin.yml descriptions, so [re-read])

### Delegation and escalation
- Owner may grant any of the four levels. A manager may do everything the owner may do about the trust list: the command checks
  only "manage", so a manager can give manager level to others, give `public` build rights, and `/untrust` other managers.
  A manager cannot resize, abandon or delete the claim, and `/untrust all` (clear everything) is owner-only. [raw]
- `/trust Name` typed outside any claim applies to ALL of your own claims (a bulk form). [raw]
- Admins: `deleteclaims` lets you edit or delete any non-admin claim; `ignoreclaims` lets you build and change trust in any
  claim; `adminclaims` lets you edit every admin claim; `transferclaim` (default op) is the only way to give a claim to
  another player. A player cannot give his own claim away. [raw]

### Automation
- GriefPrevention judges machines by position. A piston is allowed to move blocks only inside one claim, or between claims
  with the same owner (the check compares owner ids). Four modes exist for pistons (ignored, claims-only, everywhere in a
  fast box check, everywhere precise). A code comment from the author explains why claims-only exists: ice pushed out of a
  claim plus a torch gives water outside it. Fluids and TNT/sand cannons are also checked by owner (docs/search snippet,
  [secondary]). `/claimexplosions` switches explosions on or off for one claim. [raw]
- Command blocks are not part of its model: they are operators and act outside it. There is no per-machine identity.

### Admin interface
- Typed commands (about 38), a golden shovel for claims, a stick as "investigation tool", config file and a messages file,
  `/gpreload` ("does NOT totally reload", says its own description). Data in flat files or a database.
- Typed-only claiming works: `/claim [radius]` makes a claim centred on you; `/extendclaim N` pushes a border in the direction
  you face. This matters for R7. [raw]
- "Why refused": the refusal line (below). Audit: deletions and resizes by anyone but the owner are logged; actions done in
  ignore-claims mode are NOT logged (docs warn: do not give it to anyone you do not trust fully). Transfers and expiries are
  written to the admin-activity log. Undo: none for trust; a rollback tool is not part of it (the docs say it is not rollback).
  [raw] (admin guide text, `GriefPrevention.java`, `DataStore.java`)
- Extension points: events for trust changes, transfers and expiry (expiry can be cancelled by another plugin). [raw]

### Safety
- Safe defaults: admin nodes need op; ordinary players get only claim commands; strangers get nothing in claims.
- Hazards: `ignoreclaims` is unlogged; a manager can open a claim to the public; `deleteclaims` doubles as "may edit any claim";
  `/transferclaim` with no name turns a claim into an admin claim, which never expires and is editable by every holder of the
  admin-claims node; claims are not removed when a player is banned (the guide says appeals are the reason and suggests aliasing
  `/ban` so it also deletes the player's claims). New accounts can be made by griefers; "SmartBan" bans the IP of a banned player
  for 24 hours against never-seen accounts. [raw]
- Lockout: `/trapped` frees a player stuck in someone's claim (long cooldown). [raw]

### Task table T1 to T13

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 friend builds in one area | Stand in the claim, `/trust Friend` (1 command; the claim itself is 2 shovel clicks or `/claim`). Reply names player, level and scope. "Only their own things" is not expressible (no per-block owner); nearest is a sub-area (`/subdivideclaims`, 2 clicks, then `/trust` inside it). | 5 for an area; 2 for "own things" | [raw] |
| T2 moderator role | n/a: no roles. Its admin verbs are separate nodes (`softmute`, `separate`, `deleteclaims`, `ignoreclaims`...). Kick, ban, gamemode are not its business. | n/a | [raw] |
| T3 one command for an hour | Not possible: a trust entry holds a level and nothing else (no expiry). Time-boxing needs another plugin and only works for permission nodes, not for claim trust. | 1 | [raw] |
| T4 why is X refused | The refused player sees a line naming the claim owner and the missing level (build, containers, access or manage). Owner or manager types `/trustlist` standing in the claim (1 command) to see explicit entries by level. Admins in range of the bypass node also get "To override, use /ignoreclaims". No per-player effective-rights report, and a plugin event can change the verdict without changing the story. | 4 | [raw] |
| T5 strip a griefer, keep out, survive rename | `/deleteallclaims Griefer` (1) plus a ban (not its job). Trust is stored by account id, so a rename changes nothing. Trust the griefer holds in OTHER people's claims stays until each owner runs `/untrust Griefer` (no admin sweep found by me). | 3 | [raw] + [inference] |
| T6 install a pack of 15 commands | As an example of shipping: user nodes default true, admin nodes default op, one parent node grants all 23 admin nodes at once. Denying one child needs the permission plugin. | 4 | [raw] |
| T7 audit | `/claimslist Player` (node `claimslistother`) lists a player's claims; `/trustlist` lists one claim. No "where is X trusted" query; "who may run Y" is the permission plugin's job. | 2 | [raw] + [inference] |
| T8 delegate | `/managetrust Helper` (1 command). Helper manages trust in that claim only, but can make more managers and open it to `public`. Cannot resize, abandon, clear all. | 4 | [raw] |
| T9 bootstrap, recovery | n/a: relies on the server's op list and permission plugin. | n/a | [raw] |
| T10 team of three shares one thing | Land only: `/trust B`, `/trust C` (2 commands), or `/containertrust` for chests only; change with `/untrust`; instant. Vehicle or script data: n/a. If the owner leaves, only an admin can hand the claim over. | 5 for land | [raw] |
| T11 automation with limited power | Machines act as "whoever owns the claim they sit in"; moves between different owners are refused; explosions per claim. Nothing to list or revoke per machine. Command blocks: outside it. | 3 | [raw] + [inference] |
| T12 change now, undo, audit | Trust edits are effective and saved at once, no restart; `/gpreload` for config; no undo; third-party deletions and resizes logged, bypass-mode actions not. | 3 | [raw] |
| T13 all except one | No deny entries. Nearest: carve a room out with `/restrictsubclaim` (it inherits nothing from the parent). Area only. | 3 | [raw] |

### Scores
- S1 predictability: 5. One chain, one entry per player, one inheritance switch; about three precedence ideas in total. [raw]
- S2 explainability: 4. Good refusal text, `/trustlist`, `/claimslist`; no effective-rights query. [raw]
- S3 safety: 4. Good defaults; unlogged bypass and the "manager makes managers" step are the holes. [raw]
- S4 scale: n/a for 100 commands by 20 roles (it has no command roles). For land it handled 1,400 claims and 5,000 players in a
  7-minute file-to-database move on the author's test server (docs, [raw]), and `/trust` outside a claim is a bulk form.
- S5 learning curve: 5. A shovel, four verbs, and a manual book (`/claimbook`). [raw]

### Pain points and incidents
- Piston/fluid/TNT-cannon crossings: a changelog line says pistons stopped moving blocks outside claims from version 8.1.7
  ([secondary], search snippet of the plugin page; I did not open the changelog). The code carries the author's own reasons.
- Creative worlds: griefers could claim land, build something awful and abandon it. Fix: in creative-rules worlds unclaimed
  blocks revert to nature, with a warning and a couple of minutes to undo. [raw] (admin docs)
- Banned owners keep their claims by design (appeals); admins alias `/ban`. [raw]
- `ignoreclaims` is unlogged. [raw]
- Door lock default stays off because "near-strangers" tend to receive build trust when too many things need it. [raw] (docs)
- A forum post says per-world WorldGuard TNT settings overrode GriefPrevention's (two plugins on one region); same snippet.
  [secondary]

### Lessons for R1 to R9
- R1: copy storing grants by stable id and resolving a nickname only at the moment of the command (and accepting the raw id as
  the exact form). Avoid its special words (`public`, `all`) sharing the same namespace as player names; use a marker.
- R2: the "machine acts as owner of the area it sits in, cross-owner moves refused" rule is cheap and effective; our owned
  blocks can do better: run as the owner and check every effect at its target.
- R3/R4: one decision function that returns a reason (or nothing) is the right shape, with an event hook to let packs veto. Make
  the hook return a reason too, or explanations rot.
- R5: copy the non-grantable owner level, the chain of four, and "manage" as its own level. Fix its gap: make "may grant level L"
  require holding L plus an explicit grant right (Towny's per-rank rule), so a manager cannot make managers.
- R7: `/claim`, `/extendclaim N` and `/trust Name` work with a one-line command box.
- R8: bypass is an explicit mode ("respecting claims" by default, one command to switch). Log it.
- R9: the refusal line is the model: who owns it, which level you lack, and (for admins only) how to override.

### Sources
- https://github.com/GriefPrevention/GriefPrevention : `src/main/resources/plugin.yml`, `ClaimPermission.java`, `Claim.java`,
  `GriefPrevention.java`, `DataStore.java`, `BlockEventHandler.java`, `CleanupUnusedClaimTask.java`, `Messages.java` [raw]
- https://dev.bukkit.org/projects/grief-prevention/pages/how-to-guide-for-administrators and `.../setup-and-configuration` [raw]
  (the project's own docs; an external wiki is linked from the project page, not read)
- Search results naming the Sponge port's trust pages and a GP3D fork [secondary]; not relied on.

---

## 2. WorldGuard (Paper/Spigot plugin; its docs repo `EngineHub/WorldGuardDocs` master and the plugin source, read 2026-11-23)

Depth: deep

The page from my starting list, `/regions/priority/`, does not exist. The right page is `/regions/priorities/` (source file
`priorities.rst`). The docs are in a separate repo, `EngineHub/WorldGuardDocs`, and rendered at worldguard.enginehub.org.

### Model in five lines
1. Protects "regions" (boxes, polygons, or the whole world) from players, mobs and machines. Outside every region nothing is
   protected unless you set up the world-wide `__global__` region.
2. Subjects: players and permission-plugin groups (written `g:name`) listed as region owners or members; everyone else is a
   non-member. "Region groups" (all, members, owners, nonmembers, nonowners) are NOT permission groups; they only say whom a
   flag applies to.
3. Flags with allow and deny: `build`, `block-break`, `interact`, `use`, `chest-access`, `pvp`, `entry`, `blocked-cmds` and
   many more. A flag may be set for one region group at a time per region.
4. Inheritance: one parent per region; a child inherits owners, members, and flags it does not set. A priority integer (default
   0, any 32-bit value) decides between overlapping regions.
5. Defaults: inside a region members and owners may build and non-members may not (the `build` result is implied by
   membership); every other protection flag follows that result unless set. Players need permission nodes to use any
   WorldGuard command ("By default, no one can use WorldGuard", unless op rules are on). [re-read] (docs pages agree)

### Granularity
- Per region times flag times group. Per command for WorldGuard's own commands (`worldguard.heal`, ...). Region commands per
  region name (`worldguard.region.addmember.own.<region>`). Per material, entity type and item for an optional build-by-node
  mode (`worldguard.build.block.place.<material>`, off by default). Commands can be blocked or whitelisted per region and
  per group with the flags `blocked-cmds` and `allowed-cmds`. [re-read]

### Groups, roles, teams
- Members and owners are players or `g:<group>` entries from the permission plugin. A region can inherit its member list from
  a parent; a player may be in many regions. There is no priority between groups; priority is on regions.
- Owner vs member: owners also count as members; the difference matters only for flags aimed at owners and for the per-region
  command nodes (`own` = owner, `member` = member or owner).

### When rules conflict (the documented algorithm, paraphrased from `priorities.rst`, `flags.rst`, `global-region.rst`)
1. Overlapping regions: for "may this member build", only the regions of the highest priority at that spot are considered.
   (Docs also say: with no priorities, a player must be a member of ALL overlapping regions.)
2. For a flag value, the highest-priority region that defines the flag wins.
3. A region that does not set a flag takes the parent's value.
4. If values still clash at the same priority: for allow/deny flags, deny wins; for other kinds the result is not defined.
5. The global region counts as an ordinary region at the lowest possible priority; `passthrough` is implicitly "allow" there,
   which makes it a non-protecting region; adding a member or owner to it flips passthrough to deny.
6. A flag aimed at a region group (say `-g nonmembers`) tests membership of the region ON WHICH the flag is set, or of one of
   its children; being a member of a different overlapping region does not count.
7. The `build` flag is set implicitly from membership if no highest-priority region sets it; all other protection flags
   default to the result of `build`.
8. Machines and other non-players: treated as members of the regions they are inside (all, or only the highest priority ones,
   by a config option); an unknown cause is treated as a non-member. A parent with higher priority than its child overrides
   the child (inheritance only works when children have the same or higher priority). [re-read] (3 docs pages plus a code sample)

### Delegation and escalation
- Two separate axes: membership says who may BUILD; node patterns say who may ADMINISTER. Node patterns have three forms:
  `worldguard.region.<verb>.own.<region>`, `.member.<region>`, `.<region>`. Verbs include `addmember`, `addowner`,
  `removemember`, `removeowner`, `flag`, `setpriority`, `setparent`, `redefine`, `remove`, `info`, `teleport`.
- You can give someone `addmember.own.*` without `addowner.own.*`, so they can add helpers but cannot make owners. For flags:
  `worldguard.region.flag.regions.own.*` says on which regions and `worldguard.region.flag.flags.<flag>.<value>.own.*` says
  which flag and value (for example only `chest-access`). So "may set flag X to Y on regions I own" is expressible. [re-read]
- Players can claim land themselves with `/rg claim <name>` (rudimentary, with count and volume limits and overlap rules); the
  caller becomes owner. [re-read]
- Bypass: `worldguard.region.bypass.<world>` (and a toggle command); ops and holders of all nodes bypass automatically and
  "it will appear that protection does not work" (the docs' own warning). [re-read]

### Automation
- Machines (pistons, dispensers, TNT, fire) count as members of the regions they sit in, so a piston inside a plot may work
  inside it but not push across a border. Setting `block-break` to deny on a region stops pistons there too, because pistons are
  "members" too; the docs say to use membership or `passthrough` instead. The flag `nonplayer-protection-domains` merges
  regions into one domain for machines so pistons may cross. [re-read]
- Command blocks: not covered anywhere in the pages I read. Vanilla commands like `/fill` do not raise the block events that
  regions listen to, so I expect a command block to ignore regions. [inference, NOT verified]

### Admin interface
- `/rg define`, `addmember`, `addowner`, `flag`, `flags` (a paginated list in which you click values to change them), `info`,
  `list -p <player>`, `setpriority`, `setparent`, `claim`, `teleport`, `save`/`load`; a wand (WorldEdit selection) for the
  shape; a config file; `/wg report`; `/wg debug` commands that simulate an action to find which plugin blocks it (node
  `worldguard.debug.event`; the docs warn it lets a moderator impersonate players). [re-read]
- "Why refused": a message from the `deny-message` flag (set per region), with `%what%` replaced by the action ("place that
  block", "break that block", "open that", "take that", "use that"); the code remembers when it last told the player (metadata
  key `lastMessage`), so it does not spam. I take that as a rate limit. [raw] + [inference]
- Audit and undo: I found none for region edits in the pages read. [not found, limited search]

### Safety
- Safe default (members only). Footguns that the docs themselves list: ops bypass everything; `build allow` on the global
  region does nothing; setting `build` or any protection flag to protect or unprotect a whole region is the wrong tool (use
  membership or `passthrough`); `block-break deny` breaks pistons; parent-priority trap; a flag on the global region aimed
  at "nonmembers" means non-members of the global region, which includes the members of every plot because nobody is a
  member of the global region. The warnings are a good sign for honesty and a bad sign for learnability. [re-read]
- Lockout/bootstrap: console or op grants `worldguard.*` first. [re-read]

### Task table T1 to T13

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 friend builds in one area | Select with the wand (2 clicks), `/rg define house Friend`: the friend is an owner, nobody else may build. 3 steps. Must not be op or hold the bypass node. Players can do it themselves with `/rg claim` if given the node. "Only their own things": n/a (areas only). | 5 | [re-read] |
| T2 moderator role | n/a for kick/mute/ban (not WorldGuard). Its own moderator verbs are nodes (`worldguard.notify`, `.region.bypass.<world>`, `worldguard.god`...): a node bundle in the permission plugin. | n/a | [re-read] |
| T3 one command for an hour | Membership has no expiry in the docs read. | 1 | [re-read] |
| T4 why is X refused | `/rg info` at the spot (owners, members, flags, priority, parent; a list if in several regions), `/rg flags` (clickable), `/wg debug` to simulate the action. About 3 commands. No automatic "player X, action Y, rule Z" answer. | 4 | [re-read] |
| T5 strip a griefer | `/rg list -p Griefer` to find his regions, then `/rg removemember` or `removeowner` per region (no bulk form by player; `-a` clears a region's list). Keep out of an area: flag `entry deny` (non-members by default). Ban is separate. Names were migrated to account ids (`/rg migrateuuid`). | 3 | [re-read] |
| T6 pack of 15 commands | n/a: it does not govern other plugins' commands. Related: `blocked-cmds`/`allowed-cmds` deny or allow named commands per region and per group. | n/a | [re-read] |
| T7 audit | `/rg list -p X` (regions where X is owner or member), `/rg info R`; node questions go to the permission plugin. | 4 | [re-read] |
| T8 delegate | 1 to 3 node lines in the permission plugin (patterns above). Escalation is limited at node level: `addmember` without `addowner`, flag-and-value nodes. You must know the pattern table. | 4 | [re-read] |
| T9 bootstrap | Docs state no one has rights until nodes or op are given; recovery is the console. | 4 | [re-read] |
| T10 team of three shares one thing | Land: `/rg define team A B C`, or `/rg addmember team B C`; or add a permission group `g:red`; change with `addmember`/`removemember` (1 command). A vehicle or script: n/a. | 5 | [re-read] |
| T11 automation | By position: a machine is a member of the regions it is in; borders block pushes unless domains are merged. Nothing to list or revoke per machine. Command blocks: unverified. | 3 | [re-read] + [inference] |
| T12 change now, undo, audit | Flags and members take effect at once. Undo and "who changed what": none found. | 3 | [re-read] + [not found] |
| T13 all except one | Documented: deny beats allow at equal priority, higher priority overrides, a small child region can override its parent's flag (the heal-room-in-the-arena example). Predictable but you must know the parent-priority trap. | 4 | [re-read] |

### Scores
- S1 predictability: 3. At least seven interacting rules; the three docs pages are precise, which helps. [re-read]
- S2 explainability: 3. `/rg info` and debug help; nothing per player. [re-read]
- S3 safety: 3. Safe default but many documented footguns, and ops bypass silently. [re-read]
- S4 scale: 4. Templates (a shapeless parent region carrying flags), inheritance, `g:` groups keep rule sets short. [re-read]
- S5 learning curve: 3. Excellent docs, but wand + regions + flags + groups + priority + parent is a lot to learn.

### Pain points and incidents
- The docs' own warning list above (ops bypass, pistons, TNT cannons, global region) is the record of past confusion. [re-read]
- A `build` flag on the global region is "strongly discouraged": it breaks all pistons in the world. [re-read]
- Not searched further for dated incidents (budget).

### Lessons for R1 to R9
- R4: copy "deny beats allow at equal priority" plus a priority number; copy the shapeless template region as a policy object
  (for us: a named policy that a pack, a tag or a team can point at).
- R4/R6: per-command-per-place-per-group denial (`blocked-cmds` with a group) is a good idea that you may not need yet.
- R5/T8: copy "may add members" and "may add owners" as separate rights, and "may set flag X to value Y".
- R7: a clickable flag list in a command-only interface is a cheap GUI substitute.
- R8: avoid "op bypasses silently"; show bypass state and log it.
- R9: a `%what%` template in the refusal line is enough to make refusals specific; add the rule's id.

### Sources
- https://github.com/EngineHub/WorldGuardDocs : `source/regions/priorities.rst`, `flags.rst`, `global-region.rst`,
  `claiming.rst`, `commands.rst`, `source/permissions.rst`, `source/build-perms.rst`, `source/developer/regions/flag-calculation.rst` [raw]
  (rendered: https://worldguard.enginehub.org/en/latest/regions/flags/ [raw HTML text])
- https://github.com/EngineHub/WorldGuard : `RegionProtectionListener.java` (deny message code) [raw]

---

## 3. Towny (Paper plugin; GitHub wiki of TownyAdvanced/Towny, read 2026-11-23)

Depth: deep (but see the gaps: the wiki page "How Towny Works" was cut at 100,000 of 131,000 characters by the fetch tool, and all
Towny facts come through the summarising fetch tool, so they are [read], not [raw])

I chose Towny as the representative of rank matrices because its permission matrix, rank file and command list are documented in
one wiki with exact command strings. Factions and Lands were not read.

### Model in five lines
1. A town is the team: residents, a mayor, plots ("town blocks", one chunk each). Nations group towns. Land is claimed by the
   town; a plot may be owned by the town or personally by a resident.
2. Each plot has a "perm line": four actions (build, destroy, switch = use doors, containers, levers, itemuse = use items such
   as ender pearls, boats, leads) times groups on or off. Town-owned plots use four groups R, N, A, O (resident, nation,
   allies, outsiders); personally owned plots use F, T, A, O (friend, town, allies, outsiders).
3. Extra layers: per-plot per-player overrides (`/plot perm add Name`), "trust" (full four rights on a plot, or the whole town,
   or a whole other town), toggles for pvp, explosion, fire and mobs, and outlaws (per town ban list).
4. Roles: ranks (assistant, helper, sheriff, ...) are bundles of permission nodes defined in `townyperms.yml`, for towns and
   for nations. A player may hold several ranks. The file's comments say Towny pushes these sets to the server's permission
   system itself, so an admin does not have to enter them in a permission plugin.
5. Defaults: a town plot takes the town's perm line and a personal plot takes the resident's line (the wiki's wording suggests
   the line is copied at claim time; see the conflict list). Mayors have `towny.command.town.*`; ordinary residents get
   claim-a-plot-for-sale, plot toggles and `plot set perm`; claiming town land is for mayors and assistants. [read]

### Granularity
- Per plot, per town, per nation; per action (4); per group (4 on town lines, 4 on personal lines); per command node
  (`towny.command.town.rank.helper`); per rank bundle; "item groups" for switch and itemuse (BOATS, MINECARTS, WOOD_DOORS,
  BUTTONS, SHULKER_BOXES). [read]

### Groups, roles, teams
- Rank assignment: `/town rank add Name rankname` (mayor, or an assistant for the ranks they may give), `/town ranklist`.
  Nation ranks with `/nation rank add ...`. Admin: `/townyadmin town <town> rank add Name rank`.
- Rank files accept: a node with a leading minus removes a power granted by a wildcard (for example `-towny.command.plot.set.jail`);
  `towny.rankpriority.N` (display priority) and `towny.rankprefix.<text>`; `towny.ranklimit.N` (how many people may hold the
  rank); `towny.town_level_requirement.N` (rank only available when the town has grown to level N; ranks above the current level
  are removed when the population drops and come back when it recovers). Names `nomad`, `default`, `mayor`, `king`, `ranks`
  cannot be renamed. [read]

### When rules conflict (no single algorithm text found; these overrides are documented one by one)
1. Which line applies: town-owned plot uses the town's line, personally owned plot uses the resident's line.
2. Changing the default line does NOT reach plots that already exist; you must run `/town set perm reset` (or `/res set perm reset`)
   to apply it to all plots. That reads like a copy rather than a reference. [inference from the wiki's wording]
3. A per-plot override (`/plot perm add Name`) gives that named player their own build/destroy/switch/itemuse regardless of the line.
4. Trust gives full rights regardless of lines (a config switch decides whether town trust also covers personal plots).
5. Town toggles for explosion and fire beat plot toggles; a world-wide `forcepvp` beats everything about pvp; arenas always allow pvp.
6. The word "ally" means different things on a personal plot (your nation and its allies) and on a town plot (the nation's
   allies only), and several server wikis I saw disagree about it. [read] [secondary]

### Delegation and escalation
- Per-rank grant nodes: the default assistant rank holds `towny.command.town.rank.vip` and `towny.command.town.rank.helper`, so
  an assistant may hand out vip and helper but not assistant or sheriff. The mayor's `towny.command.town.*` covers every rank.
  (The file does not spell the pattern out; I infer it from those nodes.) With `towny.ranklimit.N` a rank can be capped. [read]
  + [inference]
- Admin recovery: `/townyadmin set mayor <town> <resident>` (also `npc` for a town run by the server) and
  `/townyadmin town <town> rank|trust|outlaw|toggle|set ...` (act for a town). [read]

### Automation
- Pistons work only between blocks of the same owner type or in the wilderness ("piston protection"); explosions and fire have
  toggles; hostile mob spawning has a toggle; jailer ranks use a node. No command-block model. [read]

### Admin interface
- Commands: `/town set perm ...`, `/plot set perm ...`, `/plot perm` (show the line of the plot you stand in), `/plot perm gui`,
  `/town trust add Name`, `/plot trust add Name`, `/town outlaw add Name`, `/town toggle ...`; admin `/townyadmin ...` (short
  `/ta`), live editing of ranks (`/ta townyperms townrank addrank <name>`, `/ta townyperms group <name> addperm <node>`),
  config files. A "perm HUD" node exists for everybody (`towny.command.plot.perm.hud`); I did not see what it displays. [read]
- Audit/undo: none seen. `townyperms.yml` needs a reload command after editing (per a guide, [secondary]).

### Safety
- Several server wikis advise against giving outsiders any permission. The perm line can be changed by anyone who holds the
  plot, so defaults matter. Towns that cannot pay upkeep are deleted and residents who cannot pay plot tax lose the plot: land
  ownership is tied to the in-game economy. [read] [secondary]

### Task table T1 to T13

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1 friend builds in one area | Standing in your plot: `/plot trust add Friend` (1). Town-wide: `/town trust add Friend`. Or friend list plus line: `/res friend add Friend`, `/plot set perm friend build on`. Three overlapping ways to say the same thing. | 4 | [read] |
| T2 moderator role | Town level: `/ta townyperms townrank addrank moderator`, `/ta townyperms group moderator addperm <node>`, `/town rank add X moderator` (3 commands). Server-level kick/mute/ban: n/a. | 4 | [read] |
| T3 one command for an hour | Nothing found. | n/a | [not found] |
| T4 why is X refused | `/plot perm` shows the plot's line, `/town` shows the town's line. Refusal text and a per-player explainer: not read. | 3 | [read] |
| T5 strip a griefer | `/town outlaw add X` per town; ban separate; server-wide stripping: not verified. | 3 | [read] |
| T6 pack of 15 commands | The plugin ships its own role file (`townyperms.yml`), so no manual node entry; wildcard `towny.command.town.*` for the mayor. | 4 | [read] |
| T7 audit | `/town ranklist [town]` lists residents and ranks (1). Nodes by reading the rank file. No "everything X can do". | 3 | [read] |
| T8 delegate | Give the assistant rank (1 command); it may hand out only the ranks it holds a node for; `towny.ranklimit.N` caps count. | 4 | [read] + [inference] |
| T9 bootstrap, recovery | Founder is mayor; `/townyadmin set mayor <town> <resident>` (1) recovers a town. | 5 | [read] |
| T10 team of three | `/plot trust add B`, `/plot trust add C` (2); `/plot trust remove` later; group plots with `/plot group`. Non-land things: n/a. | 5 | [read] |
| T11 automation | Toggles per town/plot (explosion, fire, mobs, pvp), piston protection; no per-machine identity. | 3 | [read] |
| T12 change now, undo, audit | Perm changes immediate; `/town set perm reset` re-applies the default line to all plots (a bulk "back to default"); no undo or audit seen. | 3 | [read] |
| T13 all except one | Rank bundles accept `-node` inside a wildcard; per-plot overrides for one person. | 4 | [read] |

### Scores
- S1 predictability: 3. Two different group sets (RNAO on town plots, FTAO on personal plots), copy-on-claim defaults, and at
  least six override layers. [read]
- S2 explainability: 3. The line is displayed; no reason trace. [read]
- S3 safety: 4. Per-rank grant nodes and caps; but outsider switches and trust switches are easy to open. [read]
- S4 scale: 4. Rank bundles per town and nation; bulk reset command. [read]
- S5 learning curve: 3. Many words (town, nation, plot, resident, friend, trust, rank, toggle, perm line), but the wiki lists every
  command.

### Pain points and incidents
- Google Groups threads: a user set outsider switch/itemuse off on a plot and still saw a non-resident use switch blocks (single
  report; cause not shown). Another asks how to let strangers use only some blocks. [secondary]
- Defaults not reaching existing plots is documented behaviour that surprises admins. [read]
- Searching for dated exploit reports found none in the budget. [not found, limited search]

### Lessons for R1 to R9
- R3/R5: copy-on-creation of defaults (Towny) is a frozen copy; reference the default (WorldGuard's inheritance) so later
  changes apply, or provide a bulk "re-apply" like `/town set perm reset` and say so in the output.
- R4/T8: copy the per-rank grant node and the rank cap; copy "negate inside wildcard" for T13.
- R2: Towny's "NPC mayor" is an explicit stand-in identity for a server-run town. A named stand-in is a better ownerless
  representation than an empty owner (GriefPrevention's null owner shows as "an administrator").
- R6: shipping the pack's role bundle with the pack is what lets an admin decide without a manual.
- R9: show the perm line on demand (`/plot perm`); a HUD node exists but I did not verify it.

### Sources
- https://github.com/TownyAdvanced/Towny/wiki : `How-Towny-Works`, `Towny-Commands`, `Default-Townyperms.yml` fetched as raw
  wiki markdown through the summarising tool [read]; the sidebar listing [read].
- Google Groups threads and server wikis from a search [secondary].

---

## 4. Command blocks and automation in vanilla Minecraft and Paper (with datapack functions, signs, and plugins that restrict them)

Depth: deep

Checked: minecraft.wiki pages (read through the summarising tool, so [read]); Paper's source repository master (patch files and
Java files, [raw]); Paper docs `docs.papermc.io/paper/reference/permissions` and `.../world-configuration` ([raw] HTML text);
three Spigot Jira issues ([raw]). The Minecraft version of these sources is the 26.x line (Paper's source already uses new
permission classes).

### Model in five lines
1. A command block stores one command and runs it when powered, with its OWN rights, never those of the person who placed or
   last edited it. The block records no owner. Vanilla Java: the block runs at operator level 2 ("level 2 in Java, level 1 in
   Bedrock"), which is why it cannot run `/op`, `/ban`, `/kick`, `/stop`, `/whitelist` and similar level 3 and 4 commands. [read]
2. Who may place or edit: only an operator in creative mode, with `op-permission-level` at least 2, and only when the server
   allows command blocks: the property `enable-command-block` (default false on a dedicated server). In the 1.21.9 to 1.21.11
   snapshots this property was replaced by a game rule (first `commandBlocksEnabled`, later `command_blocks_work`); the wiki
   page does not state the game rule's default. In singleplayer, "Allow Cheats" must be on. [read]
3. Why people use them: the wiki itself names the purpose: a block can let a player use a command he could not otherwise use,
   for example give one item without general `/give`. That is delegation of power by building a machine. [read]
4. On Paper, a command block's sender is a single shared permission object with operator status on, shared by ALL command
   blocks; its op flag cannot be changed ("Cannot change operator status of a block"). [raw] (`CraftBlockCommandSender.java`)
5. Paper adds three dials: a node `minecraft.commandblock` (creative players holding it may edit blocks without being op), a
   world setting `command-blocks.permissions-level` (default 2, the vanilla level blocks run at) and a world setting
   `command-blocks.force-follow-perm-level` (default true: a command in a block needs BOTH the vanilla level and the plugin
   node; with false, either one is enough, the old behaviour). [raw] (patch + config class + docs agree: [re-read])

### Granularity
- Vanilla: one number for every block, and command by command through the level a command asks for.
- Paper: one node per vanilla command, `minecraft.command.<name>` (docs list every command with the default for ordinary
  players: almost all "No"; `help`, `list`, `me`, `msg` and `chase` are "Yes"); a parent node `minecraft` for all vanilla
  commands; special nodes: `minecraft.commandblock`, `minecraft.command.selector` (use `@a`, `@e` and so on),
  `minecraft.nbt.place`, `minecraft.nbt.copy`, `minecraft.debugstick`, `minecraft.admin.command_feedback` (see the
  command broadcasts). Even level-0 commands now carry a node so they can be denied. [re-read] (docs and `Commands.java.patch`)
- Vanilla itself has moved from a bare number to permission objects: Paper's patches show `net.minecraft.server.permissions`
  classes, a permission-set per command source, and a named permission for target selectors
  (`COMMANDS_ENTITY_SELECTORS`) next to the old level names. I could not read Mojang's own notes (the feedback site refused me),
  so what changed in `ops.json` and whether selectors still follow level 2 is not confirmed. [raw] (Paper patches) + [inference]

### Groups, roles, teams
- Vanilla: levels 0 to 4 only. Paper: the permission plugin's groups apply to PLAYERS; the command block sender is one shared
  op object, so you cannot give one block a different group. Whether a permission plugin can scope nodes to that sender is
  not verified. [raw] + [inference]

### When rules conflict
- Paper's `CommandSourceStack.hasPermission` (read in the patch): for normal sources the result is "vanilla level passes OR the
  plugin node passes" (the upstream rule). A deny node can therefore never take a command away from an op. For command blocks
  (by default) it is "BOTH must pass"; the same code also applies "both" to the game's no-actor "null source", which it treats
  as holding every plugin node but which still needs the vanilla level. If the server sets the Spigot option that ignores vanilla permissions
  (named `ignore-vanilla-permissions` in `commands.yml`, from memory [recalled]; the code field is `ignoreVanillaPermissions`),
  only plugin nodes count. [raw]
- Whose rights does `/execute as @p run X` use? Vanilla keeps the command source's permission set and only changes the
  executing entity (I believe so from how the source object is cloned in the patch, [inference]; the wiki page I fetched
  does not say). In 2016 Spigot checked the permissions of the player picked by the selector, so a nested `/execute` in a
  command block failed when that player lacked the plugin node (SPIGOT-2134, fixed 2016-04-20 [raw]). In 2017 a regular
  player who completed an advancement saw "no permission" for the reward function; the maintainer's rule was "Functions execute
  with the permissions of their executor" and "everything has unique permissions including command blocks" (SPIGOT-3491, [raw]).
  What a 2026 Paper does for `/execute as @p run X` is NOT tested here.

### Delegation and escalation
- `minecraft.commandblock` is a dangerous grant: it lets a non-op in creative mode write any command that the block may run
  (level 2 by default: `/give`, `/gamemode`, `/tp`, `/fill`, `/setblock`, `/function`...). Edit power equals run power. [raw]
- In 2015 a user asked Spigot for exactly this node ("Permissions for Command Block usage", SPIGOT-995); it was closed "Won't
  Fix" with the answer that a plugin can do it; Paper has the node today. [raw]
- Vanilla's own safer pattern is the `/trigger` objective: a level-0 command that sets a score, which a block or function at
  level 2 reacts to, so the player gets one narrow verb. [recalled; not checked on the wiki]
- Datapack functions: the server property `function-permission-level` (default 2) sets the level for functions that the
  server itself starts; a function a player calls with `/function` uses the context of the caller, and the wiki does not say
  that a function can raise the caller's level. Advancement-reward functions: the wiki page does not state a level; Spigot's
  2017 report shows them running as the player. [read] + [raw]

### Automation (the central question for R2 and T11)
- Who it runs as: an anonymous block (its name defaults to "@", from memory [recalled]) at level 2 (vanilla), or one shared op
  sender (Paper). No owner, no per-block level. [read] + [raw]
- Limiting power: only globally: lower or raise the one number for all blocks (`permissions-level`), turn every block off (the
  property or game rule), or AND the plugin layer (`force-follow-perm-level`). No per-block or per-owner limit exists in what I read.
- Seeing who it runs as: not shown anywhere; revoke = break the block or switch all off.
- Other vanilla traps:
  - Dispensers were stopped from placing command blocks in 1.8.6 "due to security concerns"; since 15w36a players who cannot
    use commands can no longer place them. [read]
  - Signs can run commands when clicked (`run_command` click events), at level 2 AS the clicking player, but today only when
    the sign's block data flag `allow_op_features` is set, which defaults to false and is set through block data editing
    (that is, by an operator). [read] (wiki summary; I did not see in which version the flag arrived)
  - Spigot's silent-command-block option can hide a block's output from the console, so the built-in log has a mute switch.
    The vanilla game rule `logAdminCommands` is the built-in record. [raw] (patch) + [recalled] (rule default)
- CC:Tweaked command computers follow the same rule as command blocks (section 7).

### Admin interface
- Vanilla: `server.properties` (`enable-command-block`, `function-permission-level`, `op-permission-level`), game rules (runtime),
  `ops.json`, commands. Paper: the two world settings above, `minecraft.*` nodes through the permission plugin,
  `commands.yml` overrides (`command-block-overrides` lets command blocks use a plugin's version of a command, from
  memory [recalled]).
- Plugins that restrict command blocks: Safe Command Block (whitelist or blacklist of commands, optional world whitelist;
  nodes `.access` to edit and `.bypass` to skip the lists; page last released 2013 with a 2016 beta; needs ProtocolLib; not
  maintained as far as the page shows) [read]; listings for CommandGuard, CBlocker, MT-CommandBlocker and an abandoned
  "CommandBlock+" (restrict commands per world or add nodes to plugin commands) [secondary, listing text only]; a plugin
  "RunPlayer" that runs a command as a player with a temporary wrapper [secondary]. I found NO plugin that logs command
  block executions with a person attached; two searches only, so the negative is weak. [secondary]
- "Why refused": a command block's failure text goes to its output box; nothing explains a rule. [recalled]

### Safety
- Defaults: vanilla blocks off on dedicated servers (property default false); only creative ops edit. Paper's `force-follow` default
  true is the safe default. Wildcard hazard: `minecraft.*`-style grants and op status give all of it.
- The real hazard is structural: ownerless, global-level, edit power equals run power, anyone who can press the button
  triggers the block's power (a pressure plate wired to `/give` is a delegation to everyone).

### Task table T1 to T13 (most tasks do not apply to a block)

| Task | How it is done (steps) | Score | Tag |
|---|---|---|---|
| T1, T2, T5, T9, T10 | n/a: command blocks hold no land, roles, bans or teams. | n/a | |
| T3 one command for an hour | Not a feature. Possible with the vanilla `/trigger` objective plus a timer score read by a block; you must know the trick. | 3 | [recalled] + [inference] |
| T4 why refused | Block output text only; no rule trace. | 2 | [recalled] |
| T6 pack of 15 commands | Paper lists a node and default for every vanilla command and a parent node `minecraft`; that is the shape R6 wants. | 4 | [re-read] |
| T7 audit | No list of command blocks, nor of what each runs as. Only the permission plugin's own queries for players. | 1 | [inference] |
| T8 delegate | `minecraft.commandblock` gives edit power equal to everything level 2 allows. Escalation by design. The only brake is the global level and the AND rule. | 1 | [raw] |
| T11 automation with limited power | Limit = one global number for all blocks; "run as" = anonymous shared operator; revoke = break the block or switch all off. | 2 | [raw] + [inference] |
| T12 change now, undo, audit | The game rule is runtime (the property was not); no undo; admin commands are broadcast to ops and logged by game rules, with a Spigot mute switch. | 3 | [read] + [raw] |
| T13 all except one | Only through plugin nodes, and with "both layers" a denied node blocks that command in all blocks (not run by me). | 2 | [inference] |

### Scores
- S1 predictability: 2. Two systems, one OR/AND flag, and the "op by default" sender. [raw]
- S2 explainability: 1. Nothing says why. [inference]
- S3 safety: 1 for the model (ownerless, global, edit equals run), 4 for the Paper default flag. I score the model: 1.
- S4 scale: n/a.
- S5 learning curve: 4 to use, but easy to get wrong because the safe-looking per-command nodes interact with the level number.

### Pain points and incidents (dated)
- 2015-06-22 SPIGOT-995: node for editing command blocks requested; 2015-07-13 closed Won't Fix ("plugin can do this"). [raw]
- 2016-04-05 SPIGOT-2134: nested `/execute` in a command block failed because the second selected player lacked the node; fixed
  2016-04-20; a follow-up reports a server freeze with three nested executes on a Paper fork. [raw]
- 2017-08-04 SPIGOT-3491: reward functions of advancements checked the player's plugin permissions; the maintainer said functions
  run with their executor's permissions, and added a `commands.yml` switch described as bypassing command permission checks
  completely. Resolved 2017-08-05. [raw]
- 1.8.6: dispensers stop placing command blocks; 15w36a: players who cannot use commands cannot place blocks. [read]
- Paper pull request 7404, "Fix cmd permission levels for command blocks", added the two world settings above. I did NOT
  establish the merge date: a search summary said January 2024, but that stamp is on a mirror's copy of a later version of the
  patch file, and the pull request number suggests an earlier date. The summary also said hard-coded early returns for a few
  commands (for example pardon and reload) were replaced by level checks. [secondary] (mirror of the patch; the merged source I
  read confirms the two settings)
- 25w35a (1.21.9 line): the property `enable-command-block` replaced by a game rule. [read]
- Signs: click-event commands now need a block flag. [read]

### Lessons for R1 to R9
- R2: do not copy "ownerless + fixed level + edit power equals run power". Do copy "the block runs at its own level, looked up at
  run time" and make the level per block or per owner. For an ownerless block, require an explicit operator act to create it
  and list all of them in one command (T7). Name the stand-in ("server block") rather than leave it empty.
- R3: Paper's AND rule between two layers is the right idea for a block: its effective power is the intersection of what the
  owner may do now and what the block was marked to do.
- R4: copy one node per command with a documented default; copy a parent node that gathers a whole pack.
- R5/T8: never let "may edit a block" be weaker than "may do what the block does"; tie the two together or require both.
- R6: vanilla's `/trigger` is the pattern for R2's "a shared function marked to run as its owner": a narrow verb the caller may
  use, that runs with owner power.
- R8: a singleplayer owner is op: say loudly in the UI when something runs with operator power.
- R9: record which block ran which command as which actor; the vanilla log only says "a command block did X".

### Sources
- https://docs.papermc.io/paper/reference/permissions [raw]; https://docs.papermc.io/paper/reference/world-configuration [raw]
- https://github.com/PaperMC/Paper : `paper-server/src/main/java/org/bukkit/craftbukkit/command/CraftBlockCommandSender.java`,
  `.../io/papermc/paper/configuration/WorldConfiguration.java`, patches `.../net/minecraft/commands/CommandSourceStack.java.patch`,
  `Commands.java.patch`, `.../server/network/ServerGamePacketListenerImpl.java.patch`,
  `.../arguments/selector/EntitySelectorParser.java.patch` [raw]
- https://hub.spigotmc.org/jira/browse/SPIGOT-995, SPIGOT-2134, SPIGOT-3491 (issue pages fetched as HTML) [raw]
- https://minecraft.wiki/w/Command_Block, `/Permission_level`, `/Commands/execute`, `/Function_(Java_Edition)`, `/Sign`
  [read] (the Sign and Command Block pages are long: only the first 100,000 characters were summarised)
- https://dev.bukkit.org/projects/safe-command-block [read]
- Search listings on Modrinth and Hangar for blocker plugins and a mirror of the Paper patch [secondary]

---

## 5. LWC / LWCX and Lockette (container and door protection)

Depth: light

- LWCX wiki ([read], via raw wiki files). Protection types: private (default; only the owner at first), public (everybody may
  open, nobody can claim it for themselves), password (a password entered once per login), donation (others may put items in but
  cannot take out), display (others may look but not add or take). Most servers auto-protect a new chest as private.
- Sharing: `/cprivate Name1 Name2` while creating, or `/cmodify Name` then hit the block; prefix `-` removes; `g:` is a group from
  the permission plugin; `town:Name` is a Towny town; `@Name` makes a co-manager who may edit the list but not remove the
  protection and cannot remove the owner. So T1 and T10 are "1 command plus 1 click per block". [read]
- Nodes: `lwc.protect` (players: create/remove own), `lwc.mod` (open any protection, cannot remove), `lwc.admin` (everything),
  `lwc.admin.remove` (remove any protection with `/remove` or by breaking the block), `lwc.admin.<command>` (one admin
  command), `lwc.deny` (a NEGATIVE node that blocks interaction with anything LWC can protect). Open and remove are separate
  rights; a deny node exists. [read]
- Owner gone or banned: a "Protection cleanup" wiki page exists but my fetch of it failed (404 on the raw name), so what it
  does is unknown. An admin with `lwc.admin.remove` can break or remove the protection: the same "clear by breaking" case
  the owner described. [read] + [not found]
- Lockette: signs on a door or chest (first line `[Private]`, owner on line 2, up to two more names on lines 3 and 4, extra
  signs for more users). [recalled; not checked]
- Lessons: R5: per-thing owner + list + co-manager-who-cannot-destroy is the closest precedent for "blocks and scripts have
  owners and are shared by name or group"; copy the open/remove split and the negative node; the types (donation, display)
  are small policies on one object that a "sharing mode" field could carry.

Sources: https://github.com/pop4959/LWCX/wiki (Getting Started, Permissions; raw wiki markdown via the fetch tool) [read].

## 6. FTB Teams and FTB Chunks (a modern team model)

Depth: light

- FTB Teams `TeamRank` (source [raw]): ENEMY -100, NONE 0, ALLY 50, INVITED 75, MEMBER 100, OFFICER 500, OWNER 1000. Ranks are
  integers with helper tests like "member or better"; enemy is below none, so a rank can mean "denied". A player is always owner
  of his own personal team [raw]; party and server teams exist too [recalled].
- FTB Chunks claims chunks for a team. Each claim behaviour is a team property with three modes (allies, private, public):
  block edit, block interact (on one platform combined), entity interact, attacking non-living entities, claim visibility
  on the map; plus switches: explosions, mob griefing, pvp, and "fake players". Defaults (config class [raw]): block edit,
  interact and entity interact = allies; explosions off; mob griefing off; claim visibility public.
- Fake players are the automation actors (quarries, deployers). Defaults: all fake players NOT allowed; fake players whose id
  equals a permitted real player ARE allowed (so a machine acts with its placer's id); an allow-list by name exists. This is
  the nearest precedent for R2's owned blocks. [raw]
- Team-based changes are made in a properties GUI; officer/owner roles claim chunks; a separate "FTB Ranks" mod handles permission
  nodes (limits on number of claimed chunks); I did not read it. One add-on's notes say only the owner edits claim config
  while officers manage players. [secondary]
- T1: invite the friend to the team (GUI) so rank member means allies for block edit; or switch a mode to public. T10: the
  team IS the sharing group; members change by invitation and kick. [inference from the model]
- Lessons: R2/R5: numeric rank power with a negative "enemy" side (copy); machine = owner's id with allow-list and default deny
  for unknown machines (copy); three-state mode per behaviour instead of per-person rules (adapt for R5's read/call/write).

Sources: https://github.com/FTBTeam/FTB-Teams (`TeamRank.java`, `PrivacyMode.java`), https://github.com/FTBTeam/FTB-Chunks
(`FTBChunksProperties.java`, `FTBChunksWorldConfig.java`) [raw]; search listings [secondary].

## 7. ComputerCraft / CC:Tweaked command computers (scripting with privilege)

Depth: light

- Source [raw] (`cc-tweaked/CC-Tweaked`): a "command computer" is a computer that may run operator commands from a script
  (`commands.exec(...)`). It is allowed only for players who could edit a command block: with the config
  `command_require_creative` (default true) the test is the vanilla one (creative and level 2); if the server turns that off, level 2
  alone is enough. The computer's own level is 2 by default (an `AdminComputer` interface with a method returning 2).
  `commands.exec` first checks that the server allows command blocks at all and answers "Command blocks disabled by server"
  otherwise. Another config, `command_block_enabled` (default FALSE), controls a separate peripheral that lets any computer
  drive an adjacent command block.
- `commands.list()` walks the command tree and returns what THIS computer may run: an "effective power of the actor" listing, the
  very thing T7 wants. [raw]
- Lessons: R2: privileged scripting is gated by editing rights (same rule as command blocks), and the script runs at a fixed
  server-chosen level; copy the gate, but tie the level to the owner of the script (R2). Copy the "list what this actor may run"
  call for R9.

---

## 8. Answers to the specific questions

### T1 ("a friend may build here") and T10 ("a team shares a thing"): what people type and see

| System | T1 (admin or player types) | T10 (team of three, change later) | What you see afterwards |
|---|---|---|---|
| GriefPrevention | `/trust Friend` in the claim | `/trust B`, `/trust C`; `/untrust B` | confirmation naming player, level, scope; `/trustlist` |
| WorldGuard | wand 2 clicks, `/rg define house Friend` | `/rg addmember team B C`, `removemember`, or `g:group` | `/rg info team` (owners, members, flags) |
| Towny | `/plot trust add Friend` | `/plot trust add B`, `add C`, `remove` | `/plot perm`, `/town` screen |
| LWC | `/cprivate Friend`, hit the block | `/cmodify B C g:team`, `-B` to remove | `/cinfo` |
| FTB Chunks | invite to team (GUI) | team rank changes | map colours; property GUI |
| Command block | n/a | n/a | n/a |

### Ownership: transfer, abandon, lost or banned owner

| System | Abandon | Transfer | Owner gone | Owner banned |
|---|---|---|---|---|
| GriefPrevention | `/abandonclaim` (reports the claim blocks you have left; a claim with sub-areas needs `/abandontoplevelclaim`); a configurable return ratio can cost you part of the claim blocks | admin only: `/transferclaim Player`; with no name it becomes an admin claim that never expires; sub-areas cannot be transferred | claims expire after inactivity only if configured (`ChestClaimDays` for tiny first claims, `AllClaimDays` for all); the expiry can be cancelled by another plugin | nothing automatic; admin runs `/deleteallclaims Player` or `/deleteclaim` in place; the guide suggests aliasing `/ban` |
| WorldGuard | admin `/rg remove <region>`; an owner holding the node may `/rg removeowner` | `/rg addowner`, `removeowner` | no expiry feature found | not handled; `/rg list -p` then remove |
| Towny | `/plot unclaim`; unpaid plot tax loses the plot; unpaid town upkeep deletes the town | `/plot forsale`, `/plot claim`; admin `/townyadmin set mayor` | not read (the wiki section was cut off) | outlaw per town; not read further |
| LWC | `/cremove` | not found | cleanup page not read | admin with `lwc.admin.remove` |

### What went wrong in the field, and what fixed it
- Pistons, fluids and TNT cannons crossing a border: GriefPrevention compares owners of source and target and has four piston
  modes; WorldGuard counts machines as members of the regions they sit in, plus a switch to merge regions; Towny has
  similar-ownership piston protection. [raw] [read]
- Claim, grief and abandon: creative-rules worlds restore nature on unclaim. [raw]
- Admin bypass unlogged: documented, not fixed. [raw]
- Command blocks: edit-power node refused in 2015; nested `/execute` and advancement functions failing or checking the wrong
  actor in 2016 and 2017; Paper's later two settings; Mojang's gates (creative+op in 15w36a, dispensers in 1.8.6, signs behind a flag,
  property replaced by a game rule in 25w35a). [raw] [read]
- Two systems on one region (WorldGuard and GriefPrevention TNT flags) overriding each other. [secondary]

### How players SEE what they may do where
- GriefPrevention: a refusal line naming owner and level; the investigation tool answers "That block has been claimed by Name" or
  "No one has claimed this block"; a node lets admins see all nearby claims at once; admins can see how long an owner has been
  offline (`seeinactivity`); a manual book. The visual outline drawn when you click a claim is from memory [recalled].
- WorldGuard: `deny-message` per region with `%what%`; `greeting`/`farewell` and title flags when entering (and `entry-deny-message`);
  `/rg info` where you stand.
- Towny: the perm line on `/plot perm` and `/town`; a map command and a HUD node exist but their display was not read.
- FTB Chunks: a claim map with public visibility by default (property `claim_visibility`).

---

## 9. Lessons for R1 to R9 (copy, avoid, adapt)

- R1 identity as a per-install key; strangers join:
  - Copy: store grants by key, resolve the nickname at command time (GriefPrevention), show the key's short form when two
    nicknames are close; accept the exact key as an argument.
  - Avoid: special words in the nickname space (`public`, `all`); use a marker (`g:`, `@`, `-`).
  - Adapt: a new key costs nothing, so a key ban is weak. Every system above leans on "strangers hold nothing" plus
    play-time budgets (claim blocks) plus IP tricks (SmartBan). Our only lever is: new keys start with zero rights and a small budget.
- R2 actors: see section 4 lessons. Precedents for the ownerless case: GriefPrevention null owner (shown as "an administrator"),
  Towny NPC mayor, WorldGuard "no owners". Pick one explicit stand-in and list them.
- R3 sets of groups and live lookup: all land systems look up at each action; only Towny freezes a copy (the plot line). Copy the
  live lookup; avoid copies. Copy FTB's integer rank power for ladders, but do not use it for unordered groups.
- R4 per command/pack/tag, allow and deny: WorldGuard's rule set (priority, deny wins at equal priority, parent) is the
  clearest allow+deny algorithm; Towny's `-node`; Paper's node per command. Avoid OR between two layers.
- R5 sharing: GriefPrevention's chain (access, containers, build, manage, each level including the ones below) is a model for
  a short ordered chain behind R5's read/call/write; LWC's per-thing list with a co-manager and the open/remove split;
  Towny's per-rank grant node to stop self-escalation.
- R6 new pack: ship the role bundle (Towny), a user/admin split with one parent node (GriefPrevention), a documented default per
  command (Paper), "no one by default" (WorldGuard). Show a one-line "new pack X: 15 commands, default op only".
- R7 typed commands only: GriefPrevention `/claim [radius]`, `/extendclaim N`; Towny and FTB claim the cell you stand in; WorldGuard
  needs a selection (hard with typed commands); WorldGuard's clickable flag list.
- R8 singleplayer owner = op: GriefPrevention's explicit "ignore claims" mode (default respecting), WorldGuard's silent op
  bypass is the thing to avoid. Always log the mode.
- R9 explainability: GriefPrevention's refusal line, WorldGuard's `%what%` and debug simulation, Towny's perm line. Better:
  return `{decision, layer, rule, owner, level}` to one renderer. A hook that can change a verdict must supply a reason.

## 10. What I could not read or check

- Nothing was run: no server, no mod. Current Paper behaviour for `/execute as @p run X` and for WorldGuard regions versus command
  blocks is NOT tested (I give [inference] only).
- Towny: only wiki pages through a summarising tool; "How Towny Works" cut at 100k of 131k characters (mayor departure, ruins,
  inactivity deletion, outlaw effects not read); refusal text and the perm HUD not seen; Factions and Lands not read.
- LWC: protection cleanup page failed (404 on the raw name) so the owner-gone behaviour is unknown; Lockette from memory.
- FTB: FTB Ranks and the claim limits, and any GUI text, not read.
- Mojang's own notes on the new permission objects (feedback.minecraft.net refused me with 403); the `command_blocks_work` default;
  whether `enable-command-block` still exists in the newest `server.properties`; the version in which signs got `allow_op_features`.
- GriefPrevention docs defaults may be older than the code; its external wiki (r.griefprevention.com/docs) not read.
- Incident search was thin for WorldGuard and Towny (one search each); claims of "nothing found" there are weak.
- Limits exceeded: about 90 tool calls (limit about 70); 11 of 20 searches used.

## 11. Source index (reading method)

- [raw] GitHub raw files via `gh api` (read only): GriefPrevention, WorldGuardDocs, WorldGuard, Paper, CC-Tweaked, FTB-Teams, FTB-Chunks.
- [raw] HTML pages turned to text by a script: dev.bukkit.org GriefPrevention docs; worldguard.enginehub.org flags page;
  docs.papermc.io permissions and world-configuration; hub.spigotmc.org Jira issues SPIGOT-995, -2134, -3491.
- [read] through the summarising fetch tool: minecraft.wiki pages (Command_Block, Permission_level, Commands/execute,
  Function_(Java_Edition), Sign, Java_Edition_1.21.11), Towny wiki raw markdown, LWCX wiki raw markdown, Safe Command Block page.
- [secondary] search results only: Google Groups Towny threads, GriefPrevention changelog snippets, plugin listings.
