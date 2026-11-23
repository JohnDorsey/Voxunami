# Card 5: permission models, human-factors evidence and evaluation method (theory and method stream)

> Raw notes of research stream 5 of the permissions study: permission models, studies of people and evaluation method (AWS IAM, Cedar, Kubernetes RBAC, Zanzibar, Shiro, sudo, Deno, the confused deputy, Saltzer and Schroeder, a playtester protocol). Written by a research agent on 2026-11-23 and kept as it wrote it, except that local file paths were removed and two file names lost their code quotes. The survey that draws on these notes is `permissions-survey.md`; it says which claims were checked again. The tags are the agent's own: [re-read] and [raw] mean the source's own text was read, [read] a summary by the fetch tool, [secondary] a second-hand source, [recalled] memory, [inference] reasoning from cited facts.


Checked 2026-11-23. Desk research only. Searches used: 9 of the 20 allowed (queries in section 12). About 70 tool calls.
Nothing was installed and no repository was touched. The only files written are this card and scratch text extracts in `card5-work/` beside it.

## 0. How to read this card

Tags (the brief's):
- [re-read] the source's own text was read (a paper PDF converted to text with `pdftotext`, or a raw page fetched with curl).
- [read] read through the fetch tool, which summarises with a small model, so wording and numbers are the summary's.
- [secondary] search snippet, or a citation inside another paper.
- [recalled] from memory, not checked this round.
- [inference] my reasoning from cited facts, with a confidence word (low, medium, high).

Two process notes. (1) The Read tool cannot render PDFs on this machine (no `pdftoppm`), so I saved PDFs through the fetch tool and ran `pdftotext` on them. That is why the main human-factors papers are [re-read] and most vendor documentation is [read]. (2) Where I quote a number from a table in a paper I checked it against the abstract or the body text; I did not read any paper twice end to end.

Nothing here comes from a study of a game server. Every human-factors study below used file permissions, firewalls, phones or home file sharing, with students or office workers. How far each carries over to a game admin typing commands is my judgement, flagged where I make it.

## 1. What matters most (ten findings)

1. **The precedence rule changes how often people get it right, and one controlled study measured it.** [re-read] In "More than Skin Deep" (CHI 2011) 54 students, none of whom had been an administrator, did file-permission tasks on a simulated server of about 500 users and 500 files. With the interface held the same, changing only the conflict rule moved accuracy by 50, 38, 55, 22, 78 and 50 percentage points across six tasks (five of them statistically significant). "The narrower rule wins; deny decides only between peers" did best when the author had to fix a conflict, because the fix was a small local rule. It did worst (50% against 88%) on the one task where a group-level deny had to beat a person's own allow. Details in 2.2; consequences for R4, T5 and T13 in section 11.
2. **Show the effective result, not only the rules.** [re-read] Two CMU studies (24 and 36 participants) found that a dialog showing who ends up with what lifted success from 25% to 100% on one task, and from 0% to 89% on a conflict task, compared with the Windows file-permissions dialog. The typical failure was a person who believed they had finished while a conflicting rule still refused access.
3. **Every large system that has a deny makes it the strongest rule and independent of order.** [read] AWS IAM, Cedar, Google Cloud and Azure. Kubernetes RBAC and Apache Shiro have no deny at all. Order-dependent lists survive in file-system ACLs and in sudo ("the last match wins") [re-read] and in firewalls, where errors rise with rule-set complexity [secondary].
4. **"Everything except X" is where systems hurt people.** [re-read, read] The sudo manual says subtracting a command from ALL is trivially bypassed (copy it under another name). AWS warns that Allow with NotAction can grant more than intended. Kubernetes does not offer it. For T13 the lesson is to attach the denial to the action that finally happens, not to the name that was typed.
5. **Delegation has two proven ceilings and each has a documented hole.** [read] Kubernetes: you may hand out only what you hold, unless you also hold the special `escalate` or `bind` verbs. AWS: a boundary policy fixes the maximum for people you delegate to, and AWS's own example notes that the delegate can still create users who reach things the delegate cannot.
6. **Authority exercised on behalf of someone is where escalation happens.** [re-read, read] Hardy's confused deputy (1988); Kubernetes treats "may create a workload" as holding the rights of every service account the workload can name; Deno says its `run` permission is equal to everything. A function that runs with its owner's power and takes a name from its caller has the same shape.
7. **Three different questions need three tools.** [read, re-read] "Why was this refused?" (AWS denial messages, Cedar diagnostics, the AWS simulator), "what can X do?" (Kubernetes rules review, `can-i --as`) and "who can do Y?" (Zanzibar's Expand; no built-in tool in Kubernetes). AWS names only one policy type in a denial message even when several deny; Cedar lists every determining policy.
8. **Attach grants to the key, not the name.** [read] Kubernetes warns that a new account reusing a deleted user's name inherits that user's rights. Nothing in the permission literature stops a banned person returning with a fresh key (R1); that is an abuse-handling problem (join gates, earned trust) outside permission models. [recalled]
9. **For third-party packs the safe default is "nothing", and granting should be one step.** [read] AWS classifies every action as List, Read, Write, Tagging or Permissions management, so an admin can grant a class. Kubernetes lets a pack ship a role fragment that the built-in roles pick up by label. Both leave the decision with someone other than the admin (AWS: the service author labels; Kubernetes: the pack author opts in). Wildcards also cover things added later.
10. **Test by outcome, with small groups, in several rounds.** [read, re-read] The CMU studies scored the final state against the goal and counted extra changes as wrong. Nielsen's formula says five users find about 85% of problems if each user finds about 31%; three rounds of five beat one round of fifteen. Different evaluators find very different problems (any-two agreement 5% to 65% across eleven studies [secondary]), so use two coders.

## 2. What studies of people say (assignment part a)

### 2.1 The studies, with numbers

| Study | What it measured | Who, how many | Key numbers | Tag |
|---|---|---|---|---|
| Maxion and Reeder 2005, "Salmon" (Int. J. Human-Computer Studies 63; reproduced as chapter 3 of Reeder's thesis) | success rate, error types and time on 3 permission-setting tasks in the Windows XP file-permissions dialog (XPFP) versus Salmon, a dialog that shows effective permissions up front | 24 (12 per dialog), technical students, think-aloud, between-subjects | XPFP success 58%, 25%, 75% against Salmon 83%, 100%, 100%. Goal errors (not knowing what to do) fell 80%, 94% and 100% on the three tasks. About 3 times faster on one task. Goal errors were the dominant error type and were traced to the relevant information not being shown | [re-read] |
| Reeder et al., CHI 2008, "Expandable Grids" | accuracy and time on 20 tasks (view, change, conflict, compare groups, detect an unauthorised group as in the "Memogate" case, precedence), small and large policies; a matrix view (the Grid) versus the Windows XP dialog | 36 (18 each), technical students who had never been administrators | 27 tests, 14 significant, all in favour of the Grid (6 on accuracy, 8 on time). Small-scale accuracy Grid v Windows: view-complex 94% v 17%; change-complex 61% v 0%; conflict-complex 89% v 0%. Large-scale: 100% v 39%; 67% v 17%; 100% v 6%. No significant difference on the simple tasks. The Grid slowed down more than Windows as policies grew | [re-read] thesis ch. 6; paper abstract [read] |
| Reeder et al., CHI 2011, "More than Skin Deep" | accuracy on six tasks under three conditions (below) | 54 (18 per condition), technical students, never administrators, think-aloud, Latin-square order | five of six accuracy differences significant; gaps between the two conflict rules of 50, 38, 55, 22, 78, 50 points; time results mostly not significant | [re-read] |
| Wool, firewall configuration errors (IEEE Computer 2004; revisit arXiv 2009, IEEE Internet Computing 2010) | count of 12 objectively defined configuration errors per rule set against a complexity measure (rules + objects + interfaces) | 37 Check Point rule sets (2004); 84 rule sets of two vendors (2010) | errors rise with complexity; most rule sets poorly written, a few good; the revisit found the same and no sign that newer software versions had fewer errors | [secondary] for 37, 84, 12 and the measure; [read] for the revisit's abstract |
| Bauer et al., CHI 2009, "Real life challenges in access-control management" | interviews about managing physical and file access control | 13 policy professionals in 5 organizations | three challenge areas; one is that systems cannot always express the policy wanted. The other two were not in the snippet I saw | [secondary] (full paper fetch failed) |
| Mazurek et al., CHI 2010, home data sharing | interviews plus hypothetical scenarios about who may see which files | 33 users in 15 households | people build ad hoc mechanisms that do not always work; wanted policies are complex and multi-dimensional (who is present, where, which device) and are not only role-shaped; writing the policy in advance is often insufficient and people want to adjust it in reaction to requests; mental models are misaligned with current systems | [re-read] abstract and key findings |
| Felt et al., SOUPS 2012, Android install-time permissions | attention, comprehension and behaviour at the install screen | Internet survey 308; lab 25 (24 in the attention breakdown) | 17% paid attention to permissions (both studies); 42% of lab participants did not know permissions existed; 3% of survey respondents answered all three comprehension questions correctly; 24% of lab participants showed competent but imperfect understanding | [re-read] |
| Wijesekera et al., USENIX Security 2015, runtime permissions field study | how often apps used protected resources, and what people would have blocked | 36 participants, 27 million logged data points | at least 80% of participants would have blocked at least one request; participants wanted to block over a third of all requests | [read] abstract page |
| Role engineering (no controlled study found) | cost and growth of roles | | illustrative only: an organisation of a thousand people can end with thousands of roles; identifying roles is cited as the costliest step in adopting role-based control (a NIST-commissioned study, cited second-hand in a 2007 IBM paper); I found no published before-and-after role count for a named organisation | [secondary] |
| "Memogate", US Senate Judiciary Committee (report dated March 2004) | incident | | confidential memos on a shared NTFS server were reachable by the other party partly because an inexperienced administrator set permissions wrongly (as summarised in Reeder's thesis, which cites the Senate Sergeant at Arms report) | [re-read] thesis text; the report itself not read |
| Cloud misconfiguration; AWS policy syntax | an NDSS 2026 mixed-methods study of cloud configuration challenges (found authentication and access control the most pervasive); a University of Waterloo thesis with a 20-participant test of a simpler read/write/execute policy syntax against AWS's | | snippets only | [secondary] |

What none of these studies did: test a game server; test a typed-command interface; test months-later recall of intent (Reeder et al. say this is hard to simulate in a lab [re-read]); use participants who were experienced administrators (the CMU participants were deliberately novices or occasional authors).

### 2.2 The precedence study in detail (the most relevant single paper)

Setup [re-read]: a hypothetical music-department file server; a teaching-assistant scenario; users and groups as who, folders and files as what. Three conditions: GS = Expandable Grid with the authors' "specificity" rule; GW = the same Grid with Windows' rule; WW = the Windows XP dialog with Windows' rule. Because GS and GW share the interface, their difference is the rule alone. A task counted as correct only if the final effective permissions matched the goal and no extraneous change was made.

The rules, as the paper describes them [re-read]:
- Windows: when two conflicting rules differ only in the resource, the more specific resource wins; when they differ only in the principal (user versus group, or two groups), deny wins; when they differ in both, the more specific resource wins.
- Authors' rule: the rule on the more specific principal or resource wins, in both dimensions; deny wins only when specificity cannot decide (peer groups, or a rule more specific in one dimension and less in the other).

| Task | Structure and goal | GS | GW | WW |
|---|---|---|---|---|
| Charles | user allow against group deny, goal: keep the allow | 0.56 | 0.06 | 0.00 |
| Kent | the same structure, goal: keep the deny | 0.50 | 0.88 | 0.78 |
| Lance | two-dimensional conflict, goal: deny | 0.94 | 0.39 | 0.00 |
| Adria | two-dimensional conflict, goal: allow | 0.61 | 0.83 | 0.39 |
| Jana | two groups (peers), one allows and one denies, goal: allow | 0.89 | 0.11 | 0.44 |
| Pablo | the same structure as Jana | 0.72 | 0.22 | 0.39 |

How the authors read it [re-read]: whichever rule gives the intended answer by default wins that task; where the default is wrong, the rule that lets the author fix it with one local, additive change wins. Under deny-wins the Jana fix needs a group-level change (remove the deny that others rely on, or remove Jana from the group), and many participants set a personal allow, saw nothing change, and believed they were done because effective permissions were three clicks away in the Windows dialog. Their own stated risk: with specificity, a group-level deny does not override a person's earlier personal allow, so unauthorised access can persist unnoticed (the Kent task); they argue a view of effective permissions mitigates it. They also say an unusable model is itself a risk, because frustrated people set looser policies or bypass the system.

Limits [re-read]: minutes-long lab tasks; novices; file-system shapes only (users, groups, folders); one task pair per structure; the authors do not know how often each structure occurs in real life, so they cannot claim specificity is always better.

### 2.3 Reading the studies together [inference, medium confidence]

- The commonest error is not a typo but a wrong picture of the result (goal errors; "I thought I was done"). Tools that show the result remove more errors than any syntax choice.
- A rule is usable when the common intent is the default and the exception is a local change. Which rule that is depends on how admins think: "this player is an exception to the group" is more natural than "remove the group's deny".
- Errors grow with the size and tangle of the rule set (firewalls). Keep the number of rules, layers and precedence rules small, and give a checker that finds shadowed, redundant and conflicting rules.
- People's real policies are contextual and change in reaction to requests (home study). A request-and-approve path ("Alice asked for /warp: allow once?") matches how people actually work.
- Install-time lists are mostly not read (Android 2012); a per-use prompt is not a cure either (the 2015 field study found people would have blocked over a third of the requests they saw logged) [inference]. Prompts for scripts should be rare and in context.

### 2.4 Role engineering [secondary, low confidence on numbers]

The search found the claim, not hard data: role count grows when each unique access need becomes a role (a bank example in one source: role times location gives 9 roles, a reusable decomposition 6); roles are rarely deleted; duplicates are common; mining roles from existing permissions is a research field of its own. [secondary] My reading: the owner's R3 plan (sets of groups, a built-in op and everyone, roles and teams later as plain groups) avoids up-front role engineering. The risk is the cross-product (role x team x area). Grants to a (group, scope) pair, rather than a role per combination, keep the count linear. [inference, medium]

## 3. The models big systems settled on (assignment part b)

### 3.1 AWS IAM (documentation read 2026-11-23, [read] unless marked)

- Protects cloud resources from actions by principals (users, roles, role sessions, federated sessions). Everything is denied unless some policy allows it.
- Same-account identity-based and resource-based policies combine as a union; an explicit deny in either overrides an allow. A permissions boundary, an organization service control policy (SCP), a resource control policy (RCP) and a session policy each combine by intersection: the action must be allowed by all that apply. The page's rule in a phrase: an explicit deny in any policy type results in a denial.
- Exceptions to the "intersection" picture: a resource-based policy that names an IAM user's ARN is not limited by a boundary's implicit deny; the same holds for a role-session ARN. The exact step order of the evaluation flow chart is on a sub-page I did not read; I remember it as deny check, organization policies, resource-based, identity-based, boundary, session. [recalled]
- Delegation (worked example in the boundaries page): an administrator lets a helper create users only if the user is created with a named boundary (condition key `iam:PermissionsBoundary`), and denies the helper the right to edit or remove that boundary. The page itself notes that this lets the helper create users who can reach things the helper cannot, so the administrator effectively trusts the helper with that service.
- Explanation: a policy simulator that evaluates identity policies, boundaries, SCPs and resource policies you supply, makes no real request, and names which statement allowed or denied each result; it can differ from the live account and does not show matched SCP statements. Denial messages follow the pattern "not authorized to perform X because no [type] policy allows it" or "with an explicit deny in a [type] policy", sometimes with the policy's ARN. If several policy types deny, only one type appears; if several policies of one type deny, the count is not given; some services do not use the format.
- Classification: every action carries an access level (List, Read, Tagging, Write, Permissions management), assigned by AWS; the docs advise restricting or monitoring "Permissions management" because those actions control access itself.
- Foot-guns the documentation itself lists: Allow plus NotAction can grant more than intended (it matches everything not named, including later additions); a NotPrincipal with Deny always denies principals that have a boundary, so some people lose access they should have; condition key names are case-insensitive so two keys differing only in case can cause unexpected denial.
- Time-limited access: temporary credentials expire, and the permissions of a session are the intersection of the role's policies and the session policy passed when the session is made.
- Lesson [inference]: layered ceilings are powerful and every layer is one more place to look when a request is refused; AWS's answer is to name the layer type in the message.

### 3.2 Cedar (AWS's open authorization language; docs and paper abstract read, [read])

- Decision rule, in three steps: if any `forbid` policy is satisfied the result is Deny; otherwise if any `permit` policy is satisfied it is Allow; otherwise Deny. Policy order does not affect the decision.
- The response carries the decision and the "determining policies" (the satisfied permits for an Allow, the satisfied forbids for a forbid Deny, none for a default Deny), plus the IDs and details of any policy that errored.
- A policy that errors is skipped and does not decide anything; the documentation contrasts this with denying on any error (which would let one bad policy block everyone). [inference, medium] For a `forbid`, skipping means a broken guardrail stops guarding without stopping the server, the opposite of fail-safe defaults; the diagnostics report it, and the schema validator is what is meant to catch it earlier.
- The paper (Cutler et al., arXiv 2403.04651) says the language is meant to be ergonomic, fast, safe and analysable; it has a validator that uses optional typing; policies have a logical encoding so one can check, for example, that rewriting a policy set does not change what is authorised; the evaluator is modelled in Lean and written in Rust; it reports Cedar performing "far better" than OpenFGA and Rego, with no figures in the abstract. Properties proved in Lean were not enumerated in what I read.
- Lesson [inference]: the closest thing to a model designed to be explained and checked. Its cost is that there is no specificity, so "this person is an exception to a group's deny" means rewriting the forbid with an `unless` condition, not adding a local allow.

### 3.3 Kubernetes RBAC (docs read as raw markdown files, [read])

- Permissions are purely additive; there are no deny rules. A Role is namespaced and a ClusterRole is cluster-wide; bindings attach roles to users, groups or service accounts.
- Escalation prevention is enforced by the API itself: you can create or update a role only if you already hold every permission in it at the same scope, or hold the `escalate` verb on roles; you can create a binding only if you already hold the referenced role's permissions, or hold the `bind` verb on that role.
- Bootstrap: the group `system:masters` is bound to `cluster-admin` and is the credential for creating the first roles. Membership bypasses RBAC and any authorization webhook, and the good-practices page says it cannot be revoked through bindings.
- Aggregated ClusterRoles: labelling a ClusterRole `rbac.authorization.k8s.io/aggregate-to-admin`, `-edit` or `-view` makes the built-in role include its rules. This is how a new extension's resource types reach the standard roles without the admin editing them. The default roles are re-reconciled at start-up (missing permissions restored) unless the annotation `rbac.authorization.kubernetes.io/autoupdate` is set to false.
- Authorizer chain: authorizers run in the configured order and the first that approves or denies decides; "no opinion" passes on; if all have no opinion the request is refused (HTTP 403).
- Explanation tools: `kubectl auth can-i VERB RESOURCE` (a self-check by the API), `--as USER` to impersonate (admins), `--as system:serviceaccount:NAMESPACE:NAME` for service accounts, and review APIs: a self check, a check for any subject, a namespace-limited check, and a rules review that returns the set of actions a subject can perform in a namespace. A "who can do this" query is not in these pages; third-party tools exist. [recalled]
- Hazards listed by the good-practices page: creating a pod or workload grants the rights of any service account in the namespace and access to its secrets; `escalate`, `bind` and `impersonate` are escalation verbs; `get` on `nodes/proxy` is not read-only and bypasses audit; `list` and `watch` on secrets also expose contents; wildcards also cover resource types added later; a new account that reuses a deleted user's name inherits that name's rights; bindings to `system:unauthenticated` give access to anyone who can reach the server; review RBAC periodically for redundancy and escalation paths.
- Lesson [inference]: additive-only makes the model easy to predict, at the price that "all except X" cannot be said and that the real defence is a list of "escalation verbs" the admin must know. The idea worth copying is escalation prevention built into the API so the check cannot be forgotten by a plugin.

### 3.4 Zanzibar and relationship-based control (paper read as text, [re-read])

- Google's global authorization system (USENIX ATC 2019, Pang et al.). Data is a set of relation tuples written `object#relation@user`, where the user may itself be a userset `object#relation` (so a team's members can be named once and reused). Namespace configuration holds rewrite rules that make one relation imply another (the paper's example: viewers include owners) using union, intersection and exclusion.
- APIs: Read, Write, Watch, Check and Expand. Expand returns the effective userset for an object-and-relation as a tree, which the paper calls crucial for clients that need the complete set of users and groups with access: this is a "who can" query.
- Consistency: "zookies" (opaque timestamps) are passed with checks to avoid the "new enemy" problem, where a person removed from a group could still see content added just afterwards because the check used a stale copy of the rules.
- Scale (for context, not a design target): more than two trillion access control lists, millions of checks per second, 95th-percentile latency under 10 ms, availability above 99.999% over three years.
- Lesson for sharing (R5, T10) [inference]: store grants as data tuples, make teams usersets, let object types declare which relations imply which, and answer both "check" and "expand" from the same data. On a single server the consistency machinery is unnecessary as long as every check reads current state (complete mediation).

### 3.5 Apache Shiro permission strings ([read])

- A permission states what can be done, not who can do it. Colon-separated parts, comma lists inside a part, `*` for all values; trailing parts may be omitted and mean "all" (so `printer:print` equals `printer:print:*`). A third part names an instance (`printer:print:epsoncolor`).
- Granting and checking are asymmetric: grants may be broad, checks should be as specific as possible; checking `printer:print` demands access to all printers. Matching is by implication, not equality.
- No deny or negation is described. Documented pitfalls: separators in user-supplied IDs change the meaning of a grant (an empty trailing ID can broaden it to all instances); matching is case-insensitive by default; under legacy parsing a trailing separator is silently dropped (so `printer:print:` grants every printer); strict parsing becomes the default in Shiro 3.1.
- Lesson [inference]: a segment-and-wildcard string syntax is compact and familiar to plugin authors, but every place a name enters a string needs escaping, and the parser should reject empty parts rather than reading them as "all".

### 3.6 Others in brief

| System | One-line model | What to take | Tag |
|---|---|---|---|
| Google Cloud IAM deny policies | deny policies are checked before allow policies; a deny always wins except for principals or permissions listed as exceptions; they are inherited down organisation, folder and project and a deny lower down cannot cancel one higher up; a condition that cannot be evaluated counts as true (the deny applies); conditions may reference only resource tags; limit of 500 policies and 500 rules per resource; changes are eventually consistent | an exceptions field is the usable way to say "deny, except these"; unknown means deny; documented propagation delay (T12) | [read] |
| Azure RBAC deny assignments | block actions even where a role assignment grants them, support excluded principals and "do not apply to child scopes"; customers cannot create them directly (customer-managed ones were in private preview on a page dated 2026-11-05); Azure creates them for protected resources | a deny that only the platform can write cannot be mis-layered by admins, at the cost of "all except" being expressed another way | [read] |
| Windows file ACLs | the system walks ACEs in order until the request is decided; the preferred order is explicit entries before inherited ones, and within each group deny before allow, nearer ancestors first | order carries meaning; an explicit allow on a child is evaluated before an inherited deny from a parent. [inference] consistent with CHI 2011's description of "specificity on resources" | [read] |
| sudo | the last matching entry wins, not the most specific; subtracting commands from ALL with `!` is "ineffective"; `sudo -l` lists what you may run (that meaning is [recalled]) | order-dependence plus subtraction is the combination its own manual warns against | [re-read] |
| Deno (a scripting runtime) | sandboxed by default; `--allow-read=./data` style scoped grants; `--deny-*` overrides `--allow-*`; prompts on a terminal, else a catchable `NotCapable` error; `--allow-run` and `--allow-ffi` are to be treated as `--allow-all`; `Deno.permissions.query/request/revoke` | the nearest model for player-written scripts with declared, scoped permissions and revocation at run time | [read] |
| Open Policy Agent (Rego) | policies are code that return data; decision logs and an explain mode exist; no built-in precedence | default deny and "deny wins" are conventions the author writes, not features | [recalled] |
| Java Security Manager | a fine-grained permission-policy system for code, deprecated for removal around 2021 for being rarely used and costly (JEP 411; my fetch of the JEP failed with HTTP 403) | the fine-grained, file-configured permission system that went unused is the negative example | [recalled] |

### 3.7 What the models share [inference, medium-high]

Default deny; deny, where it exists, is the strongest and order-independent; ceilings (boundaries, SCPs, escalation checks) are separate from grants; a "can X do Y, and why" tool exists next to the model, not later; wildcards are for grants and are dangerous in checks and in "all except"; and none of them uses one rule for everything, they layer a small number of rules. The designs that face ordinary admins directly (file ACLs, sudo) are the ones whose ordering and negation rules are documented as traps.

## 4. Capabilities and the confused deputy (assignment part c)

### 4.1 Hardy 1988, from the author's own page ([re-read], curl of the page text)

A compiler at a timesharing company was allowed, by a "home files license", to write files in its own directory, including a statistics file. A user named the billing file (in the same directory) as the place for the compiler's debugging output. The compiler passed the name to the operating system, which allowed the write because of the compiler's own license, and the billing data was lost. Hardy's diagnosis: the compiler held authority from two sources (its invoker and its own license) and had no way to say which one it meant for each file. A "switch hats" system call was tried and made things more complex; he notes the rules for opening a file had grown to fourteen boolean operators. The capability answer: the compiler holds a direct handle to its statistics file and uses the handle the invoker supplies for the output, so the handle both names the file and authorises the write, and no text name is needed. He also challenges the reader to show that access lists do not solve this.

### 4.2 Object capabilities and Capsicum ([recalled], not read this round)

Miller's work (the 2003 "Capability Myths Demolished" with Yee and Shapiro; the 2006 thesis "Robust Composition") describes the object-capability model: no ambient authority, a reference is the permission, only connectivity creates connectivity, revocation by a forwarding "caretaker" that can be switched off, and least authority as the working rule. Capsicum (Watson et al., 2010) adds a capability mode to FreeBSD in which a process loses access to global names and keeps only handles. The relevant idea: authority is passed explicitly with the request, and a sandboxed component holds only what it was given.

### 4.3 The same shape in modern systems

- Kubernetes: the right to create a workload is the right to use any service account in that namespace. [read]
- Deno: running a subprocess escapes the sandbox, so `--allow-run` equals everything. [read]
- sudo: a command list for someone who has ALL is advisory. [re-read]
- AWS: passing a role to a service, and the documented "confused deputy" cross-service case, handled with source-account and source-ARN conditions. [recalled]

### 4.4 What this means for command blocks and shared functions (R2, R3, T11) [inference]

Options for "whose power does a shared function run with":

| Option | What happens | Advantage | Drawback |
|---|---|---|---|
| A. The caller's power (the owner's default in R2) | no amplification | cannot be a deputy; nothing to audit beyond the caller | a helpful public function (a spawn kit, a home command) cannot do what the caller may not |
| B. The owner's power, when the owner marks the function | setuid style | public services are possible | confused deputy whenever the function acts on a name or place its caller supplied; the owner's whole power is exposed |
| C. Intersection of caller and owner (Java stack inspection [recalled]) | both must hold the right | safe | cannot offer a service the caller lacks the right to, so it removes the point of B |
| D. The owner's power limited to a declared ceiling (AWS session policy, Deno flags, Android manifest) | effective power = owner's current power intersected with what the function declared | marked functions stay small; the declaration can be shown to the caller and the admin | needs a declaration format and a check that the declaration is honest |
| E. The caller passes a handle for the target (capability style) | the function holds no power of its own over targets | removes the confused deputy | heavier to build; needs handles for players, regions, ships |

My view: keep A as the default; offer D for marked functions with the call-site recording "on behalf of CALLER"; treat E as the direction to move if scripts grow. Medium confidence; no usability study compares these.

Also, [inference, medium-high]:
- R3 ("looked up every time") is the caretaker pattern: the check goes through current state, so demoting a block's owner shrinks the block at once. The side effect is a block that quietly stops working, so the refusal must say "owner X lacks Y".
- An ownerless block that runs as an anonymous operator is ambient authority at its widest. Least privilege points the other way: ownerless means no power, or a named "automation" group with a short list.
- A function that takes a player name, a position or a variable name from its caller must check what the caller may do to that target, not only what the owner may do. That is Hardy's lesson in one sentence.
- "Who can create or edit the automated thing" is itself the privilege to gate (Kubernetes workload creation; a shared function that an op's command block calls).

## 5. Saltzer and Schroeder (1975) applied to a game server (assignment part d)

The ten principles and a one-line gloss are from the paper's own page ([read], web.mit.edu/Saltzer/www/publications/protection/Basic.html). The right-hand column is my application [inference, medium confidence].

| Principle | What it says | What it implies for the game server | Touches |
|---|---|---|---|
| Economy of mechanism | keep the design small and simple so flaws are findable | budget the precedence rules: Cedar has one (forbid beats permit); the CHI 2011 Windows rule needed a 3-by-4 table of cases. Aim for at most two rules that fit on one card | R4, S1 |
| Fail-safe defaults | base access on permission, so the default is none | a new pack's commands go to nobody (or op only); a rule that fails to parse must never become "allow", and a broken deny should stop the load, not be skipped (contrast Cedar's skip-on-error); unknown group name means no grant | R6, R4 |
| Complete mediation | check every access, including during setup, recovery and maintenance | console, command blocks, scheduled scripts, function calls and chat-triggered actions go through the same check, with no op fast path that skips it; caches must be invalidated when a rule changes (R3 already says "looked up every time") | R2, R3, R9 |
| Open design | do not depend on secrecy | the algorithm is documented in `help` and in the admin guide; the rule file is readable by admins | R9 |
| Separation of privilege | two independent conditions before access | changing permissions, creating ops, marking a function "run as owner" and installing a pack need a grant and a second condition (the console, or a confirm step, or a second admin) | T8, T9, R2 |
| Least privilege | give each program and user only what the task needs | a moderator is an enumerated list; a command block or script gets a declared ceiling; ownerless blocks get nothing or a small "automation" group, not operator | T2, T11, R2 |
| Least common mechanism | shared mechanisms are paths between users | shared functions and the variables they keep are shared mechanisms: keep per-caller state by default and make sharing an explicit grant | R5, R2 |
| Psychological acceptability | the interface must match the user's mental image, or they translate and err | "moderators may kick but not teleport" should be sayable in about that form; a model that cannot say "all except" pushes admins into enumerations they get wrong; the CHI 2011 result is the best evidence that the model, not only the interface, decides this | R7, T13 |
| Work factor | compare the cost of beating the mechanism with the attacker's resources | a fresh key costs a griefer nothing (R1), so a ban is a speed bump, not a lock; safety must come from default-low trust for new keys, not from bans | R1, T5 |
| Compromise recording | reliably record that a breach happened, which can substitute for prevention | an append-only log of every permission change (who, what, when, from where) and of every privileged execution with "on behalf of"; this is also what makes T12 and T4 answerable | T12, R9 |

## 6. Evaluation methods suited to this study (assignment part e)

| Method | What it is | Good for | Cost | Known limits | Evidence |
|---|---|---|---|---|---|
| Cognitive walkthrough | an evaluator walks the correct action sequence for a task and asks, at each step: will the user try to achieve the right effect; will they notice the right action is available; will they connect it with the effect they want; will they see progress after doing it (Wharton, Rieman, Lewis, Polson 1994) | before any participant; finds hidden commands, wrong vocabulary, missing feedback | a few hours, two people | finds only what the evaluators can imagine; evaluators disagree | questions [recalled]; evaluator effect [secondary] |
| Heuristic evaluation | three to five evaluators inspect alone against a list, then merge | broad cheap pass | hours | each evaluator finds a different subset: across eleven studies of walkthrough, heuristic evaluation and thinking aloud, average agreement between any two evaluators was 5% to 65% (Hertzum and Jacobsen; attribution recalled); a review of 33 studies put the median at 27% (range 4% to 75%) | [secondary] |
| Heuristic lists for admin tools | Nielsen's ten heuristics; the IT-security-management heuristics of Jaferian et al. (SOUPS 2011, built on activity theory, tested against Nielsen's on a commercial identity management system: the two sets found different kinds of problems, so they recommend using both); Whitten and Tygar's five "problematic properties of security" | admin interfaces where several people cooperate | | | Jaferian [secondary]; Whitten and Tygar's five (unmotivated user, abstraction, lack of feedback, barn door, weakest link) [re-read] in Reeder's thesis chapter 2 |
| Task-based user test with outcome scoring | participants do realistic tasks; score the final state against the goal | the thing the CMU studies did; catches false belief of success | days | small groups find usability problems but not small effects | [re-read] |
| Think-aloud | participants narrate while working (Ericsson and Simon's instructions) | why they do what they do | free with the above | can change behaviour and slow work, so do not use the session for fine timing | the CMU studies used it [re-read] |
| Hesitation analysis | pauses in the action log, over a threshold, mark probable interface defects (Reeder and Maxion 2006) | cheap, objective, works from logs | low | threshold needs a pilot | [recalled] |
| Questionnaires (SUS, one-question ease rating) | a ten-item usability scale scored 0 to 100; a seven-point "how easy was this task" | comparing rounds | minutes | blunt | [recalled] |
| Follow-up diary or re-explanation | ask testers days later to explain their own configuration | memory of intent | modest | small numbers | the CMU authors name this as the thing a lab cannot recreate [re-read] |

Numbers for planning [read, NN/g article on small tests]: the share of problems found with n users is N(1 - (1 - L)^n); L is about 31% on average; five users find roughly 85%; three rounds of five improve a design more than one study of fifteen, because fixes introduce new problems; if there are several quite different user groups, test each (3 to 4 per group for two groups, 3 per group for three or more); quantitative usability metrics need about 20 users. The CMU studies used 12 to 18 per condition and could only detect large effects (20 to 90 points).

How the CMU studies scored (copy this) [re-read]: correct means the final effective permissions match the goal and no extraneous change was made; the experimenter sets up each task in a prepared state; task order is counterbalanced with a Latin square; participants think aloud; accuracy is the main measure and time is secondary.

## 7. Ready-to-run protocol for playtesters

A protocol for testing a permission prototype with game players. Everything in this section is my design from the sources above [inference]; the thresholds are suggestions, not literature.

### 7.1 What it can and cannot tell you

It tells you whether ordinary admins can do the thirteen jobs on your prototype, how often they believe they did it while the result is wrong, where they hesitate, and which words or defaults mislead them. It does not tell you whether the model is secure (that needs a design review and attack tests) or whether people remember their intent a month later (add the diary follow-up in 7.11).

### 7.2 Before any participant (about two hours)

1. Walkthrough: two people, tasks T1 to T13, the four questions at every step. For typed commands, "notice the action is available" means "can it be found with help, completion or a suggestion". Write a failure story for every "no".
2. Heuristic pass: each person alone with Nielsen's ten, the admin-tool heuristics and Whitten and Tygar's five; then merge. Fix what is obvious before spending participants.
3. Build the oracle: a script that, for each task, runs a fixed set of probe actions as each named test actor and compares allowed or refused with the expected table, and diffs the effective permissions of everyone the task did not mention (extraneous change). Score by behaviour, not by reading the participant's config, because different configurations can give the same behaviour.
4. Prepare a saved server state per task (same names, same traps) and a reset command.
5. Pilot with one person outside the project; repair unclear task wording.

### 7.3 Participants

- Groups: A = has administered a game server or used a permission plugin; B = plays but has never administered. Optional C = people who were handed a delegated role.
- Five per group per round, up to three rounds, fixes between rounds (the NN/g advice above). People do not repeat across rounds because they learn. Do not use friends who know the design.
- If you ever compare two designs by numbers, plan at least 18 to 20 per condition; at 18 per condition the CMU studies saw only effects of 20 points or more.

### 7.4 Session (60 minutes, one facilitator, one note-taker)

| Time | What |
|---|---|
| 0 to 5 | greeting, recording consent, the script ("we test the system, not you"), think-aloud instructions |
| 5 to 10 | warm-up task that is not about permissions (teleport yourself, place a block), to practise typing commands aloud; teach nothing about permissions |
| 10 to 45 | six task cards (about 5 minutes each; stop at 6): the core set T1, T2, T4, T5, T6, T13 for everyone; in later rounds rotate in T3, T7, T8, T10, T11, T12 two at a time so each extra gets about 5 participants over three rounds |
| 45 to 52 | prediction items (7.6) |
| 52 to 60 | debrief questions, T9 as a spoken question, usability scale |

### 7.5 Task cards

Wording rules: describe the goal and the people in game terms; give a reason; never use the system's own words (deny, grant, group, node, role) unless the participant uses them first. Each card has a starting state, an oracle and, in half of them, a seeded trap.

| ID | Card text (what the participant reads) | Oracle: pass if | Seeded trap |
|---|---|---|---|
| T1 | "Your friend Alice wants to help build the harbour. She may build and break blocks inside the harbour and nowhere else. Nobody else may." | Alice changes blocks at 3 probe positions inside and not at 3 outside; Bob cannot inside; nothing else about Alice changed | Alice is already in a visitors group that forbids building |
| T2 | "Dana will help moderate. She can kick, mute and ban troublemakers. She must not be able to change anyone's game mode or teleport anyone." | kick, mute, ban on Bob allowed; game mode and teleport (self and others) refused; cannot ban the owner; cannot edit permissions | a pack provides a second command that also changes game mode |
| T3 | "Let Eli use the time command for one hour, then it should stop by itself. Nothing else for Eli." | allowed at minute 59, refused at 61, nothing else changed, nobody had to clean up | none |
| T4 | "Fay says the warp command does not work for her. Find out why and tell me which rule is responsible. Change nothing yet." | participant names the correct rule and makes no change | a team rule forbids it and a personal rule seems to allow it |
| T5 | "Bob has been griefing. Take everything away from him and keep him out. He will rename himself and come back." | after rename and reconnect (scripted) the same key is refused and holds no grants; in the debrief the participant says what a brand-new key can do | Bob also has a personal allow for one command in addition to his groups |
| T6 | "You installed the Markets pack with 15 new commands. Until you say so nobody but you may use it. Then let the traders use all of it except the reset command." | before: all 15 refused to others; after: traders allowed 14 and refused reset; a 16th command added by a pack update is refused to traders unless the card says new ones should be included | the pack's help text suggests one command is "safe for everyone" |
| T7 | "Who can use the fill command right now? Separately: what can Cy do?" | both lists equal the oracle's exact sets, including inherited ones; no change made | one person gets fill through a team and another through a personal grant |
| T8 | "Gus runs the Red team. Let him add and remove Red members and nothing else. Check that he cannot make anyone an admin." | Gus changes Red membership; cannot add anyone to the admin group, edit rules, join other teams, or give Red more than it has | Gus is also on a team that has a broader right |
| T9 | (spoken) "This server is brand new. Who is the admin? Now suppose the only admin lost their key. What would you do?" | names where the root key is recorded and a recovery procedure that needs no one else's permission | none |
| T10 | "Make a team of three, Hal, Ida and Jo, who can all use and edit the cargo ship Argo and its script data. Nobody else. Later Ida leaves and Kai joins." | first three allowed and outsiders refused for use, edit and data; after the change Ida refused, Kai allowed, the ship's own settings unchanged | Ida also has a personal grant on the ship |
| T11 | "Place a command block that gives new players a welcome kit. It should have only the power it needs. Later tell me who it runs as, and switch it off without deleting it." | the kit is given; a probe that makes the block run a forbidden command fails; the participant states the right identity; after switching off the block fails with a readable reason | the block's owner is an admin |
| T12 | "Let Lee build. Check that it works right now, without restarting. Then take it back. Then tell me who changed the rules last and when." | immediate effect proved by a probe; after undo the effective permissions equal the starting state; the participant finds the change record | a second admin changed something just before |
| T13 | "Let builders use everything tagged building except the fill command. Check it with a test." | fill refused to builders by every route available in the prototype (typed name, alias, wrapped inside a function or command block); the other tagged commands allowed; a newly tagged command allowed if the card says so | an alias for fill exists |

### 7.6 Prediction items (reading, not writing)

Give a short rule file and ask: "Can Dana use the fill command in the nether? Which rule decides?" Six items, each with one conflict type: (a) two groups with opposite rules; (b) a personal allow against a group deny; (c) a broad grant with a narrow exception; (d) a tag-wide grant with one command denied; (e) a team rule and a personal rule on a shared object; (f) a grant that has expired. Score only if both the answer and the deciding rule are right. Run half of the items with the system's explain tool available and half without (order randomised), which measures how much the tool helps.

### 7.7 What to record

Screen and audio; the prototype's own command log with timestamps; the configuration before and after (diff); the oracle result; number of commands, of help or list commands, and of explain commands; time to first correct action and to done; hesitations (a pause of ten seconds or more between actions; set the threshold after the pilot); the hint level used; the participant's confidence (1 to 5) in answer to "did it work?" before the oracle is run; a one-question ease rating (1 to 7); and each error classified as: slip (typing), wrong model (misread a rule), omission (did half), over-grant (gave more than asked), lock-out (removed own power), wrong target (confusable names), wrong tool (file versus command). Take verbatim quotes of expectations ("I expected it to ...").

### 7.8 Scoring

- Outcome per task: 2 = oracle passes, no extraneous change, no hint; 1 = oracle passes but a level-1 or level-2 hint was used, or a harmless extra change; 0 = oracle fails, a level-3 hint was used, or time ran out. A separate flag X = unsafe result (more access than asked, or a lock-out), whatever the number. Calibration failure = confidence 4 or 5 with outcome 0 or X.
- The brief's five scores: S1 predictability = share of prediction items right (answer and rule); S2 explainability = share of T4 and T7 done correctly with the system's own tools; S3 safety = X flags per ten task runs, plus the lock-out count; S4 scale = a non-human test of a configuration with 100 commands and 20 groups (file size, lines, time and steps for a bulk change); S5 learning curve = time to first correct action on the first task, and the fall in help lookups between the first and the sixth task.

### 7.9 Success criteria (suggested; decide and write them down before round 1)

- A task passes a round for a group if at least 4 of 5 reach outcome 1 or 2 with no X.
- Any X is a blocking defect for that task. Any failure cause that hit 2 or more of 5 participants is fixed before the next round.
- Prediction items: at least 80% correct including the rule.
- T4: at least 4 of 5 name the right rule without changing anything. T13: at least 4 of 5 check their work in some way without being told to.
- After round 3, report the remaining failure causes. Do not report the design as usable if a core task still fails by these rules.

### 7.10 Avoiding leading the participant

- The facilitator reads from a script. Allowed prompts only: "keep talking", "what are you thinking?", "what would you expect to happen?", "what would you do next?". To "is this right?" answer "what would tell you?".
- Every task ends with the same two questions for everyone, never only for those who seem unsure: "Are you sure it worked? How could you tell?"
- Hint ladder, recorded each time: level 1 "there is built-in help" (do not name the command); level 2 name the area of the documentation; level 3 show them (counts as a fail). Offer level 1 after three minutes without progress.
- Give the same documents a real admin would have (the pack's readme, `help`) and log whether they are used. No cheat sheet.
- Counterbalance task order with a Latin square, as the CMU studies did; keep T9 and the debrief last.
- Seed every trap identically. Do not defend the design during the session.
- Two people code hesitations and errors independently from the recordings, compare, and discuss differences (any-two agreement between evaluators can be as low as 5%, section 6). Write the coding sheet and success criteria before round 1 so they are not fitted afterwards.

### 7.11 After each round

Make a problem list: ID, task, description, evidence (participant and time), cause class (vocabulary, missing feedback, hidden state, default, missing tool, wrong model), severity on Nielsen's 0 to 4 scale [recalled], a proposed change. Count how many participants hit each problem; fix those seen by two or more first. Report by group, not pooled. For memory of intent, ask three testers per round to re-explain their own configuration a week later and note what they cannot.

### 7.12 Known weaknesses of this protocol

Observed people are more careful. A prototype lacking a feature pushes people to workarounds: log them as "wanted but absent". Short tasks cannot show how a rule set ages. Small groups cannot show small differences between designs. The protocol finds usability faults, not security faults.

## 8. Design-space table (deliverable 1)

One block per decision. "Chosen by" lists systems read in this stream; game plugins are covered by other cards and are not repeated here.

| Decision | Option | Chosen by | Advantage | Drawback | Tag |
|---|---|---|---|---|---|
| Deny semantics | No deny at all (additive only) | Kubernetes RBAC, Shiro | the result is the union of grants; nothing to rank | cannot say "all except"; wildcards also cover later additions | [read] |
| Deny semantics | Deny always wins, independent of order | AWS IAM, Cedar `forbid`, Google Cloud deny, Azure deny assignments | one sentence; usable as a guardrail layer; easy to explain | a person-level exception to a group deny means editing the deny; with group conflicts under deny-wins novices scored 11% and 22% on two tasks | [read]; [re-read] |
| Deny semantics | Deny exists but only the platform writes it | Azure (customer-made ones were private preview on a page dated 2026-11-05) | admins cannot layer denies badly | admins cannot write a deny of their own | [read] |
| Deny semantics | Ordered rules, first or last match | Windows ACLs (canonical order), sudo (last match) | familiar; exact control | order sensitivity; shadowed rules; error counts rise with rule-set size in firewalls | [read]; [re-read]; [secondary] |
| Deny semantics | Subtraction inside a grant (NotAction, exclusion, `!`) | AWS NotAction, Zanzibar exclusion, sudo `!` | reads like the admin's own thought, "all except" | AWS warns it can grant more than intended; sudo calls subtraction from ALL ineffective | [read]; [re-read] |
| Precedence among conflicts | Deny beats allow at any scope | AWS, Cedar, Google Cloud | simple, fail-safe | cannot express "this person is the exception" locally | [read] |
| Precedence among conflicts | Narrower scope wins, deny decides only between peers | the CHI 2011 proposal; partly Windows (resource side) | four of six tasks better; fixes are local and additive | a group-level deny does not beat a personal allow (Kent task 50% against 88%) | [re-read] |
| Precedence among conflicts | Position (last or first match) | sudo, ACL lists | no ranking concept to learn | last match is "not necessarily the most specific" | [re-read] |
| Precedence among conflicts | Intersection of layers | AWS (boundary, SCP, RCP, session) | each layer can be reasoned about alone | more layers, more places to look when refused | [read] |
| Precedence among conflicts | First decisive authorizer in a chain | Kubernetes authorizer modes | modular; "no opinion" passes on | chain order matters | [read] |
| Precedence among conflicts | Numeric priority or weight | not in my sources (see other cards) | flexible | not studied here | n/a |
| Inheritance | Down a resource tree | Google Cloud, Azure scopes, NTFS | one rule at the top covers a subtree | a deny lower down cannot cancel one higher up (Google); with group inheritance gives two-dimensional conflicts (Lance 0.39 and Adria 0.83 even with the Grid) | [read]; [re-read] |
| Inheritance | Through nested subjects (usersets) | Zanzibar | a team is defined once and reused | depth makes "why" harder; cycles need a rule | [re-read] |
| Inheritance | One relation implies another | Zanzibar rewrite rules (viewers include owners) | fewer grants to write | a change at the top changes many results silently | [re-read] |
| Wildcards | Whole-part `*`, missing trailing parts mean all | Shiro | compact | trailing separator dropped means "all"; separators in names change meaning; case-insensitive by default | [read] |
| Wildcards | Glob inside names (`s3:*`) | AWS | short policies | covers actions added later; unsafe for deny and NotAction | [read] |
| Wildcards | Limited or none, with groups instead | Google Cloud deny (some services only); Cedar action groups | predictable | someone must maintain the groups | [read]; [recalled] for Cedar |
| Wildcards | A class instead of a wildcard | AWS access levels | grant "all Read" of a service in one step | trust in the labels | [read] |
| Scopes and contexts | Namespaces | Kubernetes | limits the blast radius | cluster-wide roles still exist | [read] |
| Scopes and contexts | Hierarchy of account, folder, resource | AWS Organizations, Google Cloud, Azure | delegation per subtree | the total picture is hard to see | [read] |
| Scopes and contexts | Per-object tuples | Zanzibar | exact sharing | many tuples; needs a view | [re-read] |
| Scopes and contexts | Conditions on time, tags, device | AWS, Google Cloud, Cedar | expressive; home-sharing interviews show people want who, where and which device | invisible in a list; Google treats a condition it cannot evaluate as true for a deny | [read]; [re-read] |
| Tags and attributes | Tags on actions set by the author | AWS access levels | grant a class in one step | a mislabelled action is mis-granted | [read] |
| Tags and attributes | Labels that plug an extension's rules into built-in roles | Kubernetes aggregation | extension reaches standard roles with no admin edit | the extension's author decides what the admin's roles gain | [read] |
| Tags and attributes | Resource tags in deny conditions | Google Cloud | per-resource switch | only tags are allowed in the condition | [read] |
| Tags and attributes | Entity attributes in conditions | Cedar | flexible | harder to audit "who" | [read] |
| Default policy | Default deny | AWS, Cedar, Kubernetes, Google Cloud, Deno | Saltzer and Schroeder's fail-safe default | new things are unusable until granted, so the refusal must teach | [read] |
| Default policy | Default deny plus built-in roles and groups | Kubernetes defaults, "everyone" | usable at once | default bindings to anonymous groups are warned against; default roles are rewritten at start-up unless opted out | [read] |
| Default policy | Prompt at first use | Deno on a terminal, Android runtime | in context | Android field study: at least 80% would block at least one request; habituation not measured here | [read] |
| Delegation ceiling | Cannot grant what you do not hold; explicit escape verbs | Kubernetes `escalate`, `bind` | no second policy to maintain | the escape verbs and `impersonate` become the sensitive rights | [read] |
| Delegation ceiling | A ceiling policy chosen by the superior | AWS permissions boundary; organization SCPs | explicit; delegate can create things for others | AWS's own note says the delegate effectively gets that service; some resource policies bypass it | [read] |
| Delegation ceiling | A class of permission-changing actions to watch | AWS "Permissions management" level | one place to look | only as good as the labels | [read] |
| Explain tooling | The refusal names the deciding layer | AWS messages | free, at the moment of need | one layer shown when several deny; some services lack it | [read] |
| Explain tooling | The decision returns all determining policies | Cedar | complete | aimed at developers, not players | [read] |
| Explain tooling | Dry run with matched statements | AWS policy simulator | test before applying | can differ from the live account | [read] |
| Explain tooling | Yes or no query, with impersonation | Kubernetes `can-i --as` | any admin can try it | no reason is given | [read] |
| Explain tooling | List what a subject can do | Kubernetes rules review; `sudo -l` | direct answer to "what can X do" | per subject and namespace | [read]; [recalled] for sudo -l |
| Explain tooling | Reverse query, "who has access" | Zanzibar Expand | audit | none in Kubernetes core; third-party tools | [re-read]; [recalled] |
| Explain tooling | Effective-permissions view in the editor | the Salmon dialog, the Expandable Grid | 25% to 100%, 0% to 89% accuracy in studies | the Grid slowed more as policies grew | [re-read] |
| Temporary grants | Expiring credentials, with a ceiling for the session | AWS temporary credentials and session policies | revocation by expiry | a session is not edited mid-way | [read] |
| Temporary grants | Time conditions on a grant | AWS and Google Cloud conditions | one rule | needs a clock; propagation delay in Google | [recalled]; [read] for the delay |
| Temporary grants | Revoke at run time | `Deno.permissions.revoke` | a script drops power after setup | for programs, not people | [read] |
| Audit | Periodic review for redundancy and escalation paths | Kubernetes good-practices page | cheap | needs a lister | [read] |
| Audit | A stream of changes | Zanzibar Watch API; cloud trail logs | complete history | storage | [re-read]; [recalled] |
| Audit | Record that a compromise happened | Saltzer and Schroeder | detection can stand in for prevention | only after the fact | [read] |

No usability studies were found for any of the rows on tags, wildcards in packs, temporary grants or audit.

## 9. Combinations that have been shown or reported to confuse people (deliverable 2)

### 9A. Shown by a study

| # | Combination | Evidence | Tag |
|---|---|---|---|
| 1 | Deny-wins plus overlapping groups, where the author sets a personal allow to fix a refusal (Jana and Pablo tasks) | accuracy 11% and 22% in the Grid with the Windows rule, 44% and 39% in the Windows dialog, against 89% and 72% with the specificity rule; many people believed they were done because effective permissions were three clicks away | [re-read] |
| 2 | A conflict in two directions at once, one rule narrower in who, the other narrower in what (Lance and Adria tasks) | Windows dialog 0% on Lance and 39% on Adria; the paper calls the Windows choice "likely to confuse" | [re-read] |
| 3 | Rules shown, effective result hidden | XP dialog success 25% on one task against 100% with the effective view; 94% fewer goal errors; Grid 0.89 against 0.00 on a conflict task | [re-read] |
| 4 | Narrower-wins plus a group-level deny that should override a personal allow (Kent task) | 50% with the specificity rule against 88% with deny-wins; the paper names the security risk of stale personal allows | [re-read] |
| 5 | Long lists of capabilities presented once, at install | 17% looked, 3% could answer three questions, 42% of lab participants unaware permissions existed | [re-read] |
| 6 | Growing rule sets | firewall errors rise with a complexity measure; no improvement in newer versions in the revisit | [secondary]; [read] |
| 7 | Policies written in advance for needs that are social and reactive | 33 home users; people want to adjust in reaction to requests; mental models misaligned | [re-read] |
| 8 | Matrix view with large policies | Grid slowed more than Windows as the policy grew (cause unclear: practice or search) | [re-read] |

### 9B. Documented warnings, reports or my reasoning (not a study)

| # | Combination | Source | Tag |
|---|---|---|---|
| 1 | sudo: `ALL` with `!command` subtracted | the manual says it is trivially bypassed by copying the command under another name, so "advisory at best"; negated regexes fall to a relative path | [re-read] |
| 2 | sudo: several matching lines | last match wins, not the most specific | [re-read] |
| 3 | AWS: Allow with NotAction | docs: may grant more than intended | [read] |
| 4 | AWS: NotPrincipal with Deny on principals that have a boundary | docs: always denies them | [read] |
| 5 | AWS: a resource policy that names an IAM user or role session | not limited by the boundary's implicit deny | [read] |
| 6 | AWS: several policy types deny | message shows one type only | [read] |
| 7 | AWS: condition keys differing only in case | docs: case-insensitive, may deny unexpectedly | [read] |
| 8 | Shiro: empty trailing part, or a separator inside a name | silently broadens a grant to all instances | [read] |
| 9 | Kubernetes: wildcards and later-added resource types; name reuse; start-up reconciliation of default roles; `system:masters`; workload creation; `nodes/proxy` get | good-practices and RBAC pages | [read] |
| 10 | Cedar: a `forbid` that errors | skipped, so a broken guardrail stops guarding (reported in diagnostics) | [read], [inference] |
| 11 | Google Cloud: deny with an unevaluable condition; deny lower in the tree; changes in flight | the deny applies; lower deny cannot cancel higher; eventual consistency | [read] |
| 12 | Windows: explicit allow on a child against an inherited deny | the explicit entry is evaluated first | [read]; [inference] on the outcome |
| 13 | Zanzibar: removing a member, then using a stale copy of the rules | the "new enemy" problem | [re-read] |
| 14 | Deno: `run` or `ffi` allowed | equal to everything | [read] |
| 15 | A function that runs as its owner and takes a name from its caller | the confused deputy | [re-read] |
| 16 | A denial attached to a typed name, while an alias, a wrapper function or `execute ... run` reaches the same action | by analogy with sudo's copy-the-binary bypass; not tested on any game | [inference], medium-high |

## 10. Where the literature gives a clear answer, and where it does not (deliverable 3)

| Item | Clear? | What the literature says | Gap |
|---|---|---|---|
| R1 public-key identity, unverified nickname | Partly | authorise on the stable identifier; Kubernetes warns that a reused name inherits rights [read]. Petnames and Zooko's triangle explain why a readable unique name cannot also be secure [recalled] | returning with a fresh key is not a permission-model question; earned-trust ladders on wikis and forums are practice I did not study |
| R2 actors; blocks and functions with someone's authority | Clear on the danger, no usability evidence | Hardy [re-read]; Kubernetes, Deno, sudo [read, re-read]; options in 4.4 | no study compares "caller's power" with "owner's power" defaults for ordinary users |
| R3 sets of groups; looked up every time | Clear in principle | complete mediation [read]; the check through current state acts as a revocable handle [recalled] | cost of checking every command not measured here |
| R4 grant and deny per command, subcommand, pack, tag | Partly | options in section 8; one controlled study of precedence on file permissions | nothing on per-pack, per-tag or per-subcommand grants |
| R5 ownership and sharing | Partly | Zanzibar tuples, usersets, Expand [re-read]; home-sharing needs [re-read] | nothing on sharing scripts and the variables they keep |
| R6 third-party packs | Partly | default deny [read]; classes [read]; aggregation labels [read]; scoped declared permissions [read] | no study of an admin installing a plugin |
| R7 typed commands and files | Partly | effective-view evidence is strong but graphical [re-read]; `sudo -l`, `can-i` are typed tools [read] | nothing tests typed-only administration |
| R8 owner as operator; root key in a file | Partly | `system:masters` shows the need and the cost of a root the system cannot revoke [read] | no evidence on recovery |
| R9 explainable decisions | Clear it can be done | AWS messages and simulator, Cedar diagnostics, `can-i` [read] | how much to tell the refused player; the CHI 2011 paper cites work on regulating that feedback [secondary] |
| T1 friend builds in one area | Model clear | scopes and per-object grants [read, re-read] | no study; trap evidence from conflicts with group rules [re-read] |
| T2 moderator role | Clear on method | list what is allowed instead of "all except" [re-read, read]; role explosion warning [secondary] | nothing on moderator roles; escalation inside a role (can a moderator ban the owner) is not in my sources |
| T3 one command for an hour | Mechanism clear | expiring credentials [read]; conditions [recalled] | no usability study |
| T4 why refused, which rule | Clear | effective view [re-read]; denial messages [read]; all determining policies [read] | one-layer-only messages are a known weakness |
| T5 strip a griefer; survive rename | Partly | key binding [read]; deny layer [read]; the Kent hazard [re-read] | ban evasion with a new key has no permission-literature answer |
| T6 plugin with 15 commands | Partly | default deny; classes; labels; wildcard-later hazard [read] | no study |
| T7 audit lists | Mechanism clear | Expand [re-read]; rules review [read] | reverse query absent from Kubernetes core; matrix slows at scale [re-read] |
| T8 delegate team membership | Clear on two patterns and their holes | Kubernetes escalation prevention; AWS boundary [read] | no usability study of delegated administration |
| T9 bootstrap and recovery | Partly | `system:masters` [read]; recovery as a host-level act [inference] | no studies |
| T10 team of three shares a thing | Model clear | usersets [re-read] | no study |
| T11 automation with limited power | Model clear | session ceilings [read]; Deno flags [read]; Hardy [re-read]; the identity of the caller appears in the request (role-session ARN, `--as system:serviceaccount`) [read] | none on "see who it runs as" usability |
| T12 immediate change, undo, audit | Partly | dry run and equivalence checking exist [read]; propagation delay documented [read]; new-enemy problem [re-read] | no usability evidence on undo |
| T13 everything in a category except one | Clear on hazards | sudo [re-read]; NotAction [read]; Google's exceptions field [read]; the Jana and Kent results [re-read] | whether tag-based "all except" is predictable to admins: no study |

## 11. What I would do (opinion; confidence stated)

1. **Two layers, not one rule.** Everyday grants (groups, teams, personal) where a narrower rule wins, so "Alice is the exception" is a local addition (CHI 2011). Above them a short guardrail layer written only by root, where deny always wins and nothing local can undo it (bans, quarantine, "nobody may", pack ceilings), which is how AWS, Cedar and Google Cloud use deny. The Kent result is why: a safety deny must never live in the layer where a personal allow can beat it. Confidence: medium. The study did not test two layers or games.
2. **Write the precedence rule in two sentences, print it in `help`, and make every explanation cite the sentence that applied.** Confidence: high that fewer, documented rules help; medium on the exact rule.
3. **Ship three queries from the first release: why (player, action), what (player), who (action).** Three questions, three tools (findings 7). Name every deciding rule, not one. Confidence: high.
4. **Make the effective view a command.** A filtered matrix (player or group against command class) is the best-supported aid. Confidence: medium-high; evidence is graphical, so a text version is untested.
5. **For "all except", attach the exception to the final action** (after alias expansion and function wrapping), say in the docs that denies bind actions, not typed names, and ship a probe command that tries the routes. Confidence: medium-high.
6. **New packs default to nobody.** Require each pack to label its commands by class (read, write, permission-changing, runs-as-someone), print "15 commands: 9 read, 4 write, 2 permission-changing" at install with the one-line grant, and keep the permission-changing class behind its own grant. Packs label their own commands, so show the label to the admin. Confidence: medium.
7. **Delegation: default to "cannot grant what you do not hold", with one named escalation right**; add a boundary per delegate later if team managers need it. Gate creation of automated things as a privilege of its own. Confidence: medium-high.
8. **Functions and blocks: caller's power by default.** Marked functions run with the owner's current power limited to a declared ceiling, recorded as "on behalf of CALLER"; ownerless blocks get no power. A marked function must check what its caller may do to a target the caller names. Confidence: medium.
9. **Key-based grants; names are labels shown with a short key fingerprint; new keys start low.** A ban stops one key, not the person, so safety rests on low default trust. Confidence: medium; the permission literature is silent on the second half.
10. **Run the protocol early**, five testers per group, three rounds, success criteria written first. Confidence: high that it is cheap and informative; the thresholds are mine.

## 12. Sources, queries, and what was not read

Searches (9 of 20): (1) Expandable Grids CHI 2008 study; (2) Maxion and Reeder "Salmon" paper; (3) Felt et al. Android permissions 2012; (4) Wool firewall errors; (5) role engineering and role explosion; (6) Bauer et al. real-life access-control challenges; (7) Hertzum and Jacobsen evaluator effect; (8) Jaferian et al. heuristics for IT security management tools; (9) usability of cloud policy authoring. Nothing else was searched.

Own text read (re-read): the CHI 2011 paper (https://users.ece.cmu.edu/~lbauer/papers/2011/chi2011-semantics.pdf); Reeder's thesis CMU-CS-08-143, chapters 2, 3 and 6 and the abstract (https://www.csd.cmu.edu/sites/default/files/phd-thesis/CMU-CS-08-143.pdf); Felt et al. 2012 technical report (https://www2.eecs.berkeley.edu/Pubs/TechRpts/2012/EECS-2012-26.pdf); the Zanzibar paper (https://www.usenix.org/system/files/atc19-pang.pdf); Mazurek et al. 2010 abstract and key findings (https://www.pdl.cmu.edu/ftp/Storage/CHI2010.pdf); Hardy 1988 (http://cap-lore.com/CapTheory/ConfusedDeputy.html, plain-HTTP curl); the sudoers manual (https://www.sudo.ws/docs/man/sudoers.man/, curl, passages on last match and `!`).

Read through the summarising fetch tool ([read]): AWS IAM pages (evaluation logic, permissions boundaries, NotAction, policy simulator, access-denied troubleshooting, access levels); Cedar (https://docs.cedarpolicy.com/auth/authorization.html and arXiv 2403.04651); Kubernetes raw markdown (rbac.md, rbac-good-practices.md, authorization.md); Shiro (https://shiro.apache.org/permissions.html); Google Cloud deny overview; Azure deny assignments; Windows order of ACEs; Deno security page; NN/g "why you only need to test with 5 users"; Saltzer and Schroeder (https://web.mit.edu/Saltzer/www/publications/protection/Basic.html); the CHI 2008 abstract (https://scholars.duke.edu/publication/1494103); the arXiv Wool revisit; the USENIX abstract page for Wijesekera et al.

Fetches that failed: IEEE Xplore for Wool 2004 (HTTP 418); the Wool PDF on the author's site (redirect not followed); core.ac.uk copy of the Salmon paper (403); Bauer et al. 2009 PDF (connection reset); UPenn KeyKOS copy of Hardy (404; used another host); cap-lore over HTTPS (certificate error; used HTTP); JEP 411 (403); the USENIX Wijesekera PDF (redirect not followed; used the abstract page).

Not read at all, so everything on them is [recalled] or [secondary]: Miller's thesis and "Capability Myths Demolished"; the Capsicum paper; AWS's confused-deputy page and Access Analyzer; Open Policy Agent documentation; Java stack inspection papers and JEP 411; Reeder and Maxion on hesitation analysis; Whitten and Tygar's original paper (their five properties are quoted from Reeder's thesis); Wharton et al.'s walkthrough guide; Nielsen's heuristics page; Smetters and Good "How users use access control"; Kapadia et al. on regulated denial feedback; Wool's 2004 full text; the primary sources on role engineering; Cedar's schema and template documentation; the AWS evaluation flow-chart sub-page; any study of Minecraft or other game permission plugins (other streams).

Scratch extracts of the PDFs are in `card5-work/` beside this file and can be deleted.

