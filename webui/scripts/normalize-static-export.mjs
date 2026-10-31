import { readdir, rename, rmdir, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export async function normalizeStaticExport(root) {
  const directories = [];
  const moves = [];
  async function walk(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const source = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        directories.push(source);
        await walk(source);
      } else if (entry.isFile() && entry.name.endsWith(".txt")) {
        const parts = path.relative(root, source).split(path.sep);
        const segment = parts.findIndex((part) => part.startsWith("__next."));
        if (segment >= 0 && segment < parts.length - 1) {
          // Next's exporter can preserve Windows separators instead of encoding segment slashes as dots.
          const destination = path.join(
            root,
            ...parts.slice(0, segment),
            parts.slice(segment).join("."),
          );
          moves.push({ source, destination });
        }
      }
    }
  }
  await walk(root);
  const destinations = new Set();
  for (const { destination } of moves) {
    if (destinations.has(destination))
      throw new Error(`Duplicate export target: ${destination}`);
    destinations.add(destination);
    try {
      await stat(destination);
    } catch (error) {
      if (error.code === "ENOENT") continue;
      throw error;
    }
    throw new Error(`Export target already exists: ${destination}`);
  }
  for (const { source, destination } of moves)
    await rename(source, destination);
  let removedDirectories = 0;
  for (const directory of directories.sort((a, b) => b.length - a.length)) {
    if ((await readdir(directory)).length === 0) {
      await rmdir(directory);
      removedDirectories++;
    }
  }
  return { renamedFiles: moves.length, removedDirectories };
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  const root = fileURLToPath(new URL("../out/", import.meta.url));
  console.log(
    "Static export normalization:",
    await normalizeStaticExport(root),
  );
}
