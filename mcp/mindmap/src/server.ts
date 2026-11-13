#!/usr/bin/env node
import { randomBytes } from "node:crypto";
import { realpathSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { defaultIdGenerator, collectIds } from "./id.js";
import { lint } from "./lint.js";
import { LEVELS } from "./parse.js";
import { MapStore, MAX_SLUG, StoreError, type StoreConfig, countErrors, countWarns, formatLint, slugProblem } from "./store.js";
import { syncAll } from "./sync.js";
import type { MapNode, MindMap } from "./types.js";

type Text = { content: Array<{ type: "text"; text: string }>; isError?: boolean };
const ok = (text: string): Text => ({ content: [{ type: "text", text }] });
const fail = (text: string): Text => ({ content: [{ type: "text", text }], isError: true });

const slugSchema = z.string().max(MAX_SLUG).superRefine((v, ctx) => {
  const bad = slugProblem(v);
  if (bad) ctx.addIssue({ code: "custom", message: bad });
});

/** Turn thrown errors into tool errors so the agent sees the message. */
function safe<A>(fn: (a: A) => Promise<Text>): (a: A) => Promise<Text> {
  return async (a) => {
    try {
      return await fn(a);
    } catch (e) {
      if (e instanceof StoreError) return fail(e.message);
      return fail(`internal error: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
}
const colorSchema = z.enum(["1", "2", "3", "4", "5", "6"]);
const opSchema = z.discriminatedUnion("op", [
  z.object({ op: z.literal("add"), parent: z.string(), label: z.string(), after: z.string().optional() }),
  z.object({ op: z.literal("rename"), id: z.string(), label: z.string() }),
  z.object({ op: z.literal("move"), id: z.string(), parent: z.string(), after: z.string().optional() }),
  z.object({ op: z.literal("remove"), id: z.string() }),
  z.object({ op: z.literal("link"), from: z.string(), to: z.string(), label: z.string() }),
  z.object({ op: z.literal("unlink"), from: z.string(), to: z.string() }),
  z.object({ op: z.literal("color"), id: z.string(), color: colorSchema }),
  z.object({ op: z.literal("note"), id: z.string(), note: z.string().nullable() }),
]);

function tree(map: MindMap): string {
  const out: string[] = [];
  const walk = (n: MapNode, depth: number): void => {
    const meta = [`id:${n.id}`, ...(n.color ? [`color:${n.color}`] : []), ...(n.note ? [`note:${n.note}`] : [])];
    out.push(`${"  ".repeat(depth)}${depth === 0 ? "#" : "-"} ${n.label} [${meta.join(" ")}]`);
    n.children.forEach((c) => walk(c, depth + 1));
  };
  walk(map.root, 0);
  return out.join("\n");
}

export function createServer(store: MapStore): McpServer {
  const server = new McpServer({ name: "mindmap", version: "0.1.0" });

  server.registerTool(
    "map_list",
    { description: "List maps with level, node count, lint counts and sync status: in-sync | owner-edited (mirror differs from the last sync; run map_sync) | mirror-missing (a mirror file is absent) | not-downloaded (iCloud placeholder) | no-mirror." },
    safe(async () => {
      const state = await store.loadState();
      const slugs = await store.listSlugs(store.cfg.dir);
      if (slugs.length === 0) return ok("no maps yet; use map_create");
      const lines: string[] = [];
      for (const slug of slugs) {
        const m = await store.loadMap(slug);
        const sync = await store.syncStatus(slug, state);
        if (!m.ok) {
          lines.push(`${slug}: ${m.error} sync=${sync}`);
          continue;
        }
        const l = lint(m.map);
        lines.push(`${slug}: level=${m.map.level} nodes=${collectIds(m.map.root).length} lint=${countErrors(l)}E/${countWarns(l)}W sync=${sync}`);
      }
      return ok(lines.join("\n"));
    }),
  );

  server.registerTool(
    "map_read",
    { description: "Read a map as an indented tree with ids, colors, notes, cross-links and lint.", inputSchema: { slug: slugSchema } },
    safe(async ({ slug }) => {
      const m = await store.loadMap(slug);
      if (!m.ok) return fail(m.error);
      const links = m.map.links.map((l) => `  ${l.from} -> ${l.to}: ${l.label}`);
      return ok(
        [
          `map ${slug} level=${m.map.level}${m.map.colorMeaning ? ` color_meaning="${m.map.colorMeaning}"` : ""}`,
          tree(m.map),
          links.length ? `links:\n${links.join("\n")}` : "links: none",
          formatLint(lint(m.map)),
        ].join("\n"),
      );
    }),
  );

  server.registerTool(
    "map_lint",
    { description: "Check a map against the Bastion map rules.", inputSchema: { slug: slugSchema } },
    safe(async ({ slug }) => {
      const m = await store.loadMap(slug);
      return m.ok ? ok(formatLint(lint(m.map))) : fail(m.error);
    }),
  );

  const proposalText = (p: { id: string; slug: string; diff: string; lint: Parameters<typeof formatLint>[0]; newIds: string[] }): string =>
    [
      `proposalId: ${p.id} (map ${p.slug}; nothing written yet, call map_apply)`,
      `newIds: ${p.newIds.length ? p.newIds.join(", ") : "none"}`,
      formatLint(p.lint),
      countErrors(p.lint) > 0 ? "apply will be refused until lint errors are fixed" : "",
      "diff:",
      p.diff,
    ]
      .filter((s) => s !== "")
      .join("\n");

  server.registerTool(
    "map_propose",
    { description: "Dry run: apply ops in memory, return proposalId, diff, lint and new ids. Never writes.", inputSchema: { slug: slugSchema, ops: z.array(opSchema).min(1) } },
    safe(async ({ slug, ops }) => {
      const r = await store.propose(slug, ops);
      return r.ok ? ok(proposalText(r.p)) : fail(r.error);
    }),
  );

  server.registerTool(
    "map_create",
    {
      description: "Propose a new map (same apply path as map_propose). Branches are the main branches under the center.",
      inputSchema: {
        slug: slugSchema,
        level: z.enum(LEVELS as unknown as [string, ...string[]]),
        center: z.string().min(1),
        branches: z.array(z.string().min(1)),
        colorMeaning: z.string().optional(),
      },
    },
    safe(async ({ slug, level, center, branches, colorMeaning }) => {
      const r = await store.proposeCreate(slug, level as MindMap["level"], center, branches, colorMeaning);
      return r.ok ? ok(proposalText(r.p)) : fail(r.error);
    }),
  );

  server.registerTool(
    "map_apply",
    { description: "Apply a proposal: write md, canvas and diagram, copy to the mirror, update the change log. No owner approval needed.", inputSchema: { proposalId: z.string() } },
    safe(async ({ proposalId }) => {
      const r = await store.apply(proposalId);
      if (!r.ok) return fail(`refused: ${r.error}`);
      const mirror = store.cfg.mirror ? "mirrored" : "mirror not configured (project only)";
      return ok([`applied ${r.p.slug}: ${r.p.summary}; ${mirror}`, ...r.warnings.map((w) => `warning: ${w}`), formatLint(r.p.lint)].join("\n"));
    }),
  );

  server.registerTool(
    "map_sync",
    { description: "Two-way sync with the Obsidian mirror. Owner edits made on the phone win and are adopted into the project." },
    safe(async () => {
      const res = await syncAll(store);
      if (!res.configured) return fail("mirror not configured (set MINDMAP_MIRROR)");
      if (res.reports.length === 0) return ok("no maps to sync");
      const lines: string[] = [];
      for (const r of res.reports) {
        const status = r.errors.length ? "ERROR" : r.actions.length ? r.actions.join("; ") : "in sync";
        lines.push(`${r.slug}: ${status}`);
        r.errors.forEach((e) => lines.push(`  error: ${e}`));
        r.warnings.forEach((w) => lines.push(`  warning: ${w}`));
        if (r.actions.length) lines.push("  " + formatLint(r.lint).replace(/\n/g, "\n  "));
      }
      return ok(lines.join("\n"));
    }),
  );

  server.registerTool(
    "map_relayout",
    { description: "Clear owner-moved card positions (pins) for a map, regenerate the canvas and mirror it.", inputSchema: { slug: slugSchema } },
    safe(async ({ slug }) => {
      const r = await store.relayout(slug);
      return r.ok ? ok([`relayout ${slug}: cleared ${r.cleared} pin(s), canvas regenerated`, ...r.warnings.map((w) => `warning: ${w}`)].join("\n")) : fail(r.error);
    }),
  );

  return server;
}

const expandHome = (p: string): string => (p === "~" || p.startsWith("~/") ? path.join(os.homedir(), p.slice(1)) : p);

export function configFromEnv(env: NodeJS.ProcessEnv = process.env): StoreConfig {
  const here = path.dirname(fileURLToPath(import.meta.url)); // <repo>/mcp/mindmap/{src,dist}
  const dir = env["MINDMAP_DIR"] ? path.resolve(expandHome(env["MINDMAP_DIR"])) : path.resolve(here, "..", "..", "..", "docs", "maps");
  const mirror = env["MINDMAP_MIRROR"] ? path.resolve(expandHome(env["MINDMAP_MIRROR"])) : undefined;
  return {
    dir,
    mirror,
    now: () => new Date().toISOString(),
    genId: defaultIdGenerator,
    genProposalId: () => randomBytes(6).toString("hex"),
  };
}

async function main(): Promise<void> {
  const store = new MapStore(configFromEnv());
  await store.cleanupStaleTmp();
  const server = createServer(store);
  await server.connect(new StdioServerTransport());
}

const entry = process.argv[1];
if (entry && realpathSync(entry) === realpathSync(fileURLToPath(import.meta.url))) {
  main().catch((e) => {
    console.error(e);
    process.exit(1);
  });
}
