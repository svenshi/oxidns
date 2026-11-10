// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

import {readdir, readFile} from 'node:fs/promises';
import {dirname, join, relative} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const docsRoot = join(here, '..');
const repoRoot = join(docsRoot, '..');
const zhRoot = join(docsRoot, 'docs');
const enRoot = join(
  docsRoot,
  'i18n',
  'en',
  'docusaurus-plugin-content-docs',
  'current',
);
const pluginCategories = ['server', 'executor', 'matcher', 'provider'];

async function walk(root, extensions) {
  const files = [];

  async function visit(directory) {
    for (const entry of await readdir(directory, {withFileTypes: true})) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        await visit(path);
      } else if (extensions.some((extension) => entry.name.endsWith(extension))) {
        files.push(path);
      }
    }
  }

  await visit(root);
  return files.sort();
}

function difference(left, right) {
  return [...left].filter((value) => !right.has(value)).sort();
}

function extractReferencePlugins(markdown) {
  const plugins = new Set();
  for (const heading of markdown.matchAll(/^##\s+(.+)$/gm)) {
    for (const name of heading[1].matchAll(/`([^`]+)`/g)) {
      plugins.add(name[1]);
    }
  }
  return plugins;
}

function extractCatalogPlugins(markdown, category) {
  const plugins = new Set();
  const linkPattern = new RegExp(
    `\\[([^\\]]+)\\]\\(${category}(?:\\.mdx|/[^)#]+\\.mdx)#[^)]+\\)`,
    'g',
  );

  for (const link of markdown.matchAll(linkPattern)) {
    for (const name of link[1].matchAll(/`([^`]+)`/g)) {
      plugins.add(name[1]);
    }
  }
  return plugins;
}

function extractLegacyAnchors(markdown) {
  return new Set(
    [...markdown.matchAll(/<span id="([^"]+)"><\/span>/g)].map(
      (match) => match[1],
    ),
  );
}

function reportSetDifference(errors, label, expected, actual) {
  const missing = difference(expected, actual);
  const extra = difference(actual, expected);
  if (missing.length > 0) {
    errors.push(`${label}: missing ${missing.join(', ')}`);
  }
  if (extra.length > 0) {
    errors.push(`${label}: unexpected ${extra.join(', ')}`);
  }
}

async function checkLocaleParity(errors) {
  const extensions = ['.md', '.mdx'];
  const zhFiles = new Set(
    (await walk(zhRoot, extensions)).map((path) => relative(zhRoot, path)),
  );
  const enFiles = new Set(
    (await walk(enRoot, extensions)).map((path) => relative(enRoot, path)),
  );
  reportSetDifference(errors, 'English document tree', zhFiles, enFiles);
}

async function checkPluginCatalog(errors) {
  const localeRoots = [
    ['zh-Hans', zhRoot],
    ['en', enRoot],
  ];
  const localePluginSets = new Map();

  for (const [locale, root] of localeRoots) {
    const overview = await readFile(
      join(root, 'plugin-reference', 'overview.md'),
      'utf8',
    );
    const allPlugins = new Set();

    for (const category of pluginCategories) {
      const landing = await readFile(
        join(root, 'plugin-reference', `${category}.mdx`),
        'utf8',
      );
      const referencePlugins = new Set();
      for (const path of await walk(
        join(root, 'plugin-reference', category),
        ['.md', '.mdx'],
      )) {
        const reference = await readFile(path, 'utf8');
        for (const plugin of extractReferencePlugins(reference)) {
          referencePlugins.add(plugin);
        }
      }
      const overviewPlugins = extractCatalogPlugins(overview, category);
      const landingPlugins = extractCatalogPlugins(landing, category);
      const legacyAnchors = extractLegacyAnchors(landing);
      reportSetDifference(
        errors,
        `${locale} ${category} overview`,
        referencePlugins,
        overviewPlugins,
      );
      reportSetDifference(
        errors,
        `${locale} ${category} landing`,
        referencePlugins,
        landingPlugins,
      );
      const missingLegacyAnchors = difference(referencePlugins, legacyAnchors);
      if (missingLegacyAnchors.length > 0) {
        errors.push(
          `${locale} ${category} legacy anchors: missing ${missingLegacyAnchors.join(', ')}`,
        );
      }
      for (const plugin of referencePlugins) {
        allPlugins.add(plugin);
      }
    }
    localePluginSets.set(locale, allPlugins);
  }

  reportSetDifference(
    errors,
    'English plugin reference',
    localePluginSets.get('zh-Hans'),
    localePluginSets.get('en'),
  );

  const registeredPlugins = new Set();
  for (const path of await walk(join(repoRoot, 'src', 'plugin'), ['.rs'])) {
    const source = await readFile(path, 'utf8');
    for (const match of source.matchAll(/^\s*#\[plugin_factory\("([^"]+)"\)\]/gm)) {
      registeredPlugins.add(match[1]);
    }
    for (const match of source.matchAll(/register_plugin_factory!\(\s*"([^"]+)"/g)) {
      if (!match[1].startsWith('test_')) {
        registeredPlugins.add(match[1]);
      }
    }
  }
  reportSetDifference(
    errors,
    'Chinese plugin reference versus Rust registry',
    registeredPlugins,
    localePluginSets.get('zh-Hans'),
  );
}

const errors = [];
await checkLocaleParity(errors);
await checkPluginCatalog(errors);

if (errors.length > 0) {
  console.error('Documentation content checks failed:');
  for (const error of errors) {
    console.error(`- ${error}`);
  }
  process.exitCode = 1;
} else {
  console.log('Documentation content checks passed.');
}
