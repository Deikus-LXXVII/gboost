import type { IdGenerator, MapNode, MindMap } from "../src/index.js";

/** Deterministic generator: n00001, n00002, ... skipping existing ids. */
export function seqGen(start = 1): IdGenerator {
  let n = start;
  return (existing) => {
    for (;;) {
      const id = `n${String(n++).padStart(5, "0")}`;
      if (!existing.has(id)) return id;
    }
  };
}

export const node = (id: string, label: string, children: MapNode[] = [], extra: Partial<MapNode> = {}): MapNode => ({
  id,
  label,
  children,
  ...extra,
});

/** 5 colored branches, each with one child. */
export function baseMap(): MindMap {
  const b = (i: number): MapNode =>
    node(`b0000${i}`, `Branch ${i}`, [node(`c0000${i}`, `Child ${i}`)], { color: String(i) as MapNode["color"] });
  return {
    slug: "t",
    level: "context",
    colorMeaning: "zone",
    root: node("root01", "Bastion", [b(1), b(2), b(3), b(4), b(5)]),
    links: [],
  };
}
