# Mindmap MCP and skill for GBOOST

Status: draft for review (2026-11-12). Research: Grok report from 2026-11-12. Key facts were checked against
jsoncanvas.org/spec/1.0, the Obsidian 1.15 changelog (Pele, Mermaid 12.1 syntax), Mindmap NextGen 1.17.0
(package.json) and d3-flextree 2.1.2.

## Goal

Agents draw and update mind maps of GBOOST in Obsidian. The owner can see them and edit them from a phone.
Maps are an exception to the GBOOST principle "propose, the owner approves, then apply": the owner leaves
tracking and changes in the project's Obsidian vault to the agent (decision of 2026-11-12). Every change is
still visible afterwards: a line in `docs/maps/CHANGELOG.md`, and the agent commits `docs/maps/` with its work.

## Decisions taken

- The source of truth is a Markdown outline: one file per map. Generated from it, and overwritten on each run:
  `.canvas` (JSON Canvas 1.0) and a Mermaid block inside the note.
- Viewing in Obsidian 1.15 (installed): Mindmap NextGen reads the outline. Pele draws Mermaid without plugins.
- The canon lives in the project, under `docs/maps/`, and is versioned in git. Mirror: iCloud vault
  `lxxvii-obsidian/GBOOST/`.
- Sync runs both ways, without approval. Project → iCloud is a plain copy. If a file in iCloud changed
  (hash differs from the last sync), the MCP never overwrites it blindly. It reads the owner's edit as ops,
  applies them to the canon first, then replays any pending agent ops on top. The owner's edit wins a conflict.
- The MCP is written in TypeScript (official MCP SDK, d3-flextree). It writes files directly and needs no
  Obsidian plugins or REST API.

## Map rules (lint, also go into the skill)

1. One center: the system name.
2. 5–7 main branches. Fewer than 3 or more than 9 is an error; 8–9 is a warning.
3. Depth up to 3. A 4th level is a warning, a 5th is an error: move it to a separate map and link it.
4. A label is 1–4 words, with no sentence punctuation. Details go into a linked note.
5. Each main branch gets a color (presets "1"–"6"). Children inherit it. Color carries one meaning per map,
   written in the frontmatter as `color_meaning`.
6. Cross-links are few (at most 15% of the node count), and each one must have a label.
7. One map covers one C4 level: `context | container | component`. `concept` is for maps of ideas.
8. The trust boundary and the approval points are marked with groups or colors, never buried in deep nodes.

## Canonical file format (draft)

```markdown
---
map: bastion-context
level: context
color_meaning: trust zone
links:
  - { from: k3f9a2, to: x81bc0, label: approves }
---
# GBOOST
- Owner <!-- id:k3f9a2 -->
  - Phone <!-- id:x81bc0 -->
- Agents <!-- id:p0q7m1 -->
```

The root is `#`. Branches are a nested list. The id is an HTML comment `<!-- id:… -->`.

Spike result (2026-11-12, markmap-lib 0.18.12, the version inside NextGen 1.17.0):
- `%%id%%` and `^id` end up in the node label and are visible on the map.
- `<!-- id -->` is HTML that the browser does not draw. Obsidian's reading mode hides it too. → We use HTML comments.
- If a fenced block (` ```mermaid `) follows the list in the same section, markmap drops the whole list.
  → Mermaid goes into a separate generated note `<slug>.diagram.md`, not into the outline.
- Checked in a live Obsidian 1.15.0 through `obsidian-cli eval` (reading the plugin's rendered SVG). The outcome
  matches the above: the node shows only "Html", while `%%id%%` and `^id` stay in the labels; a fenced block
  in the same note drops the list. In a separate note, Pele draws `mindmap` (svg `pele pele-mindmap`).
- The Obsidian CLI (`/Applications/Obsidian.app/Contents/MacOS/obsidian-cli`, turned on in settings) can open
  notes, run plugin commands (`obsidian-mindmap-nextgen:mindmapnextgen:unpinned`) and run `eval`. The MCP
  and the tests use it to check what is rendered when Obsidian is running. It is optional, not required.

## Types (sketch)

```ts
type NodeId = string;                       // 6 chars [a-z0-9], stable across renames
type C4Level = "context" | "container" | "component" | "concept";
type ColorPreset = "1" | "2" | "3" | "4" | "5" | "6";

interface MapNode { id: NodeId; label: string; note?: string; color?: ColorPreset; children: MapNode[] }
interface CrossLink { from: NodeId; to: NodeId; label: string }
interface MindMap { slug: string; level: C4Level; colorMeaning?: string; root: MapNode; links: CrossLink[] }

type Op =
  | { op: "add"; parent: NodeId; label: string; after?: NodeId }
  | { op: "rename"; id: NodeId; label: string }
  | { op: "move"; id: NodeId; parent: NodeId; after?: NodeId }
  | { op: "remove"; id: NodeId }
  | { op: "link"; from: NodeId; to: NodeId; label: string }
  | { op: "unlink"; from: NodeId; to: NodeId }
  | { op: "color"; id: NodeId; color: ColorPreset };

interface Proposal { id: string; slug: string; baseHash: string; ops: Op[]; diff: string; lint: LintIssue[];
                     source: "agent" | "icloud" }
interface LintIssue { rule: number; severity: "error" | "warn"; node?: NodeId; message: string }
```

Pure core: parse(md) → MindMap; serialize(MindMap) → md; applyOps; lint; layout → Canvas; toMermaid.
I/O (files, iCloud, state) is kept in a thin layer at the edge.

## MCP tools

| Tool | What it does | Writes? |
|---|---|---|
| `map_list` | Maps, levels, sync status | no |
| `map_read(slug)` | Tree with ids, plus lint | no |
| `map_lint(slug)` | Rule-check report | no |
| `map_propose(slug, ops)` | Dry run: applies ops in memory, returns diff, lint and proposal id | no |
| `map_apply(proposalId)` | Writes md, renders .canvas and Mermaid, copies to iCloud, appends to `docs/maps/CHANGELOG.md` | yes |
| `map_create(slug, level, center, branches)` | A new map, also through propose/apply | yes, via apply |
| `map_sync()` | Compares hashes. iCloud changes become a proposal with `source: "icloud"` | no |

`map_apply` refuses if the file changed after the proposal (baseHash) or if lint has errors.
Sync state lives in `docs/maps/.sync.json`: the hash of each file in the project and in iCloud at the last sync.

### No owner approval

The owner left map changes to the agent (2026-11-12), so `map_apply` has no approval gate. Background:
MCP elicitation does not work in Claude Code under T3 (the client advertises `elicitation`, but `elicitInput`
returns `{"action":"decline"}` at once and shows no form), and in a bypass-mode session a permission `ask`
rule did not prompt either. The safety that stays: baseHash, lint errors block apply, the owner's iCloud edits
are never overwritten, and every apply leaves a git-visible diff and a changelog line.

## Canvas layout

- A left-to-right tree, d3-flextree (cards have different sizes). Card width is 160–320 px, set by label
  length. Spacing is 32 px between siblings and 120 px between levels.
- Hierarchy edges have no arrows (`fromEnd/toEnd: none`). A cross-link gets an arrow and a `label`.
- A group marks the trust boundary or level. Groups come earlier in the `nodes` array than their children.
- If the owner moved a node in Canvas by hand, its coordinates are kept (pins in `.sync.json`). A full
  re-layout runs only on an explicit command.

## Skill `mindmap`

`.claude/skills/mindmap/SKILL.md`: the rules above, how to pick a C4 level, the propose → apply loop (no owner
approval for maps), and how to read lint results. The skill calls the MCP. It never edits maps by hand.

## Steps

1. Spike: id format and Pele — **done** (see above). MCP elicitation under T3 — **done, does not work**
   (auto-decline, no form). Not needed: maps go without owner approval. Spike removed.
2. Core: parse/serialize round-trip, ops, lint. Tests cover every rule. — **done** (2026-11-12), `mcp/mindmap/`,
   87 tests. Format details fixed during the work:
   - The root has an id too: `# GBOOST <!-- id:root01 -->`.
   - Node metadata: `<!-- id:xxxxxx color:N note:"path" -->`. The note is a JSON string, `<`/`>` escaped.
   - Phone edits are tolerated: text after the comment joins the label; tab, 2- or 4-space indents (one unit
     per file); a list item without an id, or a copied line with a duplicate id, gets a fresh id.
   - Unknown frontmatter keys (Obsidian `tags`, `aliases`) are kept in `extra` and written back.
   - Extra op `note`. Lint rule 0: bad or duplicate ids. Rule 8 is not linted (skill only).
3. Layout → .canvas, plus Mermaid. A JSON Canvas 1.0 validator goes in the tests. — **done**. Columns per
   depth (widest card + 120 px). Labels are escaped for Markdown (canvas) and Mermaid. Note paths may not
   contain `| [ ] # ^`, `..` or a leading `/`. No groups yet: the format has no way to mark them.
4. MCP server, stdio, with the tools above. Connect it to the project's `.mcp.json`. — **done**: server `mindmap`
   (`mcp/mindmap/dist/server.js`; run `npm run build` in `mcp/mindmap` after checkout). Extra tool
   `map_relayout`. Proposals live in server memory and are lost on restart. Mutating tools run one at a time.
5. Sync with iCloud, plus the conflict test (the file changed on both sides). — **done**. Mirror:
   `lxxvii-obsidian/GBOOST/maps/`. Every mirror write re-checks the file's hash right before it and refuses
   if the owner changed it. Owner edits win. Lost material goes to the project dir, never the vault:
   `<slug>.md.conflict-<ts>`, `<slug>.canvas.conflict-<ts>` (canvas edits other than moves). iCloud
   placeholders block writes. Git commits of `docs/maps/` are made by the agent session, not by the MCP.
6. Skill. First map: GBOOST at the context level. — **done**: `.claude/skills/mindmap/SKILL.md`,
   `docs/maps/bastion-context.md` (18 nodes, checked in Obsidian: canvas, Pele, NextGen).
