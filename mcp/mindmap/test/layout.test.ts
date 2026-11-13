import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { canvasToJson, layout, parse, type Canvas, type MapNode, type MindMap } from "../src/index.js";
import { validateCanvas } from "./canvas-validator.js";
import { baseMap, node, seqGen } from "./helpers.js";

const md = readFileSync(new URL("./fixtures/bastion-context.md", import.meta.url), "utf8");
const parsed = parse(md, seqGen());
if (!parsed.ok) throw new Error("fixture");
const fixture: MindMap = parsed.map;

const overlap = (a: Canvas["nodes"][number], b: Canvas["nodes"][number]) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

describe("layout", () => {
  const c = layout(fixture);
  const byId = new Map(c.nodes.map((n) => [n.id, n]));

  it("passes the JSON Canvas 1.0 validator", () => {
    expect(validateCanvas(JSON.parse(canvasToJson(c)))).toEqual([]);
    expect(validateCanvas(layout(baseMap()))).toEqual([]);
  });

  it("validator rejects broken canvases", () => {
    const bad = { nodes: [{ id: "a", type: "text", x: 0, y: 0, width: 1, height: 1, text: "t", color: "9" }], edges: [{ id: "a", fromNode: "a", toNode: "zz", toEnd: "x" }] };
    const errs = validateCanvas(bad);
    expect(errs.join("\n")).toMatch(/bad color/);
    expect(errs.join("\n")).toMatch(/duplicate id a/);
    expect(errs.join("\n")).toMatch(/zz does not exist/);
    expect(errs.join("\n")).toMatch(/bad toEnd/);
  });

  it("root at x=0, centred on y=0, integer coordinates", () => {
    const r = byId.get("root01")!;
    expect(r.x).toBe(0);
    expect(r.y + r.height / 2).toBe(0);
    for (const n of c.nodes) for (const v of [n.x, n.y, n.width, n.height]) expect(Number.isInteger(v)).toBe(true);
  });

  it("no two cards overlap", () => {
    for (const [i, a] of c.nodes.entries()) for (const b of c.nodes.slice(i + 1)) expect(overlap(a, b), `${a.id}/${b.id}`).toBe(false);
  });

  it("parent is left of children with at least a 120 px level gap; siblings 32 px apart at least", () => {
    for (const e of c.edges.filter((e) => e.id.startsWith("h-"))) {
      const p = byId.get(e.fromNode)!;
      const ch = byId.get(e.toNode)!;
      expect(ch.x).toBeGreaterThanOrEqual(p.x + p.width + 120);
    }
    const branches = fixture.root.children.map((b) => byId.get(b.id)!);
    for (let i = 1; i < branches.length; i++) {
      expect(branches[i]!.y - (branches[i - 1]!.y + branches[i - 1]!.height)).toBeGreaterThanOrEqual(32);
    }
  });

  it("widths are in 160..320 and grow with the label", () => {
    for (const n of c.nodes) {
      expect(n.width).toBeGreaterThanOrEqual(160);
      expect(n.width).toBeLessThanOrEqual(320);
    }
    const m = baseMap();
    m.root.children[0]!.label = "A fairly long label here ok ok ok ok ok ok ok";
    const long = layout(m).nodes.find((n) => n.id === "b00001")!;
    expect(long.width).toBe(320);
    expect(long.height).toBeGreaterThan(layout(baseMap()).nodes.find((n) => n.id === "b00001")!.height);
  });

  it("text, notes and colors", () => {
    expect(byId.get("root01")!.text).toBe("Bastion");
    expect(byId.get("root01")!.color).toBeUndefined();
    expect(byId.get("agt000")!.text).toBe("[[docs/notes/agents|Agents]]");
    expect(byId.get("own001")!.color).toBe("1"); // inherited
    expect(byId.get("own000")!.color).toBe("1");
  });

  it("hierarchy edges", () => {
    const e = c.edges.find((e) => e.id === "h-own000-own001")!;
    expect(e).toEqual({ id: "h-own000-own001", fromNode: "own000", fromSide: "right", fromEnd: "none", toNode: "own001", toSide: "left", toEnd: "none", color: "1" });
    expect(c.edges.filter((e) => e.id.startsWith("h-"))).toHaveLength(c.nodes.length - 1);
  });

  it("cross-links get arrow, label and sides by position", () => {
    const x = c.edges.filter((e) => e.id.startsWith("x-"));
    expect(x.map((e) => e.id)).toEqual(["x-own001-gat001", "x-agt001-ext001"]);
    expect(x[0]).toMatchObject({ toEnd: "arrow", label: "approves", fromSide: "bottom", toSide: "top" }); // same column
    expect(x[1]!.label).toBe("calls, read-only");
    const m = baseMap();
    m.links = [{ from: "c00001", to: "b00002", label: "back" }, { from: "b00002", to: "c00001", label: "fwd" }];
    const e = layout(m).edges.filter((e) => e.id.startsWith("x-"));
    expect([e[0]!.fromSide, e[0]!.toSide]).toEqual(["left", "right"]);
    expect([e[1]!.fromSide, e[1]!.toSide]).toEqual(["right", "left"]);
  });

  it("order: nodes pre-order, hierarchy edges before links", () => {
    expect(c.nodes.slice(0, 4).map((n) => n.id)).toEqual(["root01", "own000", "own001", "own002"]);
    const kinds = c.edges.map((e) => e.id[0]).join("");
    expect(kinds).toMatch(/^h+x+$/);
  });

  it("pins keep x,y and do not move the subtree; unknown pins are ignored", () => {
    const base = layout(fixture);
    const p = layout(fixture, { agt000: { x: 1000, y: -500 }, nope: { x: 1, y: 1 } });
    const a = p.nodes.find((n) => n.id === "agt000")!;
    expect([a.x, a.y]).toEqual([1000, -500]);
    for (const id of ["agt001", "agt002", "own000", "root01"]) {
      expect(p.nodes.find((n) => n.id === id)).toEqual(base.nodes.find((n) => n.id === id));
    }
    expect(p.nodes).toHaveLength(base.nodes.length);
    expect(validateCanvas(p)).toEqual([]);
  });

  it("is deterministic", () => {
    expect(canvasToJson(layout(fixture))).toBe(canvasToJson(layout(structuredClone(fixture))));
    const s = canvasToJson(c);
    expect(s.startsWith("{\n\t\"nodes\": [")).toBe(true);
    expect(s.endsWith("}\n")).toBe(true);
  });

  it("single-node map", () => {
    const m: MindMap = { slug: "s", level: "concept", root: node("root01", "Only"), links: [] };
    const r = layout(m);
    expect(r.nodes).toHaveLength(1);
    expect(r.edges).toEqual([]);
  });

  it("cross-depth: deep cards under a narrow parent do not hit a wide sibling", () => {
    const m: MindMap = {
      slug: "s",
      level: "concept",
      root: node("root01", "R", [
        node("wide01", "w".repeat(40)),
        node("narr01", "a", [1, 2, 3, 4, 5].map((i) => node(`leaf0${i}`, `leaf ${i}`))),
      ]),
      links: [],
    };
    const r = layout(m);
    expectNoOverlap(r);
    const x = (id: string) => r.nodes.find((n) => n.id === id)!;
    expect(x("leaf01").x).toBeGreaterThanOrEqual(x("wide01").x + x("wide01").width + 120);
  });

  it("no overlap on generated trees (wide, deep, unbalanced, long labels)", () => {
    let n = 0;
    const mk = (label: string, kids: MapNode[] = []) => node(`g${String(n++).padStart(5, "0")}`, label, kids);
    const chain = (d: number): MapNode => (d === 0 ? mk("end") : mk("step ".repeat(d % 4 + 1), [chain(d - 1)]));
    const long = "very long label ".repeat(8);
    const trees = [
      mk("root", Array.from({ length: 12 }, (_, i) => mk(`b${i}`, [mk(long), mk("x")]))),
      chain(8),
      mk("root", [chain(5), mk(long, [mk("a"), mk("b", [mk("c"), mk(long)])]), mk("z")]),
      mk("日本語のとても長いラベルです".repeat(3), [mk("😀".repeat(30), [mk("Привет мир ".repeat(6))])]),
    ];
    for (const root of trees) expectNoOverlap(layout({ slug: "g", level: "concept", root, links: [] }));
  });

  it("ignores non-finite pins and picks hierarchy sides for pinned nodes", () => {
    const base = layout(fixture);
    const bad = layout(fixture, { own000: { x: Number.NaN, y: 0 }, agt000: { x: 0, y: Infinity } });
    expect(bad).toEqual(base);
    const p = layout(fixture, { agt001: { x: -1000, y: 400 } });
    const e = p.edges.find((e) => e.id === "h-agt000-agt001")!;
    expect([e.fromSide, e.toSide]).toEqual(["left", "right"]);
    const e2 = p.edges.find((e) => e.id === "h-own000-own001")!;
    expect([e2.fromSide, e2.toSide]).toEqual(["right", "left"]);
  });

  it("wikilink and Markdown escaping in card text", () => {
    const m = baseMap();
    m.root.children[0]!.label = "a|b [[c]] d";
    m.root.children[0]!.note = "docs/n.md";
    m.root.children[1]!.label = "# *x* _y_ `z` [[w]] a|b";
    m.root.children[2]!.label = "1. item";
    m.root.children[3]!.label = "- dash";
    m.root.children[4]!.label = "> quote";
    m.root.children[4]!.note = "../evil|x";
    const t = (id: string) => layout(m).nodes.find((n) => n.id === id)!.text;
    expect(t("b00001")).toBe("[[docs/n|a\\|b c d]]");
    expect(t("b00002")).toBe("\\# \\*x\\* \\_y\\_ \\`z\\` \\[\\[w\\]\\] a\\|b");
    expect(t("b00003")).toBe("1\\. item");
    expect(t("b00004")).toBe("\\- dash");
    expect(t("b00005")).toBe("\\> quote"); // unsafe note path: plain text, no wikilink
  });

  it("never emits -0", () => {
    const m = baseMap();
    const json = canvasToJson(layout(m, { b00001: { x: -0.2, y: -0.4 } }));
    expect(json).not.toContain("-0");
  });

  it("width counts code points, CJK as two units, 10 px per unit", () => {
    const w = (label: string) => layout({ slug: "s", level: "concept", root: node("root01", label), links: [] }).nodes[0]!.width;
    expect(w("x".repeat(20))).toBe(232);
    expect(w("日".repeat(10))).toBe(232);
    expect(w("😀".repeat(10))).toBe(232);
  });
});

function expectNoOverlap(c: Canvas): void {
  for (const [i, a] of c.nodes.entries()) for (const b of c.nodes.slice(i + 1)) expect(overlap(a, b), `${a.id}/${b.id}`).toBe(false);
}
