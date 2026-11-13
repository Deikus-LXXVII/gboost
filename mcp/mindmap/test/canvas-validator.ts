import type { Canvas } from "../src/index.js";

const SIDES = ["top", "right", "bottom", "left"];
const ENDS = ["none", "arrow"];

const isColor = (c: unknown): boolean => typeof c === "string" && /^([1-6]|#[0-9a-fA-F]{6}|#[0-9a-fA-F]{3})$/.test(c);
const isInt = (v: unknown): boolean => typeof v === "number" && Number.isInteger(v);

/** Validates JSON Canvas 1.0 (text nodes only, as Bastion writes). Returns a list of problems. */
export function validateCanvas(input: unknown): string[] {
  const errs: string[] = [];
  const c = input as Canvas;
  if (typeof c !== "object" || c === null) return ["canvas is not an object"];
  if (!Array.isArray(c.nodes)) errs.push("nodes must be an array");
  if (!Array.isArray(c.edges)) errs.push("edges must be an array");
  if (errs.length) return errs;

  const ids = new Set<string>();
  const nodeIds = new Set<string>();
  const checkId = (id: unknown, where: string): void => {
    if (typeof id !== "string" || id === "") errs.push(`${where}: id must be a non-empty string`);
    else if (ids.has(id)) errs.push(`${where}: duplicate id ${id}`);
    else ids.add(id);
  };

  for (const n of c.nodes as unknown as Record<string, unknown>[]) {
    const w = `node ${String(n.id)}`;
    checkId(n.id, w);
    if (typeof n.id === "string") nodeIds.add(n.id);
    for (const k of ["x", "y", "width", "height"]) if (!isInt(n[k])) errs.push(`${w}: ${k} must be an integer`);
    if (n.color !== undefined && !isColor(n.color)) errs.push(`${w}: bad color ${String(n.color)}`);
    switch (n.type) {
      case "text":
        if (typeof n.text !== "string") errs.push(`${w}: text node needs string text`);
        break;
      case "file":
        if (typeof n.file !== "string") errs.push(`${w}: file node needs string file`);
        break;
      case "link":
        if (typeof n.url !== "string") errs.push(`${w}: link node needs string url`);
        break;
      case "group":
        if (n.label !== undefined && typeof n.label !== "string") errs.push(`${w}: label must be a string`);
        break;
      default:
        errs.push(`${w}: unknown type ${String(n.type)}`);
    }
  }

  for (const e of c.edges as unknown as Record<string, unknown>[]) {
    const w = `edge ${String(e.id)}`;
    checkId(e.id, w);
    for (const k of ["fromNode", "toNode"]) {
      if (typeof e[k] !== "string") errs.push(`${w}: ${k} must be a string`);
      else if (!nodeIds.has(e[k] as string)) errs.push(`${w}: ${k} ${String(e[k])} does not exist`);
    }
    for (const k of ["fromSide", "toSide"]) if (e[k] !== undefined && !SIDES.includes(e[k] as string)) errs.push(`${w}: bad ${k}`);
    for (const k of ["fromEnd", "toEnd"]) if (e[k] !== undefined && !ENDS.includes(e[k] as string)) errs.push(`${w}: bad ${k}`);
    if (e.color !== undefined && !isColor(e.color)) errs.push(`${w}: bad color ${String(e.color)}`);
    if (e.label !== undefined && typeof e.label !== "string") errs.push(`${w}: label must be a string`);
  }
  return errs;
}
