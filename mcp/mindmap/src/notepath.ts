/** Returns an error message if `path` is unsafe as an Obsidian note path, else undefined. */
export function notePathError(path: string): string | undefined {
  if (path === "") return "note must be a non-empty string";
  if (/[|[\]#^]/.test(path)) return "note path must not contain | [ ] # ^";
  if (path.startsWith("/")) return "note path must be relative";
  if (path.split(/[\\/]/).includes("..")) return "note path must not contain .. segments";
  return undefined;
}
