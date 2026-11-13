import { mkdir, readdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { type Harness, createMap, makeHarness } from "./harness.js";

let h: Harness;
afterEach(async () => h?.close());

const idOf = (tree: string, label: string): string => new RegExp(`${label} \\[id:(\\w+)`).exec(tree)?.[1] as string;
const pid = (t: string): string => /proposalId: (\S+)/.exec(t)?.[1] as string;
const sub = async (root: string, prefix: string): Promise<string[]> => (await readdir(root)).filter((f) => f.startsWith(prefix));

describe("owner edits are never lost", () => {
  it("relayout refuses while the mirror md has an unsynced phone edit", async () => {
    h = await makeHarness();
    await createMap(h);
    const edited = (await h.read("mirror", "demo.md")).trimEnd() + "\n- Phone idea\n";
    await h.write("mirror", "demo.md", edited);
    const r = await h.call("map_relayout", { slug: "demo" });
    expect(r.isError).toBe(true);
    expect(r.text).toContain("owner edited");
    expect(await h.read("mirror", "demo.md")).toBe(edited);
    // after sync it works
    await h.call("map_sync");
    expect((await h.call("map_relayout", { slug: "demo" })).isError).toBe(false);
    expect(await h.read("mirror", "demo.md")).toContain("Phone idea");
  });

  it("publish guard: an edit appearing between pre-check and write is not overwritten", async () => {
    h = await makeHarness();
    await createMap(h);
    const state = await h.store.loadState();
    const m = await h.store.loadMap("demo");
    if (!m.ok) throw new Error(m.error);
    await h.write("mirror", "demo.canvas", "{\"nodes\":[],\"edges\":[],\"phone\":1}");
    await expect(h.store.publish(m.map, state)).rejects.toThrow(/owner edited demo\.canvas/);
    expect(await h.read("mirror", "demo.canvas")).toContain("phone");
  });

  it("canvas with content edits is saved as a project conflict file before regeneration", async () => {
    h = await makeHarness();
    await createMap(h);
    const canvas = JSON.parse(await h.read("mirror", "demo.canvas"));
    canvas.nodes[1].text = "Hacked";
    const text = JSON.stringify(canvas);
    await h.write("mirror", "demo.canvas", text);
    const s = await h.call("map_sync");
    expect(s.text).toMatch(/saved as demo\.canvas\.conflict-/);
    const [name] = await sub(h.dir, "demo.canvas.conflict-");
    expect(await h.read("dir", name as string)).toBe(text);
    expect(await sub(h.mirror as string, "demo.canvas.conflict")).toEqual([]);
    expect(await h.read("mirror", "demo.canvas")).not.toContain("Hacked");
  });

  it("invalid canvas JSON is saved too", async () => {
    h = await makeHarness();
    await createMap(h);
    await h.write("mirror", "demo.canvas", "{not json");
    const s = await h.call("map_sync");
    expect(s.text).toContain("not valid JSON");
    const [name] = await sub(h.dir, "demo.canvas.conflict-");
    expect(await h.read("dir", name as string)).toBe("{not json");
  });

  it("first sync without layout state creates no pins", async () => {
    h = await makeHarness();
    await createMap(h);
    const st = JSON.parse(await h.read("dir", ".sync.json"));
    st.maps = {};
    await h.write("dir", ".sync.json", JSON.stringify(st));
    const canvas = JSON.parse(await h.read("mirror", "demo.canvas"));
    canvas.nodes[1].x += 500;
    await h.write("mirror", "demo.canvas", JSON.stringify(canvas, null, "\t") + "\n");
    const s = await h.call("map_sync");
    expect(s.isError).toBe(false);
    expect(JSON.parse(await h.read("dir", ".sync.json")).maps.demo.pins).toEqual({});
    expect(await sub(h.dir, "demo.canvas.conflict-")).toHaveLength(1); // the moved layout is kept as a copy
  });

  it("any placeholder blocks apply and relayout and nothing is written next to it", async () => {
    h = await makeHarness();
    await createMap(h);
    const { rm: rmf } = await import("node:fs/promises");
    await rmf(`${h.mirror}/demo.canvas`);
    await h.write("mirror", ".demo.canvas.icloud", "x");
    const r = await h.call("map_relayout", { slug: "demo" });
    expect(r.isError).toBe(true);
    expect(r.text).toContain("not downloaded");
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "rename", id: idOf(tree, "Owner"), label: "X" }] });
    expect((await h.call("map_apply", { proposalId: pid(p.text) })).text).toContain("not downloaded");
    expect(await h.ls("mirror")).not.toContain("demo.canvas");
  });

  it("apply refuses with precise messages: owner deleted file, mirror has unknown map", async () => {
    h = await makeHarness();
    await createMap(h);
    await rm(`${h.mirror}/demo.md`);
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "rename", id: idOf(tree, "Owner"), label: "X" }] });
    expect((await h.call("map_apply", { proposalId: pid(p.text) })).text).toContain("owner deleted demo.md");
    const s = await h.call("map_sync");
    expect(s.text).toContain("deleted from the mirror; restored from the project");
    expect(await h.ls("mirror")).toContain("demo.md");

    await h.write("mirror", "fresh.md", "---\nmap: fresh\nlevel: concept\n---\n# Idea\n- One\n- Two\n- Three\n");
    const c = await h.call("map_create", { slug: "other", level: "concept", center: "C", branches: ["a", "b", "c"] });
    expect(c.isError).toBe(false);
    // create of an existing (mirror-only) map name is refused up front
    const dup = await h.call("map_create", { slug: "fresh", level: "concept", center: "C", branches: ["a", "b", "c"] });
    expect(dup.isError).toBe(true);
    expect(dup.text).toContain("already exists");
  });
});

describe("slugs", () => {
  it("map_create rejects existing (any case), reserved, dotted and long slugs", async () => {
    h = await makeHarness();
    await createMap(h);
    const args = { level: "context", center: "B", branches: ["a", "b", "c"] };
    await h.write("dir", "Mixed.md", "x");
    expect((await h.call("map_create", { slug: "mixed", ...args })).text).toContain("already exists");
    expect((await h.call("map_create", { slug: "demo", ...args })).text).toContain("already exists");
    expect((await h.call("map_create", { slug: "changelog", ...args })).isError).toBe(true);
    expect((await h.call("map_create", { slug: ".hidden", ...args })).isError).toBe(true);
    expect((await h.call("map_create", { slug: "a".repeat(65), ...args })).isError).toBe(true);
    expect((await h.call("map_create", { slug: "a".repeat(64), ...args })).isError).toBe(false);
  });

  it("apply of a create re-checks existence case-insensitively", async () => {
    h = await makeHarness();
    const c = await h.call("map_create", { slug: "late", level: "context", center: "B", branches: ["a", "b", "c"] });
    await h.write("dir", "LATE.md", "x");
    const r = await h.call("map_apply", { proposalId: pid(c.text) });
    expect(r.isError).toBe(true);
    expect(r.text).toMatch(/already exists|now exists/); // the first form on case-sensitive filesystems
  });
});

describe("serialization", () => {
  it("concurrent sync + relayouts leave consistent state and all changelog lines", async () => {
    h = await makeHarness();
    await createMap(h);
    await h.write("mirror", "demo.md", (await h.read("mirror", "demo.md")).trimEnd() + "\n- Phone idea\n");
    const rs = await Promise.all([h.call("map_sync"), h.call("map_relayout", { slug: "demo" }), h.call("map_relayout", { slug: "demo" })]);
    expect(rs.map((r) => r.isError)).toEqual([false, false, false]);
    const log = (await h.read("dir", "CHANGELOG.md")).split("\n");
    expect(log.filter((l) => l.includes("owner edit adopted"))).toHaveLength(1);
    expect(log.filter((l) => l.includes("relayout"))).toHaveLength(2);
    const st = JSON.parse(await h.read("dir", ".sync.json"));
    for (const f of ["demo.md", "demo.canvas", "demo.diagram.md"]) {
      expect(st.files[f].project).toBe(st.files[f].mirror);
      expect(await h.read("mirror", f)).toBe(await h.read("dir", f));
    }
  });

  it("two concurrent applies of one proposal: exactly one succeeds", async () => {
    h = await makeHarness();
    await createMap(h);
    const tree = (await h.call("map_read", { slug: "demo" })).text;
    const p = await h.call("map_propose", { slug: "demo", ops: [{ op: "add", parent: idOf(tree, "Owner"), label: "Phone" }] });
    const rs = await Promise.all([h.call("map_apply", { proposalId: pid(p.text) }), h.call("map_apply", { proposalId: pid(p.text) })]);
    expect(rs.filter((r) => !r.isError)).toHaveLength(1);
  });
});

describe("sync robustness", () => {
  it("a corrupt .sync.json gives a clear message", async () => {
    h = await makeHarness();
    await createMap(h);
    await h.write("dir", ".sync.json", "{oops");
    const r = await h.call("map_sync");
    expect(r.isError).toBe(true);
    expect(r.text).toContain(".sync.json is corrupt; delete it to rebuild (owner edits will win)");
    expect((await h.call("map_list")).isError).toBe(true);
  });

  it("deleting a corrupt .sync.json rebuilds; owner edit wins", async () => {
    h = await makeHarness();
    await createMap(h);
    await rm(`${h.dir}/.sync.json`);
    await h.write("mirror", "demo.md", (await h.read("mirror", "demo.md")).replace("Owner", "Phone"));
    const s = await h.call("map_sync");
    expect(s.isError).toBe(false);
    expect(await h.read("dir", "demo.md")).toContain("Phone");
  });

  it("one failing map does not stop the others; state is saved per map", async () => {
    h = await makeHarness();
    await createMap(h, "aaa");
    await createMap(h, "bbb");
    // aaa: mirror canvas replaced by a directory -> reading fails for that slug only
    await rm(`${h.mirror}/aaa.canvas`);
    await mkdir(`${h.mirror}/aaa.canvas`);
    await h.write("mirror", "bbb.md", (await h.read("mirror", "bbb.md")).replace("Owner", "Phone"));
    const s = await h.call("map_sync");
    expect(s.text).toMatch(/aaa: ERROR/);
    expect(s.text).toContain("bbb: adopted owner edit");
    expect(await h.read("dir", "bbb.md")).toContain("Phone");
  });

  it("missing mirror directory is reported and not created", async () => {
    h = await makeHarness();
    await createMap(h);
    await rm(h.mirror as string, { recursive: true });
    const s = await h.call("map_sync");
    expect(s.text).toContain(`mirror not found: ${h.mirror}`);
    expect((await h.call("map_relayout", { slug: "demo" })).text).toContain("mirror not found");
    await expect(readdir(h.mirror as string)).rejects.toThrow();
  });

  it("pure copies and regenerations are not logged", async () => {
    h = await makeHarness();
    await createMap(h);
    const before = await h.read("dir", "CHANGELOG.md");
    await h.write("dir", "demo.md", (await h.read("dir", "demo.md")).replace("Owner", "Boss"));
    await h.call("map_sync"); // copy to mirror
    await rm(`${h.mirror}/demo.canvas`);
    await h.call("map_sync"); // regenerate
    expect(await h.read("dir", "CHANGELOG.md")).toBe(before);
  });
});

describe("tmp files", () => {
  it("stale tmp files are removed on start, others are kept", async () => {
    h = await makeHarness();
    await createMap(h);
    await writeFile(path.join(h.dir, ".demo.md.tmp-deadbeef"), "x");
    await writeFile(path.join(h.mirror as string, ".demo.canvas.tmp-0123abcd"), "x");
    await writeFile(path.join(h.mirror as string, ".keep.md"), "x");
    expect(await h.store.cleanupStaleTmp()).toBe(2);
    expect(await readdir(h.dir)).not.toContain(".demo.md.tmp-deadbeef");
    expect(await readdir(h.mirror as string)).toEqual(expect.arrayContaining([".keep.md"]));
  });
});

describe("durability, no-baseline canvas, bad names", () => {
  it("writeAtomic fsyncs file and directory and leaves no tmp file", async () => {
    h = await makeHarness();
    await h.store.writeAtomic(h.dir, "x.txt", "hello");
    expect(await h.read("dir", "x.txt")).toBe("hello");
    expect((await h.ls("dir")).some((f) => f.includes(".tmp-"))).toBe(false);
    const { fsyncDir } = await import("../src/store.js");
    await expect(fsyncDir(h.dir)).resolves.toBeUndefined();
    await expect(fsyncDir(path.join(h.dir, "x.txt"))).resolves.toBeUndefined(); // not a dir: ignored
  });

  it("no baseline: a hand-arranged mirror canvas is saved before it is overwritten", async () => {
    h = await makeHarness();
    await createMap(h);
    const st = JSON.parse(await h.read("dir", ".sync.json"));
    st.maps = {};
    await h.write("dir", ".sync.json", JSON.stringify(st));
    const canvas = JSON.parse(await h.read("mirror", "demo.canvas"));
    canvas.nodes[1].x += 500;
    const text = JSON.stringify(canvas, null, "\t") + "\n";
    await h.write("mirror", "demo.canvas", text);
    const s = await h.call("map_sync");
    expect(s.text).toContain("no layout baseline");
    const [name] = await sub(h.dir, "demo.canvas.conflict-");
    expect(await h.read("dir", name as string)).toBe(text);
    expect(await h.read("mirror", "demo.canvas")).not.toBe(text);
  });

  it("no baseline and identical positions: no copy", async () => {
    h = await makeHarness();
    await createMap(h);
    await rm(`${h.dir}/.sync.json`);
    await h.call("map_sync");
    expect(await sub(h.dir, "demo.canvas.conflict-")).toEqual([]);
  });

  it("mirror md files with invalid names are reported, not imported", async () => {
    h = await makeHarness();
    await h.write("mirror", "My Map.md", "---\nmap: my-map\nlevel: concept\n---\n# I\n- a\n- b\n- c\n");
    const s = await h.call("map_sync");
    expect(s.text).toContain("My Map.md ignored: map file names must be lowercase a-z, 0-9, -");
    expect(await h.ls("dir")).not.toContain("My Map.md");
    expect(await h.ls("mirror")).toEqual(["My Map.md"]);
  });
});
