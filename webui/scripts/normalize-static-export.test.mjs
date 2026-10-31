import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, it } from "vitest";
import { normalizeStaticExport } from "./normalize-static-export.mjs";

const roots = [];
async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "oxidns-export-"));
  roots.push(root);
  await mkdir(path.join(root, "logs/__next.!KGNvbnNvbGUp/logs"), {
    recursive: true,
  });
  await writeFile(
    path.join(root, "logs/__next.!KGNvbnNvbGUp/logs/__PAGE__.txt"),
    "page",
  );
  await writeFile(
    path.join(root, "logs/__next.!KGNvbnNvbGUp/logs.txt"),
    "layout",
  );
  await writeFile(path.join(root, "logs/__next._tree.txt"), "tree");
  return root;
}
afterEach(async () => {
  for (const root of roots.splice(0))
    await rm(root, { recursive: true, force: true });
});

it("matches the static segment protocol, preserves payloads, and is idempotent", async () => {
  const root = await fixture();
  await mkdir(path.join(root, "_next/empty-build-id"), { recursive: true });
  expect(await normalizeStaticExport(root)).toEqual({
    renamedFiles: 2,
    removedDirectories: 4,
  });
  expect(
    await readFile(
      path.join(root, "logs/__next.!KGNvbnNvbGUp.logs.__PAGE__.txt"),
      "utf8",
    ),
  ).toBe("page");
  expect(
    await readFile(
      path.join(root, "logs/__next.!KGNvbnNvbGUp.logs.txt"),
      "utf8",
    ),
  ).toBe("layout");
  expect(await readFile(path.join(root, "logs/__next._tree.txt"), "utf8")).toBe(
    "tree",
  );
  expect(await normalizeStaticExport(root)).toEqual({
    renamedFiles: 0,
    removedDirectories: 0,
  });
});

it("rejects a collision before moving any payload", async () => {
  const root = await fixture();
  await writeFile(
    path.join(root, "logs/__next.!KGNvbnNvbGUp.logs.txt"),
    "existing",
  );
  await expect(normalizeStaticExport(root)).rejects.toThrow("already exists");
  expect(
    await readFile(
      path.join(root, "logs/__next.!KGNvbnNvbGUp/logs/__PAGE__.txt"),
      "utf8",
    ),
  ).toBe("page");
  expect(
    await readFile(
      path.join(root, "logs/__next.!KGNvbnNvbGUp.logs.txt"),
      "utf8",
    ),
  ).toBe("existing");
});
