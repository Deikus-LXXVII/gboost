export type NodeId = string; // 6 chars [a-z0-9], stable across renames
export type C4Level = "context" | "container" | "component" | "concept";
export type ColorPreset = "1" | "2" | "3" | "4" | "5" | "6";

export interface MapNode {
  id: NodeId;
  label: string;
  note?: string;
  color?: ColorPreset;
  children: MapNode[];
}
export interface CrossLink {
  from: NodeId;
  to: NodeId;
  label: string;
}
export interface MindMap {
  slug: string;
  level: C4Level;
  colorMeaning?: string;
  root: MapNode;
  links: CrossLink[];
  /** Unknown frontmatter keys (e.g. Obsidian tags/aliases), preserved verbatim and in order. */
  extra?: Record<string, unknown>;
}

export type Op =
  | { op: "add"; parent: NodeId; label: string; after?: NodeId }
  | { op: "rename"; id: NodeId; label: string }
  | { op: "move"; id: NodeId; parent: NodeId; after?: NodeId }
  | { op: "remove"; id: NodeId }
  | { op: "link"; from: NodeId; to: NodeId; label: string }
  | { op: "unlink"; from: NodeId; to: NodeId }
  | { op: "color"; id: NodeId; color: ColorPreset }
  | { op: "note"; id: NodeId; note: string | null };

export interface LintIssue {
  rule: number; // 0 = structural (ids), 1-7 = map rules
  severity: "error" | "warn";
  node?: NodeId;
  message: string;
}

export interface Proposal {
  id: string;
  slug: string;
  baseHash: string;
  ops: Op[];
  diff: string;
  lint: LintIssue[];
  source: "agent" | "icloud";
}

/** Returns a fresh id that is not in `existing`. Injected so the core stays pure. */
export type IdGenerator = (existing: Set<NodeId>) => NodeId;

export interface ParseError {
  line: number; // 1-based
  message: string;
}
export type ParseResult =
  | { ok: true; map: MindMap; assignedIds: NodeId[] }
  | { ok: false; errors: ParseError[] };

export type OpResult =
  | { ok: true; map: MindMap; newIds: NodeId[] }
  | { ok: false; error: { opIndex: number; message: string } };
