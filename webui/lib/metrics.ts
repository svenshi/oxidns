// Prometheus text-format parsing and plugin-level metric curation.
//
// The backend exposes a single Prometheus endpoint (`/metrics`). Plugin series
// carry a `plugin_tag` label and are associated with the matching
// `PluginInstance` (whose `name` is the tag). Global network series are grouped
// separately by their `outbound_profile` label.
//
// Metric labels, card-priority lists, and derived metric specs are defined
// alongside each plugin kind in `lib/plugin-definitions/` — this file derives
// its runtime data structures from those definitions rather than duplicating them.

import {
  getLocalizedPluginKindDefinition,
  getLocalizedPluginKindDefinitions,
  pluginKindDefinitions,
} from "./plugin-definitions";
import type { DerivedMetricSpec } from "./plugin-definitions/shared";
import { DEFAULT_LOCALE, WEBUI, t as translate, type Locale } from "./i18n";

export interface MetricSeries {
  name: string;
  kind?: MetricKind;
  help?: string;
  /** Labels excluding `plugin_tag` (kept for dimensional breakdowns). */
  labels: Record<string, string>;
  value: number;
}

export type MetricKind =
  | "counter"
  | "gauge"
  | "histogram"
  | "summary"
  | "untyped";

export interface MetricGroup {
  name: string;
  label: string;
  help?: string;
  series: MetricSeries[];
  /** Sum of all series values for this metric name. */
  total: number;
  highValue: boolean;
}

/** Plugin tag -> flat list of its series. */
export type PluginMetricsMap = Record<string, MetricSeries[]>;
export type OutboundMetricsMap = Record<string, MetricSeries[]>;

export interface ParsedMetrics {
  byTag: PluginMetricsMap;
  outbound: OutboundMetricsMap;
  help: Record<string, string>;
  kind: Record<string, MetricKind>;
}

const SAMPLE_RE = /^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{[^}]*\})?\s+(.+?)(?:\s+\d+)?$/;
const OUTBOUND_NETWORK_METRICS = new Set([
  "network_resolver_cache_hit_total",
  "network_resolver_cache_miss_total",
  "network_resolver_refresh_total",
  "network_resolver_refresh_latency_ms_total",
  "network_resolver_error_total",
  "network_upstream_pool_refresh_total",
  "network_upstream_pool_refresh_latency_ms_total",
]);

function unescapeLabelValue(raw: string): string {
  return raw.replace(/\\(["\\n])/g, (_m, ch) => (ch === "n" ? "\n" : ch));
}

function parseLabels(block: string | undefined): Record<string, string> {
  if (!block) return {};
  const inner = block.slice(1, -1).trim();
  if (!inner) return {};
  const labels: Record<string, string> = {};
  // Labels are `key="value"` pairs; values may contain commas, so match
  // explicitly rather than splitting on `,`.
  const re = /([a-zA-Z_][a-zA-Z0-9_]*)="((?:[^"\\]|\\.)*)"/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(inner)) !== null) {
    labels[m[1]] = unescapeLabelValue(m[2]);
  }
  return labels;
}

function parseValue(raw: string): number {
  const trimmed = raw.trim();
  if (trimmed === "+Inf") return Number.POSITIVE_INFINITY;
  if (trimmed === "-Inf") return Number.NEGATIVE_INFINITY;
  if (trimmed === "NaN") return Number.NaN;
  const v = Number(trimmed);
  return Number.isNaN(v) ? 0 : v;
}

export function parsePrometheusMetrics(text: string): ParsedMetrics {
  const byTag: PluginMetricsMap = {};
  const outbound: OutboundMetricsMap = {};
  const help: Record<string, string> = {};
  const kind: Record<string, MetricKind> = {};

  for (const rawLine of text.split("\n")) {
    const line = rawLine.trim();
    if (!line) continue;
    if (line.startsWith("#")) {
      const helpMatch = /^#\s+HELP\s+(\S+)\s+(.*)$/.exec(line);
      if (helpMatch) help[helpMatch[1]] = helpMatch[2];
      const typeMatch = /^#\s+TYPE\s+(\S+)\s+(\S+)\s*$/.exec(line);
      if (typeMatch) kind[typeMatch[1]] = normalizeMetricKind(typeMatch[2]);
      continue;
    }
    const match = SAMPLE_RE.exec(line);
    if (!match) continue;
    const [, name, labelBlock, valueRaw] = match;
    const labels = parseLabels(labelBlock);
    const tag = labels["plugin_tag"];
    const outboundProfile = labels["outbound_profile"];
    const rest: Record<string, string> = {};
    for (const [k, v] of Object.entries(labels)) {
      if (k !== "plugin_tag" && k !== "outbound_profile") rest[k] = v;
    }
    const series = {
      name,
      kind: kind[name],
      help: help[name],
      labels: rest,
      value: parseValue(valueRaw),
    };
    if (tag) {
      (byTag[tag] ??= []).push(series);
      continue;
    }
    if (outboundProfile && OUTBOUND_NETWORK_METRICS.has(name)) {
      (outbound[outboundProfile] ??= []).push(series);
    }
  }

  return { byTag, outbound, help, kind };
}

function normalizeMetricKind(raw: string): MetricKind {
  switch (raw) {
    case "counter":
    case "gauge":
    case "histogram":
    case "summary":
      return raw;
    default:
      return "untyped";
  }
}

// ---------------------------------------------------------------------------
// Derived constants from plugin definitions.
// ---------------------------------------------------------------------------

const localeMetricLabels = new Map<Locale, Record<string, string>>();
const localeMetricHelp = new Map<Locale, Record<string, string>>();

function metricLabelsFor(locale: Locale): Record<string, string> {
  const cached = localeMetricLabels.get(locale);
  if (cached) return cached;
  const labels = Object.fromEntries(
    getLocalizedPluginKindDefinitions(locale).flatMap((def) =>
      Object.entries(def.metrics?.metricLabels ?? {}),
    ),
  );
  localeMetricLabels.set(locale, labels);
  return labels;
}

function metricHelpFor(locale: Locale): Record<string, string> {
  const cached = localeMetricHelp.get(locale);
  if (cached) return cached;
  const help = Object.fromEntries(
    getLocalizedPluginKindDefinitions(locale).flatMap((def) =>
      Object.entries(def.metrics?.metricHelp ?? {}),
    ),
  );
  localeMetricHelp.set(locale, help);
  return help;
}

/**
 * Global ordered list of high-value metric names, derived by concatenating each
 * plugin's `cardPriority` list in definition order (first occurrence wins).
 * Used for fallback ordering and sorting in the detail view.
 */
const HIGH_VALUE_ORDER: string[] = (() => {
  const seen = new Set<string>();
  const order: string[] = [];
  for (const def of pluginKindDefinitions) {
    for (const name of def.metrics?.cardPriority ?? []) {
      if (!seen.has(name)) {
        seen.add(name);
        order.push(name);
      }
    }
  }
  return order;
})();

/** Set of high-value metric names for O(1) lookup. */
const HIGH_VALUE_METRICS = new Set(HIGH_VALUE_ORDER);

// ---------------------------------------------------------------------------
// Curation: friendly labels + which metrics are worth surfacing on cards.
// ---------------------------------------------------------------------------

export function metricLabel(
  name: string,
  locale: Locale = DEFAULT_LOCALE,
): string {
  const labels = metricLabelsFor(locale);
  if (labels[name]) return labels[name];
  return name
    .replace(/_total$/, "")
    .replace(/_/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

export function formatMetricValue(
  value: number,
  locale: Locale = DEFAULT_LOCALE,
): string {
  if (!Number.isFinite(value)) return String(value);
  if (Number.isInteger(value))
    return new Intl.NumberFormat(locale).format(value);
  return value.toFixed(2);
}

export interface DisplayMetric {
  label: string;
  value: string;
}

function sumByName(series: MetricSeries[]): Map<string, number> {
  const totals = new Map<string, number>();
  for (const s of series) {
    totals.set(s.name, (totals.get(s.name) ?? 0) + s.value);
  }
  return totals;
}

function metricValue(
  totals: Map<string, number>,
  name: string,
): number | undefined {
  return totals.get(name);
}

function metricRatio(
  totals: Map<string, number>,
  numerator: string,
  denominator: string,
): number | undefined {
  const top = totals.get(numerator);
  const bottom = totals.get(denominator);
  if (top === undefined || !bottom || bottom <= 0) return undefined;
  return top / bottom;
}

function formatPercent(value: number): string {
  return `${(value * 100).toFixed(value >= 0.995 || value < 0.1 ? 1 : 0)}%`;
}

function pushDisplayMetric(
  out: DisplayMetric[],
  seen: Set<string>,
  label: string,
  value: string,
  limit: number,
) {
  if (out.length >= limit || seen.has(label)) return;
  seen.add(label);
  out.push({ label, value });
}

function pushRawMetric(
  out: DisplayMetric[],
  seen: Set<string>,
  totals: Map<string, number>,
  name: string,
  limit: number,
  locale: Locale,
) {
  const value = metricValue(totals, name);
  if (value === undefined) return;
  pushDisplayMetric(
    out,
    seen,
    metricLabel(name, locale),
    formatMetricValue(value, locale),
    limit,
  );
}

function averageLatencyForPrefix(
  totals: Map<string, number>,
  prefix: string,
): number | undefined {
  const sum = totals.get(`${prefix}_latency_sum_ms`);
  const count = totals.get(`${prefix}_latency_count`);
  if (sum === undefined || !count || count <= 0) return undefined;
  return sum / count;
}

/** Derive average latency for any `<x>_latency_sum_ms` / `<x>_latency_count` pair. */
function derivedLatency(
  totals: Map<string, number>,
  locale: Locale,
): DisplayMetric[] {
  const out: DisplayMetric[] = [];
  for (const [name, sum] of totals) {
    const m = /^(.*)_latency_sum_ms$/.exec(name);
    if (!m) continue;
    const count = totals.get(`${m[1]}_latency_count`);
    if (!count || count <= 0) continue;
    out.push({
      label: translate(locale, WEBUI.metrics.averageLatency),
      value: `${(sum / count).toFixed(1)} ms`,
    });
  }
  return out;
}

function applyDerivedSpec(
  spec: DerivedMetricSpec,
  totals: Map<string, number>,
  out: DisplayMetric[],
  seen: Set<string>,
  limit: number,
) {
  switch (spec.kind) {
    case "latency": {
      const latency = averageLatencyForPrefix(totals, spec.prefix);
      if (latency !== undefined) {
        pushDisplayMetric(
          out,
          seen,
          spec.label,
          `${latency.toFixed(1)} ms`,
          limit,
        );
      }
      break;
    }
    case "percent": {
      const ratio = metricRatio(totals, spec.numerator, spec.denominator);
      if (ratio !== undefined) {
        pushDisplayMetric(out, seen, spec.label, formatPercent(ratio), limit);
      }
      break;
    }
    case "percent_of_sum": {
      const numerator = totals.get(spec.numerator);
      const total = spec.terms.reduce(
        (acc, t) => acc + (totals.get(t) ?? 0),
        0,
      );
      if (numerator !== undefined && total > 0) {
        pushDisplayMetric(
          out,
          seen,
          spec.label,
          formatPercent(numerator / total),
          limit,
        );
      }
      break;
    }
  }
}

function pushDerivedCardMetrics(
  out: DisplayMetric[],
  seen: Set<string>,
  totals: Map<string, number>,
  pluginKind: string | undefined,
  limit: number,
  locale: Locale,
) {
  if (!pluginKind) return;
  const def = getLocalizedPluginKindDefinition(pluginKind, locale);
  for (const spec of def?.metrics?.derivedCard ?? []) {
    if (out.length >= limit) break;
    applyDerivedSpec(spec, totals, out, seen, limit);
  }
}

function cardMetricPriority(pluginKind: string | undefined): string[] {
  if (!pluginKind) return HIGH_VALUE_ORDER;
  const def = pluginKindDefinitions.find((d) => d.kind === pluginKind);
  return def?.metrics?.cardPriority ?? HIGH_VALUE_ORDER;
}

/** Up to `limit` high-value metrics for compact card display. */
export function selectCardMetrics(
  series: MetricSeries[] | undefined,
  pluginKind?: string,
  limit = 4,
  locale: Locale = DEFAULT_LOCALE,
): DisplayMetric[] {
  if (!series || series.length === 0) return [];
  const totals = sumByName(series);
  const result: DisplayMetric[] = [];
  const seen = new Set<string>();

  pushDerivedCardMetrics(result, seen, totals, pluginKind, limit, locale);

  for (const name of cardMetricPriority(pluginKind)) {
    pushRawMetric(result, seen, totals, name, limit, locale);
    if (result.length >= limit) break;
  }

  if (result.length < limit) {
    for (const dm of derivedLatency(totals, locale)) {
      pushDisplayMetric(result, seen, dm.label, dm.value, limit);
      if (result.length >= limit) break;
    }
  }

  return result.slice(0, limit);
}

function labelKeyLabel(key: string, locale: Locale): string {
  switch (key) {
    case "name":
      return translate(locale, WEBUI.metrics.name);
    case "kind":
      return translate(locale, WEBUI.metrics.kind);
    case "reason":
      return translate(locale, WEBUI.metrics.reason);
    case "result":
      return translate(locale, WEBUI.metrics.result);
    case "upstream_index":
      return translate(locale, WEBUI.metrics.upstream);
    default:
      return key;
  }
}

function labelValueLabel(key: string, value: string, locale: Locale): string {
  if (key === "kind" && value === "fresh")
    return translate(locale, WEBUI.metrics.fresh);
  if (key === "kind" && value === "stale")
    return translate(locale, WEBUI.metrics.stale);
  if (key === "reason" && value === "truncated") {
    return translate(locale, WEBUI.metrics.truncated);
  }
  if (key === "reason" && value === "no_ttl")
    return translate(locale, WEBUI.metrics.noTtl);
  if (key === "reason" && value === "low_positive_ttl")
    return translate(locale, WEBUI.metrics.lowPositiveTtl);
  if (key === "result" && value === "started") {
    return translate(locale, WEBUI.metrics.started);
  }
  if (key === "result" && value === "success") {
    return translate(locale, WEBUI.metrics.success);
  }
  if (key === "result" && value === "failed") {
    return translate(locale, WEBUI.metrics.failed);
  }
  return value;
}

function describeLabels(
  labels: Record<string, string>,
  locale: Locale,
): string {
  const entries = Object.entries(labels);
  if (entries.length === 0) return "";
  return entries
    .map(([k, v]) => {
      const key = labelKeyLabel(k, locale);
      const value = labelValueLabel(k, v, locale);
      return `${key}=${value}`;
    })
    .join(", ");
}

export interface MetricRow {
  name: string;
  kind?: MetricKind;
  help?: string;
  label: string;
  highValue: boolean;
  /** Single total when one series, or a labelled breakdown when many. */
  total: number;
  breakdown: { key: string; value: number }[];
}

/** Group a plugin's series by metric name for the full detail view. */
export function groupMetricRows(
  series: MetricSeries[],
  locale: Locale = DEFAULT_LOCALE,
): MetricRow[] {
  const byName = new Map<string, MetricSeries[]>();
  for (const s of series) {
    const bucket = byName.get(s.name);
    if (bucket) {
      bucket.push(s);
    } else {
      byName.set(s.name, [s]);
    }
  }

  const rows: MetricRow[] = [];
  for (const [name, list] of byName) {
    const total = list.reduce((acc, s) => acc + s.value, 0);
    const hasDimensions = list.some((s) => Object.keys(s.labels).length > 0);
    const showBreakdown = list.length > 1 || hasDimensions;
    rows.push({
      name,
      kind: list[0]?.kind,
      help: metricHelpFor(locale)[name] ?? list[0]?.help,
      label: metricLabel(name, locale),
      highValue: HIGH_VALUE_METRICS.has(name),
      total,
      breakdown: showBreakdown
        ? list.map((s, index) => ({
            key:
              describeLabels(s.labels, locale) ||
              (list.length > 1
                ? translate(locale, WEBUI.metrics.series, { index: index + 1 })
                : translate(locale, WEBUI.metrics.defaultSeries)),
            value: s.value,
          }))
        : [],
    });
  }

  const orderIndex = (n: string) => {
    const i = HIGH_VALUE_ORDER.indexOf(n);
    return i === -1 ? Number.MAX_SAFE_INTEGER : i;
  };
  rows.sort((a, b) => {
    if (a.highValue !== b.highValue) return a.highValue ? -1 : 1;
    const oa = orderIndex(a.name);
    const ob = orderIndex(b.name);
    if (oa !== ob) return oa - ob;
    return a.name.localeCompare(b.name);
  });
  return rows;
}
