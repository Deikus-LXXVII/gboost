import { afterEach, describe, expect, it } from "vitest";
import { type Harness, createMap, makeHarness } from "./harness.js";

let h: Harness;
afterEach(async () => h?.close());

const idOf = (tree: string, label: string): string => new RegExp(`${label} \\[id:(\\w+)`).exec(tree)?.[1] as string;
const pid = (t: string): string => /proposalId: (\S+)/.exec(t)?.[1] as string;

describe("map_sync", () => {
  it("is a no-op when in sync", async () => {
    h = await makeHarness();
    await createMap(h);
    const before = await h.read("dir", "CHANGELOG.md");
    const s = await h.call("map_sync");
    expect(s.text).toContain("demo: in sync");
    expect(await h.read("dir", "CHANGELOG.md")).toBe(before);
  });

  it("adopts an owner edit, assigns ids and writes them back to the mirror", async () => {
    h = await makeHarness();
    await createMap(h);
    const md = await h.read("mirror", "demo.md");
    await h.write("mirror", "demo.md", md.trimEnd() + "\n- Phone idea\n");
    expect((await h.call("map_list")).text).toContain("sync=owner-edited");
    const s = await h.call("map_sync");
    expect(s.text).toContain("adopted owner edit");
    expect(s.text).toContain("assigned ids to 1");
    const proj = await h.read("dir", "demo.md");
    expect(proj).toMatch(/- Phone idea <!-- id:\w{6} -->/);
    expect(await h.read("mirror", "demo.md")).toBe(proj);
    expect(await h.read("dir", "demo.diagram.md")).toContain("Phone idea");
    expect(await h.read("mirror", "demo.canvas")).toContain("Phone idea");
    expect(await h.read("dir", "CHANGELOG.md")).toMatch(/demo owner: owner edit adopted/);
    expect((await h.call("map_list")).text).toContain("sync=in-sync");
    // a later agent apply works again
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "rename", id: idOf(tree, "Owner"), label: "Boss" }] });
    expect((await h.call("map_apply", { proposalId: pid(p.text) })).isError).toBe(false);
  });

  it("leaves both sides untouched when the mirror edit does not parse", async () => {
    h = await makeHarness();
    await createMap(h);
    const projBefore = await h.read("dir", "demo.md");
    await h.write("mirror", "demo.md", "no frontmatter\n- x\n");
    const s = await h.call("map_sync");
    expect(s.text).toContain("does not parse");
    expect(await h.read("dir", "demo.md")).toBe(projBefore);
    expect(await h.read("mirror", "demo.md")).toBe("no frontmatter\n- x\n");
  });

  it("keeps a canvas move as a pin across a later apply; relayout clears it", async () => {
    h = await makeHarness();
    await createMap(h);
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const owner = idOf(tree, "Owner");
    const canvas = JSON.parse(await h.read("mirror", "demo.canvas"));
    const card = canvas.nodes.find((n: { id: string }) => n.id === owner);
    card.x += 1000;
    card.y += 777;
    const moved = { x: card.x, y: card.y };
    await h.write("mirror", "demo.canvas", JSON.stringify(canvas, null, "\t") + "\n");
    const s = await h.call("map_sync");
    expect(s.text).toContain("pins");
    expect(s.text).not.toContain("warning");
    const posOf = async (root: "dir" | "mirror") =>
      JSON.parse(await h.read(root, "demo.canvas")).nodes.find((n: { id: string }) => n.id === owner);
    expect(await posOf("dir")).toMatchObject(moved);

    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "add", parent: idOf(tree, "Agents"), label: "Claude" }] });
    await h.call("map_apply", { proposalId: pid(p.text) });
    expect(await posOf("dir")).toMatchObject(moved);
    expect(await posOf("mirror")).toMatchObject(moved);

    const r = await h.call("map_relayout", { slug: "demo" });
    expect(r.text).toContain("cleared 1 pin");
    const after = await posOf("mirror");
    expect({ x: after.x, y: after.y }).not.toEqual(moved);
    expect(JSON.parse(await h.read("dir", ".sync.json")).maps.demo.pins).toEqual({});
  });

  it("canvas edits other than positions are ignored with a warning", async () => {
    h = await makeHarness();
    await createMap(h);
    const canvas = JSON.parse(await h.read("mirror", "demo.canvas"));
    canvas.nodes[1].text = "Hacked";
    canvas.nodes.push({ id: "extra", type: "text", x: 0, y: 0, width: 100, height: 50, text: "new card" });
    await h.write("mirror", "demo.canvas", JSON.stringify(canvas));
    const s = await h.call("map_sync");
    expect(s.text).toContain("Edit the outline, not the canvas");
    expect(await h.read("mirror", "demo.canvas")).not.toContain("Hacked");
    expect(await h.read("dir", "demo.canvas")).not.toContain("new card");
  });

  it("overwrites an edited diagram with a warning", async () => {
    h = await makeHarness();
    await createMap(h);
    await h.write("mirror", "demo.diagram.md", "my edit");
    const s = await h.call("map_sync");
    expect(s.text).toContain("generated");
    expect(await h.read("mirror", "demo.diagram.md")).toContain("mermaid");
  });

  it("copies project changes and missing files to the mirror", async () => {
    h = await makeHarness();
    await createMap(h);
    const md = (await h.read("dir", "demo.md")).replace("Owner", "Boss");
    await h.write("dir", "demo.md", md); // outside the tools
    const s = await h.call("map_sync");
    expect(s.text).toContain("copied to the mirror");
    expect(await h.read("mirror", "demo.md")).toBe(md);
    expect(await h.read("mirror", "demo.diagram.md")).toContain("Boss");
    // delete mirror files
    const { rm } = await import("node:fs/promises");
    await rm(`${h.mirror}/demo.canvas`);
    expect((await h.call("map_list")).text).toContain("sync=mirror-missing");
    await h.call("map_sync");
    expect(await h.ls("mirror")).toEqual(["demo.canvas", "demo.diagram.md", "demo.md"]);
  });

  it("changes on both sides: owner wins, project version kept as conflict file", async () => {
    h = await makeHarness();
    await createMap(h);
    const md = await h.read("dir", "demo.md");
    await h.write("dir", "demo.md", md.replace("Owner", "ProjectName"));
    await h.write("mirror", "demo.md", md.replace("Owner", "PhoneName"));
    const s = await h.call("map_sync");
    expect(s.text).toContain("CONFLICT");
    const files = await h.ls("dir");
    const conflict = files.find((f) => f.startsWith("demo.md.conflict-")) as string;
    expect(conflict).toBeDefined();
    expect(await h.read("dir", conflict)).toContain("ProjectName");
    expect(await h.read("dir", "demo.md")).toContain("PhoneName");
    expect(await h.read("mirror", "demo.md")).toContain("PhoneName");
    expect(await h.ls("mirror")).toEqual(["demo.canvas", "demo.diagram.md", "demo.md"]);
  });

  it("imports a map created in the mirror", async () => {
    h = await makeHarness();
    await h.write(
      "mirror",
      "fresh.md",
      "---\nmap: fresh\nlevel: concept\n---\n# Idea\n- One\n- Two\n- Three\n",
    );
    const s = await h.call("map_sync");
    expect(s.text).toContain("imported new map");
    expect(await h.read("dir", "fresh.md")).toMatch(/# Idea <!-- id:\w{6} -->/);
    expect(await h.ls("dir")).toEqual(expect.arrayContaining(["fresh.canvas", "fresh.diagram.md", "CHANGELOG.md"]));
    expect(await h.read("mirror", "fresh.md")).toBe(await h.read("dir", "fresh.md"));
    expect((await h.call("map_list")).text).toContain("fresh: level=concept");
  });

  it("rejects a mirror map whose frontmatter slug differs from the file name", async () => {
    h = await makeHarness();
    await h.write("mirror", "fresh.md", "---\nmap: other\nlevel: concept\n---\n# Idea\n- One\n");
    const s = await h.call("map_sync");
    expect(s.text).toContain("does not match");
    expect(await h.ls("dir")).not.toContain("fresh.md");
  });

  it("skips maps whose files are iCloud placeholders", async () => {
    h = await makeHarness();
    await createMap(h);
    const { rm } = await import("node:fs/promises");
    await rm(`${h.mirror}/demo.md`);
    await h.write("mirror", ".demo.md.icloud", "placeholder");
    expect((await h.call("map_list")).text).toContain("sync=not-downloaded");
    const s = await h.call("map_sync");
    expect(s.text).toContain("not downloaded");
    expect(await h.ls("mirror")).not.toContain("demo.md");
    // apply is refused too
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "rename", id: idOf(tree, "Owner"), label: "X" }] });
    expect((await h.call("map_apply", { proposalId: pid(p.text) })).text).toContain("not downloaded");
  });
});
