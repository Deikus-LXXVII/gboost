---
name: mindmap
description: Draw or update a GBOOST mind map in the owner's Obsidian vault (docs/maps, mirrored to iCloud) through the mindmap MCP. Use when asked to map, diagram or visualize GBOOST's structure, components, ideas or trust zones, or to change an existing map.
---

# GBOOST mind maps

Maps live in `docs/maps/<slug>.md` (the canon, a Markdown outline). The `mindmap` MCP generates
`<slug>.canvas` and `<slug>.diagram.md` and mirrors all three to the iCloud vault
`lxxvii-obsidian/GBOOST/maps/`, where the owner views and edits them on a phone.

Never edit map files by hand: always go through the MCP tools. Map changes need no owner approval
(owner's decision, 2026-11-12), but every change must pass lint and is logged in `docs/maps/CHANGELOG.md`.

## Loop

1. `map_sync` first. The owner may have edited on the phone; their edits win and get imported.
2. `map_list` / `map_read(slug)` to see the current tree with node ids.
3. `map_propose(slug, ops)` (or `map_create` for a new map). Read the diff and lint.
4. Fix lint errors with more ops; think about warnings. Then `map_apply(proposalId)`.
5. If apply refuses because the owner edited on the phone or the file changed: `map_sync`, re-read,
   propose again. Do not retry the old proposal.
6. Commit `docs/maps/` with the rest of your work.

Ops: `add {parent, label, after?}`, `rename {id, label}`, `move {id, parent, after?}`, `remove {id}`
(removes the subtree and its links), `link {from, to, label}`, `unlink {from, to}`,
`color {id, color: "1".."6"}`, `note {id, note: "path/to/note.md" | null}`.

## Rules (lint checks 0–7; 8 is on you)

1. One center: the system name.
2. 5–7 main branches. <3 or >9 is an error, 8–9 a warning.
3. Depth up to 3. Level 4 warns, level 5 is an error: split it into a separate map and link to it
   with a `note`.
4. Labels: 1–4 words, no sentence punctuation. Details go into a linked note.
5. Each main branch has a color ("1"–"6"); children inherit it. One meaning per map, written as
   `color_meaning` (e.g. "trust zone").
6. Cross-links are few (≤15% of nodes) and each has a label.
7. One map = one C4 level.
8. Show the trust boundary and the approval points with colors on main branches, never deep in the tree.

## Choosing the level

- `context`: GBOOST as one box, with the people and outside systems around it (owner, phone, agents,
  model APIs). Start here.
- `container`: what runs inside GBOOST (gateway, approval queue, vault, audit log), one map.
- `component`: inside one container. One map per container, slug `<container>-components`.
- `concept`: ideas, options, research; not architecture.

Slugs: `bastion-context`, `bastion-containers`, `<container>-components`, `concept-<topic>`.

## Layout

Owner-moved cards in Canvas are kept (pins). `map_relayout(slug)` resets them; use it only when
the owner asks.
