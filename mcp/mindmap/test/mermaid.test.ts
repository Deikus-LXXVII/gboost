import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { parse, toMermaidNote } from "../src/index.js";
import { baseMap, seqGen } from "./helpers.js";

const md = readFileSync(new URL("./fixtures/bastion-context.md", import.meta.url), "utf8");
const parsed = parse(md, seqGen());
if (!parsed.ok) throw new Error("fixture");

describe("toMermaidNote", () => {
  it("fixture snapshot", () => {
    expect(toMermaidNote(parsed.map)).toMatchInlineSnapshot(`
      "---
      generated_from: bastion-context.md
      # Generated file. Do not edit: changes are overwritten on the next run.
      ---
      \`\`\`mermaid
      mindmap
        root01(("Bastion"))
          own000["Owner"]
            own001["Phone"]
            own002["Desktop"]
          agt000["Agents"]
            agt001["Claude Code"]
            agt002["Grok sister"]
          gat000["Gateway"]
            gat001["Approval queue"]
          vlt000["Vault"]
            vlt001["Obsidian maps"]
          ext000["External services"]
            ext001["Model APIs"]
          aud000["Audit log"]
      \`\`\`

      ## Links

      - Phone → Approval queue: approves
      - Claude Code → Model APIs: calls, read-only
      "
    `);
  });

  it("escapes quotes and keeps labels in the id[\"...\"] form", () => {
    const m = baseMap();
    m.root.children[0]!.label = 'Say "hi" (a) [b]';
    const out = toMermaidNote(m);
    expect(out).toContain('  b00001["Say #quot;hi#quot; (a) [b]"]');
  });

  it("has frontmatter first and links in their own section after the block", () => {
    const out = toMermaidNote(parsed.map);
    expect(out.startsWith("---\ngenerated_from: bastion-context.md\n")).toBe(true);
    const block = out.indexOf("```mermaid");
    const end = out.indexOf("```", block + 3);
    expect(out.slice(end + 3).trimStart().startsWith("## Links")).toBe(true);
    expect(out).toContain("- Phone → Approval queue: approves");
  });

  it("omits the Links section when there are no links", () => {
    expect(toMermaidNote(baseMap())).not.toContain("## Links");
  });

  it("escapes #, quotes, backticks, angle brackets; empty label becomes #nbsp;", () => {
    const m = baseMap();
    m.root.children[0]!.label = 'a#b "q" `c` <i>';
    m.root.children[1]!.label = "";
    const out = toMermaidNote(m);
    expect(out).toContain('b00001["a#35;b #quot;q#quot; #96;c#96; #lt;i#gt;"]');
    expect(out).toContain('b00002["#nbsp;"]');
  });

  it("escapes Markdown in the Links list", () => {
    const m = baseMap();
    m.root.children[0]!.label = "*A*";
    m.links = [{ from: "b00001", to: "b00002", label: "# [[x]] a|b" }];
    expect(toMermaidNote(m)).toContain("- \\*A\\* → Branch 2: \\# \\[\\[x\\]\\] a\\|b");
  });
});
