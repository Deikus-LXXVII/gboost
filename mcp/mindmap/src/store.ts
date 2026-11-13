import { createHash, randomBytes } from "node:crypto";
import { mkdir, open, readFile, readdir, rename, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { lineDiff } from "./diff.js";
import { collectIds } from "./id.js";
import { canvasToJson, layout } from "./layout.js";
import { lint } from "./lint.js";
import { toMermaidNote } from "./mermaid.js";
import { applyOps } from "./ops.js";
import { parse } from "./parse.js";
import { serialize } from "./serialize.js";
import type { IdGenerator, LintIssue, MindMap, NodeId, Op } from "./types.js";

export const SLUG_PATTERN = /^[a-z0-9-]+$/;
export const MAX_SLUG = 64;
export const RESERVED_SLUGS = ["changelog"];
/** Returns a problem description, or undefined if the slug is usable. */
export function slugProblem(slug: string): string | undefined {
  if (slug.startsWith(".")) return "slug must not start with a dot";
  if (!SLUG_PATTERN.test(slug)) return "slug must match [a-z0-9-]+";
  if (slug.length > MAX_SLUG) return `slug is longer than ${MAX_SLUG} characters`;
  if (RESERVED_SLUGS.includes(slug)) return `"${slug}" is a reserved name`;
  return undefined;
}

/** A failure with a message meant for the agent (no stack trace). */
/** fsync a directory so a rename in it is durable. Platforms that cannot do it are ignored. */
export async function fsyncDir(dir: string): Promise<void> {
  let fh;
  try {
    fh = await open(dir, "r");
    await fh.sync();
  } catch (e) {
    const code = (e as NodeJS.ErrnoException).code;
    if (code !== "EISDIR" && code !== "EINVAL" && code !== "EPERM" && code !== "ENOTSUP") throw e;
  } finally {
    await fh?.close();
  }
}

export class StoreError extends Error {}

export interface Hashed {
  text: string;
  hash: string;
}
export const ABSENT = "absent";

export interface StoreConfig {
  /** Canon directory (docs/maps). */
  dir: string;
  /** Obsidian mirror directory; undefined = sync tools report "mirror not configured". */
  mirror?: string | undefined;
  now: () => string;
  genId: IdGenerator;
  genProposalId: () => string;
}

export type Pos = { x: number; y: number };
export interface FileState {
  project?: string;
  mirror?: string;
}
export interface MapState {
  layout: Record<NodeId, Pos>;
  pins: Record<NodeId, Pos>;
}
export interface SyncState {
  files: Record<string, FileState>;
  maps: Record<string, MapState>;
}

export type Source = "agent" | "owner";

export interface StoredProposal {
  id: string;
  slug: string;
  kind: "ops" | "create";
  baseHash: string;
  map: MindMap;
  md: string;
  diff: string;
  lint: LintIssue[];
  newIds: NodeId[];
  summary: string;
}

export const sha256 = (data: string | Buffer): string => createHash("sha256").update(data).digest("hex");

export const rels = (slug: string) => ({
  md: `${slug}.md`,
  canvas: `${slug}.canvas`,
  diagram: `${slug}.diagram.md`,
});

export const countErrors = (l: LintIssue[]): number => l.filter((i) => i.severity === "error").length;
export const countWarns = (l: LintIssue[]): number => l.filter((i) => i.severity === "warn").length;
export const formatLint = (l: LintIssue[]): string =>
  l.length === 0
    ? "lint: clean"
    : "lint:\n" +
      l.map((i) => `  ${i.severity.toUpperCase()} rule ${i.rule}${i.node ? ` [${i.node}]` : ""}: ${i.message}`).join("\n");

export class MapStore {
  private proposals = new Map<string, StoredProposal>();
  private tail: Promise<unknown> = Promise.resolve();
  constructor(readonly cfg: StoreConfig) {}

  /** Runs `fn` after every earlier exclusive operation finished. All mutating operations go through here. */
  exclusive<T>(fn: () => Promise<T>): Promise<T> {
    const run = this.tail.then(fn, fn);
    this.tail = run.catch(() => undefined);
    return run;
  }

  /** Remove leftover temp files of interrupted writes. Called once on server start. */
  async cleanupStaleTmp(): Promise<number> {
    let n = 0;
    for (const root of [this.cfg.dir, this.cfg.mirror]) {
      if (!root) continue;
      let names: string[] = [];
      try {
        names = await readdir(root);
      } catch {
        continue;
      }
      for (const f of names) {
        if (/^\..+\.tmp-[0-9a-f]{8}$/.test(f)) {
          await rm(path.join(root, f), { force: true });
          n++;
        }
      }
    }
    return n;
  }

  /** The mirror directory must already exist: the vault is the owner's, we never create it. */
  async requireMirror(): Promise<string> {
    const m = this.cfg.mirror;
    if (!m) throw new StoreError("mirror not configured (set MINDMAP_MIRROR)");
    try {
      if ((await stat(m)).isDirectory()) return m;
    } catch {
      /* fall through */
    }
    throw new StoreError(`mirror not found: ${m}`);
  }
  async hasPlaceholder(rel: string): Promise<boolean> {
    return !!this.cfg.mirror && (await this.readBytes(this.cfg.mirror, `.${rel}.icloud`)) !== undefined;
  }
  async readHashed(root: string, rel: string): Promise<Hashed | undefined> {
    const b = await this.readBytes(root, rel);
    return b === undefined ? undefined : { text: b.toString("utf8"), hash: sha256(b) };
  }
  /** True if `<slug>.md` exists in the project or the mirror, ignoring case (APFS is case-insensitive). */
  async existsAnyCase(slug: string): Promise<string | undefined> {
    for (const root of [this.cfg.dir, this.cfg.mirror]) {
      if (!root) continue;
      let names: string[] = [];
      try {
        names = await readdir(root);
      } catch {
        continue;
      }
      const hit = names.find((n) => n.toLowerCase() === `${slug}.md`);
      if (hit) return `${hit} (${root === this.cfg.dir ? "project" : "mirror"})`;
    }
    return undefined;
  }

  // ---- file primitives ----
  async readBytes(root: string, rel: string): Promise<Buffer | undefined> {
    try {
      return await readFile(path.join(root, rel));
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === "ENOENT") return undefined;
      throw e;
    }
  }
  async readText(root: string, rel: string): Promise<string | undefined> {
    return (await this.readBytes(root, rel))?.toString("utf8");
  }
  async hashOf(root: string, rel: string): Promise<string | undefined> {
    const b = await this.readBytes(root, rel);
    return b === undefined ? undefined : sha256(b);
  }
  async writeAtomic(root: string, rel: string, content: string, mk = true): Promise<void> {
    const target = path.join(root, rel);
    if (mk) await mkdir(path.dirname(target), { recursive: true });
    const tmp = path.join(path.dirname(target), `.${path.basename(target)}.tmp-${randomBytes(4).toString("hex")}`);
    try {
      const fh = await open(tmp, "w");
      try {
        await fh.writeFile(content, "utf8");
        await fh.sync();
      } finally {
        await fh.close();
      }
      await rename(tmp, target);
    } catch (e) {
      await rm(tmp, { force: true });
      throw e;
    }
    await fsyncDir(path.dirname(target));
  }
  async listSlugs(root: string): Promise<string[]> {
    let names: string[];
    try {
      names = await readdir(root);
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === "ENOENT") return [];
      throw e;
    }
    const out: string[] = [];
    for (const n of names) {
      if (!n.endsWith(".md") || n === "CHANGELOG.md" || n.endsWith(".diagram.md")) continue;
      const slug = n.slice(0, -3);
      if (SLUG_PATTERN.test(slug)) out.push(slug);
    }
    return out.sort();
  }

  // ---- state ----
  async loadState(): Promise<SyncState> {
    const text = await this.readText(this.cfg.dir, ".sync.json");
    if (text === undefined) return { files: {}, maps: {} };
    try {
      const raw = JSON.parse(text) as Partial<SyncState> | null;
      if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("not an object");
      return { files: raw.files ?? {}, maps: raw.maps ?? {} };
    } catch {
      throw new StoreError("docs/maps/.sync.json is corrupt; delete it to rebuild (owner edits will win)");
    }
  }
  async saveState(state: SyncState): Promise<void> {
    await this.writeAtomic(this.cfg.dir, ".sync.json", JSON.stringify(state, null, 2) + "\n");
  }
  mapState(state: SyncState, slug: string): MapState {
    return (state.maps[slug] ??= { layout: {}, pins: {} });
  }
  async appendChangelog(source: Source, slug: string, summary: string): Promise<void> {
    const old = (await this.readText(this.cfg.dir, "CHANGELOG.md")) ?? "# Map change log\n\n";
    const line = `- ${this.cfg.now()} ${slug} ${source}: ${summary.replace(/\s+/g, " ")}\n`;
    await this.writeAtomic(this.cfg.dir, "CHANGELOG.md", (old.endsWith("\n") ? old : old + "\n") + line);
  }

  // ---- rendering / publishing ----
  /** Generate canvas + diagram for `map`, recording the produced layout in state. */
  render(map: MindMap, state: SyncState): { canvas: string; diagram: string } {
    const ms = this.mapState(state, map.slug);
    const ids = new Set(collectIds(map.root));
    for (const id of Object.keys(ms.pins)) if (!ids.has(id)) delete ms.pins[id];
    const c = layout(map, ms.pins);
    ms.layout = Object.fromEntries(c.nodes.map((n) => [n.id, { x: n.x, y: n.y }]));
    return { canvas: canvasToJson(c), diagram: toMermaidNote(map) };
  }

  /**
   * Refuse to overwrite a mirror file the owner changed since the last sync. `consumed` holds mirror hashes the
   * caller already read and took into account (adopted md, absorbed canvas, warned-about diagram).
   */
  async guardMirror(rel: string, content: string, state: SyncState, consumed: Record<string, string>): Promise<void> {
    if (await this.hasPlaceholder(rel)) throw new StoreError(`${rel} is not downloaded from iCloud yet; wait and run map_sync`);
    const cur = await this.hashOf(this.cfg.mirror as string, rel);
    if (cur === undefined) return;
    if (cur === state.files[rel]?.mirror || cur === sha256(content) || cur === consumed[rel]) return;
    throw new StoreError(`owner edited ${rel} on the phone; run map_sync`);
  }

  /**
   * Write md (optional new content; otherwise the project's current file is kept), canvas and diagram to the
   * project, then copy all three to the mirror. Updates hashes in `state`. Every mirror write is guarded (see
   * guardMirror); all guards run once before anything is written, and again right before each mirror write.
   */
  async publish(map: MindMap, state: SyncState, mdContent?: string, consumed: Record<string, string> = {}): Promise<void> {
    const slug = map.slug;
    const r = rels(slug);
    const { canvas, diagram } = this.render(map, state);
    const md = mdContent ?? (await this.readText(this.cfg.dir, r.md));
    if (md === undefined) throw new StoreError(`${r.md} is missing in the project`);
    const out: Array<[string, string]> = [[r.md, md], [r.canvas, canvas], [r.diagram, diagram]];
    if (this.cfg.mirror) {
      await this.requireMirror();
      for (const [rel, content] of out) await this.guardMirror(rel, content, state, consumed);
    }
    for (const [rel, content] of out) await this.writeAtomic(this.cfg.dir, rel, content);
    for (const [rel, content] of out) {
      const fs: FileState = { project: sha256(content) };
      if (this.cfg.mirror) {
        await this.guardMirror(rel, content, state, consumed);
        await this.writeAtomic(this.cfg.mirror, rel, content, false);
        fs.mirror = fs.project as string;
      }
      state.files[rel] = fs;
    }
  }

  // ---- reading ----
  async loadMap(slug: string): Promise<{ ok: true; map: MindMap; md: string } | { ok: false; error: string }> {
    const md = await this.readText(this.cfg.dir, rels(slug).md);
    if (md === undefined) return { ok: false, error: `map "${slug}" not found` };
    const p = parse(md, this.cfg.genId);
    if (!p.ok) return { ok: false, error: `parse error in ${slug}.md: ` + p.errors.map((e) => `line ${e.line}: ${e.message}`).join("; ") };
    return { ok: true, map: p.map, md };
  }

  async syncStatus(slug: string, state: SyncState): Promise<string> {
    const mirror = this.cfg.mirror;
    if (!mirror) return "no-mirror";
    const r = rels(slug);
    let missing = false;
    let edited = false;
    for (const rel of [r.md, r.canvas, r.diagram]) {
      const mh = await this.hashOf(mirror, rel);
      if (mh === undefined) {
        if ((await this.readBytes(mirror, `.${rel}.icloud`)) !== undefined) return "not-downloaded";
        missing = true;
      } else if (mh !== state.files[rel]?.mirror) edited = true;
    }
    if (edited) return "owner-edited";
    return missing ? "mirror-missing" : "in-sync";
  }

  // ---- proposals ----
  getProposal(id: string): StoredProposal | undefined {
    return this.proposals.get(id);
  }
  dropProposal(id: string): void {
    this.proposals.delete(id);
  }

  async propose(slug: string, ops: Op[]): Promise<{ ok: true; p: StoredProposal } | { ok: false; error: string }> {
    const cur = await this.loadMap(slug);
    if (!cur.ok) return cur;
    const res = applyOps(cur.map, ops, this.cfg.genId);
    if (!res.ok) return { ok: false, error: `op ${res.error.opIndex} failed: ${res.error.message}` };
    const md = serialize(res.map);
    const summary = `${ops.length} op${ops.length === 1 ? "" : "s"} (${[...new Set(ops.map((o) => o.op))].join(", ")})`;
    return { ok: true, p: this.store({ slug, kind: "ops", baseHash: sha256(cur.md), map: res.map, md, before: cur.md, newIds: res.newIds, summary }) };
  }

  async proposeCreate(
    slug: string,
    level: MindMap["level"],
    center: string,
    branches: string[],
    colorMeaning?: string,
  ): Promise<{ ok: true; p: StoredProposal } | { ok: false; error: string }> {
    const bad = slugProblem(slug);
    if (bad) return { ok: false, error: bad };
    const taken = await this.existsAnyCase(slug);
    if (taken) return { ok: false, error: `map "${slug}" already exists: ${taken}` };
    const used = new Set<string>();
    const fresh = (): string => {
      const id = this.cfg.genId(used);
      used.add(id);
      return id;
    };
    const rootId = fresh();
    const map: MindMap = {
      slug,
      level,
      ...(colorMeaning !== undefined ? { colorMeaning } : {}),
      root: {
        id: rootId,
        label: center,
        children: branches.map((label, i) => ({
          id: fresh(),
          label,
          color: String((i % 6) + 1) as "1",
          children: [],
        })),
      },
      links: [],
    };
    const existing = await this.readText(this.cfg.dir, rels(slug).md);
    const md = serialize(map);
    return {
      ok: true,
      p: this.store({
        slug,
        kind: "create",
        baseHash: existing === undefined ? ABSENT : sha256(existing),
        map,
        md,
        before: existing ?? "",
        newIds: [...used],
        summary: `created (${level}, ${used.size} nodes)`,
      }),
    };
  }

  private store(a: {
    slug: string;
    kind: "ops" | "create";
    baseHash: string;
    map: MindMap;
    md: string;
    before: string;
    newIds: NodeId[];
    summary: string;
  }): StoredProposal {
    const p: StoredProposal = {
      id: this.cfg.genProposalId(),
      slug: a.slug,
      kind: a.kind,
      baseHash: a.baseHash,
      map: a.map,
      md: a.md,
      diff: lineDiff(a.before, a.md),
      lint: lint(a.map),
      newIds: a.newIds,
      summary: a.summary,
    };
    this.proposals.set(p.id, p);
    return p;
  }

  /**
   * Look at the owner's canvas in the mirror (if it changed since the last sync): keep moved cards as pins,
   * save a canvas with other edits (or invalid JSON) as `<slug>.canvas.conflict-<ts>` in the PROJECT dir, and
   * return the mirror hash that was consumed so the following publish may overwrite exactly that content.
   */
  async absorbCanvasPins(
    slug: string,
    state: SyncState,
    map: MindMap,
  ): Promise<{ changed: boolean; warnings: string[]; consumed: Record<string, string> }> {
    const mirror = this.cfg.mirror;
    const r = rels(slug);
    const warnings: string[] = [];
    const none = { changed: false, warnings, consumed: {} as Record<string, string> };
    if (!mirror) return none;
    const got = await this.readHashed(mirror, r.canvas);
    if (got === undefined || got.hash === state.files[r.canvas]?.mirror) return none;
    const consumed = { [r.canvas]: got.hash };
    const saveCopy = async (why: string): Promise<void> => {
      const name = `${r.canvas}.conflict-${this.cfg.now().replace(/[:.]/g, "-")}`;
      await this.writeAtomic(this.cfg.dir, name, got.text);
      warnings.push(`${r.canvas}: ${why}; the mirror version was saved as ${name} in the project and the canvas regenerated. Edit the outline, not the canvas`);
    };
    type Cv = { nodes?: Array<Record<string, unknown>>; edges?: Array<Record<string, unknown>> };
    let mc: Cv;
    try {
      mc = JSON.parse(got.text) as Cv;
      if (mc === null || typeof mc !== "object") throw new Error("not an object");
    } catch {
      await saveCopy("is not valid JSON");
      return { changed: false, warnings, consumed };
    }
    const ms = this.mapState(state, slug);
    const layoutKnown = Object.keys(ms.layout).length > 0;
    let changed = false;
    if (layoutKnown) {
      for (const n of mc.nodes ?? []) {
        const id = n["id"] as string;
        const prev = ms.layout[id];
        const x = n["x"];
        const y = n["y"];
        if (prev && typeof x === "number" && typeof y === "number" && (prev.x !== x || prev.y !== y)) {
          ms.pins[id] = { x, y };
          changed = true;
        }
      }
    }
    // Compare everything except positions with the last generated canvas (or a fresh one if none exists).
    const strip = (o: Record<string, unknown>): string => {
      const { x: _x, y: _y, ...rest } = o;
      return JSON.stringify(Object.entries(rest).sort(([a], [b]) => a.localeCompare(b)));
    };
    const keyed = (list: Array<Record<string, unknown>> | undefined) => new Map((list ?? []).map((o) => [String(o["id"]), strip(o)]));
    let base: Cv;
    const projText = await this.readText(this.cfg.dir, r.canvas);
    try {
      base = projText !== undefined ? (JSON.parse(projText) as Cv) : (JSON.parse(canvasToJson(layout(map, {}))) as Cv);
    } catch {
      base = JSON.parse(canvasToJson(layout(map, {}))) as Cv;
    }
    let other = false;
    for (const [a, b] of [
      [keyed(base.nodes), keyed(mc.nodes)],
      [keyed(base.edges), keyed(mc.edges)],
    ] as const) {
      if (a.size !== b.size) other = true;
      for (const [k, v] of a) if (b.get(k) !== v) other = true;
    }
    if (other) {
      await saveCopy("cards, text or edges were edited in the canvas; only positions are kept");
    } else if (!layoutKnown) {
      // No baseline: we cannot tell moves from the layout we are about to write, so keep a copy if they differ.
      const ours = new Map(layout(map, ms.pins).nodes.map((n) => [n.id, n]));
      const differs = (mc.nodes ?? []).some((n) => {
        const o = ours.get(String(n["id"]));
        return !o || o.x !== n["x"] || o.y !== n["y"];
      });
      if (differs) await saveCopy("no layout baseline and card positions differ from the generated layout");
    }
    return { changed, warnings, consumed };
  }

  /** Why the mirror blocks an apply/relayout of `slug`, or undefined if it does not. */
  async mirrorBlock(slug: string, state: SyncState, projectHash: string | undefined): Promise<string | undefined> {
    const mirror = this.cfg.mirror;
    if (!mirror) return undefined;
    await this.requireMirror();
    const r = rels(slug);
    for (const rel of [r.md, r.canvas, r.diagram]) {
      if (await this.hasPlaceholder(rel)) return `${rel} is not downloaded from iCloud yet; wait and run map_sync`;
    }
    const mm = await this.hashOf(mirror, r.md);
    const s = state.files[r.md]?.mirror;
    if (mm === undefined) {
      return s === undefined ? undefined : `owner deleted ${r.md} in the mirror; run map_sync (it is restored from the project), then propose again`;
    }
    if (mm === s) return undefined;
    if (s === undefined && mm === projectHash) return undefined;
    if (s === undefined && projectHash === undefined) {
      return `the mirror has ${r.md} that is not in the project (owner created it); run map_sync to import it`;
    }
    return `owner edited ${r.md} on the phone (mirror changed since last sync): run map_sync, then propose again`;
  }

  apply(id: string, source: Source = "agent"): Promise<{ ok: true; p: StoredProposal; warnings: string[] } | { ok: false; error: string }> {
    return this.exclusive(async () => {
      const p = this.proposals.get(id);
      if (!p) return { ok: false as const, error: `unknown proposal "${id}" (proposals live in memory, or it was already applied; propose again)` };
      const rel = rels(p.slug).md;
      const cur = await this.hashOf(this.cfg.dir, rel);
      if ((cur ?? ABSENT) !== p.baseHash) {
        return { ok: false as const, error: `${rel} changed since the proposal${p.baseHash === ABSENT ? " (the file now exists)" : cur === undefined ? " (the file was deleted)" : ""}; propose again` };
      }
      if (p.kind === "create") {
        const hit = await this.existsAnyCase(p.slug);
        if (hit) return { ok: false as const, error: `${hit} already exists; propose again` };
      }
      const errs = p.lint.filter((i) => i.severity === "error");
      if (errs.length > 0) return { ok: false as const, error: `lint errors block apply:\n${formatLint(errs)}` };
      const state = await this.loadState();
      const block = await this.mirrorBlock(p.slug, state, cur);
      if (block) return { ok: false as const, error: block };
      const abs = await this.absorbCanvasPins(p.slug, state, p.map);
      await this.publish(p.map, state, p.md, abs.consumed);
      await this.saveState(state);
      await this.appendChangelog(source, p.slug, p.summary);
      this.dropProposal(id);
      return { ok: true as const, p, warnings: abs.warnings };
    });
  }

  relayout(slug: string): Promise<{ ok: true; cleared: number; warnings: string[] } | { ok: false; error: string }> {
    return this.exclusive(async () => {
      const cur = await this.loadMap(slug);
      if (!cur.ok) return cur;
      const state = await this.loadState();
      const block = await this.mirrorBlock(slug, state, sha256(cur.md));
      if (block) return { ok: false as const, error: block };
      const abs = await this.absorbCanvasPins(slug, state, cur.map);
      const ms = this.mapState(state, slug);
      const cleared = Object.keys(ms.pins).length;
      ms.pins = {};
      await this.publish(cur.map, state, undefined, abs.consumed);
      await this.saveState(state);
      await this.appendChangelog("agent", slug, `relayout (cleared ${cleared} pin${cleared === 1 ? "" : "s"})`);
      return { ok: true as const, cleared, warnings: abs.warnings };
    });
  }
}
