import { mkdtemp, readFile, writeFile, readdir, mkdir } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { createServer } from "../src/server.js";
import { MapStore } from "../src/store.js";
import { seqGen } from "./helpers.js";

export interface Harness {
  dir: string;
  mirror: string | undefined;
  store: MapStore;
  call(name: string, args?: Record<string, unknown>): Promise<{ text: string; isError: boolean }>;
  read(root: "dir" | "mirror", rel: string): Promise<string>;
  write(root: "dir" | "mirror", rel: string, content: string): Promise<void>;
  ls(root: "dir" | "mirror"): Promise<string[]>;
  close(): Promise<void>;
}

export async function makeHarness(opts: { mirror?: boolean } = {}): Promise<Harness> {
  const base = await mkdtemp(path.join(os.tmpdir(), "mindmap-test-"));
  const dir = path.join(base, "maps");
  const mirror = opts.mirror === false ? undefined : path.join(base, "vault-maps");
  await mkdir(dir, { recursive: true });
  if (mirror) await mkdir(mirror, { recursive: true });
  let t = 0;
  let pid = 0;
  const store = new MapStore({
    dir,
    mirror,
    now: () => new Date(Date.UTC(2026, 10, 12, 10, 0, t++)).toISOString(),
    genId: seqGen(),
    genProposalId: () => `p${++pid}`,
  });
  const server = createServer(store);
  const client = new Client({ name: "test", version: "0" });
  const [a, b] = InMemoryTransport.createLinkedPair();
  await Promise.all([server.connect(a), client.connect(b)]);
  const root = (r: "dir" | "mirror"): string => (r === "dir" ? dir : (mirror as string));
  return {
    dir,
    mirror,
    store,
    async call(name, args = {}) {
      const r = await client.callTool({ name, arguments: args });
      const text = (r.content as Array<{ text: string }>).map((c) => c.text).join("\n");
      return { text, isError: r.isError === true };
    },
    read: (r, rel) => readFile(path.join(root(r), rel), "utf8"),
    write: (r, rel, c) => writeFile(path.join(root(r), rel), c, "utf8"),
    ls: async (r) => (await readdir(root(r))).sort(),
    close: () => client.close(),
  };
}

/** create + apply a 5-branch map named `slug`; returns the new ids text. */
export async function createMap(h: Harness, slug = "demo"): Promise<string> {
  const c = await h.call("map_create", {
    slug,
    level: "context",
    center: "Bastion",
    branches: ["Owner", "Agents", "Vault", "Mirror", "Server"],
    colorMeaning: "trust zone",
  });
  if (c.isError) throw new Error(c.text);
  const id = /proposalId: (\S+)/.exec(c.text)?.[1] as string;
  const a = await h.call("map_apply", { proposalId: id });
  if (a.isError) throw new Error(a.text);
  return c.text;
}
