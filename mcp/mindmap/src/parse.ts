import { parse as parseYaml, YAMLParseError } from "yaml";
import { isValidId } from "./id.js";
import { notePathError } from "./notepath.js";
import type {
  C4Level,
  ColorPreset,
  CrossLink,
  IdGenerator,
  MapNode,
  NodeId,
  ParseError,
  ParseResult,
} from "./types.js";

export const LEVELS: readonly C4Level[] = ["context", "container", "component", "concept"];
export const COLORS: readonly ColorPreset[] = ["1", "2", "3", "4", "5", "6"];

interface Draft {
  label: string;
  id?: string;
  note?: string;
  color?: ColorPreset;
  children: Draft[];
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

function parseFrontmatter(
  text: string,
  firstLine: number,
  errors: ParseError[],
): { slug: string; level: C4Level; colorMeaning?: string; links: CrossLink[]; extra?: Record<string, unknown> } | undefined {
  let raw: unknown;
  try {
    // failsafe schema: every scalar stays a string (ids like 123456 are not numbers)
    raw = parseYaml(text, { schema: "failsafe", logLevel: "error" });
  } catch (e) {
    const line = e instanceof YAMLParseError && e.linePos ? firstLine + e.linePos[0].line - 1 : firstLine;
    const msg = e instanceof Error ? e.message.split("\n")[0] : String(e);
    errors.push({ line, message: `malformed frontmatter: ${msg}` });
    return undefined;
  }
  const before = errors.length;
  const err = (message: string): void => void errors.push({ line: firstLine, message });
  if (!isRecord(raw)) {
    err("frontmatter must be a YAML mapping with map, level");
    return undefined;
  }
  const known = new Set(["map", "level", "color_meaning", "links"]);
  // Other keys (Obsidian Properties: tags, aliases, ...) are preserved verbatim, in order.
  const extra: Record<string, unknown> = {};
  for (const k of Object.keys(raw)) if (!known.has(k)) extra[k] = raw[k];

  const slug = raw["map"];
  if (typeof slug !== "string" || slug.trim() === "") err('frontmatter "map" (slug) is required');
  const level = raw["level"];
  if (typeof level !== "string" || !(LEVELS as readonly string[]).includes(level)) {
    err(`unknown level ${JSON.stringify(level)}; expected one of ${LEVELS.join(", ")}`);
  }
  const cm = raw["color_meaning"];
  if (cm !== undefined && typeof cm !== "string") err('"color_meaning" must be a string');

  const links: CrossLink[] = [];
  const rawLinks = raw["links"];
  if (rawLinks !== undefined && rawLinks !== null && rawLinks !== "") {
    if (!Array.isArray(rawLinks)) err('"links" must be a list');
    else {
      rawLinks.forEach((l: unknown, i) => {
        if (
          !isRecord(l) ||
          typeof l["from"] !== "string" ||
          typeof l["to"] !== "string" ||
          typeof l["label"] !== "string"
        ) {
          err(`links[${i}] must have string from, to, label`);
        } else {
          links.push({ from: l["from"], to: l["to"], label: l["label"] });
        }
      });
    }
  }
  if (errors.length > before) return undefined;
  return {
    slug: slug as string,
    level: level as C4Level,
    ...(typeof cm === "string" ? { colorMeaning: cm } : {}),
    links,
    ...(Object.keys(extra).length > 0 ? { extra } : {}),
  };
}

const META_TOKEN = /\s*([a-z]+):("(?:[^"\\]|\\.)*"|[^\s"]+)/y;

function tokenize(body: string): [string, string][] | undefined {
  const out: [string, string][] = [];
  let pos = 0;
  while (body.slice(pos).trim() !== "") {
    META_TOKEN.lastIndex = pos;
    const m = META_TOKEN.exec(body);
    if (!m) return undefined;
    pos = META_TOKEN.lastIndex;
    out.push([m[1] as string, m[2] as string]);
  }
  return out;
}

function parseText(text: string, lineNo: number, errors: ParseError[]): Omit<Draft, "children"> {
  // The metadata comment may sit anywhere in the line (a phone edit can add text after it).
  let found: { start: number; end: number; tokens: [string, string][] } | undefined;
  for (const m of text.matchAll(/<!--([\s\S]*?)-->/g)) {
    const tokens = tokenize(m[1] as string);
    if (tokens) {
      found = { start: m.index, end: m.index + m[0].length, tokens };
      break;
    }
  }
  const hasMarker = (l: string): boolean => l.includes("<!--") || l.includes("-->");
  if (!found) {
    const label = text.trim();
    if (hasMarker(label)) errors.push({ line: lineNo, message: `malformed or unrecognized HTML comment in "${label}"` });
    return { label };
  }
  const label = [text.slice(0, found.start).trim(), text.slice(found.end).trim()].filter(Boolean).join(" ");
  if (hasMarker(label)) {
    errors.push({ line: lineNo, message: `malformed or unrecognized HTML comment in "${label}"` });
  }
  const draft: Omit<Draft, "children"> = { label };
  const seen = new Set<string>();
  for (const [key, val] of found.tokens) {
    if (seen.has(key)) {
      errors.push({ line: lineNo, message: `duplicate "${key}" in node comment` });
      continue;
    }
    seen.add(key);
    if (key === "id") {
      if (!isValidId(val)) errors.push({ line: lineNo, message: `invalid id "${val}"; expected 6 characters of [a-z0-9]` });
      draft.id = val;
    } else if (key === "color") {
      if ((COLORS as readonly string[]).includes(val)) draft.color = val as ColorPreset;
      else errors.push({ line: lineNo, message: `invalid color "${val}"; expected 1-6` });
    } else if (key === "note") {
      try {
        const v: unknown = JSON.parse(val);
        if (typeof v !== "string") throw new Error("not a string");
        const err = notePathError(v);
        if (err) throw new Error(err);
        draft.note = v;
      } catch {
        errors.push({ line: lineNo, message: `invalid note ${val}: expected a double-quoted, safe relative path` });
      }
    } else {
      errors.push({ line: lineNo, message: `unknown node key "${key}"` });
    }
  }
  return draft;
}

export function parse(md: string, genId: IdGenerator): ParseResult {
  const lines = md.replace(/\r\n?/g, "\n").split("\n");
  const errors: ParseError[] = [];
  const fail = (line: number, message: string): ParseResult => ({ ok: false, errors: [{ line, message }] });

  if ((lines[0] ?? "").trim() !== "---") return fail(1, "missing frontmatter: file must start with ---");
  let end = -1;
  for (let j = 1; j < lines.length; j++) {
    if ((lines[j] ?? "").trim() === "---") {
      end = j;
      break;
    }
  }
  if (end < 0) return fail(1, "unterminated frontmatter: missing closing ---");

  const fm = parseFrontmatter(lines.slice(1, end).join("\n"), 2, errors);

  let root: Draft | undefined;
  const stack: Draft[] = [];
  let unit: string | undefined;
  for (let k = end + 1; k < lines.length; k++) {
    const raw = lines[k] as string;
    const lineNo = k + 1;
    if (raw.trim() === "") continue;
    if (/^#(\s|$)/.test(raw)) {
      if (root) {
        errors.push({ line: lineNo, message: "second # line: a map has exactly one center" });
        continue;
      }
      root = { ...parseText(raw.slice(1), lineNo, errors), children: [] };
      continue;
    }
    const m = /^([ \t]*)-(?: ([^\n]*))?$/.exec(raw);
    if (!m) {
      errors.push({ line: lineNo, message: `unexpected line "${raw.trim()}"; expected "- " list item` });
      continue;
    }
    if (!root) {
      errors.push({ line: lineNo, message: "list item before the # center line" });
      continue;
    }
    const indent = m[1] as string;
    let depth = 0;
    if (indent !== "") {
      if (unit === undefined) {
        // the first indented line fixes the unit: a tab, 2 spaces or 4 spaces
        if (indent.startsWith("\t")) unit = "\t";
        else if (indent === "  " || indent === "    ") unit = indent;
      }
      if (unit === undefined) {
        errors.push({ line: lineNo, message: "indent must be a tab, 2 spaces or 4 spaces" });
        continue;
      }
      if (indent.length % unit.length !== 0 || indent !== unit.repeat(indent.length / unit.length)) {
        const name = unit === "\t" ? "tabs" : `${unit.length}-space indents`;
        errors.push({ line: lineNo, message: `mixed indentation: this file uses ${name}` });
        continue;
      }
      depth = indent.length / unit.length;
    }
    if (depth > stack.length) {
      errors.push({ line: lineNo, message: `indent jumps from level ${stack.length} to ${depth}` });
      continue;
    }
    stack.length = depth;
    const node: Draft = { ...parseText(m[2] ?? "", lineNo, errors), children: [] };
    (depth === 0 ? root : (stack[depth - 1] as Draft)).children.push(node);
    stack.push(node);
  }
  if (!root && !errors.some((e) => e.message.startsWith("list item before"))) {
    errors.push({ line: end + 2, message: "missing # center line" });
  }
  if (errors.length > 0 || !root || !fm) {
    return { ok: false, errors: errors.sort((a, b) => a.line - b.line) };
  }

  // Assign ids to nodes without one, in document order.
  const existing = new Set<NodeId>();
  const collect = (d: Draft): void => {
    if (d.id !== undefined) existing.add(d.id);
    d.children.forEach(collect);
  };
  collect(root);
  const assignedIds: NodeId[] = [];
  const claimed = new Set<NodeId>();
  const build = (d: Draft): MapNode => {
    let id = d.id;
    if (id !== undefined && claimed.has(id)) id = undefined; // copy-pasted line: first keeps the id
    if (id === undefined) {
      id = genId(existing);
      if (!isValidId(id) || existing.has(id)) {
        throw new Error(`id generator returned an unusable id "${id}"`);
      }
      existing.add(id);
      assignedIds.push(id);
    }
    claimed.add(id);
    const n: MapNode = { id, label: d.label, children: [] };
    if (d.note !== undefined) n.note = d.note;
    if (d.color !== undefined) n.color = d.color;
    n.children = d.children.map(build);
    return n;
  };
  const map = {
    slug: fm.slug,
    level: fm.level,
    ...(fm.colorMeaning !== undefined ? { colorMeaning: fm.colorMeaning } : {}),
    root: build(root),
    links: fm.links,
    ...(fm.extra !== undefined ? { extra: fm.extra } : {}),
  };
  return { ok: true, map, assignedIds };
}
