import { describe, expect, it } from "vitest";
import { parse } from "../src/index.js";
import { seqGen } from "./helpers.js";

const fm = "---\nmap: m\nlevel: context\n---\n";

function errs(md: string) {
  const r = parse(md, seqGen());
  if (r.ok) throw new Error("expected parse failure");
  return r.errors;
}

describe("parse errors (structured, never thrown)", () => {
  it("no frontmatter", () => {
    expect(errs("# A\n")[0]).toMatchObject({ line: 1, message: expect.stringContaining("frontmatter") });
  });
  it("unterminated frontmatter", () => {
    expect(errs("---\nmap: m\n# A\n")[0]?.message).toMatch(/unterminated/);
  });
  it("malformed frontmatter yaml", () => {
    const e = errs("---\nmap: [oops\nlevel: context\n---\n# A\n");
    expect(e[0]?.message).toMatch(/malformed frontmatter/);
    expect(e[0]?.line).toBeGreaterThanOrEqual(2);
  });
  it("frontmatter missing map", () => {
    expect(errs("---\nlevel: context\n---\n# A\n").some((e) => /"map"/.test(e.message))).toBe(true);
  });
  it("unknown level", () => {
    expect(errs("---\nmap: m\nlevel: galaxy\n---\n# A\n")[0]?.message).toMatch(/unknown level/);
  });
  it("bad link entry", () => {
    expect(errs("---\nmap: m\nlevel: context\nlinks:\n  - { from: a }\n---\n# A\n")[0]?.message).toMatch(/links\[0\]/);
  });
  it("no # line", () => {
    expect(errs(fm + "- A\n")[0]).toMatchObject({ message: expect.stringContaining("before the # center") });
    expect(errs(fm)[0]?.message).toMatch(/missing # center/);
  });
  it("two # lines", () => {
    const e = errs(fm + "# A\n- x\n# B\n");
    expect(e).toEqual([{ line: 7, message: expect.stringContaining("second #") }]);
  });
  it("bad indent jump", () => {
    const e = errs(fm + "# A\n- x\n  - y\n      - z\n");
    expect(e[0]).toMatchObject({ line: 8, message: expect.stringContaining("indent jumps") });
  });
  it("odd first indent (1 or 3 spaces, 6 spaces) is rejected", () => {
    for (const ind of [" ", "   ", "      "]) {
      expect(errs(fm + `# A\n- x\n${ind}- y\n`)[0]?.message).toMatch(/tab, 2 spaces or 4 spaces/);
    }
  });
  it("mixed indentation units are an error naming the unit", () => {
    expect(errs(fm + "# A\n- x\n\t- y\n  - z\n")[0]).toMatchObject({ line: 8, message: expect.stringMatching(/mixed indentation.*tabs/) });
    expect(errs(fm + "# A\n- x\n  - y\n    - z\n\t- w\n").some((e) => /mixed indentation.*2-space/.test(e.message))).toBe(true);
    expect(errs(fm + "# A\n- x\n    - y\n  - z\n")[0]?.message).toMatch(/mixed indentation.*4-space/);
  });
  it("invalid id format has a line number", () => {
    expect(errs(fm + "# A\n- x <!-- id:ABC -->\n")[0]).toMatchObject({ line: 6, message: expect.stringContaining("invalid id") });
    expect(errs(fm + "# A <!-- id:toolong1 -->\n")[0]).toMatchObject({ line: 5 });
  });
  it("a foreign or malformed comment is an error, not silently kept", () => {
    expect(errs(fm + "# A\n- x <!-- hello --> <!-- id:abc123 -->\n")[0]?.message).toMatch(/unrecognized HTML comment/);
    expect(errs(fm + "# A\n- x <!-- id:abc123 --> <!-- id:abcdef -->\n")[0]?.message).toMatch(/unrecognized HTML comment/);
  });
  it("first item indented", () => {
    expect(errs(fm + "# A\n  - x\n")[0]?.message).toMatch(/indent jumps/);
  });
  it("non-list body line", () => {
    expect(errs(fm + "# A\nhello\n")[0]?.message).toMatch(/unexpected line/);
  });
  it("malformed node metadata", () => {
    expect(errs(fm + "# A\n- x <!-- id:abc123 color -->\n")[0]?.message).toMatch(/malformed or unrecognized/);
    expect(errs(fm + "# A\n- x <!-- id:abc123 flavor:1 -->\n")[0]?.message).toMatch(/unknown node key/);
    expect(errs(fm + "# A\n- x <!-- color:9 -->\n")[0]?.message).toMatch(/invalid color/);
    expect(errs(fm + '# A\n- x <!-- id:aaaaaa note:"../x" -->\n')[0]?.message).toMatch(/invalid note/);
    expect(errs(fm + "# A\n- x <!-- note:unquoted -->\n")[0]?.message).toMatch(/double-quoted/);
    expect(errs(fm + "# A\n- x <!-- id:aaaaaa id:bbbbbb -->\n")[0]?.message).toMatch(/duplicate "id"/);
  });
  it("collects several errors, sorted by line", () => {
    const e = errs(fm + "# A\n- x\n      - y\nhello\n");
    expect(e.map((x) => x.line)).toEqual([7, 8]);
  });
});
