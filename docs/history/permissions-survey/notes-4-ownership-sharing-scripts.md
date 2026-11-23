# Card 4: ownership, sharing and script privileges

> Raw notes of research stream 4 of the permissions study: ownership, sharing and script privileges (Second Life, Space Engineers, Roblox, Discord, Luanti trusted mods, and in brief WoW, EVE, Google Drive, Notion, OpenComputers). Written by a research agent on 2026-11-23 and kept as it wrote it, except that local file paths were removed and two file names lost their code quotes. The survey that draws on these notes is `permissions-survey.md`; it says which claims were checked again. The tags are the agent's own: [re-read] and [raw] mean the source's own text was read, [read] a summary by the fetch tool, [secondary] a second-hand source, [recalled] memory, [inference] reasoning from cited facts.


Agent 4 of 5. Desk research, checked 2026-11-23. In depth: Second Life (LSL scripts, object permissions, group roles, land; a note on
OpenSim), Space Engineers, Roblox, Discord, Luanti trusted mods. In brief: World of Warcraft guild ranks, EVE Online corporation roles,
Google Drive and Notion sharing, OpenComputers.

## 0. How this was made, and what failed

- Searches used: 15 of the 20 allowed (Second Life 5, EVE 2, one each for Space Engineers, Discord, WoW, Android prompt studies,
  OpenComputers, Luanti, Roblox backdoors, OpenSim). About 79 tool calls, a little over the "about 70" in the brief: most of the extra were
  raw reads of public source files, which is why many findings are [re-read].
- Tag use in this card. [re-read] = I read the raw text of a file or page (public repository files through the read-only contents API),
  and confirmed the point from a second angle (a second passage or a second file). Where only one raw pass was made I say "single raw read".
  [read] = through the fetch tool that summarises, so numbers and wording may be the summary's. [secondary], [recalled], [inference] as in the
  brief.
- Could not read: Discord support pages (403) so "view server as role" comes from a search summary; Second Life knowledge base (403);
  Blizzard support article 316 (sends you to a login); docs.luanti.org Lua environment page (connection reset); the OpenSim wiki (SSL error)
  and its config file (host blocked automated reads); the Second Life per-parcel script limits (no current page found); Roblox plugin
  permission prompts (not in the docs repo, not searched further); OpenComputers "user rights" page; Space Engineers game as it is today
  (the source I read is Keen's published snapshot, last commit 2022-01-06).
- Nothing was installed, run or changed. Reads of public source used `gh api` read-only: file contents, repository tree listings, default
  branch names and one latest-commit date. `curl` was tried three times on a wiki and two config hosts and was blocked each time.

## 1. Short answers to the specific questions

### 1.1 When a script acts for an owner, how is a stranger stopped from using the owner's power through it?

The problem has a name, "confused deputy": a helper (script, block, bot) holds power of its own and can be steered by someone who lacks it.
What the systems do, best first:

1. Roblox: capability sets only shrink inward [re-read]. A sandboxed container holds a set of named capabilities (41 by my count: 34 engine
   areas plus 7 others). A container inside another gets the common part of the two sets, and an empty overlap means nothing is allowed.
   Every engine call is tagged in the reference with the capabilities it needs, and a refusal names the first missing one. The page also
   says a callback runs with the capabilities of the code that registered it, which is the documented way for outside code to lend a
   sandboxed script one narrow power. A later passage says a sandboxed caller may not fire an event or function whose container has more
   capabilities than its own. I read those two passages as partly in tension and did not settle which wins. The feature is a client beta.
2. Space Engineers: reach is recomputed from the owner on every run [re-read]. Before each run a programmable block marks each block on its
   connected grids as reachable only if the block's OWNER could use it (owner, same faction with sharing on, shared with all, or owned by
   nobody). If the owner changes faction or sharing changes, the next run sees it.
3. Second Life: power is split by whose body is affected [read]. A script uses its owner's own powers (rezzing, giving) with no prompt. Any
   power over a person (animate, take controls, teleport, track camera) needs THAT person's yes, given to that one script, one person at a
   time. Powers over money, linking, estate management and land sale can only be granted by the object's owner. So a stranger who touches
   a scripted object can be asked about the stranger's own avatar, never about the owner's power.
4. Luanti: the asker is identified by where the call comes from, not by a name it passes [re-read]. A mod gets the sandbox-free environment
   only if its name is in the operator's list AND it asks directly from its own main file at load time (not from a callback, not from a
   coroutine). The code comment says why: another mod could override a function and steal what it returns. This is also the weakest
   implementation I found: advisory GHSA-44f7-f48m-vxcw (Minetest up to 5.8.0) and CVE-2026-40960 (5.0.0 up to 5.15.2, score 8.1, published
   2026-04-16) both let an untrusted mod obtain a trusted mod's access when at least one mod is listed as trusted [read, secondary]. One fix
   (commit 0faf529, "coroutine confusion") rejects asks made from inside a coroutine [read]. The attack itself I did not read.
5. Discord: an app that wants to change who may use its commands cannot do it with its own power. It needs a delegated token from a USER
   who holds Manage Guild and Manage Roles, can run that command, and may manage the people or roles touched [re-read]. Each interaction
   also carries the app's own permissions in that place (app_permissions) [re-read]; the invoking member's permissions come with the member
   object [recalled]. Intersecting the two is left to the app author.

What none of them stops: whoever can EDIT code that runs as the owner. Space Engineers: the server event that replaces a programmable
block's program checks only that the sender is a "scripter" (when that rank is switched on); in the lines I read there is no check that
the sender owns or may use that block, and the run-request event has no check in those lines either [re-read, single raw read, 2022
snapshot; the terminal screen on the client probably requires block access and I did not read that]. Roblox: free-model backdoors give the
model's author the game's server authority; the docs call sandboxing "the most effective defense" and say they plan to sandbox Creator Store
assets automatically in future, so today the default is all-or-nothing [re-read]. Luanti: a trusted mod has the whole host by design.
Opinion (confidence high that it is a real hazard, medium on the best fix): write access to code that runs with someone's power IS that
power. The design should either treat such a write grant as the same grade as handing over the power, or make a marked function lose its
mark (or run with the common part of the owner's and the last editor's power) when anyone but the owner edits it. No system I read does
the second; Roblox's "common part when nested" is the nearest.

### 1.2 Ask at the time of use, or grant in advance? What does the evidence say?

- Prompt at use: Second Life script permissions [read]; Android runtime prompts (the only experimental evidence found). In advance: SL
  object masks, group roles, parcel options; Roblox group roles and capabilities; Luanti's operator list; Discord's install checklist and
  per-command overrides; Space Engineers share modes.
- Evidence, all from phone studies except the Second Life anecdotes. A 36-person field study of Android found at least 80 percent of
  participants would have blocked at least one request and judged over a third of requests invasive [read: abstract of arXiv 1504.03747].
  A later six-week study of 157 people on Android 6 runtime prompts found 84 percent of requests accepted, with refusals running from 10
  percent (calendar) to 23 percent (microphone) [secondary: search summary of Bonne et al., SOUPS 2017]. People mostly click yes: a prompt
  informs but does not filter.
- Second Life: the debit permission (take money from the owner's account) is all-or-nothing with no cap on the amount, and has been the
  basis of scams [secondary: wiki proposal page and archived forum threads, 2005 to 2007; the official scams page]. A 2007 developer-list
  message argued any prompt that can cost money should look like a payment dialog, because of "click click click oops" reflex approval
  [read]. Mitigations that exist: the viewer allows at most 5 dialogs per 10 seconds from one agent to another, dialogs never time out, and
  a chat notice records object, owner, region, position and what was granted [read].
- A prompt needs a person present. Second Life asks "an avatar in the same region" [read]. Command blocks, scheduled scripts and bots run
  unattended, so for them only an advance grant can work. SL's answer is that the script's own owner implicitly consented by placing it.
- I found no study comparing prompts with advance grants on game servers. Opinion, confidence medium: prompts suit rare, self-explaining
  powers over the person who is sitting there (move me, take my controls, send me away), and should show the scope. Admin-grade power
  (run commands, change blocks, spend) should be granted in advance, by an admin, as a visible, revocable entry that states power, target,
  limit and expiry. SL's debit prompt shows what happens when the prompt carries no limit.

### 1.3 How is ownership handed over, shared with a group, or lost?

| System | Hand over | Share with a group | Lost or orphaned |
|---|---|---|---|
| Second Life | needs the Transfer bit; on transfer the next-owner mask becomes the owner mask; owner always may delete and move; "right of first sale" keeps one copy passable [read] | set the object's group and tick group bits; or deed it: owner and group masks merge into one value, needs the group ability to deed [read] | group disbands if under 2 members for 48 hours and cannot be recovered [read]; a group whose owners are gone has no recovery: 2026 feature request, 47 votes, "tracked", no date, duplicate of an old bug [read] |
| Space Engineers | owner set when a block's computer part is welded; owner may change block owner and share mode (code path exists) [re-read]; whole-grid hand-over not described [secondary] | per block: none, faction, all [re-read] | owner 0 means nobody: everybody counts as owner and anyone can take it; only a server admin can set that [secondary]; what happens on leaving varies by version [secondary] |
| Roblox | group owner / user owner of an experience; Edit goes to friends only for user-owned experiences, by role for group-owned [re-read] | roles with Edit, per game or for all group games [re-read] | "Roblox cannot help arbitrate or resolve disagreements" in a group [re-read] |
| Discord | server owner holds every permission [re-read]; transfer by the owner [recalled] | roles | recovery is a central support case [recalled]; not usable without a central service |
| Google Drive | Share menu, "Transfer ownership" [read] | share with named people or groups | n/a |
| Notion | n/a | groups added in the Share menu [read] | n/a |
| WoW | one leader; an idle leader (90 days) can be replaced by a member of rank 2 to 4 who opens a confirmation [read, secondary] | ranks | bounded, documented takeover |
| EVE | CEO only can make a Director [secondary] | roles per member | insiders with Director can empty a corporation (see 3.2) |

### 1.4 What do creators and admins say hurts?

1. Checkbox matrices get huge, and the fix is a redesign of the screen, not fewer rules. EVE's role screens took 19 screens to set roles and
   12 for hangar and container access; the redesign made it 5 and 1 [read]. Roblox groups list about 45 permissions in 13 sections
   (count of label keys in the page source: 58 minus 13 headings) [re-read].
2. "Why am I refused?" has no answer in the product. Discord's "View Server As Role" shows what a role sees but not which rule decides;
   a community request asks for a tool that says why [secondary]. Space Engineers greys out the Edit button and the wiki, not the game,
   explains it [read, secondary]. Luanti returns nil [re-read]. The best answer found is Roblox's refusal text naming the missing capability
   [re-read].
3. Nobody can take over when the owner is gone: Second Life groups (July 2026 request, 47 votes) [read]. WoW fixed it in 2011 [read].
4. Prompts that cost money and get clicked through: Second Life debit permission scams [secondary, read].
5. All-or-nothing trust: Luanti trusted mods have full host access [re-read]; Roblox third-party scripts have full authority unless the
   opt-in sandbox is turned on [re-read].
6. Hidden ordering: Discord role position decides who may manage whom, and a channel falls out of sync with its category the moment
   it is edited [re-read]; the docs call category syncing "a bit tricky".
7. A rename destroys grants: Discord says deleting or renaming a command permanently deletes its permissions [re-read].
8. Roles too coarse for sensitive data: EVE players complain that an accountant role shows all wallets; a 2022 thread got no developer reply
   [read].
9. Footguns in "empty means open": OpenComputers makes a computer open to all players when the last user is removed [read].

### 1.5 What does "grant read, call or write on a named variable or function to a named player or group" look like where something is close?

I found no system with per-variable or per-function grants for player scripts, except OpenSim's per-function lists (below). The nearest forms:

| System | Who gets it | Verbs | On what | Extras |
|---|---|---|---|---|
| Roblox asset grant (Open Cloud) | User, Group, GroupRoleset, All, Universe | Edit, Use, Download and others | batch of asset ids | `grantToDependencies` flag extends the grant to what the asset depends on; per-item errors returned [re-read] |
| Roblox package permission | user or group role set | None, NoAccess, Revoked, UseView, Edit, Own | one package | "Revoked" is its own state, apart from "no access" [re-read] |
| Roblox data stores | group roles | view, edit, delete (three separate permissions) | all group games, or one game | delete is held apart from edit and needs two-step sign-in [re-read] |
| Discord command permissions | up to 100 users, roles, channels per command | allow or deny use | one command, or the app as a whole as default | per-command overrides show "Not Synced"; rename deletes them [re-read] |
| OpenSim OSSL functions | avatar ids, group ids, or roles such as estate owner, estate manager, parcel owner | allow call | one function per `Allow_` line | threat-level default for all functions [secondary; level names recalled] |
| Space Engineers | owner decides | none, faction, all | one block | recomputed on each run [re-read] |
| SL object | owner, group, everyone, next owner | move, modify, copy, transfer | one object | owner and group merge when deeded [read] |
| SL script permission | an avatar grants it to one script | 13 named permissions | one script, one avatar at a time | direction is reversed: the thing asks the person [read] |
| Luanti | operator to a mod name | sandbox-free environment, HTTP | whole mod | init time only [re-read] |
| Google Drive | named users, groups, link | viewer, commenter, editor, owner | file or folder | expiry up to 1 year, owner can stop editors re-sharing [read] |
| Notion | users, groups, workspace | full access, edit, comment, view, create | page, inherited by sub-pages | most permissive wins; no deny [read] |

Shape worth stealing, as [inference]: a grant is (subject, verb, object, optional expiry, optional "as owner" flag). "Call" should mean
call with the CALLER's power; "call as owner" should be its own verb (Roblox keeps "use" and "edit" apart; Drive keeps "edit" and "re-share"
apart). Say in the grant whether "call" also reaches what the function touches (Roblox needs an explicit flag for dependencies). Keep a
"revoked" tombstone so the answer to "why not?" can be "taken away by X on date Y" (Roblox's Revoked state). A "write without read" verb
(Notion's "can create") suits drop boxes.

## 2. Deep cards

### 2.1 Second Life (LSL, object permissions, group roles, land; wiki pages edited 2010 to 2023, one 2026 feature thread)

Depth: deep. Most facts here are [read] (summarising fetch tool); the wiki blocks raw reads. The wiki itself says some pages are incomplete.

Model in five lines:
- Three layers that meet at objects. (1) Per-object masks: four bits (move, modify, copy, transfer) for four classes (owner, group, everyone,
  next owner), plus a base mask. Group and next-owner bits cannot exceed the owner's; "everyone" can never be modify and cannot exceed
  group; copies cannot be more permissive than the original; a group-owned (deeded) object merges owner and group into one value [read].
  (2) Group roles: a role is a template of abilities; every member is in "Everyone"; a member may hold several roles; the "Owners" role
  holds every ability, cannot be edited or removed from, and only Owners can eject Owners; 2 to 10 roles per group; at most 42 groups per
  person; a group needs 2 members and costs L$100 [read]. (3) Land: parcel options (build, run scripts, fly, safe), group powers that can
  override parcel options, and estate settings that override parcels [read].
- Subjects: avatar (account key), group, "everyone", estate owner and managers. Objects and scripts act as the avatar who owns them, or as
  the group once deeded [read, inference].
- Grant only. There is no deny in group roles (abilities add up). Bans exist as lists at group, parcel and estate level [read].
- No inheritance between roles. Layers: estate beats parcel for some switches ("block fly", "restrict pushing"); a group power can override a
  parcel option; "allow damage" is a partial exception that parcels may override [read].
- Defaults: new object has owner and base all, next-owner move or transfer, group and everyone none [read]. New group: founder is Owner.

Script privileges (LSL `llRequestPermissions`) [read]:
- A script asks a named avatar in the same region for a mask of permissions; unless auto-granted a dialog appears; the answer arrives later as
  an event. Permissions are per script and held for ONE avatar at a time; a new request replaces the old; they persist across state
  changes. The page does not say what happens on script reset, rez or transfer.
- 13 named permissions. Owner only: take money (debit), change links, silent estate management, privileged land access (land sale);
  return objects needs the owner or a group owner. About one's own avatar, anyone may grant: take controls, trigger animation, track or
  control camera, teleport, override animations, game control; attach is owner-only for normal attach and anyone for temporary attach.
  Several are granted without a dialog when the avatar sits on or wears the object.
- Script limits: Mono scripts start at 64 KB (older scripts 16 KB) [read]. A 2009 plan gave each parcel a memory pool by size, and a forum post
  quotes a "Parcel Script Memory" section in About Land showing used against a cap [secondary]. I could not read current per-parcel
  figures, so none are given here.
- OpenSim [secondary, partly recalled]: its extra script functions (OSSL) are switched on per function in a config file; each `Allow_`
  line lists avatar ids, group ids or roles (estate owner, estate manager, parcel owner); a global threat-level setting is the default
  for all functions; parcel owners are off by default. This is the closest thing I found to "grant call on a named function to a named
  player or group". Level names (None to Severe) are from memory.

Granularity: about 45 to 50 abilities in 14 sections (my count from the wiki list, which says it is incomplete); per object; per parcel option;
per estate-manager ability (the owner picks each manager's abilities) [read]. No per-command, per-pack or per-tag grants.

Groups, roles, teams: several roles per member, union of abilities, no weights. "Active group" matters for land and objects (you must wear the
right group tag) [recalled]. A group is also the shared owner of objects and land.

When rules conflict: there are no conflicts inside a group (union). Across layers: estate over parcel for the switches named above; group
power over parcel option; otherwise the narrower thing governs [read, inference]. Roughly 3 precedence rules.

Delegation and escalation [read]: "Assign Members to Assigner's Roles" only reaches roles the assigner holds (safe delegation). "Assign
Members to Any Role" and "Assign and Remove Abilities in Roles" are flagged by the wiki as able to make oneself or others near-Owner. The
page does not say outright whether you can grant abilities you lack. A search summary of the same wiki says a role creator can only give
abilities they hold [secondary]. Owners cannot be removed by others; the founder may give up the role.

Automation: a script runs as its object's owner [read]. Estate managers can disable scripts and return one user's scripted objects; a parcel
option controls who may run scripts [read]. Limiting a bot's power in practice means owning it with a second account or deeding it to a group
whose abilities are narrow [inference].

Admin interface: the viewer's floaters (group Members and Roles, About Land, estate tools); no typed commands, no files. Script prompts leave
a chat notice. No audit trail for role changes found; a July 2026 reply says a person was made group owner and could not see who did it
[read]. Temporary grant: parcel passes sold for a set time exist as an ability ("sell passes to") [read]; the hours detail is [recalled].

Safety: baseline is the Everyone role; five abilities are flagged dangerous in the wiki; Owners protected; no deny; recovery missing (see
1.3). Debit permission has no cap [secondary].

Task table (many tasks do not apply to a thing-ownership system; "n/a" means no evidence found):

| Task | How, steps | Score | Tag |
|---|---|---|---|
| T1 friend builds in one area | parcel build option set to group; friend in a group role (or give the friend a split-off parcel); about 4 steps; "only their own things" has no equivalent; the "group may build" option value is from memory | 4 | [read, recalled] |
| T2 moderator, no teleport or mode | new role, tick eject, ban-list and parcel access abilities, assign; about 6 steps; the withheld powers do not exist as abilities | 4 | [read] |
| T3 one command for an hour | no timed role; parcel passes and ban timers are the nearest | 2 | [read, recalled] |
| T4 why refused | script prompts explain themselves; for land and groups no evidence | n/a | |
| T5 strip and keep out a griefer | ban by account key at parcel, estate (all parcels) or group; new accounts evade | 4 | [read, inference] |
| T6 plugin with 15 commands | no pack concept; run-scripts parcel option and estate "disable scripts" are blunt switches | n/a | |
| T7 audit who can do Y | no evidence | n/a | |
| T8 delegate team membership | helper holds the team role plus "assign to assigner's roles" only; about 5 steps; cannot escalate unless given the two flagged abilities | 4 | [read] |
| T9 bootstrap and recovery | bootstrap: founder is Owner (5); recovery: none (1) | 1 | [read] |
| T10 team of three, one shared thing | create group (L$100, second member within 48 hours), invite, set object group and share; change members by invite or eject | 3 | [read] |
| T11 automated thing, limited power | owner by alt account or group; revoke by taking back or deleting; a granted permission can be released by the granter for animation only | 3 | [read, inference] |
| T12 immediate, undo, who changed | immediate [recalled]; no change log found | n/a | |
| T13 all but one | union only, no deny; build a role by ticking all but one of about 46 boxes | 2 | [read] |

Scores: S1 predictability 4 (union only, about 3 layer rules) [read]. S2 explainability: n/a for groups and land; script prompts 4 [read]. S3
safety 3: good baseline and flags, but no deny, uncapped debit prompt, no recovery [read, secondary]. S4 scale 2: hard caps of 10 roles and
about 47 abilities per group make 100 by 20 impossible [read]. S5 learning curve 3 (three layers: objects, groups, land) [inference, low].

Pain points: debit permission spoofing and scams (2005 to 2007 threads; 2007-05 developer-list message) [secondary, read]; leaderless group (2026-07,
tracked, no date) [read]; unexpected ownership transfer with no record (2026-07 reply) [read].

Lessons for R1 to R9: copy "assign only to roles you hold" (R3, T8); copy the permission list in a script's request (R2, R9); copy a
protected top group (R3) but ADD a recovery path (R8); avoid caps on groups that make a team per thing costly (R5, T10); avoid a prompt
with no limit (R2).

Sources: wiki.secondlife.com LlRequestPermissions, Group, Permission, LlGetObjectPermMask, Estate_Manager, LSL_Script_Memory [read];
sldev 2007-05 message [read]; feedback.secondlife.com group-leadership-succession [read]; search summaries of the scams page, the refund
permission proposal page and 2005 to 2007 forum archives [secondary]; OpenSim `osslEnable.ini` through a search summary [secondary].

### 2.2 Space Engineers (Keen's published source, last commit 2022-01-06; wiki.gg pages undated)

Depth: deep. The game may have changed since the snapshot.

Model in five lines [re-read]:
- Every terminal block has an owner (an identity id) and a share mode: none, faction or all. A player's relation to a block is one of five:
  no owner, owner, faction share, neutral, enemy. "Friendly" means the first three, and friendly is what "has access" means.
- Rule in order: owner id 0 gives "no owner" (open to all); the owner gives "owner"; sharing "all", or sharing "faction" with a faction mate,
  gives "faction share"; a player with no faction, or an owner with no faction, gives "enemy"; otherwise the relation between the two factions
  decides: "neutral" gives "neutral" and anything else gives "enemy". "Neutral" is NOT friendly. So being on neutral terms grants nothing;
  only the share mode does.
- Admin ranks form a ladder: none, scripter, moderator, space master, admin, owner, checked with "at least"; each rank has what the ones
  below have [re-read, read]. Offline play and the server's own id are Owner; everyone else comes from a promoted-users table keyed by
  Steam id. The scripter rank exists only when a world setting turns it on; when off, everyone passes the scripter check [re-read].
  Scripts also need the world's "in-game scripts" setting and experimental mode, and run on PC and dedicated servers only [read].
- Scripts: C# compiled against a whitelist of types and members with two targets, mod API and in-game, held as flag bits; the source has a
  comment doubting one entry ("Seems scary to say the least") [re-read]. The block's source sets 50,000 instructions and 10,000 method calls as limits (I take them to be per run; use not traced)
  [re-read].
- Defaults: share mode none [re-read].

Granularity: per block (owner, share mode); per world (rank, scripts on or off); nothing per command, pack or tag. A block has an owner AND an
author; the author pays the block limits [secondary]. The majority owner of a grid counts as the grid's owner for rename and paint [secondary].

Groups, roles, teams: one faction per player [recalled]; ranks are a ladder, not sets; no weights.

When rules conflict: there is nothing to conflict. One function computes the relation from owner, faction and share mode. About 5 relations,
3 share modes, 6 ranks. Local admins can bypass block access with an "use terminals" setting, and the code applies it to the local player
only [re-read].

Delegation and escalation: the host promotes and demotes from the multiplayer screen; the wiki does not say whether anyone else may [read].
The owner can change a block's owner and share mode (code path exists) [re-read].

Automation, the important part [re-read]: before each run the programmable block recomputes which blocks on its connected grids it may touch,
using ITS OWNER's access. So script power = owner's access at that moment, never frozen. An ownerless block gets owner id 0, which reaches
only no-owner blocks and blocks shared with all: the weakest actor, not the strongest [inference from the relation code]. Editing is
a separate right: the server checks the sender's scripter rank when a program is replaced, and in the lines I read nothing ties it to the
block's owner (see 1.1).

Admin interface: GUI screens and a server config file; Promote and Demote buttons; no typed commands seen; no audit trail or undo found.
"Why": the Edit button is greyed and the wiki says you need the scripter role [read]. The ladder cannot be tailored.

Safety: default share none [re-read]; unowned blocks are open to all; only an admin can set a block to nobody [secondary]; script limits
are per run. Lockout: owner and server id are Owner by construction [re-read].

Task table:

| Task | How | Score | Tag |
|---|---|---|---|
| T1 friend builds in one area | no area or per-owner build permission found | n/a | |
| T2 moderator | Moderator rank: kick, ban, show all players; fixed bundle, cannot be trimmed | 3 | [read] |
| T3 one command for an hour | promote and demote only; no timer found | 1 | [read] |
| T4 why refused | greyed button; wiki explains | 2 | [read, secondary] |
| T5 strip a griefer | ban (moderator and up); admin can move blocks to nobody | 3 | [read, secondary] |
| T6 new plugin | mods share one global whitelist; no per-mod grant found | n/a | [re-read] |
| T7 audit | no evidence | n/a | |
| T8 delegate | who may promote whom is not documented | n/a | [read] |
| T9 bootstrap, recovery | offline and server id are Owner; others by table | 4 | [re-read] |
| T10 team of three, one thing | faction plus share "faction" on each block; per block, bulk not verified; change by faction membership | 3 | [re-read] |
| T11 automated thing, limited power | programmable block runs as its owner's access; give it to a weak owner; revoke by changing owner | 3 | [re-read] |
| T12 immediate, undo, who | next run sees owner and share changes; no log found | n/a | [re-read] |
| T13 all but one | no categories, no deny | n/a | |

Scores: S1 5 (one function, small enums) [re-read]. S2 2 (greyed button, no reason) [read, secondary]. S3 3 (safe share default; edit right not
tied to ownership in the lines read; nobody-owned open) [re-read]. S4 n/a (no matrix). S5 4 (three share modes, relations in words)
[inference].

Pain points: greyed Edit button even with scripts on, answers say check block ownership [secondary: Steam thread]; script cannot touch
blocks with a different owner [secondary: Steam comment]; what happens to a leaver's blocks changes by version [secondary].

Lessons for R1 to R9: copy "recompute from the owner every run" (R2, R3); copy "no owner is the weakest actor" and change the design's
"ownerless runs as anonymous operator" unless an op set it on purpose (R2); avoid editing rights that are unrelated to the actor's owner
(R5); avoid ladders for custom roles (R3); avoid a silent greyed button (R9).

Sources: raw files in github.com/KeenSoftwareHouse/SpaceEngineers: Sandbox.Game/Game/Entities/Cube/MyIDModule.cs, MyTerminalBlock.cs,
MyCubeBlock.cs; Sandbox.Game/Game/GameSystems/MyGridTerminalSystem.cs; Sandbox.Game/Game/Entities/Blocks/MyProgrammableBlock.cs;
Sandbox.Game/Game/World/MySession.cs; VRage.Game/Entity/MyRelationsBetweenPlayerAndBlock.cs; VRage.Scripting/MyScriptWhitelist.cs
[re-read]. spaceengineers.wiki.gg Programmable_Block, Multiplayer_Screen [read]. guildorder.com ownership page [read, third party,
flags its own details as unverified]. Steam threads through search [secondary].

### 2.3 Roblox (creator-docs repository, main branch, checked 2026-11-23)

Depth: deep. Script capabilities are "experimental, client beta".

Model in five lines [re-read]:
- Groups: roles listed in hierarchy order; a member holds one or several roles; each role has permissions. A management permission works
  only on roles and members BELOW the actor's highest role; the role manager can edit only permissions they hold; granting five risky ones
  (role manager, role assigner, manage group games, group revenue manager, data store deleter) requires the GRANTER to have two-step sign-in.
- Experiences have an owner (user or group). Team Create levels: owner, edit, play, none. User-owned: play to any user or group, edit only
  to friends. Group-owned: edit by role for all group games (one permission) or per game. Collaboration also needs an age check.
- Assets and data: Open Cloud lets you grant (subject, action, assets); data stores have view, edit and delete permissions per group role.
- Scripts: a sandboxed container (model, folder or script with the Sandboxed property) limited to a Capabilities set; 41 named capabilities.
  The default for everything else is "all-or-nothing" in the docs' words.
- Defaults: not read for new roles; script containers are NOT sandboxed unless the experimental mode and the property
  are switched on.

Granularity: per permission (about 45), per role, per game, per asset, per script container with 41 capability tags, per engine member
(each class, property, method and event lists the capabilities it needs). A command level exists for Studio plugin commands: plugin or
local user [re-read].

Groups, roles, teams: several roles per member now [re-read]; the older single rank number is [recalled]. Roles are ordered. Public or
private visibility of a role does not change its permissions.

When rules conflict: group permissions add up across a member's roles [inference; the page does not say]; hierarchy limits management
only. Capabilities: common part when nested. A sandboxed script also still needs the old gates (for example loadstring must be switched on in
the server script service) [re-read]. Precedence rules: about 3.

Delegation and escalation [re-read]: strong. Lower than your own only; only permissions you hold; extra sign-in on the risky five; audit
log permission ("Activity History"); role assignment also available by Open Cloud from a game server. The capability "CapabilityControl"
guards changing the sandbox itself, and the docs warn against giving third-party assets network, data store, asset loading, capability
control or loadstring.

Automation: server scripts carry the experience's authority, not a player's. Third-party code is contained by capabilities. API keys are
"configurable keys that allow granular access" [re-read, from the API description]. The plugin manifest declares requested permissions
[re-read]; prompts for them I did not find.

Admin interface: Creator Dashboard (roles, members, audit), Studio's Manage Collaborators window, the Properties window for capabilities,
Open Cloud for scripts. "Why": capability errors name the action, the target and the first missing capability [re-read]; for group
permissions no why-tool found.

Safety: dangerous default for scripts (full authority); sandboxing documented as the defence and automatic sandboxing only planned
[re-read]; one capability is named differently in two docs pages (AssetRequire against LoadUnownedAsset), a sign of a beta in motion
[re-read]; capabilities do not yet cover every class, and unlisted ones fail as "Unassigned" inside a sandbox [re-read].

Task table:

| Task | How | Score | Tag |
|---|---|---|---|
| T1 friend edits one area | Team Create edit for a friend covers the whole place; no area scope found | 2 | [re-read, inference] |
| T2 moderator | role with kick and ban permissions, no edit or revenue; about 5 steps; kick and ban only for members below | 5 | [re-read] |
| T3 one command for an hour | no timed grant; script it with Open Cloud | 2 | [re-read, inference] |
| T4 why refused | scripts: yes, names the missing capability; groups: none found | 4 for scripts | [re-read] |
| T5 strip a griefer | ban API by account; alts evade | 4 | [re-read, recalled] |
| T6 new plugin with 15 commands | insert model; sandbox container plus capability set: about 3 steps, off by default | 3 | [re-read] |
| T7 audit | audit log shows changes; "who can do X" not found | 3 | [re-read] |
| T8 delegate | role manager and role assigner below own rank; risky five need sign-in; cannot grant what they lack | 5 | [re-read] |
| T9 bootstrap, recovery | creator is owner; recovery: Roblox will not arbitrate | 1 for recovery | [re-read] |
| T10 team of three, one thing | group-owned experience, a role with edit; change by role assignment | 4 | [re-read] |
| T11 automated thing, limited power | sandbox container; capabilities visible as a property; revoke by changing it; scoped API keys | 4 | [re-read] |
| T12 immediate, undo, who | audit log and undo for edits; take-effect timing not read | 3 | [re-read] |
| T13 all but one | capabilities are allow-only: tick 40 of 41 | 2 | [re-read] |

Scores: S1 predictability 4 for groups, 5 for capabilities [re-read]. S2 3 (errors good, groups silent) [re-read]. S3 3 (good delegation
rules and sign-in gate; unsafe default for scripts) [re-read]. S4 3 (45 permissions by roles, 41 tags; no bulk view found) [re-read,
inference]. S5 3 [inference].

Pain points: toolbox backdoors in free models, with many developer-forum threads (thread ids suggest 2019 to 2025; I did not date them) that
let an attacker run code on the server [secondary]; the official page calls moderation "not guaranteed" [re-read].

Lessons: copy capability tags with error text (R4, R9); copy "intersection when nested" and "can only pass on what you hold" (R2, T8);
copy the extra sign-in on the risky few (R3); avoid an unsafe default (R6); avoid single-friend-only edit rules (R5).

Sources: raw files in github.com/Roblox/creator-docs content/en-us: scripting/capabilities.md, scripting/security/third-party-vulnerabilities.md,
projects/groups.md, projects/collaboration.md, reference/cloud/asset-permissions-api/v1.json, reference/engine/enums/PackagePermission.yaml,
CommandPermission.yaml, classes/PluginCapabilities.yaml [re-read; capabilities page read to its "module require" part only]. Developer-forum
threads through search [secondary].

### 2.4 Discord (developer docs repository, main branch, checked 2026-11-23)

Depth: deep, the best-known mass-market role system.

Model in five lines [re-read]:
- Subjects: members, roles (the default role "@everyone" has the same id as the server), apps with a managed role [recalled], channels
  (for command overrides). The server owner holds every permission; the "Administrator" permission also gives all and skips channel overrides.
- 41 permission flags by my count of table rows (a 64-bit number sent as text); the ones marked with a star need the owner's two-step
  sign-in when the server demands it.
- A member's base permission = union of @everyone and all their roles. Channel overrides then change it in a fixed order (below).
- The role hierarchy applies to MANAGEMENT only: grant roles lower than your highest; edit roles lower than yours, granting only
  permissions you hold; kick, ban and rename members whose top role is lower. The docs say that otherwise permissions do not obey the role order.
- Defaults: @everyone's base permissions; an app's commands usable by all unless its author sets a default permission; "0" means admins only.

Granularity: per permission flag; per channel (allow or deny per role or member); per category (synced copies); per app and per command
(allow or deny for up to 100 users, roles or channels) [re-read]. The app-wide default applies to every command without its own entry. No
per-tag.

Groups, roles, teams: a member holds many roles; role position is only for management; the docs give an example of one role denying and
another allowing and the member getting the access.

When rules conflict, the documented algorithm [re-read]: owner gets everything; start with @everyone's permissions; add every role's; if
Administrator, everything. Then in a channel: (1) @everyone overwrite denies, then allows; (2) ALL the member's role overwrites are
collected, their denies removed, then their allows added (so allow wins between roles); (3) the member's own overwrite denies, then allows.
The docs list this as 8 steps. Extras: denying view channel makes the rest moot; denying send messages implicitly denies mentions, files,
embeds; denying connect in voice implicitly denies more; threads inherit from their parent except send messages; categories are "synced"
rather than inherited and a channel drops out of sync when edited; timed-out members get a trimmed set. How user, role and channel command
overrides combine is not in the text I read.

Delegation and escalation: as above (lower only, only what you hold). Nothing in the rule text stops roles that carry more power than you hold from being
handed out if they sit below your top role, so position is a hidden security setting [inference from the rule text].

Automation: bots join through an install link carrying a permission number; the installer sees the list [re-read]. Command permissions
can only be changed with a user's token (not a bot token) from someone with Manage Guild and Manage Roles [re-read]. Interactions carry
the app's own permissions in that place [re-read].

Admin interface: GUI (roles, channel permissions, Server Settings > Integrations with a per-command screen showing "Not Synced"), an API,
an audit-log permission [re-read]. "View Server As Role" (owner or Administrator only) shows what one or several roles can see; it does not
show voice limits tied to a user and does not say which rule decided [secondary, search summary of the support article].

Safety: owner and Administrator are strong switches; stars on the dangerous flags with a two-step gate [re-read]; "allow beats deny
between roles" surprises people [re-read]; deleting or renaming a command deletes its permissions for good, and the bulk edit call
overwrites existing entries [re-read].

Task table:

| Task | How | Score | Tag |
|---|---|---|---|
| T1 friend builds in one area | role plus a channel override (3 or 4 clicks); "only their own things" has no equivalent | 4 | [re-read] |
| T2 moderator | one role with kick, ban, timeout; leave out manage server; about 5 clicks | 5 | [re-read] |
| T3 one command for an hour | per-command user allow exists; no expiry is mentioned in the text I read (the field list was not printed); a bot must remove it | 2 | [re-read, inference] |
| T4 why refused | algorithm is published as code, but no tool names the deciding rule; "view as role" shows what, not why | 2 | [re-read, secondary] |
| T5 strip and keep out | ban by account id; new accounts evade | 4 | [inference] |
| T6 new plugin with 15 commands | install shows the permission list; top-level command permission covers all of the app's commands in one step; author's default decides safety | 4 | [re-read] |
| T7 audit | per-command overrides can be listed; "everything X can do" only per role through view as role | 3 | [re-read, secondary] |
| T8 delegate | Manage Roles holder below the team role; can escalate if a stronger role sits lower | 3 | [re-read, inference] |
| T9 bootstrap, recovery | creator is owner; recovery by central support | 3 | [re-read, recalled] |
| T10 team of three, one thing | private channel plus role, about 4 clicks; change by assigning the role | 5 | [re-read] |
| T11 automated thing, limited power | bot's own role and install checklist; revoke by removing the bot or role | 4 | [re-read] |
| T12 immediate, undo, who | immediate [recalled]; audit log exists; no undo | 4 | [re-read, recalled] |
| T13 all but one | deny in a role or member override works; between two roles allow wins; per-command: allow app-wide, deny one command | 3 | [re-read] |

Scores: S1 predictability 3 (8-step order plus owner, Administrator, implicit and sync rules) [re-read]. S2 explainability 2 [re-read, secondary].
S3 safety 4 [re-read]. S4 scale 3 (41 flags by roles by channels; per-command lists up to 100) [inference]. S5 learning curve 3 [secondary].

Pain points: community requests for a "why can this user not read this channel" tool and a permissions testing feature [secondary: support
community posts, undated here].

Lessons: copy the published algorithm as code and the per-pack default with per-command exceptions shown as "Not Synced" (R4, R6, R9); copy
"management only follows rank, not permission maths" (R3); avoid allow-beats-deny across roles without telling the admin (S1); avoid
losing grants on rename (R4); avoid Administrator-style bypass of everything (R3).

Sources: raw files in github.com/discord/discord-api-docs developers/topics/permissions.mdx (hierarchy, overwrites, implicit, syncing),
interactions/application-commands.mdx (permissions section), topics/oauth2.mdx, interactions/receiving-and-responding.mdx [re-read].
support.discord.com View as Role FAQ and community posts through search only (403 on fetch) [secondary].

### 2.5 Luanti trusted mods (luanti master, docs and source; CVE record dated 2026-04-16)

Depth: deep, but most tasks do not apply: this is a boundary for script power, not a role system.

Model in five lines [re-read]:
- Mods run in a Lua sandbox: a whitelist of base functions; io is limited to close, flush, read, type, write (file opening is path-checked; I saw the check functions, not every wrapper); os to
  clock, date, difftime, getenv, time; two more whitelists exist (I believe client-side mods and the async environment; not checked).
- Two operator settings in the server's settings file: `secure.trusted_mods` (mods allowed to bypass the sandbox, "full access to the host
  machine") and `secure.http_mods` (mods allowed HTTP, which can reach the internet and devices on your network). A trusted mod also
  gets HTTP.
- A mod asks with `core.request_insecure_environment()` or `core.request_http_api()`. You get nil unless the calling mod is listed. The ask
  works only at load time, from the mod's own main file. The docs say never to let another mod see what comes back.
- The mod's identity is read off the call stack: called directly from the main chunk, not within a coroutine (that check is the 0faf529 fix).
- Mods cannot set `secure.*` settings through the settings API. Default for both lists is empty.

Granularity: per mod name (a namespace), two power classes. Nothing per function.
Groups, roles: none; a flat allowlist. When rules conflict: nothing to conflict.
Delegation and escalation: only the operator, in a file; a mod cannot add itself. A trusted mod that leaks the environment hands it on;
that is the documented danger.
Automation: n/a (mods are not run "as" a player).
Admin interface: a setting (advanced settings menu or file); one line shows everyone with power (audit is trivial). No reload: the ask is made
at load, so a change needs a restart [re-read, inference].
Safety: safe default (empty); all-or-nothing; real vulnerabilities in the broker (see 1.1); an ordering hazard noted on the content forum
(a mod loaded before the trusted ones could interfere; an engine issue about it was not implemented) [secondary].

Task table (only the rows with evidence):

| Task | How | Score | Tag |
|---|---|---|---|
| T4 why refused | the ask returns nil and nothing else | 1 | [re-read] |
| T6 new plugin | safe default is "not listed"; one setting line per mod grants it | 5 | [re-read] |
| T7 audit | one setting line | 5 | [re-read] |
| T12 immediate | needs restart | 1 | [re-read, inference] |
| T1, T2, T3, T5, T8, T9, T10, T11, T13 | do not apply | n/a | |

Scores: S1 5. S2 1 (nil, no reason). S3 3 (safe default, but two advisories in the broker and no middle level). S4 n/a. S5 4 [inference].

Pain points: GHSA-44f7-f48m-vxcw (Minetest up to 5.8.0) and CVE-2026-40960 (fixed in 5.15.2, published 2026-04-16) [read, secondary]; a
third-party mod exists to narrow "require" for single modules because the core setting is all-or-nothing, and its author warns it is not
safe against untrusted mods [secondary].

Lessons: copy "a pack asks by name and the operator's file decides, default empty" (R6) and "one line to audit" (R7); avoid finding the
asker by looking at the call stack (R2): hand each pack its capabilities when it is loaded and offer no ask function to intercept; avoid
"trusted means whole host" (R4): add middle levels.

Sources: doc/lua_api.md lines about request_http_api, request_insecure_environment, settings set; builtin/settingtypes.txt (Mod Security);
src/script/cpp_api/s_security.cpp (whitelists, getCurrentModName, checkWhitelisted) in github.com/luanti-org/luanti [re-read]. CVE record
(db.gcve.eu) and commit page [read]. Advisory, Unsafe Require and forum post through search [secondary]. The docs.luanti.org page could not
be fetched.

## 3. Light cards

### 3.1 World of Warcraft guild ranks (warcraft.wiki.gg Guild page [read]; patch history [secondary])
- Up to 10 ranks (default five: Initiate, Member, Veteran, Officer, Leader). The leader names them and sets, per rank, a list of rights:
  invite or remove, promote or demote, guild chat and officer chat (see, speak), public and officer notes, message of the day, guild bank
  tabs (view, deposit, withdraw). Ranks cannot be reordered. A member has one rank (implied). You can only promote, demote or remove people
  of lower rank than yours; only the leader promotes to officer [read]. Bank limits per day [recalled, not read].
- Recovery: leaders absent 90 days (raised from 30 in a 2012 hotfix) can be replaced by a member of rank 2, 3 or 4 who opens a
  confirmation; the old leader is e-mailed [read, secondary]. Classic servers lack it [secondary]. The Blizzard article itself needs a login.
- Lessons: a linear rank list is easy to explain and cannot express "kick but not mute" (R3, R4); the default newcomer rank is a ready-made
  probation group for strangers (R1); a bounded takeover rule fixes the dead-owner problem Second Life still has (R8).

### 3.2 EVE Online corporation roles (EVE news post and forum thread [read]; incidents [secondary])
- Roles are separate checkboxes, with three places for hangar and container access (headquarters, based-at, other). "Grantable roles"
  are a second, parallel set: the right to give a role without having it. Director holds all and only the CEO can make one [secondary]. Titles
  bundle roles [read].
- Redesign: roles and grantable roles merged into one list with a three-state box (none, has role, may grant); access controls became one
  matrix; role setting went from 19 screens to 5 and hangar access from 12 to 1 [read]. Titles were not yet redone.
- Roles stay too coarse: a 2022 thread says accountant roles show every wallet, structure access lists are visible only to their creator and
  are lost when a structure changes corporation; no reply from the developer [read].
- Incidents [secondary]: 2008-10 theft of about 200 billion ISK; 2011 wormhole heist and a former director clearing a corporation; 2017 year-long inside
  job of about 1.5 trillion ISK; 2023 a shares mechanic used to become director, about 2.2 trillion ISK, which the developer confirmed took
  place. Lesson: the biggest harm comes from trusted insiders and from mechanics that bypass roles, not from checkbox mistakes.
- Lessons: copy the split of "may" and "may delegate" with a three-state box (R5, T8); give bulk grids and a text view early (R7); a very
  powerful single role is a target (R3).

### 3.3 Google Drive and Notion (help pages [read])
- Drive: viewer, commenter, editor, plus owner. Sharing is to named people or groups, or by link with a role you choose. By default editors
  may change permissions and share, and the owner can switch that off ("allow editors to change permissions and share"). A folder's
  permission cannot be undercut on a file inside. Access can carry an expiry date up to a year (work or school accounts; on folders only for
  viewers). A "manage access" panel lists who has access. Ownership transfer is a menu step. Up to 600 named people per file.
- Notion: full access, can edit, can comment, can view; plus database-only "can edit content" and "can create" (add entries without seeing
  others). Sub-pages inherit. Conflicts resolve to the BROADEST access (workspace or group grants beat a narrower page rule); there is
  no deny. Guests are people outside the workspace. Anyone-with-link can have an expiry. Access requests go to editors.
- Lessons: copy the separate "re-share" switch (R5), expiry (T3), a "who has access" panel (T7, R9), and write-without-read for drop boxes.
  Avoid "most permissive wins" with no deny if T5 and T13 matter.

### 3.4 OpenComputers (ocdoc.cil.li api:computer [read])
- A computer has a user list. `addUser(name)` registers an online player who then has "full access rights"; `removeUser` takes them off;
  when the LAST user is removed the computer becomes accessible to ALL players. Shell commands useradd and userdel call these. The page does
  not say who the owner is or give limits, and the linked "user rights" page was not read.
- Lessons: avoid "empty list means everyone" (R3: keep "everyone" as an explicit group, and empty means nobody); a per-device user list with
  one all-or-nothing level is the minimum viable version of R5.

## 4. Lessons for R1 to R9 (copy, avoid, adapt)

R1 identity, strangers
- Copy: store grants against the stable id (account key in SL, Discord, Roblox, Steam id in SE), show the name; renames then cost nothing
  (T5) [inference]. Luanti keys by player name [recalled]; that is the model to avoid.
- Copy: the default group is a real, editable group (SL "Everyone" role, Discord @everyone, WoW's lowest rank). A "newcomer" tier is a ready
  made probation (WoW Initiate).
- Avoid: bans that depend on accounts being costly; none of these systems has that with free identities.

R2 actors, scripts, command blocks
- Copy: SE's run-time recomputation from the owner; Roblox's "common part when nested" and "callback runs as its registrant".
- Change: ownerless should be the WEAKEST actor (SE's owner 0), not an anonymous operator, unless an op set it on purpose.
- Add: editing a marked function by anyone but its owner clears the mark, or the function runs with the common part of the owner's and last
  editor's power. No surveyed system does this; SE shows the gap and Roblox backdoors show the cost.
- Copy: SL's rule that power over a person needs that person's yes, scoped to one script.

R3 sets of groups, looked up every time
- Confirmed: union of roles (SL, Discord, Roblox); recomputation at every use (SE). Keep rank only for "who may manage whom"; keep the
  permission arithmetic free of order (Discord says as much). Protect a top group and its last member (SL Owners), but add recovery (R8).
- Avoid: "empty means open" (OpenComputers).

R4 grant and deny per command, subcommand, pack, tag
- Per tag: Roblox lists capability tags per engine call and names the missing tag in the error; keep tags to a few dozen (41).
- Per pack with per-command exceptions: Discord's top-level default plus per-command override, with an explicit "Not Synced" state.
- Deny: only Discord has it, with a fixed, documented order; others stay allow-only (simple, easy to predict, cannot do "all but one").
  If deny is wanted, fix its place in the order and print it. Suggest: more specific subject beats less specific; at one level deny beats
  allow (opinion, since Discord, Notion and SL all choose "most permissive wins" across groups and the owner may prefer that).
- Avoid: losing grants on rename; key grants by `pack:name` and report dangling ones.

R5 ownership and sharing, read, call, write
- Verbs kept apart (Roblox Use, Edit, Download, Own; Drive edit versus re-share); expiry (Drive); a "revoked" tombstone (Roblox);
  dependency flag (Roblox); write-without-read (Notion); per-object group sharing (SL, SE) is coarse and is not per-person.
- Remember that "write" on a script that runs as its owner is owner-level power (see R2).
- Lost owner: let the server root key always reassign, and offer a bounded succession like WoW's 90-day takeover.

R6 third-party packs, safe default
- Copy Luanti: the pack asks by name, the operator's file decides, default empty, one line to audit. Copy Discord: install shows the power
  list; one top-level grant covers all the pack's commands; the author's default (admin only) is shown.
- Avoid Roblox's default of full authority with opt-in sandbox.
- Adapt: the safe default must be visible, so a new pack's commands should land in a group named like "pack-pending", usable by ops only,
  and `grant pack:X to group:G` should be one command.

R7 typed commands and files
- A grid of roles by powers explodes in any interface (EVE 19 screens to 5). Offer a text dump that prints the grid, bulk forms
  (`grant pack to group`), and caps (SL: 10 roles, 42 groups; Roblox: 45 permission names) to keep it readable.
- Copy Luanti's single-line audit; copy Roblox's audit log (who changed what) [re-read].

R8 owner and root key, no central service
- Strong base: SE has Owner for the offline player and the server id; Discord's owner has everything. None of them has a recovery path that
  does not need a central service (Discord support, Roblox "cannot arbitrate", Blizzard support). A root key in a server file is a better
  base than any surveyed. Add a rule that root-key actions are always allowed and always logged.

R9 decisions on the server, explainable
- Best found: Roblox's refusal text (action, target, first missing capability); Discord's published algorithm; SL's chat notice for
  script grants. Worst: Luanti nil, SE greyed button, Discord view-as-role without the reason.
- Suggest: each refusal returns the deciding step (which rule, which group, which level). Discord's 8 steps would make it a number.

## 5. Open questions and what a second pass should check

1. Space Engineers: does the server also check block access when a program is replaced or run? (read the terminal and network layer; the
   2022 snapshot may be stale). This decides whether the gap in 1.1 is real or only in the lines I read.
2. Roblox: which wins between a callback registered outside and the rule that a sandboxed caller cannot call a larger container (beta text
   in tension).
3. Discord: how user, role and channel command overrides combine (not in the text I read).
4. Second Life: per-parcel script limits (no current page found); what happens to script permissions on reset, rez and transfer; the
   viewer's "Debug Permissions" tool; whether a group "active tag" is still needed.
5. OpenSim: read the real config file and docs for the `Allow_` syntax (blocked here).
6. Luanti: the exact attack in CVE-2026-40960 and whether the second commit covers HTTP; the old advisory text.
7. "Nothing like it exists" for per-variable grants is a weak claim: I used 15 searches and none were aimed at per-variable grant
   systems outside games (databases, spreadsheets, object stores).

## 6. Source list

[re-read] raw text, public repositories, read-only contents API:
- github.com/discord/discord-api-docs: developers/topics/permissions.mdx, developers/interactions/application-commands.mdx,
  developers/topics/oauth2.mdx, developers/interactions/receiving-and-responding.mdx
- github.com/Roblox/creator-docs: content/en-us/scripting/capabilities.md, scripting/security/third-party-vulnerabilities.md,
  scripting/security/access-control.md, projects/groups.md, projects/collaboration.md,
  reference/cloud/asset-permissions-api/v1.json, reference/engine/enums/PackagePermission.yaml, enums/CommandPermission.yaml,
  classes/PluginCapabilities.yaml
- github.com/luanti-org/luanti (master): doc/lua_api.md, builtin/settingtypes.txt, src/script/cpp_api/s_security.cpp
- github.com/KeenSoftwareHouse/SpaceEngineers (last commit 2022-01-06): the files listed in 2.2

[read] summarising fetch tool: wiki.secondlife.com (LlRequestPermissions, Group, Permission, LlGetObjectPermMask, Estate_Manager,
LSL_Script_Memory); list-archives.secondlife.com sldev 2007-05 message; feedback.secondlife.com group-leadership-succession;
spaceengineers.wiki.gg (Programmable_Block, Multiplayer_Screen); guildorder.com space_engineers ownership page; warcraft.wiki.gg Guild;
eveonline.com news "Mosaic updates from Team Five 0"; forums.eveonline.com "Limitations of corporation roles and permissions" (2022);
support.google.com/drive/answer/2494822; notion.com/help/sharing-and-permissions; ocdoc.cil.li/api:computer; arxiv.org/abs/1504.03747;
CVE-2026-40960 record (db.gcve.eu); github.com/luanti-org/luanti commit 0faf529.

[secondary] search snippets and summaries: Second Life scams and refund-permission pages and 2005 to 2007 forum archives; Discord support
View as Role FAQ and community posts; Blizzard forum and Engadget 2011 posts on the inactive leader rule; EVE support "Roles and grantable
roles", EVE University wiki, PC Gamer and Engadget heist reports; Bonne et al. SOUPS 2017 (usenix.org); Steam discussion threads for
Space Engineers; Roblox developer-forum backdoor threads; Luanti content forum and advisory GHSA-44f7-f48m-vxcw; OpenSim osslEnable.ini
through a search summary.
