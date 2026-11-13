import { collectIds, isValidId } from "./id.js";
import { LEVELS } from "./parse.js";
import type { LintIssue, MapNode, MindMap } from "./types.js";

const wordCount = (s: string): number => s.trim().split(/\s+/).filter(Boolean).length;

export function lint(map: MindMap): LintIssue[] {
  const issues: LintIssue[] = [];
  const add = (rule: number, severity: "error" | "warn", message: string, node?: string): void => {
    issues.push({ rule, severity, message, ...(node !== undefined ? { node } : {}) });
  };

  // Rule 0: ids
  const seen = new Set<string>();
  for (const id of collectIds(map.root)) {
    if (!isValidId(id)) add(0, "error", `id "${id}" must be 6 characters of [a-z0-9]`, id);
    if (seen.has(id)) add(0, "error", `duplicate id "${id}"`, id);
    seen.add(id);
  }

  // Rule 1
  if (map.root.label.trim() === "") add(1, "error", "the center (root) label is empty; use the system name", map.root.id);

  // Rule 2
  const branches = map.root.children.length;
  if (branches < 3 || branches > 9) add(2, "error", `${branches} main branches; need 3-9 (aim for 5-7)`);
  else if (branches >= 8) add(2, "warn", `${branches} main branches; aim for 5-7`);

  // Rules 3, 4, 5 (per node)
  const usesColor = (n: MapNode): boolean => n.color !== undefined || n.children.some(usesColor);
  const walk = (n: MapNode, depth: number, branch: MapNode | undefined): void => {
    if (depth >= 5) {
      add(3, "error", `"${n.label}" is at level ${depth}; move it to a separate map and link it`, n.id);
    } else if (depth === 4) {
      add(3, "warn", `"${n.label}" is at level 4; consider moving it to a separate map`, n.id);
    }

    const label = n.label.trim();
    if (label === "") {
      if (depth > 0) add(4, "error", "empty label", n.id);
    } else {
      if (wordCount(label) > 4) add(4, "warn", `label "${label}" has more than 4 words; put details in a note`, n.id);
      if (/[.!?;]$/.test(label) || label.includes(". ")) {
        add(4, "warn", `label "${label}" contains sentence punctuation`, n.id);
      }
    }

    if (depth === 1) {
      if (n.color === undefined) add(5, "warn", `main branch "${n.label}" has no color`, n.id);
    } else if (depth > 1 && branch?.color !== undefined && n.color !== undefined && n.color !== branch.color) {
      add(5, "warn", `"${n.label}" has color ${n.color}, different from its main branch (${branch.color})`, n.id);
    }
    for (const c of n.children) walk(c, depth + 1, depth === 0 ? c : branch);
  };
  walk(map.root, 0, undefined);
  if (usesColor(map.root) && (map.colorMeaning === undefined || map.colorMeaning.trim() === "")) {
    add(5, "warn", "colors are used but color_meaning is not set in the frontmatter");
  }

  // Rule 6
  const ids = new Set(collectIds(map.root));
  const pairs = new Set<string>();
  for (const l of map.links) {
    if (l.from === l.to) add(6, "error", `link ${l.from} -> ${l.to} connects a node to itself`);
    const key = `${l.from}\u0000${l.to}`;
    if (pairs.has(key)) add(6, "error", `duplicate link ${l.from} -> ${l.to}`);
    pairs.add(key);
    if (l.label.trim() === "") add(6, "error", `link ${l.from} -> ${l.to} has no label`);
    for (const end of [l.from, l.to]) {
      if (!ids.has(end)) add(6, "error", `link ${l.from} -> ${l.to} references unknown id "${end}"`);
    }
  }
  const nodeCount = collectIds(map.root).length;
  const allowed = Math.floor(0.15 * nodeCount);
  if (map.links.length > allowed) {
    add(6, "warn", `${map.links.length} cross-links; at most ${allowed} (15% of ${nodeCount} nodes)`);
  }

  // Rule 7
  if (!(LEVELS as readonly string[]).includes(map.level)) {
    add(7, "error", `level "${String(map.level)}" must be one of ${LEVELS.join(", ")}`);
  }
  return issues;
}
