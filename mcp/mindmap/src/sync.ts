import { readdir } from "node:fs/promises";
import { lint } from "./lint.js";
import { parse } from "./parse.js";
import { serialize } from "./serialize.js";
import { type MapStore, SLUG_PATTERN, type SyncState, rels } from "./store.js";
import type { LintIssue } from "./types.js";

export interface SlugReport {
  slug: string;
  actions: string[];
  warnings: string[];
  errors: string[];
  lint: LintIssue[];
}

export type SyncResult = { configured: false } | { configured: true; reports: SlugReport[] };

async function mirrorSlugs(store: MapStore, mirror: string): Promise<{ slugs: Set<string>; placeholders: Set<string>; badNames: string[] }> {
  const slugs = new Set<string>(await store.listSlugs(mirror));
  const placeholders = new Set<string>();
  let names: string[] = [];
  try {
    names = await readdir(mirror);
  } catch {
    /* mirror dir missing yet */
  }
  const badNames = names
    .filter((n) => n.endsWith(".md") && !n.startsWith(".") && n !== "CHANGELOG.md" && !n.endsWith(".diagram.md"))
    .filter((n) => !SLUG_PATTERN.test(n.slice(0, -3)))
    .sort();
  for (const n of names) {
    const m = /^\.(.+)\.icloud$/.exec(n);
    if (!m) continue;
    const real = m[1] as string;
    const slug = real.replace(/\.diagram\.md$/, "").replace(/\.(md|canvas)$/, "");
    if (real !== slug && SLUG_PATTERN.test(slug)) placeholders.add(slug);
  }
  return { slugs, placeholders, badNames };
}

export function syncAll(store: MapStore): Promise<SyncResult> {
  return store.exclusive(() => syncAllLocked(store));
}

async function syncAllLocked(store: MapStore): Promise<SyncResult> {
  const mirror = store.cfg.mirror;
  if (!mirror) return { configured: false };
  await store.requireMirror();
  const dir = store.cfg.dir;
  const state = await store.loadState();
  const mir = await mirrorSlugs(store, mirror);
  const all = new Set<string>([...(await store.listSlugs(dir)), ...mir.slugs, ...mir.placeholders]);
  const reports: SlugReport[] = [];

  for (const slug of [...all].sort()) {
    const rep: SlugReport = { slug, actions: [], warnings: [], errors: [], lint: [] };
    reports.push(rep);
    try {
      await syncSlug(store, state, slug, mir.placeholders.has(slug), rep);
    } catch (e) {
      rep.errors.push(`failed, remaining files left as they are: ${e instanceof Error ? e.message : String(e)}`);
    }
    await store.saveState(state);
  }
  for (const f of mir.badNames) {
    reports.push({ slug: f, actions: [], warnings: [`${f} ignored: map file names must be lowercase a-z, 0-9, -`], errors: [], lint: [] });
  }
  return { configured: true, reports };
}

async function syncSlug(store: MapStore, state: SyncState, slug: string, placeholder: boolean, rep: SlugReport): Promise<void> {
  const mirror = store.cfg.mirror as string;
  const dir = store.cfg.dir;
  if (placeholder) {
    rep.warnings.push("not downloaded from iCloud yet; skipped");
    return;
  }
  const r = rels(slug);
  // Each side is read once; hash and parse use the same bytes.
  const P = await store.readHashed(dir, r.md);
  const M = await store.readHashed(mirror, r.md);
  const s = state.files[r.md];
  const projectChanged = P?.hash !== s?.project;
  const mirrorChanged = M?.hash !== s?.mirror;

  let action: "none" | "adopt" | "conflict" = "none";
  if (M !== undefined && M.hash !== P?.hash && (P === undefined || mirrorChanged)) {
    action = P === undefined || !projectChanged ? "adopt" : "conflict";
  }
  const src = action === "none" ? P : M;
  if (src === undefined) return; // no md anywhere
  const parsed = parse(src.text, store.cfg.genId);
  if (!parsed.ok) {
    rep.errors.push(
      `${action === "none" ? "project" : "mirror"} ${r.md} does not parse, nothing changed: ` +
        parsed.errors.map((e) => `line ${e.line}: ${e.message}`).join("; "),
    );
    return;
  }
  if (parsed.map.slug !== slug) {
    rep.errors.push(`frontmatter map "${parsed.map.slug}" does not match file name ${r.md}; nothing changed`);
    return;
  }
  const map = parsed.map;
  const consumed: Record<string, string> = {};
  if (action !== "none" && M) consumed[r.md] = M.hash;

  const abs = await store.absorbCanvasPins(slug, state, map);
  rep.warnings.push(...abs.warnings);
  Object.assign(consumed, abs.consumed);
  if (abs.changed) rep.actions.push("kept canvas card positions as pins");
  const D = await store.readHashed(mirror, r.diagram);
  const diagramEdited = D !== undefined && D.hash !== state.files[r.diagram]?.mirror;
  if (diagramEdited) {
    rep.warnings.push(`${r.diagram} is generated; the mirror edit was overwritten`);
    consumed[r.diagram] = D.hash;
  }
  if (M === undefined && s?.mirror !== undefined) {
    rep.warnings.push(`${r.md} was deleted from the mirror; restored from the project`);
  }

  let needsWrite = action !== "none" || abs.changed || diagramEdited || projectChanged || parsed.assignedIds.length > 0 || M === undefined || abs.warnings.length > 0;
  for (const rel of [r.canvas, r.diagram]) {
    const ph = await store.hashOf(dir, rel);
    const mh = await store.hashOf(mirror, rel);
    if (ph === undefined || mh === undefined || ph !== state.files[rel]?.project) needsWrite = true;
    if (mh !== undefined && mh !== state.files[rel]?.mirror) needsWrite = true;
  }
  if (!needsWrite) return;

  let mdContent: string | undefined;
  let log: string | undefined;
  if (action === "conflict") {
    const ts = store.cfg.now().replace(/[:.]/g, "-");
    await store.writeAtomic(dir, `${r.md}.conflict-${ts}`, (P as { text: string }).text);
    rep.warnings.push(
      `CONFLICT: ${r.md} changed in the project and in the mirror; the owner's version won, the previous project version is kept as ${r.md}.conflict-${ts}`,
    );
    mdContent = serialize(map);
    rep.actions.push("adopted owner edit (conflict)");
    log = "owner edit adopted (conflict, project version kept)";
  } else if (action === "adopt") {
    mdContent = serialize(map);
    rep.actions.push(P === undefined ? "imported new map from the mirror" : "adopted owner edit");
    log = P === undefined ? "imported from the mirror" : "owner edit adopted";
  } else if (parsed.assignedIds.length > 0) {
    mdContent = serialize(map);
    rep.actions.push("assigned ids");
    log = `assigned ${parsed.assignedIds.length} id(s)`;
  } else if (projectChanged || M === undefined) {
    rep.actions.push("copied to the mirror");
  }
  if (parsed.assignedIds.length > 0 && action !== "none") rep.actions.push(`assigned ids to ${parsed.assignedIds.length} node(s)`);

  await store.publish(map, state, mdContent, consumed);
  // Only map content changes are logged; pure copies and regenerations are not.
  if (log) await store.appendChangelog(action === "none" ? "agent" : "owner", slug, log);
  rep.lint = lint(map);
}
