import { Document, isMap, isSeq } from "yaml";
import type { MapNode, MindMap } from "./types.js";

function comment(n: MapNode): string {
  const parts = [`id:${n.id}`];
  if (n.color !== undefined) parts.push(`color:${n.color}`);
  if (n.note !== undefined) {
    // JSON string; < and > escaped so a note can never close or open an HTML comment.
    const q = JSON.stringify(n.note).replace(/</g, "\\u003c").replace(/>/g, "\\u003e")
      .replace(/\u2028/g, "\\u2028")
      .replace(/\u2029/g, "\\u2029")
      .replace(/\u0085/g, "\\u0085");
    parts.push(`note:${q}`);
  }
  return `<!-- ${parts.join(" ")} -->`;
}

function line(prefix: string, n: MapNode): string {
  return [prefix, n.label.trim(), comment(n)].filter((s) => s !== "").join(" ");
}

export function serialize(map: MindMap): string {
  const fm: Record<string, unknown> = { map: map.slug.trim(), level: map.level };
  if (map.colorMeaning !== undefined) fm["color_meaning"] = map.colorMeaning.trim();
  if (map.links.length > 0) {
    fm["links"] = map.links.map((l) => ({ from: l.from, to: l.to, label: l.label }));
  }
  for (const [k, v] of Object.entries(map.extra ?? {})) fm[k] = v;
  const doc = new Document(fm);
  const seq = doc.get("links", true);
  if (isSeq(seq)) for (const item of seq.items) if (isMap(item)) item.flow = true;

  const out: string[] = ["---", doc.toString({ lineWidth: 0 }).trimEnd(), "---"];
  out.push(line("#", map.root));
  const walk = (nodes: MapNode[], depth: number): void => {
    for (const n of nodes) {
      out.push(line(`${"  ".repeat(depth)}-`, n));
      walk(n.children, depth + 1);
    }
  };
  walk(map.root.children, 0);
  return out.join("\n") + "\n";
}
