# The documents

A map of this folder. Documents are of four kinds, and the kind decides how far to trust one and whether to read it at all.

| Kind | Folder | True now? | Read it |
|---|---|---|---|
| **Standing** | `docs/*.md` | Kept true: wrong text is a bug | When the task touches what it covers |
| **Reference** | `reference/` | Describes what the code does; a mismatch is a bug | When you use or change that interface |
| **Specs** | `specs/` | True when written; **as-built notes** at the end say what changed | For *why* a part is shaped as it is |
| **History** | `history/` | A record of its day; may be out of date | Only to look something up (`grep`), never whole |

The names of documents are unique, so the prose says "`lod-spec.md`" without a folder, and a test (`launcher/tests/docs.rs`) checks that
every such name and every `docs/...` path in the code and the documents is a real file.

## Standing

- `design-decisions.md`: what has been decided about the project's shape and the rules that hold everywhere, in standing form (what, who,
  where to read more). Read before architectural work. The story of each decision (the phase, the numbers, what was found) is in
  `history/decision-diary.md`. A number that is also in the code belongs in the code and its tests, with a pointer here.
- `plan.md`: the build plan, what is proposed, what is open.
- `future-goals.md`: owner wishes that are not scheduled, with what each would need.
- `architecture-review.md`: the direction review of 2026-10-29: findings and proposals (milestones M0 to M4, eleven decisions).
  **Proposals, not decisions**, apart from what `design-decisions.md` records as taken.

## Reference

- `pack-format.md`: writing a data pack (blocks, patches, planets). Its examples are checked by a test.
- `protocol-and-replay.md`: the message stream and replay files.
- `world-file-format.md`: the saved world.

## Specs (one per part or phase; the part's code cites them)

Plugins and content: `plugin-api-spec.md`, `property-bags-spec.md`, `messages-and-commands-spec.md`, `client-interface-spec.md`,
`entities-spec.md`, `content-package-spec.md`, `data-packs-spec.md`, `ui-spec.md`. The game Voxunami 2048: `2048-spec.md`.
The world and the sky: `lod-spec.md` (levels of detail), `planet-spec.md`, `rails-and-recipes-spec.md` (orbits, planets from data),
`great-rivers-spec.md`, `places-spec.md` (`/tp`), `sun-spec.md`, `shadow-spec.md`. Ships: `moving-grids-spec.md`.

## History

- `decision-diary.md`: the old, whole text of `design-decisions.md` (2026-10-30), kept for the stories and numbers behind the standing rules. Not maintained.
- `build-log.md`: what each phase built and measured (about 290 KB). `grep -n "^Phase" docs/history/build-log.md` lists the phases.
- `plugin-architecture-review.md`: the 2026-10-17 reasoning behind the plugin shape.
- `light-spike.md`, `lod-spike.md`, `physics-spike.md`, with their programs in `spikes/` and pictures in `multiscale-modes/`.
  The programs were written against the code of their day and are **not kept compiling**; to run one, check out the commit it names.

## Keeping them

- A phase ends by writing what it built into the build log and the decision into `design-decisions.md`; a spec gets its as-built
  notes. If it changed an interface, the reference is updated in the same commit.
- A document that is no longer true is fixed or moved to `history/`, never left to mislead. Deleting is safe in that git keeps
  everything, but the code cites specs by name, so a spec is archived, not deleted.
- The size of a document is a cost paid by every reader who loads it. A standing document that passes about 30 KB is split.
