import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { lint, parse, serialize, type MindMap } from "../src/index.js";
import { seqGen } from "./helpers.js";

const fixture = readFileSync(new URL("./fixtures/bastion-context.md", import.meta.url), "utf8");

function ok(md: string, gen = seqGen()) {
  const r = parse(md, gen);
  if (!r.ok) throw new Error(JSON.stringify(r.errors));
  return r;
}

describe("round trip", () => {
  it("fixture parses into the expected structure", () => {
    const { map, assignedIds } = ok(fixture);
    expect(assignedIds).toEqual([]);
    expect(map.slug).toBe("bastion-context");
    expect(map.level).toBe("context");
    expect(map.colorMeaning).toBe("trust zone");
    expect(map.root).toMatchObject({ id: "root01", label: "Bastion" });
    expect(map.root.children).toHaveLength(6);
    expect(map.root.children[1]?.note).toBe("docs/notes/agents.md");
    expect(map.root.children[4]?.children[0]?.note).toBe('say "hi" --> <b>');
    expect(map.links).toEqual([
      { from: "own001", to: "gat001", label: "approves" },
      { from: "agt001", to: "ext001", label: "calls, read-only" },
    ]);
  });

  it("serialize(parse(fixture)) equals the fixture (canonical)", () => {
    expect(serialize(ok(fixture).map)).toBe(fixture);
  });

  it("serialize(parse(serialize(m))) === serialize(m) and parse(serialize(m)) deep-equals m", () => {
    const m = ok(fixture).map;
    const s = serialize(m);
    const again = ok(s).map;
    expect(again).toEqual(m);
    expect(serialize(again)).toBe(s);
  });

  it("round-trips a map with no links, no color_meaning, numeric-looking slug and ids", () => {
    const m: MindMap = {
      slug: "123",
      level: "concept",
      root: { id: "123456", label: "Root", children: [{ id: "000001", label: "A", children: [] }] },
      links: [],
    };
    const again = ok(serialize(m)).map;
    expect(again).toEqual(m);
    expect(serialize(again)).toBe(serialize(m));
  });

  it("round-trips an empty-label root and tricky notes", () => {
    const m: MindMap = {
      slug: "x",
      level: "context",
      root: { id: "aaaaaa", label: "", children: [{ id: "bbbbbb", label: "B", note: "a\nb\\c \"q\" <!-- x -->", children: [] }] },
      links: [{ from: "aaaaaa", to: "bbbbbb", label: "a: b, {c}" }],
    };
    const s = serialize(m);
    const again = ok(s).map;
    expect(again).toEqual(m);
    expect(serialize(again)).toBe(s);
  });

  it("accepts CRLF input and normalizes on serialize", () => {
    const r = ok(fixture.replace(/\n/g, "\r\n"));
    expect(serialize(r.map)).toBe(fixture);
  });

  it("the fixture has no lint errors", () => {
    expect(lint(ok(fixture).map).filter((i) => i.severity === "error")).toEqual([]);
  });
});

describe("missing ids", () => {
  const md = `---
map: m
level: context
---
# Bastion
- Owner <!-- id:own000 -->
  - Phone
- Agents
- Vault <!-- color:2 -->
`;
  it("assigns fresh ids in document order via the injected generator", () => {
    const { map, assignedIds } = ok(md, seqGen());
    expect(assignedIds).toEqual(["n00001", "n00002", "n00003", "n00004"]);
    expect(map.root.id).toBe("n00001");
    expect(map.root.children.map((c) => c.id)).toEqual(["own000", "n00003", "n00004"]);
    expect(map.root.children[0]?.children[0]?.id).toBe("n00002");
    expect(map.root.children[2]?.color).toBe("2");
  });

  it("generator receives all explicit ids as existing (no collisions)", () => {
    const seen: Set<string>[] = [];
    ok(md, (existing) => {
      seen.push(new Set(existing));
      return seqGen()(existing);
    });
    expect(seen[0]?.has("own000")).toBe(true);
  });

  it("serialized output then carries the ids and is stable", () => {
    const { map } = ok(md);
    const s = serialize(map);
    expect(s).toContain("- Phone <!-- id:n00002 -->");
    expect(serialize(ok(s).map)).toBe(s);
  });

  it("reports an unusable generator result by throwing", () => {
    expect(() => parse(md, () => "own000")).toThrow(/unusable id/);
  });
});

describe("review fixes", () => {
  const head = "---\nmap: m\nlevel: context\n---\n# Bastion\n";

  it("keeps the id when text follows the comment (phone edit)", () => {
    const { map, assignedIds } = ok(head + "- Owner <!-- id:k3f9a2 --> extra words\n- Pre <!-- color:2 --> post\n");
    expect(map.root.children[0]).toMatchObject({ id: "k3f9a2", label: "Owner extra words" });
    expect(map.root.children[1]).toMatchObject({ label: "Pre post", color: "2" });
    expect(assignedIds).toHaveLength(2); // root + second child only
    expect(serialize(map)).toContain("- Owner extra words <!-- id:k3f9a2 -->");
  });

  it("comment in the middle of the label", () => {
    expect(ok(head + "- Own <!-- id:k3f9a2 --> er\n").map.root.children[0]?.label).toBe("Own er");
  });

  it("accepts tab indents and 4-space indents, serializes with 2 spaces", () => {
    const tabs = ok(head + "- A <!-- id:aaaaaa -->\n\t- B <!-- id:bbbbbb -->\n\t\t- C <!-- id:cccccc -->\n");
    const four = ok(head + "- A <!-- id:aaaaaa -->\n    - B <!-- id:bbbbbb -->\n        - C <!-- id:cccccc -->\n");
    expect(four.map).toEqual(tabs.map);
    expect(tabs.map.root.children[0]?.children[0]?.children[0]?.id).toBe("cccccc");
    expect(serialize(tabs.map)).toContain("\n  - B <!-- id:bbbbbb -->\n    - C");
  });

  it("list items with U+2028 / U+2029 / U+0085 inside parse", () => {
    const r = ok(head + "- a\u2028b <!-- id:aaaaaa -->\n");
    expect(r.map.root.children[0]?.label).toBe("a\u2028b");
  });

  it("a note with U+2028, U+2029, U+0085 round-trips", () => {
    const m: MindMap = {
      slug: "x",
      level: "context",
      root: { id: "aaaaaa", label: "R", children: [{ id: "bbbbbb", label: "B", note: "a\u2028b\u2029c\u0085d", children: [] }] },
      links: [],
    };
    const s = serialize(m);
    expect(s).not.toMatch(/[\u2028\u2029\u0085]/);
    expect(ok(s).map).toEqual(m);
    expect(serialize(ok(s).map)).toBe(s);
  });

  it("duplicate ids: first keeps it, later ones get fresh ids listed in assignedIds", () => {
    const { map, assignedIds } = ok(head + "- A <!-- id:aaaaaa -->\n- B <!-- id:aaaaaa -->\n  - C <!-- id:aaaaaa -->\n", seqGen());
    expect(map.root.children[0]?.id).toBe("aaaaaa");
    expect(map.root.children[1]?.id).toBe("n00002");
    expect(map.root.children[1]?.children[0]?.id).toBe("n00003");
    expect(assignedIds).toEqual(["n00001", "n00002", "n00003"]); // root has no id either
    expect(lint(map).filter((i) => i.rule === 0)).toEqual([]);
  });

  it("preserves unknown frontmatter keys (extra), in order, after known keys", () => {
    const md = "---\ntags:\n  - a\n  - b\nmap: m\naliases: [x, y]\nlevel: context\n---\n# R <!-- id:aaaaaa -->\n";
    const { map } = ok(md);
    expect(map.extra).toEqual({ tags: ["a", "b"], aliases: ["x", "y"] });
    expect(Object.keys(map.extra ?? {})).toEqual(["tags", "aliases"]);
    const s = serialize(map);
    expect(s.indexOf("level:")).toBeLessThan(s.indexOf("tags:"));
    expect(s.indexOf("tags:")).toBeLessThan(s.indexOf("aliases:"));
    const again = ok(s).map;
    expect(again).toEqual(map);
    expect(serialize(again)).toBe(s);
  });

  it("no extra key when there are no unknown keys", () => {
    expect(ok(fixture).map).not.toHaveProperty("extra");
  });

  it("serialize trims padded label/slug/colorMeaning so output is canonical", () => {
    const m: MindMap = {
      slug: " m ",
      level: "context",
      colorMeaning: " zone ",
      root: { id: "aaaaaa", label: "  R  ", children: [{ id: "bbbbbb", label: " B ", children: [] }] },
      links: [],
    };
    const s = serialize(m);
    expect(s).toContain("map: m\n");
    expect(s).toContain("color_meaning: zone\n");
    expect(s).toContain("# R <!--");
    expect(s).toContain("- B <!--");
    expect(serialize(ok(s).map)).toBe(s);
  });
});
