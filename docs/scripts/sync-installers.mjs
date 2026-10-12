// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

import {copyFile, mkdir} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..');
const sourceDir = join(repoRoot, 'scripts');
const staticDir = join(repoRoot, 'docs', 'static');
const targetDir = join(repoRoot, 'docs', 'static', 'scripts');
const installerFiles = [
  'install.sh',
  'install.ps1',
  'uninstall.sh',
  'uninstall.ps1',
];

await mkdir(staticDir, {recursive: true});
await mkdir(targetDir, {recursive: true});

for (const file of installerFiles) {
  const source = join(sourceDir, file);
  await copyFile(source, join(staticDir, file));
  await copyFile(source, join(targetDir, file));
}

console.log(`Synced OxiDNS installer scripts to ${staticDir} and ${targetDir}`);
