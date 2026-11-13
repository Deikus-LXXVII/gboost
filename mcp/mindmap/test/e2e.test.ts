import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { expect, it } from "vitest";

it("stdio: map_create -> map_apply -> map_read against the built server", async () => {
  const pkg = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const out = await mkdtemp(path.join(os.tmpdir(), "mindmap-e2e-"));
  execFileSync("npx", ["tsc", "-p", "tsconfig.build.json", "--outDir", path.join(out, "dist")], { cwd: pkg });
  // The build lands outside the package, so resolve dependencies from the package's node_modules.
  const { symlinkSync } = await import("node:fs");
  symlinkSync(path.join(pkg, "node_modules"), path.join(out, "node_modules"));
  const maps = path.join(out, "maps");
  const mirror = path.join(out, "mirror");
  await mkdir(mirror);
  const client = new Client({ name: "e2e", version: "0" });
  await client.connect(
    new StdioClientTransport({
      command: process.execPath,
      args: [path.join(out, "dist", "server.js")],
      env: { ...(process.env as Record<string, string>), MINDMAP_DIR: maps, MINDMAP_MIRROR: mirror },
    }),
  );
  try {
    const text = (r: unknown): string => ((r as { content: Array<{ text: string }> }).content[0] as { text: string }).text;
    const tools = (await client.listTools()).tools.map((t) => t.name).sort();
    expect(tools).toEqual(["map_apply", "map_create", "map_lint", "map_list", "map_propose", "map_read", "map_relayout", "map_sync"]);
    const c = text(await client.callTool({ name: "map_create", arguments: { slug: "e2e", level: "context", center: "Bastion", branches: ["A", "B", "C", "D", "E"], colorMeaning: "zone" } }));
    const id = /proposalId: (\S+)/.exec(c)?.[1] as string;
    expect(text(await client.callTool({ name: "map_apply", arguments: { proposalId: id } }))).toContain("applied e2e");
    const r = text(await client.callTool({ name: "map_read", arguments: { slug: "e2e" } }));
    expect(r).toContain("# Bastion [id:");
    expect(r).toContain("lint: clean");
    expect(await readFile(path.join(mirror, "e2e.md"), "utf8")).toContain("# Bastion");
  } finally {
    await client.close();
  }
}, 60_000);
