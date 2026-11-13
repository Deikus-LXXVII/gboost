import { mdEscape } from "./layout.js";
import type { MapNode, MindMap } from "./types.js";

function safe(label: string): string {
  const t = label
    .replace(/\s+/g, " ")
    .trim()
    .replace(/#/g, "#35;")
    .replace(/"/g, "#quot;")
    .replace(/`/g, "#96;")
    .replace(/</g, "#lt;")
    .replace(/>/g, "#gt;");
  return t === "" ? "#nbsp;" : t;
}

function findLabel(n: MapNode, id: string): string | undefined {
  if (n.id === id) return n.label;
  for (const c of n.children) {
    const r = findLabel(c, id);
    if (r !== undefined) return r;
  }
  return undefined;
}

/** Content of the generated `<slug>.diagram.md` note. */
export function toMermaidNote(map: MindMap): string {
  const out: string[] = [
    "---",
    `generated_from: ${map.slug}.md`,
    "# Generated file. Do not edit: changes are overwritten on the next run.",
    "---",
    "```mermaid",
    "mindmap",
    `  ${map.root.id}(("${safe(map.root.label)}"))`,
  ];
  const walk = (n: MapNode, depth: number): void => {
    for (const c of n.children) {
      out.push(`${"  ".repeat(depth + 1)}${c.id}["${safe(c.label)}"]`);
      walk(c, depth + 1);
    }
  };
  walk(map.root, 1);
  out.push("```", "");
  if (map.links.length > 0) {
    out.push("## Links", "");
    for (const l of map.links) {
      const a = findLabel(map.root, l.from) ?? l.from;
      const b = findLabel(map.root, l.to) ?? l.to;
      out.push(`- ${mdEscape(a)} → ${mdEscape(b)}: ${mdEscape(l.label)}`);
    }
    out.push("");
  }
  return out.join("\n");
}
