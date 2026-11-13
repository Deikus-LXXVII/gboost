import { randomInt } from "node:crypto";
import type { IdGenerator, MapNode, NodeId } from "./types.js";

export const ID_PATTERN = /^[a-z0-9]{6}$/;
const ALPHABET = "abcdefghijklmnopqrstuvwxyz0123456789";

export function isValidId(id: string): boolean {
  return ID_PATTERN.test(id);
}

/** Default generator: random 6-char id, retried until unused. Tests inject their own. */
export const defaultIdGenerator: IdGenerator = (existing) => {
  for (;;) {
    let id = "";
    for (let i = 0; i < 6; i++) id += ALPHABET[randomInt(ALPHABET.length)];
    if (!existing.has(id)) return id;
  }
};

/** All ids in a tree (root first, document order), duplicates included. */
export function collectIds(root: MapNode): NodeId[] {
  const out: NodeId[] = [];
  const walk = (n: MapNode): void => {
    out.push(n.id);
    n.children.forEach(walk);
  };
  walk(root);
  return out;
}
