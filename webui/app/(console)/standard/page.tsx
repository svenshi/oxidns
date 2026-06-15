"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity,
  BarChart3,
  Clock3,
  Cpu,
  HardDrive,
  RefreshCw,
  ShieldCheck,
  Timer,
  Wifi,
} from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  fetchQueryRecorderLatency,
  fetchQueryRecorderTimeseries,
  fetchQueryRecorderTopClients,
  fetchQueryRecorderTopQnames,
  fetchQueryRecords,
  type QueryRecordRow,
  type QueryRecorderLatencySummary,
  type QueryRecorderTimeseriesPoint,
  type QueryRecorderTopRow,
} from "@/lib/oxidns-api";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import type { MetricSeries } from "@/lib/metrics";
import { selectStandardSummary } from "@/lib/standard-mode/selectors";
import type { StandardUpstream } from "@/lib/standard-mode/types";
import { useAppStore } from "@/lib/store";
import { cn } from "@/lib/utils";

const STATS_WINDOW_MS = 24 * 60 * 60 * 1000;

interface OverviewStats {
  records: QueryRecordRow[];
  topClients: QueryRecorderTopRow[];
  topQnames: QueryRecorderTopRow[];
  topBlockedQnames: QueryRecorderTopRow[];
  latency: QueryRecorderLatencySummary | null;
  timeseries: QueryRecorderTimeseriesPoint[];
  sampleSize: number;
  blockedSampleSize: number;
}

const emptyStats: OverviewStats = {
  records: [],
  topClients: [],
  topQnames: [],
  topBlockedQnames: [],
  latency: null,
  timeseries: [],
  sampleSize: 0,
  blockedSampleSize: 0,
};

function formatQuestion(record: QueryRecordRow) {
  return (
    record.questions_json
      ?.map((item) => `${item.name} ${item.qtype}`)
      .join(", ") || "-"
  );
}

function formatNumber(value: number, locale: string) {
  return new Intl.NumberFormat(locale).format(Math.max(0, Math.round(value)));
}

function formatPercent(value: number | null) {
  return value == null || !Number.isFinite(value)
    ? "-"
    : `${value.toFixed(1)}%`;
}

function formatLatency(value?: number) {
  return value == null || !Number.isFinite(value)
    ? "-"
    : `${value.toFixed(1)} ms`;
}

function formatMemory(mb?: number) {
  if (mb == null || !Number.isFinite(mb)) return "-";
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${mb.toFixed(0)} MB`;
}

function progressTone(value: number) {
  if (value >= 85) return "bg-destructive";
  if (value >= 60) return "bg-amber-500";
  return "bg-emerald-500";
}

function computeQps(sampleSize: number) {
  return sampleSize > 0 ? sampleSize / (STATS_WINDOW_MS / 1000) : 0;
}

function totalFromSeries(points: QueryRecorderTimeseriesPoint[]) {
  return points.reduce((sum, item) => sum + item.total, 0);
}

function errorCountFromSeries(points: QueryRecorderTimeseriesPoint[]) {
  return points.reduce((sum, item) => sum + item.error_count, 0);
}

function normalizeUpstreamIdentity(value?: string) {
  return (value ?? "")
    .trim()
    .toLowerCase()
    .replace(/^(udp|tcp|tls|quic|doq|https|doh|h3):\/\//, "");
}

function upstreamKey(upstream: StandardUpstream) {
  return (
    normalizeUpstreamIdentity(upstream.id) ||
    normalizeUpstreamIdentity(upstream.address)
  );
}

function uniqueUpstreams(upstreams: StandardUpstream[]) {
  const seen = new Set<string>();
  const rows: StandardUpstream[] = [];
  for (const upstream of upstreams) {
    const key = upstreamKey(upstream);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    rows.push(upstream);
  }
  return rows;
}

function metricMatchesUpstream(
  metric: MetricSeries,
  upstream: StandardUpstream,
) {
  const identities = [upstream.id, upstream.name, upstream.address]
    .map(normalizeUpstreamIdentity)
    .filter(Boolean);
  const labelValues = Object.values(metric.labels)
    .map(normalizeUpstreamIdentity)
    .filter(Boolean);

  return identities.some((identity) =>
    labelValues.some(
      (value) =>
        value === identity ||
        value.includes(identity) ||
        identity.includes(value),
    ),
  );
}

function upstreamQueryCount(
  series: MetricSeries[],
  upstream: StandardUpstream,
) {
  let matched = false;
  let total = 0;
  for (const metric of series) {
    if (
      metric.name !== "forward_upstream_query_total" ||
      !Number.isFinite(metric.value) ||
      !metricMatchesUpstream(metric, upstream)
    ) {
      continue;
    }
    matched = true;
    total += metric.value;
  }
  return matched ? total : null;
}

function Sparkline({
  points,
  tone = "primary",
}: {
  points: QueryRecorderTimeseriesPoint[];
  tone?: "primary" | "destructive" | "emerald";
}) {
  const values = points.slice(-24).map((item) => item.total);
  const max = Math.max(...values, 1);
  const color =
    tone === "destructive"
      ? "bg-destructive"
      : tone === "emerald"
        ? "bg-emerald-500"
        : "bg-primary";

  return (
    <div className="mt-3 flex h-10 items-end gap-0.5">
      {Array.from({ length: 24 }).map((_, index) => {
        const value = values[index] ?? 0;
        const height = value > 0 ? Math.max(10, (value / max) * 100) : 6;
        return (
          <div
            key={index}
            className={cn("w-full rounded-sm opacity-70", color)}
            style={{ height: `${height}%` }}
          />
        );
      })}
    </div>
  );
}

function StatCard({
  title,
  value,
  icon: Icon,
  detail,
  tone = "default",
  progress,
  sparkline,
}: {
  title: string;
  value: string;
  icon: typeof Activity;
  detail?: string;
  tone?: "default" | "success" | "warning" | "danger";
  progress?: { value: number; indicatorClassName?: string };
  sparkline?: React.ReactNode;
}) {
  const valueClass =
    tone === "success"
      ? "text-emerald-500"
      : tone === "warning"
        ? "text-amber-500"
        : tone === "danger"
          ? "text-destructive"
          : "";

  return (
    <Card className="overflow-hidden">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground">
          {title}
        </CardTitle>
        <Icon className="size-4 text-muted-foreground" />
      </CardHeader>
      <CardContent>
        <div className={cn("font-mono text-2xl font-semibold", valueClass)}>
          {value}
        </div>
        {detail ? (
          <p className="mt-1 min-h-4 text-xs text-muted-foreground">{detail}</p>
        ) : null}
        {progress ? (
          <Progress
            value={progress.value}
            className="mt-3 h-1.5"
            indicatorClassName={progress.indicatorClassName}
          />
        ) : null}
        {sparkline}
      </CardContent>
    </Card>
  );
}

function SummaryRows({
  rows,
}: {
  rows: Array<{ label: string; value: string; hint?: string }>;
}) {
  return (
    <div className="divide-y rounded-md border">
      {rows.map((row) => (
        <div
          key={row.label}
          className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 px-3 py-2.5 text-sm"
        >
          <div className="min-w-0">
            <div className="truncate">{row.label}</div>
            {row.hint ? (
              <div className="truncate text-xs text-muted-foreground">
                {row.hint}
              </div>
            ) : null}
          </div>
          <div className="font-mono font-medium">{row.value}</div>
        </div>
      ))}
    </div>
  );
}

function UpstreamList({
  rows,
  empty,
  nameHeader,
  queriesHeader,
  locale,
}: {
  rows: Array<{ upstream: StandardUpstream; queries: number | null }>;
  empty: string;
  nameHeader: string;
  queriesHeader: string;
  locale: string;
}) {
  if (rows.length === 0) {
    return (
      <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
        {empty}
      </div>
    );
  }

  return (
    <div className="overflow-x-auto rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{nameHeader}</TableHead>
            <TableHead className="w-36 text-right">{queriesHeader}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map(({ upstream, queries }) => (
            <TableRow key={upstreamKey(upstream)}>
              <TableCell>
                <div
                  className={cn(
                    "min-w-0 font-medium",
                    !upstream.enabled && "text-muted-foreground",
                  )}
                >
                  {upstream.name || upstream.id}
                </div>
                <div className="mt-0.5 truncate font-mono text-xs text-muted-foreground">
                  {upstream.address}
                </div>
              </TableCell>
              <TableCell className="text-right font-mono text-sm">
                {queries == null ? "-" : formatNumber(queries, locale)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

function TopList({
  title,
  subtitle,
  rows,
  empty,
  locale,
}: {
  title: string;
  subtitle: string;
  rows: QueryRecorderTopRow[];
  empty: string;
  locale: string;
}) {
  const max = Math.max(...rows.map((row) => row.count), 1);
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{title}</CardTitle>
        <p className="text-xs text-muted-foreground">{subtitle}</p>
      </CardHeader>
      <CardContent className="space-y-3">
        {rows.length > 0 ? (
          rows.map((row) => (
            <div key={row.key} className="space-y-1.5">
              <div className="flex items-center justify-between gap-3 text-sm">
                <span className="min-w-0 truncate font-medium">{row.key}</span>
                <span className="shrink-0 font-mono text-xs text-muted-foreground">
                  {formatNumber(row.count, locale)}
                </span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full bg-primary"
                  style={{ width: `${Math.max(4, (row.count / max) * 100)}%` }}
                />
              </div>
              <div className="text-right font-mono text-[11px] text-muted-foreground">
                {formatPercent(row.share * 100)}
              </div>
            </div>
          ))
        ) : (
          <div className="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
            {empty}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

export default function StandardOverviewPage() {
  const { locale, t } = useI18n();
  const health = useAppStore((s) => s.health);
  const system = useAppStore((s) => s.system);
  const configModel = useAppStore((s) => s.configModel);
  const standardSettings = useAppStore((s) => s.standardSettings);
  const pluginMetrics = useAppStore((s) => s.pluginMetrics);
  const plugins = useAppStore((s) => s.plugins);
  const summary = useMemo(
    () => selectStandardSummary(configModel, standardSettings),
    [configModel, standardSettings],
  );
  const recorder =
    plugins.find((plugin) => plugin.name === "standard_recorder") ??
    plugins.find((plugin) => plugin.pluginKind === "query_recorder");
  const recorderName = recorder?.name;
  const [stats, setStats] = useState<OverviewStats>(emptyStats);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [statsError, setStatsError] = useState<string | null>(null);

  const metricSeries = Object.values(pluginMetrics).flat();
  const cacheHits = metricSeries
    .filter(
      (item) =>
        item.name.toLowerCase().includes("cache") &&
        item.name.toLowerCase().includes("hit"),
    )
    .reduce((sum, item) => sum + item.value, 0);
  const metricBlocked = metricSeries
    .filter((item) => item.name.toLowerCase().includes("block"))
    .reduce((sum, item) => sum + item.value, 0);

  const loadStats = useCallback(
    async (signal?: AbortSignal) => {
      if (!recorderName) {
        setStats(emptyStats);
        setStatsError(null);
        return;
      }
      const sinceMs = Date.now() - STATS_WINDOW_MS;
      setIsRefreshing(true);
      setStatsError(null);
      try {
        const [
          recordsResponse,
          topClientsResponse,
          topQnamesResponse,
          latencyResponse,
          timeseriesResponse,
          topBlockedResponse,
        ] = await Promise.all([
          fetchQueryRecords(recorderName, { limit: 8, sinceMs, signal }),
          fetchQueryRecorderTopClients(recorderName, {
            limit: 5,
            sinceMs,
            signal,
          }),
          fetchQueryRecorderTopQnames(recorderName, {
            limit: 5,
            sinceMs,
            signal,
          }),
          fetchQueryRecorderLatency(recorderName, {
            slowLimit: 5,
            sinceMs,
            signal,
          }),
          fetchQueryRecorderTimeseries(recorderName, {
            bucket: "hour",
            buckets: 24,
            sinceMs,
            signal,
          }),
          standardSettings.adBlock.enabled
            ? fetchQueryRecorderTopQnames(recorderName, {
                limit: 5,
                sinceMs,
                matcherTag: "standard_ad_rules",
                signal,
              }).catch(() => null)
            : Promise.resolve(null),
        ]);
        setStats({
          records: recordsResponse.records,
          topClients: topClientsResponse.rows,
          topQnames: topQnamesResponse.rows,
          topBlockedQnames: topBlockedResponse?.rows ?? [],
          latency: latencyResponse,
          timeseries: timeseriesResponse.points,
          sampleSize:
            latencyResponse.sample_size ||
            timeseriesResponse.sample_size ||
            totalFromSeries(timeseriesResponse.points),
          blockedSampleSize: topBlockedResponse?.sample_size ?? 0,
        });
      } catch (error) {
        if (signal?.aborted) return;
        setStats(emptyStats);
        setStatsError(
          error instanceof Error
            ? error.message
            : t(WEBUI.standardOverview.statsReadFailed),
        );
      } finally {
        if (!signal?.aborted) setIsRefreshing(false);
      }
    },
    [recorderName, standardSettings.adBlock.enabled, t],
  );

  useEffect(() => {
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      void loadStats(controller.signal);
    }, 0);
    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [loadStats]);

  const cpuPct = system?.process_cpu_percent ?? 0;
  const processMemoryMb = system?.process_memory_mb;
  const totalMemoryMb = system?.system_memory_total_mb ?? 0;
  const memoryPct =
    processMemoryMb != null && totalMemoryMb > 0
      ? Math.min((processMemoryMb / totalMemoryMb) * 100, 100)
      : 0;
  const healthStatus = health?.status ?? "unknown";
  const isHealthy = healthStatus === "ok";
  const totalQueries = stats.sampleSize;
  const blockedQueries =
    stats.blockedSampleSize > 0 ? stats.blockedSampleSize : metricBlocked;
  const blockedRate =
    totalQueries > 0
      ? Math.min((blockedQueries / totalQueries) * 100, 100)
      : null;
  const errorRate =
    totalQueries > 0
      ? Math.min(
          (errorCountFromSeries(stats.timeseries) / totalQueries) * 100,
          100,
        )
      : null;
  const qps = computeQps(totalQueries);
  const upstreamRows = useMemo(
    () =>
      uniqueUpstreams([
        ...standardSettings.upstreams,
        ...standardSettings.split.domesticUpstreams,
      ]),
    [standardSettings.split.domesticUpstreams, standardSettings.upstreams],
  );
  const upstreamMetrics = useMemo(
    () => [
      ...(pluginMetrics.standard_forward_global ?? []),
      ...(pluginMetrics.standard_forward_domestic ?? []),
    ],
    [pluginMetrics],
  );
  const upstreamStats = useMemo(
    () =>
      upstreamRows.map((upstream) => ({
        upstream,
        queries: upstreamQueryCount(upstreamMetrics, upstream),
      })),
    [upstreamMetrics, upstreamRows],
  );

  const generalRows = [
    {
      label: t(WEBUI.standardOverview.summaryDnsQueries),
      value: formatNumber(totalQueries, locale),
      hint: t(WEBUI.standardOverview.summaryDnsQueriesHint, {
        qps: qps.toFixed(2),
      }),
    },
    {
      label: t(WEBUI.standardOverview.summaryBlocked),
      value:
        blockedQueries > 0
          ? formatNumber(blockedQueries, locale)
          : summary.adBlockEnabled
            ? t(WEBUI.standardOverview.enabledState)
            : t(WEBUI.standardOverview.disabledState),
      hint:
        blockedRate == null
          ? undefined
          : t(WEBUI.standardOverview.summaryBlockedHint, {
              value: formatPercent(blockedRate),
            }),
    },
    {
      label: t(WEBUI.standardOverview.summaryCacheHits),
      value:
        cacheHits > 0
          ? formatNumber(cacheHits, locale)
          : summary.cacheEnabled
            ? t(WEBUI.standardOverview.enabledState)
            : t(WEBUI.standardOverview.disabledState),
      hint: t(WEBUI.standardOverview.summaryCacheHitsHint),
    },
    {
      label: t(WEBUI.standardOverview.summaryAvgLatency),
      value: formatLatency(stats.latency?.avg_ms),
      hint: t(WEBUI.standardOverview.summaryP95, {
        value: formatLatency(stats.latency?.p95_ms),
      }),
    },
    {
      label: t(WEBUI.standardOverview.summaryErrorRate),
      value: formatPercent(errorRate),
      hint: t(WEBUI.standardOverview.summaryErrorRateHint),
    },
    {
      label: t(WEBUI.standardOverview.summaryRuntimeVersion),
      value: system?.version ?? health?.version ?? "-",
      hint: system ? `${system.os}/${system.arch}` : undefined,
    },
  ];

  return (
    <>
      <AppHeader title={t(WEBUI.standardOverview.navTitle)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="space-y-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardOverview.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardOverview.description)}
              </p>
            </div>
            <Button
              variant="outline"
              onClick={() => void loadStats()}
              disabled={!recorderName || isRefreshing}
            >
              <RefreshCw
                className={cn("size-4", isRefreshing && "animate-spin")}
              />
              {t(WEBUI.standardOverview.refreshStats)}
            </Button>
          </div>

          {statsError ? (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {statsError}
            </div>
          ) : null}

          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-6">
            <StatCard
              title={t(WEBUI.standardOverview.serviceStatus)}
              value={healthStatus}
              icon={Activity}
              tone={isHealthy ? "success" : "danger"}
              detail={
                health?.uptime_ms
                  ? t(WEBUI.standardOverview.runningHours, {
                      hours: Math.floor(health.uptime_ms / 3600000),
                    })
                  : t(WEBUI.standardOverview.waitingStatus)
              }
            />
            <StatCard
              title={t(WEBUI.standardOverview.dnsQueries)}
              value={formatNumber(totalQueries, locale)}
              icon={BarChart3}
              detail={t(WEBUI.standardOverview.last24hQps, {
                qps: qps.toFixed(2),
              })}
              sparkline={<Sparkline points={stats.timeseries} />}
            />
            <StatCard
              title={t(WEBUI.standardOverview.ruleBlocked)}
              value={
                blockedQueries > 0
                  ? formatNumber(blockedQueries, locale)
                  : summary.adBlockEnabled
                    ? t(WEBUI.standardOverview.enabledState)
                    : t(WEBUI.standardOverview.disabledState)
              }
              icon={ShieldCheck}
              tone={blockedQueries > 0 ? "danger" : "default"}
              detail={
                blockedRate == null
                  ? t(WEBUI.standardOverview.filterDefaultOff)
                  : t(WEBUI.standardOverview.ratio, {
                      value: formatPercent(blockedRate),
                    })
              }
            />
            <StatCard
              title={t(WEBUI.standardOverview.responseLatency)}
              value={formatLatency(stats.latency?.avg_ms)}
              icon={Clock3}
              detail={t(WEBUI.standardOverview.summaryP95, {
                value: formatLatency(stats.latency?.p95_ms),
              })}
            />
            <StatCard
              title={t(WEBUI.standardOverview.cpu)}
              value={system ? `${cpuPct.toFixed(1)}%` : "-"}
              icon={Cpu}
              tone={
                cpuPct >= 85 ? "danger" : cpuPct >= 60 ? "warning" : "success"
              }
              detail={t(WEBUI.standardOverview.cpuDetail)}
              progress={{
                value: cpuPct,
                indicatorClassName: progressTone(cpuPct),
              }}
            />
            <StatCard
              title={t(WEBUI.standardOverview.memory)}
              value={formatMemory(processMemoryMb)}
              icon={HardDrive}
              tone={
                memoryPct >= 85
                  ? "danger"
                  : memoryPct >= 60
                    ? "warning"
                    : "success"
              }
              detail={
                totalMemoryMb > 0
                  ? t(WEBUI.standardOverview.memoryTotal, {
                      pct: formatPercent(memoryPct),
                      total: formatMemory(totalMemoryMb),
                    })
                  : t(WEBUI.standardOverview.processRss)
              }
              progress={{
                value: memoryPct,
                indicatorClassName: progressTone(memoryPct),
              }}
            />
          </div>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">
                {t(WEBUI.standardOverview.generalStatsTitle)}
              </CardTitle>
              <p className="text-xs text-muted-foreground">
                {t(WEBUI.standardOverview.generalStatsDesc)}
              </p>
            </CardHeader>
            <CardContent>
              <SummaryRows rows={generalRows} />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">
                {t(WEBUI.standardOverview.upstreamListTitle)}
              </CardTitle>
            </CardHeader>
            <CardContent>
              <UpstreamList
                rows={upstreamStats}
                empty={t(WEBUI.standardOverview.upstreamEmpty)}
                nameHeader={t(WEBUI.standardOverview.upstreamListName)}
                queriesHeader={t(WEBUI.standardOverview.upstreamListQueries)}
                locale={locale}
              />
            </CardContent>
          </Card>

          <div className="grid gap-4 xl:grid-cols-3">
            <TopList
              title={t(WEBUI.standardOverview.topClientsTitle)}
              subtitle={t(WEBUI.standardOverview.topClientsSubtitle)}
              rows={stats.topClients}
              empty={
                recorderName
                  ? t(WEBUI.standardOverview.topClientsEmpty)
                  : t(WEBUI.standardOverview.queryRecorderDisabled)
              }
              locale={locale}
            />
            <TopList
              title={t(WEBUI.standardOverview.topQnamesTitle)}
              subtitle={t(WEBUI.standardOverview.topQnamesSubtitle)}
              rows={stats.topQnames}
              empty={
                recorderName
                  ? t(WEBUI.standardOverview.topQnamesEmpty)
                  : t(WEBUI.standardOverview.queryRecorderDisabled)
              }
              locale={locale}
            />
            <TopList
              title={t(WEBUI.standardOverview.topBlockedTitle)}
              subtitle={t(WEBUI.standardOverview.topBlockedSubtitle)}
              rows={stats.topBlockedQnames}
              empty={
                summary.adBlockEnabled
                  ? t(WEBUI.standardOverview.topBlockedEmpty)
                  : t(WEBUI.standardOverview.filterDisabledEmpty)
              }
              locale={locale}
            />
          </div>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0">
              <div>
                <CardTitle className="text-base">
                  {t(WEBUI.standardOverview.recentQueriesTitle)}
                </CardTitle>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t(WEBUI.standardOverview.recentQueriesDesc)}
                </p>
              </div>
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <Wifi className="size-3.5" />
                {recorderName ??
                  t(WEBUI.standardOverview.queryRecorderDisabled)}
              </div>
            </CardHeader>
            <CardContent>
              {stats.records.length > 0 ? (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>
                        {t(WEBUI.standardOverview.tableTime)}
                      </TableHead>
                      <TableHead>
                        {t(WEBUI.standardOverview.tableClient)}
                      </TableHead>
                      <TableHead>
                        {t(WEBUI.standardOverview.tableQuestion)}
                      </TableHead>
                      <TableHead>RCODE</TableHead>
                      <TableHead className="text-right">
                        {t(WEBUI.standardOverview.tableLatency)}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {stats.records.map((record) => (
                      <TableRow key={record.id}>
                        <TableCell className="whitespace-nowrap text-muted-foreground">
                          {new Date(record.created_at_ms).toLocaleTimeString()}
                        </TableCell>
                        <TableCell className="font-mono text-xs">
                          {record.client_ip}
                        </TableCell>
                        <TableCell className="max-w-[420px] truncate">
                          {formatQuestion(record)}
                        </TableCell>
                        <TableCell>
                          {record.rcode ?? (record.error ? "ERROR" : "-")}
                        </TableCell>
                        <TableCell className="text-right">
                          <span className="inline-flex items-center gap-1">
                            <Timer className="size-3" />
                            {record.elapsed_ms} ms
                          </span>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              ) : (
                <div className="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
                  {t(WEBUI.standardOverview.recentQueriesEmpty)}
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}
