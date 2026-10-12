/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import type { PluginType } from "@/lib/types";
import type { PluginKindDefinition } from "@/lib/plugin-definitions";
import { createElement, type SVGProps } from "react";
import {
  getLocalizedPluginKindDefinition,
  getLocalizedPluginKindDefinitions,
  getPluginKindsByType,
  pluginKindDefinitions,
} from "@/lib/plugin-definitions";
import { DEFAULT_LOCALE, type Locale } from "@/lib/i18n";
import {
  ArrowUpRight,
  Ban,
  BarChart3,
  CheckCircle,
  Clock,
  Database,
  File,
  FileQuestion,
  FileText,
  Gauge,
  GitBranch,
  Globe,
  Hash,
  List,
  Lock,
  MapPin,
  Network,
  Pencil,
  RefreshCw,
  Reply,
  Regex,
  Route,
  Settings,
  Shield,
  Shuffle,
  Wifi,
} from "lucide-react";

export type PluginCatalogItem = PluginKindDefinition;

export const pluginKindIconMap = {
  Wifi,
  Network,
  Lock,
  Shield,
  Database,
  ArrowUpRight,
  Ban,
  CheckCircle,
  Clock,
  Pencil,
  RefreshCw,
  Reply,
  GitBranch,
  List,
  MapPin,
  Globe,
  FileQuestion,
  FileText,
  File,
  BarChart3,
  Regex,
  Route,
  Gauge,
  Hash,
  Settings,
  Shuffle,
} as const;

export const pluginCatalog: PluginCatalogItem[] = pluginKindDefinitions;

export function getPluginCatalogItem(
  kind: string,
  locale: Locale = DEFAULT_LOCALE,
): PluginCatalogItem | undefined {
  return getLocalizedPluginKindDefinition(kind, locale);
}

export function getPluginCatalogItemsByType(
  type: PluginType,
  locale: Locale = DEFAULT_LOCALE,
): PluginCatalogItem[] {
  if (locale === DEFAULT_LOCALE) return getPluginKindsByType(type);
  return getLocalizedPluginKindDefinitions(locale).filter(
    (p) => p.type === type,
  );
}

export function getSupportedPluginCatalog(
  supportedKinds?: string[],
  locale: Locale = DEFAULT_LOCALE,
): PluginCatalogItem[] {
  const catalog =
    locale === DEFAULT_LOCALE
      ? pluginCatalog
      : getLocalizedPluginKindDefinitions(locale);

  if (!supportedKinds || supportedKinds.length === 0) return catalog;

  const supported = new Set(supportedKinds);
  return catalog.filter((plugin) => supported.has(plugin.kind));
}

export function getPluginKindIconComponent(icon: string) {
  return pluginKindIconMap[icon as keyof typeof pluginKindIconMap] ?? Database;
}

export function renderPluginKindIcon(
  icon: string,
  props?: SVGProps<SVGSVGElement>,
) {
  return createElement(getPluginKindIconComponent(icon), props);
}
