import { describe, expect, it } from "vitest";
import { applyOps, effectiveColor, type MindMap, type Op } from "../src/index.js";
import { baseMap, node, seqGen } from "./helpers.js";

function run(map: MindMap, ops: Op[]) {
  const before = structuredClone(map);
  const r = applyOps(map, ops, seqGen());
  expect(map).toEqual(before); // input never mutated
  return r;
}
function good(map: MindMap, ops: Op[]) {
  const r = run(map, ops);
  if (!r.ok) throw new Error(JSON.stringify(r.error));
  return r;
}
function fails(map: MindMap, ops: Op[]) {
  const r = run(map, ops);
  if (r.ok) throw new Error("expected failure");
  return r.error;
}
const labels = (m: MindMap) => m.root.children.map((c) => c.label);

describe("add", () => {
  it("appends and returns the new id", () => {
    const r = good(baseMap(), [{ op: "add", parent: "root01", label: "  New  " }]);
    expect(r.newIds).toEqual(["n00001"]);
    expect(r.map.root.children.at(-1)).toEqual({ id: "n00001", label: "New", children: [] });
  });
  it("inserts after a sibling", () => {
    const r = good(baseMap(), [{ op: "add", parent: "root01", label: "X", after: "b00002" }]);
    expect(labels(r.map)).toEqual(["Branch 1", "Branch 2", "X", "Branch 3", "Branch 4", "Branch 5"]);
  });
  it("can build on a node added earlier in the same batch", () => {
    const r = good(baseMap(), [
      { op: "add", parent: "root01", label: "A" },
      { op: "add", parent: "n00001", label: "B" },
    ]);
    expect(r.newIds).toEqual(["n00001", "n00002"]);
  });
  it("fails: unknown parent, bad after, empty label, comment marker", () => {
    expect(fails(baseMap(), [{ op: "add", parent: "zzzzzz", label: "A" }]).message).toMatch(/unknown parent/);
    expect(fails(baseMap(), [{ op: "add", parent: "root01", label: "A", after: "c00001" }]).message).toMatch(/not a child/);
    expect(fails(baseMap(), [{ op: "add", parent: "root01", label: "  " }]).message).toMatch(/empty/);
    expect(fails(baseMap(), [{ op: "add", parent: "root01", label: "a --> b" }]).message).toMatch(/comment/);
    expect(fails(baseMap(), [{ op: "add", parent: "root01", label: "a\nb" }]).message).toMatch(/single line/);
  });
  it("rejects labels with U+2028, U+2029 or U+0085", () => {
    for (const ch of ["\u2028", "\u2029", "\u0085"]) {
      expect(fails(baseMap(), [{ op: "add", parent: "root01", label: `a${ch}b` }]).message).toMatch(/single line/);
      expect(fails(baseMap(), [{ op: "rename", id: "b00001", label: `a${ch}b` }]).message).toMatch(/single line/);
    }
  });
  it("fails when the generator returns a used or invalid id", () => {
    const r = applyOps(baseMap(), [{ op: "add", parent: "root01", label: "A" }], () => "b00001");
    expect(r).toMatchObject({ ok: false, error: { opIndex: 0 } });
    const r2 = applyOps(baseMap(), [{ op: "add", parent: "root01", label: "A" }], () => "BAD");
    expect(r2.ok).toBe(false);
  });
});

describe("rename", () => {
  it("renames, keeping the id", () => {
    const r = good(baseMap(), [{ op: "rename", id: "b00001", label: "Renamed" }]);
    expect(r.map.root.children[0]).toMatchObject({ id: "b00001", label: "Renamed" });
  });
  it("fails on unknown id / empty label", () => {
    expect(fails(baseMap(), [{ op: "rename", id: "zzzzzz", label: "A" }]).message).toMatch(/unknown/);
    expect(fails(baseMap(), [{ op: "rename", id: "b00001", label: "" }]).message).toMatch(/empty/);
  });
});

describe("move", () => {
  it("moves a subtree under another parent, after a sibling", () => {
    const r = good(baseMap(), [{ op: "move", id: "b00005", parent: "b00001", after: "c00001" }]);
    expect(r.map.root.children).toHaveLength(4);
    expect(r.map.root.children[0]?.children.map((c) => c.id)).toEqual(["c00001", "b00005"]);
    expect(r.map.root.children[0]?.children[1]?.children[0]?.id).toBe("c00005");
  });
  it("reorders within the same parent", () => {
    const r = good(baseMap(), [{ op: "move", id: "b00001", parent: "root01", after: "b00003" }]);
    expect(labels(r.map)).toEqual(["Branch 2", "Branch 3", "Branch 1", "Branch 4", "Branch 5"]);
  });
  it("rejects moving under itself or a descendant", () => {
    expect(fails(baseMap(), [{ op: "move", id: "b00001", parent: "b00001" }]).message).toMatch(/itself or its descendant/);
    expect(fails(baseMap(), [{ op: "move", id: "b00001", parent: "c00001" }]).message).toMatch(/itself or its descendant/);
  });
  it("rejects moving the root, unknown ids, bad after", () => {
    expect(fails(baseMap(), [{ op: "move", id: "root01", parent: "b00001" }]).message).toMatch(/root/);
    expect(fails(baseMap(), [{ op: "move", id: "zzzzzz", parent: "b00001" }]).message).toMatch(/unknown/);
    expect(fails(baseMap(), [{ op: "move", id: "b00001", parent: "zzzzzz" }]).message).toMatch(/unknown parent/);
    expect(fails(baseMap(), [{ op: "move", id: "b00001", parent: "root01", after: "b00001" }]).message).toMatch(/itself/);
    expect(fails(baseMap(), [{ op: "move", id: "b00001", parent: "root01", after: "c00002" }]).message).toMatch(/not a child/);
  });
});

describe("remove", () => {
  it("removes the subtree and links touching it", () => {
    const m = baseMap();
    m.links = [
      { from: "c00001", to: "b00002", label: "a" },
      { from: "b00002", to: "b00003", label: "keep" },
      { from: "b00004", to: "c00001", label: "b" },
    ];
    const r = good(m, [{ op: "remove", id: "b00001" }]);
    expect(labels(r.map)).not.toContain("Branch 1");
    expect(r.map.links).toEqual([{ from: "b00002", to: "b00003", label: "keep" }]);
  });
  it("rejects removing root or unknown id", () => {
    expect(fails(baseMap(), [{ op: "remove", id: "root01" }]).message).toMatch(/root/);
    expect(fails(baseMap(), [{ op: "remove", id: "zzzzzz" }]).message).toMatch(/unknown/);
  });
});

describe("link / unlink", () => {
  it("adds and removes a link", () => {
    const r = good(baseMap(), [{ op: "link", from: "b00001", to: "b00002", label: " uses " }]);
    expect(r.map.links).toEqual([{ from: "b00001", to: "b00002", label: "uses" }]);
    const r2 = good(r.map, [{ op: "unlink", from: "b00001", to: "b00002" }]);
    expect(r2.map.links).toEqual([]);
  });
  it("fails: unknown ids, empty label, duplicate, self-link, unlink missing", () => {
    expect(fails(baseMap(), [{ op: "link", from: "zzzzzz", to: "b00002", label: "x" }]).message).toMatch(/unknown from/);
    expect(fails(baseMap(), [{ op: "link", from: "b00001", to: "zzzzzz", label: "x" }]).message).toMatch(/unknown to/);
    expect(fails(baseMap(), [{ op: "link", from: "b00001", to: "b00002", label: " " }]).message).toMatch(/empty/);
    expect(fails(baseMap(), [{ op: "link", from: "b00001", to: "b00001", label: "x" }]).message).toMatch(/itself/);
    const m = baseMap();
    m.links = [{ from: "b00001", to: "b00002", label: "x" }];
    expect(fails(m, [{ op: "link", from: "b00001", to: "b00002", label: "y" }]).message).toMatch(/already exists/);
    expect(fails(baseMap(), [{ op: "unlink", from: "b00001", to: "b00002" }]).message).toMatch(/no link/);
  });
  it("the reverse direction is not a duplicate", () => {
    const m = baseMap();
    m.links = [{ from: "b00001", to: "b00002", label: "x" }];
    expect(good(m, [{ op: "link", from: "b00002", to: "b00001", label: "y" }]).map.links).toHaveLength(2);
  });
});

describe("color", () => {
  it("sets an explicit color", () => {
    expect(good(baseMap(), [{ op: "color", id: "c00001", color: "6" }]).map.root.children[0]?.children[0]?.color).toBe("6");
  });
  it("fails on unknown id or invalid color", () => {
    expect(fails(baseMap(), [{ op: "color", id: "zzzzzz", color: "1" }]).message).toMatch(/unknown/);
    expect(fails(baseMap(), [{ op: "color", id: "b00001", color: "7" as never }]).message).toMatch(/invalid color/);
  });
});

describe("note", () => {
  it("rejects unsafe note paths", () => {
    for (const note of ["a|b", "a[b", "a]b", "a#b", "a^b", "../x", "a/../b", "/abs"]) {
      expect(fails(baseMap(), [{ op: "note", id: "b00001", note }]).message, note).toMatch(/note path/);
    }
  });
  it("sets and clears a note", () => {
    const r = good(baseMap(), [{ op: "note", id: "b00001", note: "docs/n.md" }]);
    expect(r.map.root.children[0]?.note).toBe("docs/n.md");
    const r2 = good(r.map, [{ op: "note", id: "b00001", note: null }]);
    expect(r2.map.root.children[0]).not.toHaveProperty("note");
  });
  it("fails on unknown id or empty string", () => {
    expect(fails(baseMap(), [{ op: "note", id: "zzzzzz", note: "x" }]).message).toMatch(/unknown/);
    expect(fails(baseMap(), [{ op: "note", id: "b00001", note: "" }]).message).toMatch(/non-empty/);
  });
});

describe("batch semantics", () => {
  it("is all-or-nothing and reports the failing opIndex", () => {
    const map = baseMap();
    const e = fails(map, [
      { op: "rename", id: "b00001", label: "Fine" },
      { op: "remove", id: "b00002" },
      { op: "rename", id: "b00002", label: "gone" },
    ]);
    expect(e.opIndex).toBe(2);
  });
  it("rejects unknown op", () => {
    expect(fails(baseMap(), [{ op: "explode" } as never]).message).toMatch(/unknown op/);
  });
  it("empty batch returns an equal, separate copy", () => {
    const map = baseMap();
    const r = good(map, []);
    expect(r.map).toEqual(map);
    expect(r.map).not.toBe(map);
    expect(r.newIds).toEqual([]);
  });
});

describe("effectiveColor", () => {
  const m: MindMap = {
    ...baseMap(),
    root: node("root01", "R", [
      node("b00001", "A", [node("c00001", "x", [node("d00001", "y")]), node("c00002", "z", [], { color: "4" })], { color: "2" }),
      node("b00002", "B"),
    ]),
  };
  it("inherits from the nearest colored ancestor", () => {
    expect(effectiveColor(m, "b00001")).toBe("2");
    expect(effectiveColor(m, "d00001")).toBe("2");
    expect(effectiveColor(m, "c00002")).toBe("4");
  });
  it("is undefined when nothing is set or the id is unknown", () => {
    expect(effectiveColor(m, "b00002")).toBeUndefined();
    expect(effectiveColor(m, "root01")).toBeUndefined();
    expect(effectiveColor(m, "zzzzzz")).toBeUndefined();
  });
});
