import { flextree } from "d3-flextree";
import type { Canvas, CanvasEdge, CanvasSide, CanvasTextNode } from "./canvas.js";
import { notePathError } from "./notepath.js";
import { effectiveColor } from "./ops.js";
import type { MapNode, MindMap, NodeId } from "./types.js";

export const MIN_WIDTH = 160;
export const MAX_WIDTH = 320;
export const SIBLING_GAP = 32;
export const LEVEL_GAP = 120;

const UNIT_PX = 10; // width of one unit; CJK and emoji count as two units
const PAD_X = 32;
const LINE_PX = 24;
const PAD_Y = 32;
const MIN_HEIGHT = 64;

const norm = (v: number): number => (v === 0 ? 0 : v); // -0 -> 0
const units = (ch: string): number => ((ch.codePointAt(0) ?? 0) > 0x2e80 ? 2 : 1);
const labelUnits = (label: string): number => Array.from(label).reduce((n, ch) => n + units(ch), 0);

export function cardWidth(label: string): number {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, labelUnits(label) * UNIT_PX + PAD_X));
}

/** Greedy word wrap; words longer than a line are split. Returns the number of lines. */
export function wrappedLines(label: string, width: number): number {
  const perLine = Math.max(2, Math.floor((width - PAD_X) / UNIT_PX));
  let lines = 1;
  let cur = 0;
  for (const word of label.split(/\s+/).filter(Boolean)) {
    const len = labelUnits(word);
    if (cur > 0 && cur + 1 + len <= perLine) {
      cur += 1 + len;
      continue;
    }
    if (cur > 0) lines++;
    cur = 0;
    for (const ch of Array.from(word)) {
      const u = units(ch);
      if (cur + u > perLine) {
        lines++;
        cur = 0;
      }
      cur += u;
    }
  }
  return lines;
}

export function cardHeight(label: string, width: number): number {
  const h = wrappedLines(label, width) * LINE_PX + PAD_Y;
  const even = Math.ceil(h / 2) * 2; // even heights keep centers on integers
  return Math.max(MIN_HEIGHT, even);
}

interface Placed {
  node: MapNode;
  parent?: Placed;
  depth: number;
  width: number;
  height: number;
  x: number;
  y: number;
}

/** Escapes Markdown so a card shows the literal label. */
export function mdEscape(text: string): string {
  return text
    .replace(/\s+/g, " ")
    .trim()
    .replace(/[\\[\]*_`|<>]/g, "\\$&")
    .replace(/^([#+\-*>])/, "\\$1")
    .replace(/^(\d+)\./, "$1\\.");
}

function cardText(n: MapNode): string {
  if (!n.note || notePathError(n.note)) return mdEscape(n.label);
  const path = n.note.replace(/\.md$/, "");
  let label = n.label.replace(/\s+/g, " ").trim();
  for (let prev = ""; prev !== label; ) {
    prev = label;
    label = label.replace(/\[\[|\]\]/g, "");
  }
  return `[[${path}|${label.replace(/\|/g, "\\|")}]]`;
}

function oppositeSides(a: CanvasTextNode, b: CanvasTextNode): [CanvasSide, CanvasSide] {
  if (b.x >= a.x + a.width) return ["right", "left"];
  if (b.x + b.width <= a.x) return ["left", "right"];
  return b.y + b.height / 2 >= a.y + a.height / 2 ? ["bottom", "top"] : ["top", "bottom"];
}

export function layout(map: MindMap, pins: Record<NodeId, { x: number; y: number }> = {}): Canvas {
  // Placed records in pre-order; the flextree is built over them.
  const order: Placed[] = [];
  const build = (n: MapNode, parent?: Placed): Placed => {
    const width = cardWidth(n.label);
    return { node: n, parent, depth: parent ? parent.depth + 1 : 0, width, height: cardHeight(n.label, width), x: 0, y: 0 };
  };
  const kids = new Map<Placed, Placed[]>();
  const grow = (p: Placed): void => {
    order.push(p); // pre-order
    const list = p.node.children.map((c) => build(c, p));
    kids.set(p, list);
    list.forEach(grow);
  };
  const rootPlaced = build(map.root);
  grow(rootPlaced);

  // flextree: its x is the breadth (our vertical axis, node centre); its depth axis is unused
  // because column positions depend on the real card widths.
  const tree = flextree<Placed>({
    children: (p) => kids.get(p),
    nodeSize: (n) => [n.data.height, 1],
    spacing: SIBLING_GAP,
  });
  const laid = tree(tree.hierarchy(rootPlaced));
  const rootCentre = laid.x;
  // One column per depth: wide cards at a depth must not run into the next column.
  const colWidth: number[] = [];
  for (const p of order) {
    const d = p.depth;
    colWidth[d] = Math.max(colWidth[d] ?? 0, p.width);
  }
  const colX: number[] = [0];
  for (let d = 1; d < colWidth.length; d++) colX[d] = (colX[d - 1] ?? 0) + (colWidth[d - 1] ?? 0) + LEVEL_GAP;
  const walk = (f: typeof laid): void => {
    const p = f.data;
    p.y = norm(Math.round(f.x - rootCentre - p.height / 2));
    p.x = colX[p.depth] ?? 0;
    f.children?.forEach(walk);
  };
  walk(laid);

  // Pins override the node's own position only; descendants stay where the layout put them.
  const pinned = new Set<NodeId>();
  for (const p of order) {
    const pin = pins[p.node.id];
    if (pin && Object.hasOwn(pins, p.node.id) && Number.isFinite(pin.x) && Number.isFinite(pin.y)) {
      p.x = norm(Math.round(pin.x));
      p.y = norm(Math.round(pin.y));
      pinned.add(p.node.id);
    }
  }

  const cards = new Map<NodeId, CanvasTextNode>();
  const nodes: CanvasTextNode[] = order.map((p) => {
    const card: CanvasTextNode = {
      id: p.node.id,
      type: "text",
      x: p.x,
      y: p.y,
      width: p.width,
      height: p.height,
      text: cardText(p.node),
    };
    const color = p.parent ? effectiveColor(map, p.node.id) : undefined;
    if (color) card.color = color;
    cards.set(card.id, card);
    return card;
  });

  const edges: CanvasEdge[] = [];
  for (const p of order) {
    if (!p.parent) continue;
    const moved = pinned.has(p.node.id) || pinned.has(p.parent.node.id);
    const [fromSide, toSide] = moved ? oppositeSides(cards.get(p.parent.node.id)!, cards.get(p.node.id)!) : (["right", "left"] as const);
    const e: CanvasEdge = {
      id: `h-${p.parent.node.id}-${p.node.id}`,
      fromNode: p.parent.node.id,
      fromSide,
      fromEnd: "none",
      toNode: p.node.id,
      toSide,
      toEnd: "none",
    };
    const color = effectiveColor(map, p.node.id);
    if (color) e.color = color;
    edges.push(e);
  }
  for (const l of map.links) {
    const a = cards.get(l.from);
    const b = cards.get(l.to);
    if (!a || !b) continue; // dangling link: lint reports it, layout skips it
    const [fromSide, toSide] = oppositeSides(a, b);
    edges.push({
      id: `x-${l.from}-${l.to}`,
      fromNode: l.from,
      fromSide,
      toNode: l.to,
      toSide,
      toEnd: "arrow",
      label: l.label,
    });
  }
  return { nodes, edges };
}

/** Stable pretty JSON, tab indent like Obsidian, trailing newline. */
export function canvasToJson(c: Canvas): string {
  return JSON.stringify(c, null, "\t") + "\n";
}
