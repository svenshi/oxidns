/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { WEBUI } from "./keys";
import type { Locale, TranslationParams, TranslationTree } from "./types";
import { LOCALES } from "./types";
import { zhCNWebui } from "./locales/zh-CN/webui";
import { zhCNPluginDefined } from "./locales/zh-CN/plugin-defined";
import { zhCNDocs } from "./locales/zh-CN/docs";
import { enUSWebui } from "./locales/en-US/webui";
import { enUSPluginDefined } from "./locales/en-US/plugin-defined";
import { enUSDocs } from "./locales/en-US/docs";

export { LOCALES, WEBUI };
export type { Locale, TranslationParams } from "./types";

export const DEFAULT_LOCALE: Locale = "zh-CN";
export const LOCALE_STORAGE_KEY = "oxidns.locale";

export const localeLabels: Record<Locale, string> = {
  "zh-CN": "简体中文",
  "en-US": "English",
};

export const resources = {
  "zh-CN": {
    webui: zhCNWebui,
    plugin: zhCNPluginDefined,
    docs: zhCNDocs,
  },
  "en-US": {
    webui: enUSWebui,
    plugin: enUSPluginDefined,
    docs: enUSDocs,
  },
} as const satisfies Record<
  Locale,
  Record<"webui" | "plugin" | "docs", TranslationTree>
>;

export function isSupportedLocale(value: string | null): value is Locale {
  return LOCALES.includes(value as Locale);
}

export function getClientLocale(): Locale {
  if (typeof window === "undefined") return DEFAULT_LOCALE;
  try {
    const stored = window.localStorage.getItem(LOCALE_STORAGE_KEY);
    return isSupportedLocale(stored) ? stored : DEFAULT_LOCALE;
  } catch {
    return DEFAULT_LOCALE;
  }
}

export function tClient(key: string, params?: TranslationParams): string {
  return t(getClientLocale(), key, params);
}

export function t(
  locale: Locale,
  key: string,
  params?: TranslationParams,
): string {
  const value =
    lookup(resources[locale], key) ??
    lookup(resources[DEFAULT_LOCALE], key) ??
    key;
  return interpolate(value, params);
}

export function getResourceValue(
  locale: Locale,
  namespace: "webui" | "plugin" | "docs",
  path: string,
): string | undefined {
  return (
    lookup(resources[locale][namespace], path) ??
    lookup(resources[DEFAULT_LOCALE][namespace], path)
  );
}

export function getAllResourceValues(
  namespace: "webui" | "plugin" | "docs",
  path: string,
): string[] {
  const values = LOCALES.map((locale) =>
    lookup(resources[locale][namespace], path),
  );
  return [
    ...new Set(values.filter((value): value is string => Boolean(value))),
  ];
}

function lookup(root: TranslationTree, key: string): string | undefined {
  const parts = key.split(".");
  const direct = lookupParts(root, parts);
  if (direct) return direct;
  if (parts[0] === "webui" || parts[0] === "plugin" || parts[0] === "docs") {
    return lookupParts(root, parts.slice(1));
  }
  return undefined;
}

function lookupParts(
  root: TranslationTree,
  parts: string[],
): string | undefined {
  let current: string | TranslationTree | undefined = root;
  for (const part of parts) {
    if (!current || typeof current === "string") return undefined;
    current = current[part];
  }
  return typeof current === "string" ? current : undefined;
}

function interpolate(value: string, params?: TranslationParams): string {
  if (!params) return value;
  return value.replace(/\{([a-zA-Z0-9_]+)\}/g, (match, key) => {
    const next = params[key];
    return next === null || next === undefined ? match : String(next);
  });
}
