import { describe, expect, it } from "vitest";
import { lint, type LintIssue, type MapNode, type MindMap } from "../src/index.js";
import { baseMap, node } from "./helpers.js";

const byRule = (issues: LintIssue[], rule: number) => issues.filter((i) => i.rule === rule);

/** n main branches, colored, ids m00001.. */
function withBranches(n: number): MindMap {
  const m = baseMap();
  m.root.children = Array.from({ length: n }, (_, i) =>
    node(`m${String(i + 1).padStart(5, "0")}`, `Br ${i + 1}`, [], { color: "1" }),
  );
  return m;
}
/** A chain of `levels` nested nodes below the root. */
function chain(levels: number): MindMap {
  const m = withBranches(5);
  let cur: MapNode = m.root.children[0] as MapNode; // level 1
  for (let l = 2; l <= levels; l++) {
    const c = node(`l${String(l).padStart(5, "0")}`, `Lvl ${l}`);
    cur.children.push(c);
    cur = c;
  }
  return m;
}

describe("clean map", () => {
  it("has no issues", () => expect(lint(baseMap())).toEqual([]));
});

describe("rule 0 ids", () => {
  it("duplicate ids", () => {
    const m = baseMap();
    (m.root.children[1] as MapNode).id = "b00001";
    const r0 = byRule(lint(m), 0);
    expect(r0).toHaveLength(1);
    expect(r0[0]).toMatchObject({ severity: "error", node: "b00001", message: expect.stringContaining("duplicate") });
  });
  it("invalid id format", () => {
    const m = baseMap();
    (m.root.children[0] as MapNode).id = "BAD";
    expect(byRule(lint(m), 0)).toMatchObject([{ severity: "error", node: "BAD" }]);
  });
});

describe("rule 1 center", () => {
  it("empty root label is an error", () => {
    const m = baseMap();
    m.root.label = "  ";
    expect(byRule(lint(m), 1)).toMatchObject([{ severity: "error" }]);
    expect(byRule(lint(m), 4)).toEqual([]); // no duplicate report
  });
});

describe("rule 2 main branches", () => {
  it("3-7 ok", () => {
    for (const n of [3, 5, 7]) expect(byRule(lint(withBranches(n)), 2)).toEqual([]);
  });
  it("<3 and >9 are errors", () => {
    for (const n of [0, 2, 10]) expect(byRule(lint(withBranches(n)), 2)).toMatchObject([{ severity: "error" }]);
  });
  it("8-9 warn", () => {
    for (const n of [8, 9]) expect(byRule(lint(withBranches(n)), 2)).toMatchObject([{ severity: "warn" }]);
  });
});

describe("rule 3 depth", () => {
  it("levels 1-3 are fine", () => expect(byRule(lint(chain(3)), 3)).toEqual([]));
  it("level 4 warns", () => {
    expect(byRule(lint(chain(4)), 3)).toMatchObject([{ severity: "warn", node: "l00004" }]);
  });
  it("level 5+ errors, one issue per offending node, suggests a separate map", () => {
    const issues = byRule(lint(chain(6)), 3);
    expect(issues.map((i) => [i.severity, i.node])).toEqual([
      ["warn", "l00004"],
      ["error", "l00005"],
      ["error", "l00006"],
    ]);
    expect(issues[1]?.message).toMatch(/separate map/);
  });
});

describe("rule 4 labels", () => {
  const withLabel = (label: string): MindMap => {
    const m = baseMap();
    (m.root.children[0] as MapNode).children[0]!.label = label;
    return m;
  };
  it("1-4 words without punctuation is fine", () => {
    expect(byRule(lint(withLabel("Four words are fine")), 4)).toEqual([]);
    expect(byRule(lint(withLabel("v1.0 API")), 4)).toEqual([]);
  });
  it("more than 4 words warns", () => {
    expect(byRule(lint(withLabel("This label has five words")), 4)).toMatchObject([{ severity: "warn", node: "c00001" }]);
  });
  it("sentence punctuation warns", () => {
    for (const l of ["Done.", "Really?", "Stop!", "a;", "Hello. World"]) {
      expect(byRule(lint(withLabel(l)), 4)).toMatchObject([{ severity: "warn" }]);
    }
  });
  it("empty label errors", () => {
    expect(byRule(lint(withLabel(" ")), 4)).toMatchObject([{ severity: "error", node: "c00001" }]);
  });
});

describe("rule 5 colors", () => {
  it("main branch without color warns", () => {
    const m = baseMap();
    delete (m.root.children[2] as MapNode).color;
    expect(byRule(lint(m), 5)).toMatchObject([{ severity: "warn", node: "b00003" }]);
  });
  it("color used without color_meaning warns once; no colors means no warning", () => {
    const m = baseMap();
    delete m.colorMeaning;
    expect(byRule(lint(m), 5)).toHaveLength(1);
    m.colorMeaning = "   ";
    expect(byRule(lint(m), 5)).toHaveLength(1);
    const plain = baseMap();
    delete plain.colorMeaning;
    plain.root.children.forEach((c) => {
      delete c.color;
    });
    expect(byRule(lint(plain), 5).every((i) => i.node !== undefined)).toBe(true); // only per-branch warnings
  });
  it("child color different from its main branch warns; same color is fine", () => {
    const m = baseMap();
    (m.root.children[0] as MapNode).children[0]!.color = "2";
    expect(byRule(lint(m), 5)).toMatchObject([{ severity: "warn", node: "c00001" }]);
    (m.root.children[0] as MapNode).children[0]!.color = "1";
    expect(byRule(lint(m), 5)).toEqual([]);
  });
});

describe("rule 6 links", () => {
  it("a labeled link between known ids is fine", () => {
    const m = baseMap();
    m.links = [{ from: "b00001", to: "b00002", label: "uses" }];
    expect(byRule(lint(m), 6)).toEqual([]);
  });
  it("link without label errors", () => {
    const m = baseMap();
    m.links = [{ from: "b00001", to: "b00002", label: " " }];
    expect(byRule(lint(m), 6)).toMatchObject([{ severity: "error" }]);
  });
  it("link to an unknown id errors (each bad end)", () => {
    const m = baseMap();
    m.links = [{ from: "zzzzzz", to: "yyyyyy", label: "x" }];
    expect(byRule(lint(m), 6).map((i) => i.severity)).toEqual(["error", "error"]);
  });
  it("self-links and duplicate from->to links are errors", () => {
    const m = baseMap();
    m.links = [{ from: "b00001", to: "b00001", label: "x" }];
    expect(byRule(lint(m), 6)).toMatchObject([{ severity: "error", message: expect.stringContaining("itself") }]);
    m.links = [
      { from: "b00001", to: "b00002", label: "x" },
      { from: "b00001", to: "b00002", label: "y" },
    ];
    expect(byRule(lint(m), 6).filter((i) => i.severity === "error")).toMatchObject([{ message: expect.stringContaining("duplicate") }]);
  });
  it("cap is floor(15%) with no minimum: a 5-node map allows no links", () => {
    const m = withBranches(3);
    m.links = [{ from: "m00001", to: "m00002", label: "x" }];
    expect(byRule(lint(m), 6)).toMatchObject([{ severity: "warn" }]); // floor(0.75) = 0
  });
  it("too many links warns (15% of 11 nodes -> max 1)", () => {
    const m = baseMap();
    m.links = [
      { from: "b00001", to: "b00002", label: "a" },
      { from: "b00002", to: "b00003", label: "b" },
    ];
    expect(byRule(lint(m), 6)).toMatchObject([{ severity: "warn" }]);
    m.links.pop();
    expect(byRule(lint(m), 6)).toEqual([]);
  });
});

describe("rule 7 level", () => {
  it("invalid level errors; valid ones pass", () => {
    const m = baseMap();
    m.level = "galaxy" as never;
    expect(byRule(lint(m), 7)).toMatchObject([{ severity: "error" }]);
    for (const l of ["context", "container", "component", "concept"] as const) {
      m.level = l;
      expect(byRule(lint(m), 7)).toEqual([]);
    }
  });
});
