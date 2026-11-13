import { collectIds, isValidId } from "./id.js";
import { notePathError } from "./notepath.js";
import { COLORS } from "./parse.js";
import type { ColorPreset, IdGenerator, MapNode, MindMap, NodeId, Op, OpResult } from "./types.js";

export function findNode(root: MapNode, id: NodeId): MapNode | undefined {
  if (root.id === id) return root;
  for (const c of root.children) {
    const f = findNode(c, id);
    if (f) return f;
  }
  return undefined;
}

function findParent(root: MapNode, id: NodeId): MapNode | undefined {
  for (const c of root.children) {
    if (c.id === id) return root;
    const f = findParent(c, id);
    if (f) return f;
  }
  return undefined;
}

/** Explicit color of the node, else of its nearest colored ancestor. */
export function effectiveColor(map: MindMap, id: NodeId): ColorPreset | undefined {
  let found: ColorPreset | undefined;
  const walk = (n: MapNode, inherited: ColorPreset | undefined): boolean => {
    const c = n.color ?? inherited;
    if (n.id === id) {
      found = c;
      return true;
    }
    return n.children.some((ch) => walk(ch, c));
  };
  walk(map.root, undefined);
  return found;
}

class OpError extends Error {}
const bad = (message: string): never => {
  throw new OpError(message);
};

function cleanLabel(label: unknown): string {
  if (typeof label !== "string") return bad("label must be a string");
  const t = label.trim();
  if (t === "") return bad("label must not be empty");
  if (/[\r\n\u2028\u2029\u0085]/.test(t)) return bad("label must be a single line");
  if (t.includes("<!--") || t.includes("-->")) return bad("label must not contain HTML comment markers");
  return t;
}

function need(map: MindMap, id: NodeId, what = "node"): MapNode {
  return findNode(map.root, id) ?? bad(`unknown ${what} id "${id}"`);
}

function insertAt(parent: MapNode, node: MapNode, after: NodeId | undefined): void {
  if (after === undefined) {
    parent.children.push(node);
    return;
  }
  const i = parent.children.findIndex((c) => c.id === after);
  if (i < 0) bad(`"after" id "${after}" is not a child of "${parent.id}"`);
  parent.children.splice(i + 1, 0, node);
}

function apply(map: MindMap, op: Op, genId: IdGenerator, newIds: NodeId[]): void {
  switch (op.op) {
    case "add": {
      const parent = need(map, op.parent, "parent");
      const label = cleanLabel(op.label);
      const existing = new Set(collectIds(map.root));
      const id = genId(existing);
      if (!isValidId(id) || existing.has(id)) bad(`id generator returned an unusable id "${id}"`);
      insertAt(parent, { id, label, children: [] }, op.after);
      newIds.push(id);
      return;
    }
    case "rename": {
      const n = need(map, op.id);
      n.label = cleanLabel(op.label);
      return;
    }
    case "move": {
      const n = need(map, op.id);
      if (n.id === map.root.id) bad("the root cannot be moved");
      const target = need(map, op.parent, "parent");
      if (findNode(n, target.id)) bad("cannot move a node under itself or its descendant");
      if (op.after === n.id) bad("cannot place a node after itself");
      if (op.after !== undefined && !target.children.some((c) => c.id === op.after)) {
        bad(`"after" id "${op.after}" is not a child of "${target.id}"`);
      }
      const oldParent = findParent(map.root, n.id) as MapNode;
      oldParent.children = oldParent.children.filter((c) => c.id !== n.id);
      insertAt(target, n, op.after);
      return;
    }
    case "remove": {
      const n = need(map, op.id);
      if (n.id === map.root.id) bad("the root cannot be removed");
      const gone = new Set(collectIds(n));
      const parent = findParent(map.root, n.id) as MapNode;
      parent.children = parent.children.filter((c) => c.id !== n.id);
      map.links = map.links.filter((l) => !gone.has(l.from) && !gone.has(l.to));
      return;
    }
    case "link": {
      need(map, op.from, "from");
      need(map, op.to, "to");
      if (op.from === op.to) bad("a link cannot connect a node to itself");
      const label = cleanLabel(op.label);
      if (map.links.some((l) => l.from === op.from && l.to === op.to)) {
        bad(`link ${op.from} -> ${op.to} already exists`);
      }
      map.links.push({ from: op.from, to: op.to, label });
      return;
    }
    case "unlink": {
      const i = map.links.findIndex((l) => l.from === op.from && l.to === op.to);
      if (i < 0) bad(`no link ${op.from} -> ${op.to}`);
      map.links.splice(i, 1);
      return;
    }
    case "color": {
      const n = need(map, op.id);
      if (!(COLORS as readonly string[]).includes(op.color)) bad(`invalid color "${String(op.color)}"; expected 1-6`);
      n.color = op.color;
      return;
    }
    case "note": {
      const n = need(map, op.id);
      if (op.note === null) delete n.note;
      else if (typeof op.note !== "string" || op.note === "") bad("note must be a non-empty string or null");
      else {
        const err = notePathError(op.note);
        if (err) bad(err);
        n.note = op.note;
      }
      return;
    }
    default:
      bad(`unknown op "${String((op as { op?: unknown }).op)}"`);
  }
}

/** All-or-nothing: the input map is never mutated; on failure nothing is returned but the error. */
export function applyOps(map: MindMap, ops: Op[], genId: IdGenerator): OpResult {
  const work = structuredClone(map);
  const newIds: NodeId[] = [];
  for (let i = 0; i < ops.length; i++) {
    try {
      apply(work, ops[i] as Op, genId, newIds);
    } catch (e) {
      if (e instanceof OpError) return { ok: false, error: { opIndex: i, message: e.message } };
      throw e;
    }
  }
  return { ok: true, map: work, newIds };
}
