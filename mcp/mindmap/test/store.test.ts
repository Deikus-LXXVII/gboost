import { afterEach, describe, expect, it } from "vitest";
import { type Harness, createMap, makeHarness } from "./harness.js";

let h: Harness;
afterEach(async () => h?.close());

const rootId = (t: string): string => /# Bastion \[id:(\w+)/.exec(t)?.[1] as string;
const pid = (t: string): string => /proposalId: (\S+)/.exec(t)?.[1] as string;
const idOf = (tree: string, label: string): string => new RegExp(`${label} \\[id:(\\w+)`).exec(tree)?.[1] as string;

describe("tools, happy paths", () => {
  it("create -> apply writes md, canvas, diagram to project and mirror, changelog and state", async () => {
    h = await makeHarness();
    const c = await h.call("map_create", { slug: "demo", level: "context", center: "Bastion", branches: ["A", "B", "C", "D", "E"], colorMeaning: "zone" });
    expect(c.isError).toBe(false);
    expect(c.text).toContain("newIds:");
    expect(c.text).toContain("+ # Bastion");
    expect(await h.ls("dir")).toEqual([]);
    const a = await h.call("map_apply", { proposalId: pid(c.text) });
    expect(a.text).toContain("applied demo");
    expect(await h.ls("dir")).toEqual([".sync.json", "CHANGELOG.md", "demo.canvas", "demo.diagram.md", "demo.md"]);
    expect(await h.ls("mirror")).toEqual(["demo.canvas", "demo.diagram.md", "demo.md"]);
    expect(await h.read("mirror", "demo.md")).toBe(await h.read("dir", "demo.md"));
    expect(await h.read("dir", "CHANGELOG.md")).toMatch(/2026-11-12T10:00:\d\d\.000Z demo agent: created/);
    const st = JSON.parse(await h.read("dir", ".sync.json"));
    expect(st.files["demo.md"].project).toBe(st.files["demo.md"].mirror);
    expect(Object.keys(st.maps.demo.layout)).toHaveLength(6);
  });

  it("list, read, lint", async () => {
    h = await makeHarness();
    expect((await h.call("map_list")).text).toContain("no maps");
    await createMap(h);
    const l = await h.call("map_list");
    expect(l.text).toContain("demo: level=context nodes=6 lint=0E/0W sync=in-sync");
    const r = await h.call("map_read", { slug: "demo" });
    expect(r.text).toContain("# Bastion [id:");
    expect(r.text).toContain("- Owner [id:");
    expect(r.text).toContain("color:1");
    expect(r.text).toContain("lint: clean");
    expect((await h.call("map_lint", { slug: "demo" })).text).toBe("lint: clean");
    expect((await h.call("map_read", { slug: "nope" })).isError).toBe(true);
  });

  it("propose does not write; apply updates md, assigns ids and appends the changelog", async () => {
    h = await makeHarness();
    await createMap(h);
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const before = await h.read("dir", "demo.md");
    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "add", parent: idOf(tree, "Owner"), label: "Phone" }] });
    expect(p.text).toMatch(/newIds: \w{6}/);
    expect(p.text).toContain("+   - Phone");
    expect(await h.read("dir", "demo.md")).toBe(before);
    const a = await h.call("map_apply", { proposalId: pid(p.text) });
    expect(a.isError).toBe(false);
    expect((await h.call("map_read", { slug: "demo" })).text).toContain("Phone");
    expect(await h.read("mirror", "demo.md")).toContain("Phone");
    expect(await h.read("dir", "demo.diagram.md")).toContain("Phone");
    expect(await h.read("dir", "CHANGELOG.md")).toContain("1 op (add)");
  });

  it("propose reports op failures", async () => {
    h = await makeHarness();
    await createMap(h);
    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "remove", id: "zzzzzz" }] });
    expect(p.isError).toBe(true);
    expect(p.text).toContain("op 0");
  });

  it("works without a mirror, sync reports it", async () => {
    h = await makeHarness({ mirror: false });
    await createMap(h);
    expect((await h.call("map_list")).text).toContain("sync=no-mirror");
    const s = await h.call("map_sync");
    expect(s.isError).toBe(true);
    expect(s.text).toContain("mirror not configured");
    const st = JSON.parse(await h.read("dir", ".sync.json"));
    expect(st.files["demo.md"].mirror).toBeUndefined();
  });

  it("slug validation", async () => {
    h = await makeHarness();
    const r = await h.call("map_read", { slug: "../x" });
    expect(r.isError).toBe(true);
  });
});

describe("apply refusals", () => {
  it("refuses when the md changed after the proposal", async () => {
    h = await makeHarness();
    await createMap(h);
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const o = idOf(tree, "Owner");
    const p1 = await h.call("map_propose", { slug: "demo", ops: [{ op: "rename", id: o, label: "Boss" }] });
    const p2 = await h.call("map_propose", { slug: "demo", ops: [{ op: "rename", id: o, label: "Chief" }] });
    expect((await h.call("map_apply", { proposalId: pid(p1.text) })).isError).toBe(false);
    const r = await h.call("map_apply", { proposalId: pid(p2.text) });
    expect(r.isError).toBe(true);
    expect(r.text).toContain("changed since the proposal");
  });

  it("refuses unknown proposals and drops applied ones", async () => {
    h = await makeHarness();
    await createMap(h);
    expect((await h.call("map_apply", { proposalId: "nope" })).isError).toBe(true);
    expect((await h.call("map_apply", { proposalId: "p1" })).isError).toBe(true); // already applied
  });

  it("refuses lint errors", async () => {
    h = await makeHarness();
    await createMap(h);
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const p = await h.call("map_propose", { slug: "demo", ops: ["a", "b", "c", "d", "e"].map((label) => ({ op: "add", parent: rootId(tree), label })) });
    expect(p.text).toContain("ERROR");
    const r = await h.call("map_apply", { proposalId: pid(p.text) });
    expect(r.isError).toBe(true);
    expect(r.text).toContain("lint errors");
  });

  it("create refuses when the file now exists, and with too few branches", async () => {
    h = await makeHarness();
    const c1 = await h.call("map_create", { slug: "demo", level: "context", center: "B", branches: ["a", "b", "c"] });
    const c2 = await h.call("map_create", { slug: "demo", level: "context", center: "B", branches: ["a", "b", "c"] });
    expect((await h.call("map_apply", { proposalId: pid(c1.text) })).isError).toBe(false);
    const r = await h.call("map_apply", { proposalId: pid(c2.text) });
    expect(r.isError).toBe(true);
    expect(r.text).toContain("now exists");
    const few = await h.call("map_create", { slug: "few", level: "context", center: "B", branches: ["a"] });
    expect((await h.call("map_apply", { proposalId: pid(few.text) })).text).toContain("lint errors");
  });

  it("refuses when the owner edited the mirror copy", async () => {
    h = await makeHarness();
    await createMap(h);
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "rename", id: idOf(tree, "Owner"), label: "Boss" }] });
    await h.write("mirror", "demo.md", (await h.read("mirror", "demo.md")) + "- Phone idea\n");
    const r = await h.call("map_apply", { proposalId: pid(p.text) });
    expect(r.isError).toBe(true);
    expect(r.text).toContain("owner edited");
    expect(r.text).toContain("map_sync");
    expect(await h.read("dir", "demo.md")).not.toContain("Boss");
  });
});
