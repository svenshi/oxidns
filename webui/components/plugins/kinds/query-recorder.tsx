/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  BarChart3,
  Filter,
  Globe,
  Info,
  Loader2,
  MousePointerClick,
  PieChart as PieChartIcon,
  Radio,
  RefreshCw,
  ServerCrash,
  Timer,
  Trash2,
  TrendingUp,
  Users,
  X,
} from "lucide-react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Line,
  LineChart,
  Pie,
  PieChart,
  ResponsiveContainer,
  ReferenceDot,
  Tooltip as RechartsTooltip,
  XAxis,
  YAxis,
} from "recharts";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { DateTimePicker } from "@/components/ui/datetime-picker";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  apiHeaders,
  apiUrl,
  clearQueryRecorderHistory,
  fetchQueryRecordDetail,
  fetchQueryRecorderLatency,
  fetchQueryRecorderPluginStats,
  fetchQueryRecorderQtypeDistribution,
  fetchQueryRecorderRcodeDistribution,
  fetchQueryRecorderTimeseries,
  fetchQueryRecorderTopClients,
  fetchQueryRecorderTopQnames,
  fetchQueryRecords,
  type QueryRecordDetail,
  type QueryRecordFilters,
  type QueryRecordRow,
  type QueryRecordStatusFilter,
  type QueryRecorderDistributionResponse,
  type QueryRecorderLatencySummary,
  type QueryRecorderPluginStatsRow,
  type QueryRecorderSampleMetadata,
  type QueryRecorderTimeseriesResponse,
  type QueryRecorderTopResponse,
} from "@/lib/oxidns-api";
import { useAppStore } from "@/lib/store";
import { cn } from "@/lib/utils";
import type {
  PluginComponentDefinition,
  PluginDetailComponentProps,
} from "../types";
import {
  PluginDetailTemplate,
  PluginNotAppliedPlaceholder,
} from "../plugin-detail-template";
import { usePluginAppliedStatus } from "@/hooks/use-plugin-applied";
import { useI18n } from "@/lib/i18n/provider";
import { WEBUI } from "@/lib/i18n/keys";
import { DnsRecordDetailDialog } from "../dns-record-detail-dialog";
import { QueryRecordFlowCanvas } from "../query-record-flow";
import {
  bucketRecordFilters,
  aggregationRecordFilters,
  type AggregationDrilldownKind,
  presetWindow,
  refreshWindow,
  TIMESERIES_OPTIONS,
  timeseriesCoverage,
  timeseriesDataIntervals,
  timeseriesChartPoints,
  type TimeseriesChartPoint,
  type AggregationWindow,
} from "./query-recorder-timeseries";
import {
  QueryRecorderTimeRange,
  localDateTime,
} from "./query-recorder-time-range";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

import {
  recordTextFilterForm,
  recordTextFilters,
  type QueryRecordTextFilterForm,
  type TextMatchMode,
} from "./query-recorder-filters";

function QueryRecorderDetail(props: PluginDetailComponentProps) {
  const { t } = useI18n();
  const [tab, setTab] = useState("config");
  const [selection, setSelection] = useState(() => presetWindow());
  const [insightsView, setInsightsView] = useState("timeseries");
  const [recordDrilldown, setRecordDrilldown] = useState<{
    tag: string;
    filters: QueryRecordFilters;
    nonce: number;
  } | null>(null);
  const openAggregateRecords = (
    kind: AggregationDrilldownKind,
    key: string,
  ) => {
    setRecordDrilldown((current) => ({
      tag: props.plugin.name,
      filters: aggregationRecordFilters(kind, key, selection),
      nonce: (current?.nonce ?? 0) + 1,
    }));
    setTab("stats");
  };
  const rememberRecordFilters = (filters: QueryRecordFilters) => {
    setRecordDrilldown((current) => ({
      tag: props.plugin.name,
      filters,
      nonce: current?.nonce ?? 0,
    }));
  };
  return (
    <PluginDetailTemplate
      {...props}
      icon={<Radio className="h-5 w-5" />}
      summaryItems={[
        { label: "SQLite", value: String(props.plugin.config.path ?? "-") },
        {
          label: t(WEBUI.queryRecorder.memoryTailLabel),
          value: String(
            props.plugin.config.memory_tail ?? t(WEBUI.common.defaultValue),
          ),
        },
        {
          label: t(WEBUI.queryRecorder.retentionLabel),
          value: t(WEBUI.queryRecorder.retentionDays, {
            days: String(
              props.plugin.config.retention_days ??
                t(WEBUI.common.defaultValue),
            ),
          }),
        },
      ]}
      tabValue={tab}
      onTabValueChange={setTab}
      statsTabLabel={t(WEBUI.queryRecorder.recordsTitle)}
      metricsContent={
        <QueryRecordsPanel
          key={`${props.plugin.name}-${recordDrilldown?.nonce ?? 0}`}
          tag={props.plugin.name}
          initialFilters={
            recordDrilldown?.tag === props.plugin.name
              ? recordDrilldown.filters
              : undefined
          }
          onFiltersChange={rememberRecordFilters}
        />
      }
      extraTabs={[
        {
          value: "insights",
          label: t(WEBUI.queryRecorder.insightsTab),
          content: (
            <QueryRecorderInsightsPanel
              tag={props.plugin.name}
              selection={selection}
              onSelectionChange={setSelection}
              view={insightsView}
              onViewChange={setInsightsView}
              onSelectRecords={openAggregateRecords}
            />
          ),
        },
      ]}
    />
  );
}

type QueryRecordFilterForm = QueryRecordTextFilterForm & {
  qtype: string;
  rcode: string;
  status: QueryRecordStatusFilter;
  sinceLocal: string;
  untilLocal: string;
};

const EMPTY_FILTER_FORM: QueryRecordFilterForm = {
  ...recordTextFilterForm(),
  qtype: "all",
  rcode: "all",
  status: "all",
  sinceLocal: "",
  untilLocal: "",
};

const QTYPE_OPTIONS = [
  "A",
  "AAAA",
  "HTTPS",
  "CNAME",
  "TXT",
  "MX",
  "NS",
  "SOA",
  "SRV",
  "PTR",
  "SVCB",
  "CAA",
  "DNSKEY",
  "DS",
];

const RCODE_OPTIONS = [
  "No Error",
  "Non-Existent Domain",
  "Server Failure",
  "Query Refused",
  "Format Error",
  "Not Implemented",
];

const CHART_COLORS = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
];
const TOP_PAGE_SIZE = 20;
// Per-row height for the horizontal Top-N bar charts. Recharts' category axis
// drops tick labels when bars are packed too tightly, so the chart container
// grows with the row count to guarantee every Top-N entry stays labeled.
const CHART_ROW_HEIGHT = 24;
const CHART_MIN_HEIGHT = 240;
const CHART_VERTICAL_PADDING = 16;
// Cap the chart's visible footprint. When Load more pushes the row count past
// what fits here, the chart keeps its full per-row height and the container
// scrolls instead of growing without bound, so labels stay 1:1 either way.
const CHART_MAX_HEIGHT = 640;

// ---------------------------------------------------------------------------
// Query-record tab: filters, pagination, details, and live subscription.
// Matcher drilldown starts a fresh record view with the selected interval.
// ---------------------------------------------------------------------------

function QueryRecordsPanel({
  tag,
  initialFilters,
  onFiltersChange,
}: {
  tag: string;
  initialFilters?: QueryRecordFilters;
  onFiltersChange?: (filters: QueryRecordFilters) => void;
}) {
  const appliedStatus = usePluginAppliedStatus(tag);
  if (appliedStatus === "not-applied") {
    return <PluginNotAppliedPlaceholder />;
  }
  return (
    <QueryRecordsPanelInner
      tag={tag}
      initialFilters={initialFilters}
      onFiltersChange={onFiltersChange}
    />
  );
}

function QueryRecordsPanelInner({
  tag,
  fixedFilters,
  initialFilters,
  onFiltersChange,
}: {
  tag: string;
  fixedFilters?: QueryRecordFilters;
  initialFilters?: QueryRecordFilters;
  onFiltersChange?: (filters: QueryRecordFilters) => void;
}) {
  const { locale, t } = useI18n();
  const [records, setRecords] = useState<QueryRecordRow[]>([]);
  const [nextCursor, setNextCursor] = useState<string | undefined>();
  const [selected, setSelected] = useState<QueryRecordDetail | null>(null);
  const [filterForm, setFilterForm] = useState<QueryRecordFilterForm>(() => ({
    ...EMPTY_FILTER_FORM,
    ...recordTextFilterForm(initialFilters),
    qtype: initialFilters?.qtype ?? "all",
    rcode: initialFilters?.rcode ?? "all",
    status: initialFilters?.status ?? "all",
    sinceLocal:
      initialFilters?.sinceMs == null
        ? ""
        : localDateTime(initialFilters.sinceMs),
    untilLocal:
      initialFilters?.untilMs == null
        ? ""
        : localDateTime(initialFilters.untilMs),
  }));
  const [appliedFilters, setAppliedFilters] = useState<QueryRecordFilters>(
    () => fixedFilters ?? initialFilters ?? {},
  );
  const [loading, setLoading] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastClearCount, setLastClearCount] = useState<number | null>(null);
  // SSE controller (existing behavior).
  const abortRef = useRef<AbortController | null>(null);
  // Cancel the previous /records load when a new one starts (e.g. rapid
  // matcher-click). Without this each click piles a new SQL query onto the
  // blocking pool while the previous one is still running.
  const recordsAbortRef = useRef<AbortController | null>(null);
  const filtersRef = useRef<QueryRecordFilters>(
    fixedFilters ?? initialFilters ?? {},
  );

  useEffect(() => {
    filtersRef.current = appliedFilters;
  }, [appliedFilters]);

  const loadRecords = useCallback(
    async (filters: QueryRecordFilters, cursor?: string) => {
      recordsAbortRef.current?.abort();
      const controller = new AbortController();
      recordsAbortRef.current = controller;
      setLoading(true);
      setError(null);
      try {
        const response = await fetchQueryRecords(tag, {
          limit: 100,
          cursor,
          ...filters,
          signal: controller.signal,
        });
        if (controller.signal.aborted) return;
        setRecords((current) =>
          cursor ? [...current, ...response.records] : response.records,
        );
        setNextCursor(response.next_cursor);
      } catch (err) {
        if (controller.signal.aborted) return;
        setError(
          err instanceof Error
            ? err.message
            : t(WEBUI.queryRecorder.readRecordsFailed),
        );
      } finally {
        if (recordsAbortRef.current === controller) {
          recordsAbortRef.current = null;
          setLoading(false);
        }
      }
    },
    [tag, t],
  );

  const refresh = useCallback(
    async (filters: QueryRecordFilters = appliedFilters) => {
      await loadRecords(filters);
    },
    [appliedFilters, loadRecords],
  );

  useEffect(() => {
    const timer = window.setTimeout(
      () => void loadRecords(filtersRef.current),
      0,
    );
    return () => {
      window.clearTimeout(timer);
      abortRef.current?.abort();
      recordsAbortRef.current?.abort();
    };
  }, [loadRecords]);

  const applyFilters = () => {
    const nextFilters = {
      ...filtersFromForm(filterForm),
      rcodeBucket: appliedFilters.rcodeBucket,
    };
    // Preserve the exact drilldown boundaries when minute fields were not edited.
    if (
      appliedFilters.sinceMs != null &&
      filterForm.sinceLocal === localDateTime(appliedFilters.sinceMs)
    )
      nextFilters.sinceMs = appliedFilters.sinceMs;
    if (
      appliedFilters.untilMs != null &&
      filterForm.untilLocal === localDateTime(appliedFilters.untilMs)
    )
      nextFilters.untilMs = appliedFilters.untilMs;
    setAppliedFilters(nextFilters);
    onFiltersChange?.(nextFilters);
    void refresh(nextFilters);
  };

  const clearFilters = () => {
    setFilterForm({ ...EMPTY_FILTER_FORM });
    setAppliedFilters({});
    onFiltersChange?.({});
    void refresh({});
  };

  const clearDrilldownFilter = (key: DrilldownFilterKey) => {
    const next = { ...appliedFilters };
    delete next[key];
    setAppliedFilters(next);
    onFiltersChange?.(next);
    void loadRecords(next);
  };

  const openDetail = async (record: QueryRecordRow) => {
    setError(null);
    try {
      const detail = await fetchQueryRecordDetail(tag, record.id);
      setSelected(detail.record);
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.queryRecorder.readDetailFailed),
      );
    }
  };

  const handleClearHistory = async () => {
    recordsAbortRef.current?.abort();
    setClearing(true);
    setError(null);
    setLastClearCount(null);
    try {
      const response = await clearQueryRecorderHistory(tag);
      setRecords([]);
      setNextCursor(undefined);
      setSelected(null);
      setLastClearCount(response.cleared_records);
      await refresh(appliedFilters);
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.queryRecorder.clearHistoryFailed),
      );
    } finally {
      setClearing(false);
    }
  };

  const toggleStream = async () => {
    if (streaming) {
      abortRef.current?.abort();
      abortRef.current = null;
      setStreaming(false);
      return;
    }

    const controller = new AbortController();
    abortRef.current = controller;
    setStreaming(true);
    setError(null);
    try {
      const response = await fetch(
        apiUrl(`/plugins/${encodeURIComponent(tag)}/stream?tail=20`),
        {
          headers: { ...apiHeaders(), Accept: "text/event-stream" },
          signal: controller.signal,
        },
      );
      if (!response.ok || !response.body) {
        throw new Error(
          t(WEBUI.queryRecorder.streamHttpFailed, { status: response.status }),
        );
      }
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      while (!controller.signal.aborted) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder
          .decode(value, { stream: true })
          .replace(/\r\n/g, "\n")
          .replace(/\r/g, "\n");
        const chunks = buffer.split("\n\n");
        buffer = chunks.pop() ?? "";
        for (const chunk of chunks) {
          const event = parseSseEvent(chunk);
          if (event.event === "error") {
            setError(
              event.data
                ? parseSseErrorMessage(event.data, t)
                : t(WEBUI.queryRecorder.streamError),
            );
            continue;
          }
          if (!event.data) continue;
          const record = parseStreamRecord(event.data);
          if (!record) {
            setError(t(WEBUI.queryRecorder.streamInvalidEvent));
            continue;
          }
          if (!recordMatchesFilters(record, filtersRef.current)) continue;
          setRecords((current) =>
            [record, ...current.filter((item) => item.id !== record.id)].slice(
              0,
              200,
            ),
          );
        }
      }
    } catch (err) {
      if (!controller.signal.aborted) {
        setError(
          err instanceof Error
            ? err.message
            : t(WEBUI.queryRecorder.streamConnFailed),
        );
      }
    } finally {
      if (abortRef.current === controller) {
        abortRef.current = null;
        setStreaming(false);
      }
    }
  };

  const activeFilterCount = countActiveFilters(appliedFilters);

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
          <div className="min-w-0">
            <CardTitle className="text-sm">
              {t(WEBUI.queryRecorder.recordsTitle)}
            </CardTitle>
            <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
              <span className="rounded-full border bg-muted/30 px-2 py-0.5">
                {t(WEBUI.queryRecorder.loadedRecords, {
                  count: records.length,
                })}
              </span>
              {activeFilterCount > 0 && (
                <span className="rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-primary">
                  {t(WEBUI.queryRecorder.activeFilters, {
                    count: activeFilterCount,
                  })}
                </span>
              )}
              <span className="rounded-full border bg-muted/30 px-2 py-0.5">
                {t(WEBUI.queryRecorder.errorCount, {
                  count: records.filter((record) => record.error).length,
                })}
              </span>
              {!fixedFilters && (
                <DrilldownFilterChips
                  filters={appliedFilters}
                  onClear={clearDrilldownFilter}
                />
              )}
              {loading && (
                <span className="inline-flex items-center gap-1 rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-primary">
                  <Loader2 className="h-3 w-3 animate-spin" />
                  {t(WEBUI.queryRecorder.loadingIndicator)}
                </span>
              )}
              {clearing && (
                <span className="inline-flex items-center gap-1 rounded-full border border-destructive/30 bg-destructive/10 px-2 py-0.5 text-destructive">
                  <Loader2 className="h-3 w-3 animate-spin" />
                  {t(WEBUI.queryRecorder.clearingIndicator)}
                </span>
              )}
              {lastClearCount !== null && !clearing && (
                <span className="rounded-full border border-destructive/30 bg-destructive/10 px-2 py-0.5 text-destructive">
                  {t(WEBUI.queryRecorder.clearedCount, {
                    count: lastClearCount,
                  })}
                </span>
              )}
              {streaming && (
                <span className="rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-primary">
                  {t(WEBUI.queryRecorder.streamingStatus)}
                </span>
              )}
            </div>
          </div>
          <div className="flex flex-wrap justify-end gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={loading || clearing}
              onClick={() => void refresh(appliedFilters)}
            >
              <RefreshCw className="h-4 w-4" />
              {t(WEBUI.common.refresh)}
            </Button>
            {!fixedFilters && (
              <>
                <Button
                  variant={streaming ? "secondary" : "outline"}
                  size="sm"
                  disabled={clearing}
                  onClick={() => void toggleStream()}
                >
                  <Radio className="h-4 w-4" />
                  {streaming
                    ? t(WEBUI.queryRecorder.stopStreaming)
                    : t(WEBUI.queryRecorder.startStreaming)}
                </Button>
                <AlertDialog>
                  <AlertDialogTrigger asChild>
                    <Button variant="outline" size="sm" disabled={clearing}>
                      {clearing ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Trash2 className="h-4 w-4" />
                      )}
                      {t(WEBUI.queryRecorder.clearHistoryButton)}
                    </Button>
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogMedia className="bg-destructive/10 text-destructive">
                        <Trash2 className="h-5 w-5" />
                      </AlertDialogMedia>
                      <AlertDialogTitle>
                        {t(WEBUI.queryRecorder.clearHistoryTitle)}
                      </AlertDialogTitle>
                      <AlertDialogDescription>
                        {t(WEBUI.queryRecorder.clearHistoryDesc, { tag })}
                      </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel disabled={clearing}>
                        {t(WEBUI.common.cancel)}
                      </AlertDialogCancel>
                      <AlertDialogAction
                        variant="destructive"
                        disabled={clearing}
                        onClick={() => void handleClearHistory()}
                      >
                        {t(WEBUI.queryRecorder.clearHistoryButton)}
                      </AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              </>
            )}
          </div>
        </CardHeader>
        <CardContent className="p-4 pt-0">
          {!fixedFilters && (
            <form
              className="mb-3 rounded-md border bg-muted/20 p-3"
              onSubmit={(event) => {
                event.preventDefault();
                applyFilters();
              }}
            >
              <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-4">
                <TextFilterField
                  label={t(WEBUI.queryRecorder.qnameFilter)}
                  value={filterForm.qname}
                  match={filterForm.qnameMatch}
                  onValueChange={(qname) =>
                    setFilterForm((current) => ({ ...current, qname }))
                  }
                  onMatchChange={(qnameMatch) =>
                    setFilterForm((current) => ({ ...current, qnameMatch }))
                  }
                  placeholder="example.com"
                />
                <FilterField label="QTYPE">
                  <Select
                    value={filterForm.qtype}
                    onValueChange={(qtype) =>
                      setFilterForm((current) => ({ ...current, qtype }))
                    }
                  >
                    <SelectTrigger className="h-8 w-full font-mono">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">{t(WEBUI.common.all)}</SelectItem>
                      {QTYPE_OPTIONS.map((qtype) => (
                        <SelectItem key={qtype} value={qtype}>
                          {qtype}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </FilterField>
                <TextFilterField
                  label={t(WEBUI.queryRecorder.clientIpFilter)}
                  value={filterForm.clientIp}
                  match={filterForm.clientIpMatch}
                  onValueChange={(clientIp) =>
                    setFilterForm((current) => ({ ...current, clientIp }))
                  }
                  onMatchChange={(clientIpMatch) =>
                    setFilterForm((current) => ({ ...current, clientIpMatch }))
                  }
                  placeholder={
                    filterForm.clientIpMatch === "exact"
                      ? "192.168.1.1 / ::1"
                      : "192.168 / ::1"
                  }
                />
                <FilterField label="RCODE">
                  <Select
                    value={filterForm.rcode}
                    onValueChange={(rcode) =>
                      setFilterForm((current) => ({ ...current, rcode }))
                    }
                  >
                    <SelectTrigger className="h-8 w-full font-mono">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">{t(WEBUI.common.all)}</SelectItem>
                      {RCODE_OPTIONS.map((rcode) => (
                        <SelectItem key={rcode} value={rcode}>
                          {rcode}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </FilterField>
                <FilterField
                  label={
                    <MetricHelp
                      label="Matcher"
                      help={t(WEBUI.queryRecorder.matcherFilterHint)}
                    />
                  }
                >
                  <Input
                    value={filterForm.matcherTag}
                    onChange={(event) =>
                      setFilterForm((current) => ({
                        ...current,
                        matcherTag: event.target.value,
                      }))
                    }
                    aria-label="Matcher"
                    placeholder={t(
                      WEBUI.queryRecorder.matcherFilterPlaceholder,
                    )}
                    className="h-8 font-mono"
                  />
                </FilterField>
                <FilterField label={t(WEBUI.queryRecorder.statusFilter)}>
                  <Select
                    value={filterForm.status}
                    onValueChange={(status) =>
                      setFilterForm((current) => ({
                        ...current,
                        status: status as QueryRecordStatusFilter,
                      }))
                    }
                  >
                    <SelectTrigger className="h-8 w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">{t(WEBUI.common.all)}</SelectItem>
                      <SelectItem value="error">
                        {t(WEBUI.queryRecorder.statusError)}
                      </SelectItem>
                      <SelectItem value="has_response">
                        {t(WEBUI.queryRecorder.statusHasResponse)}
                      </SelectItem>
                      <SelectItem value="no_response">
                        {t(WEBUI.queryRecorder.statusNoResponse)}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </FilterField>
                <FilterField label={t(WEBUI.queryRecorder.startTimeFilter)}>
                  <DateTimePicker
                    value={filterForm.sinceLocal}
                    onChange={(sinceLocal) =>
                      setFilterForm((current) => ({
                        ...current,
                        sinceLocal,
                      }))
                    }
                    placeholder={t(WEBUI.queryRecorder.startTimePlaceholder)}
                  />
                </FilterField>
                <FilterField label={t(WEBUI.queryRecorder.endTimeFilter)}>
                  <DateTimePicker
                    value={filterForm.untilLocal}
                    onChange={(untilLocal) =>
                      setFilterForm((current) => ({
                        ...current,
                        untilLocal,
                      }))
                    }
                    placeholder={t(WEBUI.queryRecorder.endTimePlaceholder)}
                  />
                </FilterField>
                <div className="flex items-end justify-end gap-2 md:col-span-2 xl:col-span-4">
                  <Button type="submit" size="sm" className="h-8">
                    <Filter className="h-4 w-4" />
                    {t(WEBUI.common.apply)}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-8"
                    onClick={clearFilters}
                  >
                    <X className="h-4 w-4" />
                    {t(WEBUI.common.clear)}
                  </Button>
                </div>
              </div>
            </form>
          )}
          {error && (
            <div className="mb-3 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}
          <div className="relative overflow-hidden rounded-md border">
            {loading && (
              <div
                className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-background/60 backdrop-blur-[1px]"
                aria-live="polite"
                role="status"
              >
                <div className="inline-flex items-center gap-2 rounded-full border border-primary/30 bg-background/95 px-3 py-1.5 text-xs text-primary shadow-sm">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  {t(WEBUI.queryRecorder.loadingOverlay)}
                </div>
              </div>
            )}
            <Table className="min-w-[760px]">
              <TableHeader>
                <TableRow className="bg-muted/30 hover:bg-muted/30">
                  <TableHead>{t(WEBUI.queryRecorder.queryColumn)}</TableHead>
                  <TableHead>{t(WEBUI.queryRecorder.clientColumn)}</TableHead>
                  <TableHead>{t(WEBUI.queryRecorder.timeColumn)}</TableHead>
                  <TableHead>{t(WEBUI.queryRecorder.resultColumn)}</TableHead>
                  <TableHead>{t(WEBUI.queryRecorder.elapsedColumn)}</TableHead>
                  <TableHead>
                    <span className="inline-flex items-center gap-1">
                      {t(WEBUI.queryRecorder.recordCountColumn)}
                      <Popover>
                        <PopoverTrigger asChild>
                          <button
                            type="button"
                            className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none"
                            aria-label={t(WEBUI.queryRecorder.recordCountAria)}
                          >
                            <Info className="h-3.5 w-3.5" />
                          </button>
                        </PopoverTrigger>
                        <PopoverContent
                          side="top"
                          className="w-auto max-w-[16rem] p-2 text-xs"
                        >
                          {t(WEBUI.dnsRecord.responseRecordsTooltip)}
                        </PopoverContent>
                      </Popover>
                    </span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {records.map((record) => (
                  <TableRow
                    key={record.id}
                    className="cursor-pointer"
                    onClick={() => openDetail(record)}
                  >
                    <TableCell className="max-w-[22rem]">
                      <div className="flex min-w-0 items-center gap-2">
                        <span
                          className="truncate font-mono"
                          title={formatQuestion(record)}
                        >
                          {formatQuestionName(record)}
                        </span>
                        {formatQuestionType(record) !== "-" && (
                          <Badge variant="outline" className="font-mono">
                            {formatQuestionType(record)}
                          </Badge>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {record.client_ip}
                    </TableCell>
                    <TableCell className="font-mono text-xs text-muted-foreground">
                      {formatTime(record.created_at_ms, locale)}
                    </TableCell>
                    <TableCell>{queryStatusBadge(record)}</TableCell>
                    <TableCell className="font-mono">
                      <span
                        className={cn(
                          "inline-flex min-w-16 justify-center rounded border px-1.5 py-0.5 text-xs font-medium tabular-nums",
                          queryElapsedClassName(record.elapsed_ms),
                        )}
                        title={queryElapsedTitle(record.elapsed_ms, t)}
                      >
                        {record.elapsed_ms}ms
                      </span>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1 font-mono text-xs">
                        <span>{record.answer_count}</span>
                        <span className="text-muted-foreground">
                          / {record.authority_count} / {record.additional_count}
                        </span>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
                {!records.length && (
                  <TableRow>
                    <TableCell
                      colSpan={6}
                      className="h-24 text-center text-muted-foreground"
                    >
                      {loading
                        ? t(WEBUI.queryRecorder.loadingRecords)
                        : t(WEBUI.queryRecorder.noRecords)}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
          {nextCursor && (
            <Button
              variant="outline"
              size="sm"
              className="mt-3"
              disabled={loading}
              onClick={() => void loadRecords(appliedFilters, nextCursor)}
            >
              {t(WEBUI.common.loadMore)}
            </Button>
          )}
        </CardContent>
        <RecordDetailDialog
          record={selected}
          onClose={() => setSelected(null)}
        />
      </Card>
    </div>
  );
}

type DrilldownFilterKey = "rcodeBucket";

function DrilldownFilterChips({
  filters,
  onClear,
}: {
  filters: QueryRecordFilters;
  onClear: (key: DrilldownFilterKey) => void;
}) {
  const { t } = useI18n();
  if (!filters.rcodeBucket) return null;
  return (
    <button
      type="button"
      className="inline-flex items-center gap-1 rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-primary hover:bg-primary/20 focus-visible:ring-2 focus-visible:ring-ring"
      title={t(WEBUI.queryRecorder.clearScopeFilter)}
      onClick={() => onClear("rcodeBucket")}
    >
      {t(WEBUI.queryRecorder.specialRcodeFilter, {
        value: rcodeLabel(filters.rcodeBucket, t),
      })}
      <X className="size-3" />
    </button>
  );
}

function MetricHelp({ label, help }: { label: ReactNode; help: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="inline-flex items-center gap-1 text-left focus-visible:ring-2 focus-visible:ring-ring"
        >
          {label}
          <Info className="size-3 shrink-0 text-muted-foreground" />
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-80">{help}</TooltipContent>
    </Tooltip>
  );
}

function SampleSummary({
  sample,
  count,
  filters,
  children,
}: {
  sample: QueryRecorderSampleMetadata | null;
  count: number;
  filters: QueryRecordFilters;
  children?: ReactNode;
}) {
  const { t, locale, formatNumber } = useI18n();
  const coverage = sample ? timeseriesCoverage(sample, filters) : null;
  return (
    <div className="mt-2 space-y-2">
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <Badge variant="outline">
          {t(WEBUI.queryRecorder.sampleCount, { count: formatNumber(count) })}
        </Badge>
        {children}
        {sample?.sample_truncated && (
          <Badge variant="outline" className="border-primary/30 text-primary">
            <MetricHelp
              label={t(WEBUI.queryRecorder.limitedStatistics)}
              help={t(WEBUI.queryRecorder.limitedStatisticsHelp, {
                count: formatNumber(sample.sample_limit),
              })}
            />
          </Badge>
        )}
      </div>
      {coverage && sample?.sample_truncated && (
        <p className="break-words text-xs font-medium">
          {t(WEBUI.queryRecorder.coverageRange, {
            start: formatFullTime(coverage.sinceMs, locale),
            end: formatFullTime(coverage.untilMs, locale),
          })}
        </p>
      )}
    </div>
  );
}

function RecordLink({
  value,
  onClick,
}: {
  value: string;
  onClick: () => void;
}) {
  const { t } = useI18n();
  return (
    <button
      type="button"
      title={t(WEBUI.queryRecorder.viewMatchingRecords)}
      className="cursor-pointer rounded text-left hover:text-primary focus-visible:ring-2 focus-visible:ring-ring"
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      {value}
    </button>
  );
}

function rcodeLabel(value: string, t: TFn) {
  return value === "_ERROR"
    ? t(WEBUI.queryRecorder.statusError)
    : value === "_NO_RESPONSE"
      ? t(WEBUI.queryRecorder.statusNoResponse)
      : value === "_UNKNOWN"
        ? t(WEBUI.queryRecorder.rcodeUnknown)
        : value;
}

function MatcherStatsCard({
  tag,
  filters,
  onSelectMatcher,
  onRefresh,
  onData,
}: {
  tag: string;
  filters: QueryRecordFilters;
  onSelectMatcher: (matcherTag: string) => void;
  onRefresh: () => void;
  onData: (data: QueryRecorderSampleMetadata) => void;
}) {
  const { t } = useI18n();
  const [stats, setStats] = useState<QueryRecorderPluginStatsRow[]>([]);
  const [queryTotal, setQueryTotal] = useState(0);
  const [sample, setSample] = useState<QueryRecorderSampleMetadata | null>(
    null,
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const load = useCallback(async () => {
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    setLoading(true);
    setError(null);
    try {
      const response = await fetchQueryRecorderPluginStats(tag, {
        ...filters,
        kind: "matcher",
        signal: controller.signal,
      });
      if (controller.signal.aborted || abortRef.current !== controller) return;
      setStats(response.stats.filter((row) => row.kind === "matcher"));
      setQueryTotal(response.query_total);
      setSample(response);
      onData(response);
    } catch (err) {
      if (controller.signal.aborted || abortRef.current !== controller) return;
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.queryRecorder.readMatcherStatsFailed),
      );
    } finally {
      if (abortRef.current === controller) {
        abortRef.current = null;
        setLoading(false);
      }
    }
  }, [tag, filters, t, onData]);
  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0);
    return () => {
      window.clearTimeout(timer);
      abortRef.current?.abort();
    };
  }, [load]);
  return (
    <Card>
      <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
        <div className="min-w-0">
          <CardTitle className="flex items-center gap-2 text-sm">
            <BarChart3 className="h-4 w-4 text-primary" />
            {t(WEBUI.queryRecorder.matcherStatsTitle)}
          </CardTitle>
          <SampleSummary sample={sample} count={queryTotal} filters={filters}>
            <Badge variant="outline">
              {t(WEBUI.queryRecorder.matcherCount, { count: stats.length })}
            </Badge>
          </SampleSummary>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={loading}
          onClick={onRefresh}
        >
          <RefreshCw className="h-4 w-4" />
          {t(WEBUI.common.refresh)}
        </Button>
      </CardHeader>
      <CardContent className="p-4 pt-0">
        {error && (
          <div className="mb-3 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        <div className="overflow-hidden rounded-md border">
          <Table className="min-w-[680px]">
            <TableHeader>
              <TableRow className="bg-muted/30 hover:bg-muted/30">
                <TableHead>{t(WEBUI.queryRecorder.matcherColumn)}</TableHead>
                <TableHead>{t(WEBUI.queryRecorder.checkedColumn)}</TableHead>
                <TableHead>{t(WEBUI.queryRecorder.hitColumn)}</TableHead>
                <TableHead>
                  <MetricHelp
                    label={t(WEBUI.queryRecorder.hitRateColumn)}
                    help={t(WEBUI.queryRecorder.matcherHitRateHelp)}
                  />
                </TableHead>
                <TableHead>
                  <MetricHelp
                    label={t(WEBUI.queryRecorder.matcherCoverageColumn)}
                    help={t(WEBUI.queryRecorder.matcherCoverageHelp)}
                  />
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {stats.map((row) => (
                <TableRow
                  key={row.tag ?? "(unknown)"}
                  className={row.tag ? "group cursor-pointer" : undefined}
                  onClick={
                    row.tag ? () => onSelectMatcher(row.tag!) : undefined
                  }
                  title={
                    row.tag ? t(WEBUI.queryRecorder.clickToFilter) : undefined
                  }
                >
                  <TableCell className="font-mono">
                    {row.tag ? (
                      <button
                        type="button"
                        className="cursor-pointer rounded text-left group-hover:text-primary hover:text-primary focus-visible:ring-2 focus-visible:ring-ring"
                        onClick={(event) => {
                          event.stopPropagation();
                          onSelectMatcher(row.tag!);
                        }}
                      >
                        {row.tag}
                      </button>
                    ) : (
                      <span>—</span>
                    )}
                  </TableCell>
                  <TableCell className="font-mono">{row.checked}</TableCell>
                  <TableCell className="font-mono">{row.matched}</TableCell>
                  <TableCell className="font-mono">
                    {formatPercent(safeRatio(row.matched, row.checked))}
                  </TableCell>
                  <TableCell className="font-mono">
                    {formatPercent(row.query_share)}
                  </TableCell>
                </TableRow>
              ))}
              {!stats.length && (
                <TableRow>
                  <TableCell
                    colSpan={5}
                    className="h-20 text-center text-muted-foreground"
                  >
                    {loading
                      ? t(WEBUI.queryRecorder.loadingMatcherStats)
                      : t(WEBUI.queryRecorder.noMatcherStats)}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  );
}

// ---------------------------------------------------------------------------
// Aggregate tab: top-level insights independent from the record tab's
// filter form. A shared bounded time window drives the chart sub-tabs.
// ---------------------------------------------------------------------------

type TFn = (
  key: string,
  params?: Record<string, string | number | boolean | null | undefined>,
) => string;

interface QueryRecorderInsightsProps {
  tag: string;
  selection: AggregationWindow;
  onSelectionChange: (value: AggregationWindow) => void;
  view: string;
  onViewChange: (value: string) => void;
  onSelectRecords: (kind: AggregationDrilldownKind, key: string) => void;
}

function QueryRecorderInsightsPanel(props: QueryRecorderInsightsProps) {
  const appliedStatus = usePluginAppliedStatus(props.tag);
  if (appliedStatus === "not-applied") return <PluginNotAppliedPlaceholder />;
  return <QueryRecorderInsightsPanelInner {...props} />;
}

function QueryRecorderInsightsPanelInner({
  tag,
  selection,
  onSelectionChange,
  view,
  onViewChange,
  onSelectRecords,
}: QueryRecorderInsightsProps) {
  const { t } = useI18n();
  const [trendMetric, setTrendMetric] = useState<"avg_ms" | "p95_ms">("avg_ms");
  const [timelineSample, setTimelineSample] = useState<{
    tag: string;
    selection: typeof selection;
    data: QueryRecorderSampleMetadata;
  } | null>(null);
  const captureTimelineSample = useCallback(
    (data: QueryRecorderSampleMetadata) => {
      setTimelineSample({ tag, selection, data });
    },
    [tag, selection],
  );
  const dataCoverage = useMemo(
    () =>
      timelineSample?.tag === tag && timelineSample.selection === selection
        ? timeseriesDataIntervals(timelineSample.data, {
            sinceMs: selection.sinceMs,
            untilMs: selection.untilMs,
          })
        : [],
    [tag, selection, timelineSample],
  );
  const { range, sinceMs, untilMs, nonce } = selection;
  const filters = useMemo(() => ({ sinceMs, untilMs }), [sinceMs, untilMs]);
  const refresh = () => onSelectionChange(refreshWindow(selection));

  return (
    <div className="space-y-4">
      <Card className="py-0">
        <CardContent className="p-3">
          <QueryRecorderTimeRange
            title={
              <>
                <BarChart3 className="h-4 w-4 text-primary" />
                {t(WEBUI.queryRecorder.insightsViewTitle)}
              </>
            }
            value={selection}
            dataCoverage={dataCoverage}
            onChange={onSelectionChange}
            onRefresh={refresh}
          />
        </CardContent>
      </Card>

      <Tabs value={view} onValueChange={onViewChange}>
        <TabsList className="flex w-full flex-nowrap justify-start gap-1 overflow-x-auto overflow-y-hidden">
          <TabsTrigger value="timeseries" className="shrink-0">
            <TrendingUp className="h-3.5 w-3.5" />
            {t(WEBUI.queryRecorder.trendTab)}
          </TabsTrigger>
          <TabsTrigger value="latency" className="shrink-0">
            <Timer className="h-3.5 w-3.5" />
            {t(WEBUI.queryRecorder.latencyTab)}
          </TabsTrigger>
          <TabsTrigger value="rcode" className="shrink-0">
            <PieChartIcon className="h-3.5 w-3.5" />
            RCODE
          </TabsTrigger>
          <TabsTrigger value="matchers" className="shrink-0">
            <Filter className="h-3.5 w-3.5" />
            {t(WEBUI.queryRecorder.matchersTab)}
          </TabsTrigger>
          <TabsTrigger value="clients" className="shrink-0">
            <Users className="h-3.5 w-3.5" />
            {t(WEBUI.queryRecorder.clientsTab)}
          </TabsTrigger>
          <TabsTrigger value="qnames" className="shrink-0">
            <Globe className="h-3.5 w-3.5" />
            {t(WEBUI.queryRecorder.domainsTab)}
          </TabsTrigger>
          <TabsTrigger value="qtype" className="shrink-0">
            <BarChart3 className="h-3.5 w-3.5" />
            QTYPE
          </TabsTrigger>
        </TabsList>
        <TabsContent value="matchers" className="min-h-[40rem]">
          <MatcherStatsCard
            key={`matchers-${tag}-${sinceMs}-${untilMs}-${range}-${nonce}`}
            tag={tag}
            filters={filters}
            onSelectMatcher={(key) => onSelectRecords("matcher", key)}
            onData={captureTimelineSample}
            onRefresh={refresh}
          />
        </TabsContent>
        <TabsContent value="clients" className="min-h-[40rem]">
          <TopBucketsCard
            key={`clients-${tag}-${sinceMs}-${untilMs}-${range}-${nonce}`}
            title={t(WEBUI.queryRecorder.topClientsTitle)}
            icon={<Users className="h-4 w-4 text-primary" />}
            tag={tag}
            filters={filters}
            fetcher={fetchQueryRecorderTopClients}
            onRefresh={refresh}
            onData={captureTimelineSample}
            onSelect={(key) => onSelectRecords("client", key)}
            keyLabel={t(WEBUI.queryRecorder.clientIpKeyLabel)}
          />
        </TabsContent>
        <TabsContent value="qnames" className="min-h-[40rem]">
          <TopBucketsCard
            key={`qnames-${tag}-${sinceMs}-${untilMs}-${range}-${nonce}`}
            title={t(WEBUI.queryRecorder.topDomainsTitle)}
            icon={<Globe className="h-4 w-4 text-primary" />}
            tag={tag}
            filters={filters}
            fetcher={fetchQueryRecorderTopQnames}
            onRefresh={refresh}
            onData={captureTimelineSample}
            onSelect={(key) => onSelectRecords("qname", key)}
            keyLabel="QNAME"
          />
        </TabsContent>
        <TabsContent value="qtype" className="min-h-[40rem]">
          <DistributionCard
            key={`qtype-${tag}-${sinceMs}-${untilMs}-${range}-${nonce}`}
            title={t(WEBUI.queryRecorder.qtypeDistTitle)}
            icon={<BarChart3 className="h-4 w-4 text-primary" />}
            tag={tag}
            filters={filters}
            fetcher={fetchQueryRecorderQtypeDistribution}
            onRefresh={refresh}
            onData={captureTimelineSample}
            onSelect={(key) => onSelectRecords("qtype", key)}
            keyLabel="QTYPE"
            preferBarChart
          />
        </TabsContent>
        <TabsContent value="rcode" className="min-h-[40rem]">
          <DistributionCard
            key={`rcode-${tag}-${sinceMs}-${untilMs}-${range}-${nonce}`}
            title={t(WEBUI.queryRecorder.rcodeDistTitle)}
            icon={<PieChartIcon className="h-4 w-4 text-primary" />}
            tag={tag}
            filters={filters}
            fetcher={fetchQueryRecorderRcodeDistribution}
            onRefresh={refresh}
            onData={captureTimelineSample}
            onSelect={(key) => onSelectRecords("rcode", key)}
            keyLabel="RCODE"
            preferBarChart={false}
          />
        </TabsContent>
        <TabsContent value="latency" className="min-h-[40rem]">
          <LatencyCard
            key={`latency-${tag}-${sinceMs}-${untilMs}-${range}-${nonce}`}
            onRefresh={refresh}
            onData={captureTimelineSample}
            onSelect={(key) => onSelectRecords("qname", key)}
            tag={tag}
            filters={filters}
          />
        </TabsContent>
        <TabsContent value="timeseries" className="min-h-[40rem]">
          <TimeseriesCard
            key={`timeseries-${tag}-${sinceMs}-${untilMs}-${range}-${nonce}`}
            onData={captureTimelineSample}
            tag={tag}
            filters={filters}
            metric={trendMetric}
            setMetric={setTrendMetric}
            onRefresh={refresh}
          />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function TopBucketsCard({
  title,
  icon,
  tag,
  filters,
  onRefresh,
  onData,
  onSelect,
  fetcher,
  keyLabel,
}: {
  title: string;
  icon: ReactNode;
  tag: string;
  filters: QueryRecordFilters;
  onRefresh: () => void;
  onData: (data: QueryRecorderSampleMetadata) => void;
  onSelect: (key: string) => void;
  fetcher: (
    tag: string,
    options: QueryRecordFilters & { limit?: number; signal?: AbortSignal },
  ) => Promise<QueryRecorderTopResponse>;
  keyLabel: string;
}) {
  const { t } = useI18n();
  const [data, setData] = useState<QueryRecorderTopResponse | null>(null);
  const [limit, setLimit] = useState(TOP_PAGE_SIZE);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const load = useCallback(
    async (requestedLimit: number) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      setLoading(true);
      setError(null);
      try {
        const response = await fetcher(tag, {
          ...filters,
          limit: requestedLimit,
          signal: controller.signal,
        });
        if (controller.signal.aborted || abortRef.current !== controller) {
          return;
        }
        setData(response);
        onData(response);
        setLimit(requestedLimit);
      } catch (err) {
        if (controller.signal.aborted || abortRef.current !== controller) {
          return;
        }
        setError(
          err instanceof Error
            ? err.message
            : t(WEBUI.queryRecorder.readStatsFailed),
        );
      } finally {
        if (abortRef.current === controller) {
          abortRef.current = null;
          setLoading(false);
        }
      }
    },
    [tag, filters, fetcher, t, onData],
  );

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void load(TOP_PAGE_SIZE);
    }, 0);
    return () => {
      window.clearTimeout(timer);
      abortRef.current?.abort();
    };
  }, [load]);

  const chartData = useMemo(
    () =>
      (data?.rows ?? []).slice().map((row) => ({
        key: row.key,
        count: row.count,
        share: row.share,
      })),
    [data],
  );
  const hasMoreRows = chartData.length >= limit;
  // Vertical bar chart: each row needs its own vertical slot or recharts'
  // category axis silently drops labels to avoid overlap (issue #137). Size
  // the chart by row count so every Top-N entry keeps a readable, 1:1 label.
  const chartHeight = Math.max(
    CHART_MIN_HEIGHT,
    chartData.length * CHART_ROW_HEIGHT + CHART_VERTICAL_PADDING,
  );

  return (
    <Card className="mt-3">
      <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
        <div className="min-w-0">
          <CardTitle className="flex items-center gap-2 text-sm">
            {icon}
            {title}
          </CardTitle>
          <SampleSummary
            sample={data}
            count={data?.sample_size ?? 0}
            filters={filters}
          >
            <Badge variant="outline">
              {t(WEBUI.queryRecorder.topCount, { count: chartData.length })}
            </Badge>
          </SampleSummary>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={loading}
          onClick={onRefresh}
        >
          <RefreshCw className="h-4 w-4" />
          {t(WEBUI.common.refresh)}
        </Button>
      </CardHeader>
      <CardContent className="space-y-4 p-4 pt-0">
        {error && (
          <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        {chartData.length > 0 && (
          <div
            className="w-full overflow-y-auto"
            style={{ maxHeight: CHART_MAX_HEIGHT }}
          >
            <div className="w-full" style={{ height: chartHeight }}>
              <ResponsiveContainer width="100%" height="100%">
                <BarChart
                  style={{ cursor: "pointer" }}
                  data={chartData}
                  layout="vertical"
                  margin={{ top: 8, right: 16, left: 8, bottom: 8 }}
                >
                  <CartesianGrid
                    strokeDasharray="3 3"
                    stroke="var(--border)"
                    horizontal={false}
                  />
                  <XAxis
                    type="number"
                    stroke="var(--muted-foreground)"
                    fontSize={11}
                    allowDecimals={false}
                  />
                  <YAxis
                    type="category"
                    dataKey="key"
                    stroke="var(--muted-foreground)"
                    fontSize={11}
                    width={180}
                    interval={0}
                    tickFormatter={(value: string) => truncateMiddle(value, 28)}
                  />
                  <RechartsTooltip
                    cursor={{ fill: "var(--muted)", opacity: 0.3 }}
                    contentStyle={{
                      background: "var(--popover)",
                      border: "1px solid var(--border)",
                      borderRadius: 8,
                      color: "var(--foreground)",
                      fontSize: 12,
                    }}
                    formatter={(value: number, _name, props) => [
                      t(WEBUI.queryRecorder.timesWithPercent, {
                        count: value,
                        pct: formatPercent(
                          (props.payload?.share as number) ?? 0,
                        ),
                      }),
                      keyLabel,
                    ]}
                  />
                  <Bar
                    dataKey="count"
                    cursor="pointer"
                    onClick={(row) => onSelect(row.key)}
                    fill="var(--chart-1)"
                    radius={[0, 4, 4, 0]}
                  />
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>
        )}
        <div className="overflow-hidden rounded-md border">
          <Table className="min-w-[560px]">
            <TableHeader>
              <TableRow className="bg-muted/30 hover:bg-muted/30">
                <TableHead className="w-12">#</TableHead>
                <TableHead>{keyLabel}</TableHead>
                <TableHead>{t(WEBUI.queryRecorder.countColumn)}</TableHead>
                <TableHead>{t(WEBUI.queryRecorder.shareColumn)}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {chartData.map((row, index) => (
                <TableRow
                  key={row.key}
                  className="cursor-pointer"
                  onClick={() => onSelect(row.key)}
                  title={t(WEBUI.queryRecorder.viewMatchingRecords)}
                >
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {index + 1}
                  </TableCell>
                  <TableCell className="font-mono">
                    <RecordLink
                      value={row.key}
                      onClick={() => onSelect(row.key)}
                    />
                  </TableCell>
                  <TableCell className="font-mono">{row.count}</TableCell>
                  <TableCell className="font-mono">
                    {formatPercent(row.share)}
                  </TableCell>
                </TableRow>
              ))}
              {!chartData.length && (
                <TableRow>
                  <TableCell
                    colSpan={4}
                    className="h-20 text-center text-muted-foreground"
                  >
                    {loading
                      ? t(WEBUI.queryRecorder.loadingStats)
                      : t(WEBUI.queryRecorder.noData)}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
        {hasMoreRows && (
          <Button
            variant="outline"
            size="sm"
            disabled={loading}
            onClick={() => void load(limit + TOP_PAGE_SIZE)}
          >
            {t(WEBUI.common.loadMore)}
          </Button>
        )}
      </CardContent>
    </Card>
  );
}

function DistributionCard({
  title,
  icon,
  tag,
  filters,
  onRefresh,
  onData,
  onSelect,
  fetcher,
  keyLabel,
  preferBarChart,
}: {
  title: string;
  icon: ReactNode;
  tag: string;
  filters: QueryRecordFilters;
  onRefresh: () => void;
  onData: (data: QueryRecorderSampleMetadata) => void;
  onSelect: (key: string) => void;
  fetcher: (
    tag: string,
    options: QueryRecordFilters & { signal?: AbortSignal },
  ) => Promise<QueryRecorderDistributionResponse>;
  keyLabel: string;
  preferBarChart: boolean;
}) {
  const { t } = useI18n();
  const [data, setData] = useState<QueryRecorderDistributionResponse | null>(
    null,
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const load = useCallback(async () => {
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    setLoading(true);
    setError(null);
    try {
      const response = await fetcher(tag, {
        ...filters,
        signal: controller.signal,
      });
      if (controller.signal.aborted || abortRef.current !== controller) {
        return;
      }
      setData(response);
      onData(response);
    } catch (err) {
      if (controller.signal.aborted || abortRef.current !== controller) {
        return;
      }
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.queryRecorder.readDistFailed),
      );
    } finally {
      if (abortRef.current === controller) {
        abortRef.current = null;
        setLoading(false);
      }
    }
  }, [tag, filters, fetcher, t, onData]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void load();
    }, 0);
    return () => {
      window.clearTimeout(timer);
      abortRef.current?.abort();
    };
  }, [load]);

  const rows = useMemo(
    () =>
      (data?.rows ?? []).map((row) => ({
        ...row,
        label: rcodeLabel(row.key, t),
      })),
    [data, t],
  );

  return (
    <Card className="mt-3">
      <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
        <div className="min-w-0">
          <CardTitle className="flex items-center gap-2 text-sm">
            {icon}
            {title}
          </CardTitle>
          <SampleSummary
            sample={data}
            count={data?.sample_size ?? 0}
            filters={filters}
          >
            <Badge variant="outline">
              {t(WEBUI.queryRecorder.bucketCount, { count: rows.length })}
            </Badge>
          </SampleSummary>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={loading}
          onClick={onRefresh}
        >
          <RefreshCw className="h-4 w-4" />
          {t(WEBUI.common.refresh)}
        </Button>
      </CardHeader>
      <CardContent className="space-y-4 p-4 pt-0">
        {error && (
          <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        {rows.length > 0 && (
          <div className="h-[280px] w-full">
            <ResponsiveContainer width="100%" height="100%">
              {preferBarChart ? (
                <BarChart
                  style={{ cursor: "pointer" }}
                  data={rows}
                  margin={{ top: 8, right: 16, left: 0, bottom: 8 }}
                >
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                  <XAxis
                    dataKey="label"
                    stroke="var(--muted-foreground)"
                    fontSize={11}
                  />
                  <YAxis
                    stroke="var(--muted-foreground)"
                    fontSize={11}
                    allowDecimals={false}
                  />
                  <RechartsTooltip
                    cursor={{ fill: "var(--muted)", opacity: 0.3 }}
                    contentStyle={{
                      background: "var(--popover)",
                      border: "1px solid var(--border)",
                      borderRadius: 8,
                      color: "var(--foreground)",
                      fontSize: 12,
                    }}
                    formatter={(value: number, _name, props) => [
                      t(WEBUI.queryRecorder.timesWithPercent, {
                        count: value,
                        pct: formatPercent(
                          (props.payload?.share as number) ?? 0,
                        ),
                      }),
                      keyLabel,
                    ]}
                  />
                  <Bar
                    dataKey="count"
                    cursor="pointer"
                    onClick={(row) => onSelect(row.key)}
                    fill="var(--chart-1)"
                    radius={[4, 4, 0, 0]}
                  />
                </BarChart>
              ) : (
                <PieChart style={{ cursor: "pointer" }}>
                  <RechartsTooltip
                    contentStyle={{
                      background: "var(--popover)",
                      border: "1px solid var(--border)",
                      borderRadius: 8,
                      color: "var(--foreground)",
                      fontSize: 12,
                    }}
                    formatter={(value: number, _name, props) => [
                      t(WEBUI.queryRecorder.timesWithPercent, {
                        count: value,
                        pct: formatPercent(
                          (props.payload?.share as number) ?? 0,
                        ),
                      }),
                      props.payload?.label ?? keyLabel,
                    ]}
                  />
                  <Legend wrapperStyle={{ fontSize: 12 }} />
                  <Pie
                    data={rows}
                    dataKey="count"
                    nameKey="label"
                    cursor="pointer"
                    onClick={(row) => onSelect(row.key)}
                    outerRadius={90}
                    innerRadius={42}
                    paddingAngle={2}
                  >
                    {rows.map((entry, index) => (
                      <Cell
                        key={entry.key}
                        fill={CHART_COLORS[index % CHART_COLORS.length]}
                      />
                    ))}
                  </Pie>
                </PieChart>
              )}
            </ResponsiveContainer>
          </div>
        )}
        <div className="overflow-hidden rounded-md border">
          <Table className="min-w-[480px]">
            <TableHeader>
              <TableRow className="bg-muted/30 hover:bg-muted/30">
                <TableHead>{keyLabel}</TableHead>
                <TableHead>{t(WEBUI.queryRecorder.countColumn)}</TableHead>
                <TableHead>{t(WEBUI.queryRecorder.shareColumn)}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow
                  key={row.key}
                  className="cursor-pointer"
                  onClick={() => onSelect(row.key)}
                  title={t(WEBUI.queryRecorder.viewMatchingRecords)}
                >
                  <TableCell className="font-mono">
                    <RecordLink
                      value={rcodeLabel(row.key, t)}
                      onClick={() => onSelect(row.key)}
                    />
                  </TableCell>
                  <TableCell className="font-mono">{row.count}</TableCell>
                  <TableCell className="font-mono">
                    {formatPercent(row.share)}
                  </TableCell>
                </TableRow>
              ))}
              {!rows.length && (
                <TableRow>
                  <TableCell
                    colSpan={3}
                    className="h-20 text-center text-muted-foreground"
                  >
                    {loading
                      ? t(WEBUI.queryRecorder.loadingDist)
                      : t(WEBUI.queryRecorder.noData)}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  );
}

function LatencyCard({
  tag,
  filters,
  onRefresh,
  onData,
  onSelect,
}: {
  tag: string;
  filters: QueryRecordFilters;
  onRefresh: () => void;
  onData: (data: QueryRecorderSampleMetadata) => void;
  onSelect: (key: string) => void;
}) {
  const { t } = useI18n();
  const [data, setData] = useState<QueryRecorderLatencySummary | null>(null);
  const [slowLimit, setSlowLimit] = useState(TOP_PAGE_SIZE);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const load = useCallback(
    async (requestedSlowLimit: number) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      setLoading(true);
      setError(null);
      try {
        const response = await fetchQueryRecorderLatency(tag, {
          ...filters,
          slowLimit: requestedSlowLimit,
          signal: controller.signal,
        });
        if (controller.signal.aborted || abortRef.current !== controller) {
          return;
        }
        setData(response);
        onData(response);
        setSlowLimit(requestedSlowLimit);
      } catch (err) {
        if (controller.signal.aborted || abortRef.current !== controller) {
          return;
        }
        setError(
          err instanceof Error
            ? err.message
            : t(WEBUI.queryRecorder.readLatencyFailed),
        );
      } finally {
        if (abortRef.current === controller) {
          abortRef.current = null;
          setLoading(false);
        }
      }
    },
    [tag, filters, t, onData],
  );

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void load(TOP_PAGE_SIZE);
    }, 0);
    return () => {
      window.clearTimeout(timer);
      abortRef.current?.abort();
    };
  }, [load]);

  const histogram = useMemo(() => {
    const buckets = data?.histogram ?? [];
    return buckets.map((bucket, index) => {
      const prev = index === 0 ? 0 : (buckets[index - 1]?.lt_ms ?? 0);
      const lt = bucket.lt_ms;
      const label =
        lt === null
          ? `${prev}+ ms`
          : prev === 0
            ? `<${lt} ms`
            : `${prev}–${lt} ms`;
      // Color bars semantically: green → fast, sky → normal, amber → slow,
      // rose → very slow — same tiers as queryElapsedClassName.
      const upperBound = lt ?? Number.POSITIVE_INFINITY;
      const fill =
        upperBound <= 20
          ? "var(--color-emerald-500)"
          : upperBound <= 100
            ? "var(--color-sky-500)"
            : upperBound <= 300
              ? "var(--color-amber-500)"
              : "var(--color-rose-500)";
      return { label, count: bucket.count, fill };
    });
  }, [data]);
  const slowTop = data?.slow_top ?? [];
  const hasMoreSlowRows = slowTop.length >= slowLimit;

  return (
    <Card className="mt-3">
      <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-center">
        <div className="min-w-0">
          <CardTitle className="flex items-center gap-2 text-sm">
            <Timer className="h-4 w-4 text-primary" />
            {t(WEBUI.queryRecorder.latencyDistTitle)}
          </CardTitle>
          <SampleSummary
            sample={data}
            count={data?.sample_size ?? 0}
            filters={filters}
          ></SampleSummary>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={loading}
          onClick={onRefresh}
        >
          <RefreshCw className="h-4 w-4" />
          {t(WEBUI.common.refresh)}
        </Button>
      </CardHeader>
      <CardContent className="space-y-4 p-4 pt-0">
        {error && (
          <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        <div className="grid grid-cols-2 gap-2 md:grid-cols-5">
          <MetricChip label="P50" value={`${data?.p50_ms ?? 0} ms`} />
          <MetricChip label="P95" value={`${data?.p95_ms ?? 0} ms`} />
          <MetricChip label="P99" value={`${data?.p99_ms ?? 0} ms`} />
          <MetricChip
            label={t(WEBUI.queryRecorder.avgMetricLabel)}
            value={`${data ? data.avg_ms.toFixed(1) : "0.0"} ms`}
          />
          <MetricChip
            label={t(WEBUI.queryRecorder.maxMetricLabel)}
            value={`${data?.max_ms ?? 0} ms`}
          />
        </div>
        {histogram.length > 0 && (
          <div className="h-[260px] w-full">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart
                data={histogram}
                margin={{ top: 8, right: 16, left: 0, bottom: 8 }}
              >
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                <XAxis
                  dataKey="label"
                  stroke="var(--muted-foreground)"
                  fontSize={11}
                />
                <YAxis
                  stroke="var(--muted-foreground)"
                  fontSize={11}
                  allowDecimals={false}
                />
                <RechartsTooltip
                  cursor={{ fill: "var(--muted)", opacity: 0.3 }}
                  contentStyle={{
                    background: "var(--popover)",
                    border: "1px solid var(--border)",
                    borderRadius: 8,
                    color: "var(--foreground)",
                    fontSize: 12,
                  }}
                />
                <Bar dataKey="count" radius={[4, 4, 0, 0]}>
                  {histogram.map((entry) => (
                    <Cell key={entry.label} fill={entry.fill} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
        )}
        <div>
          <div className="mb-2 flex items-center gap-2 text-xs text-muted-foreground">
            <ServerCrash className="h-3.5 w-3.5" />
            {t(WEBUI.queryRecorder.slowQueriesHeader)}
          </div>
          <div className="overflow-hidden rounded-md border">
            <Table className="min-w-[600px]">
              <TableHeader>
                <TableRow className="bg-muted/30 hover:bg-muted/30">
                  <TableHead>QNAME</TableHead>
                  <TableHead>{t(WEBUI.queryRecorder.countColumn)}</TableHead>
                  <TableHead>
                    {t(WEBUI.queryRecorder.avgElapsedColumn)}
                  </TableHead>
                  <TableHead>
                    {t(WEBUI.queryRecorder.maxElapsedColumn)}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {slowTop.map((row) => (
                  <TableRow
                    key={row.qname}
                    className="cursor-pointer"
                    onClick={() => onSelect(row.qname)}
                    title={t(WEBUI.queryRecorder.viewMatchingRecords)}
                  >
                    <TableCell className="font-mono">
                      <RecordLink
                        value={row.qname}
                        onClick={() => onSelect(row.qname)}
                      />
                    </TableCell>
                    <TableCell className="font-mono">{row.count}</TableCell>
                    <TableCell className="font-mono">
                      {row.avg_ms.toFixed(1)} ms
                    </TableCell>
                    <TableCell className="font-mono">{row.max_ms} ms</TableCell>
                  </TableRow>
                ))}
                {!slowTop.length && (
                  <TableRow>
                    <TableCell
                      colSpan={4}
                      className="h-20 text-center text-muted-foreground"
                    >
                      {loading
                        ? t(WEBUI.queryRecorder.loadingSlowQueries)
                        : t(WEBUI.queryRecorder.noData)}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
          {hasMoreSlowRows && (
            <Button
              variant="outline"
              size="sm"
              className="mt-3"
              disabled={loading}
              onClick={() => void load(slowLimit + TOP_PAGE_SIZE)}
            >
              {t(WEBUI.common.loadMore)}
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function TimeseriesCard({
  tag,
  filters,
  onRefresh,
  metric,
  setMetric,
  onData,
}: {
  tag: string;
  filters: QueryRecordFilters;
  onRefresh: () => void;
  metric: "avg_ms" | "p95_ms";
  setMetric: (value: "avg_ms" | "p95_ms") => void;
  onData: (data: QueryRecorderTimeseriesResponse) => void;
}) {
  const { locale, t, formatNumber } = useI18n();
  const [data, setData] = useState<QueryRecorderTimeseriesResponse | null>(
    null,
  );
  const [recordBucket, setRecordBucket] = useState<TimeseriesChartPoint | null>(
    null,
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const load = useCallback(async () => {
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    setLoading(true);
    setError(null);
    try {
      const response = await fetchQueryRecorderTimeseries(tag, {
        ...filters,
        ...TIMESERIES_OPTIONS,
        signal: controller.signal,
      });
      if (controller.signal.aborted || abortRef.current !== controller) return;
      setData(response);
      onData(response);
    } catch (err) {
      if (controller.signal.aborted || abortRef.current !== controller) return;
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.queryRecorder.readTrendFailed),
      );
    } finally {
      if (abortRef.current === controller) {
        abortRef.current = null;
        setLoading(false);
      }
    }
  }, [tag, filters, t, onData]);
  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0);
    return () => {
      window.clearTimeout(timer);
      abortRef.current?.abort();
    };
  }, [load]);
  const points = useMemo(
    () => (data ? timeseriesChartPoints(data, filters) : []),
    [data, filters],
  );
  const anomalyTotal = points.reduce(
    (total, point) => total + point.anomaly_count,
    0,
  );
  const recordFilters = useMemo(
    () =>
      recordBucket && data
        ? bucketRecordFilters(recordBucket.bucket_ms, data.bucket_ms, filters)
        : undefined,
    [recordBucket, data, filters],
  );
  const bucketMs = data?.bucket_ms ?? 0;
  const bucketLabel =
    bucketMs >= 86_400_000
      ? t(WEBUI.queryRecorder.bucketDays, { count: bucketMs / 86_400_000 })
      : bucketMs >= 3_600_000
        ? t(WEBUI.queryRecorder.bucketHours, { count: bucketMs / 3_600_000 })
        : bucketMs >= 60_000
          ? t(WEBUI.queryRecorder.bucketMinutes, { count: bucketMs / 60_000 })
          : t(WEBUI.queryRecorder.bucketSeconds, { count: bucketMs / 1_000 });
  const latencyLabel =
    metric === "avg_ms" ? t(WEBUI.queryRecorder.avgLatencySeries) : "P95 (ms)";
  const renderChart = (latency: boolean) => (
    <div className="h-[220px] w-full">
      <ResponsiveContainer width="100%" height="100%">
        <LineChart
          style={{ cursor: "pointer" }}
          data={points}
          syncId={`query-recorder-${tag}`}
          syncMethod="value"
          accessibilityLayer
          margin={{ top: 8, right: 16, left: 0, bottom: 8 }}
          onClick={(state) => {
            const point = points.find(
              (item) => item.bucket_ms === Number(state?.activeLabel),
            );
            if (point) {
              setRecordBucket(point);
            }
          }}
        >
          <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
          <XAxis
            dataKey="bucket_ms"
            type="number"
            scale="time"
            domain={[
              points[0].bucket_ms,
              Math.max(
                points[points.length - 1].bucket_ms,
                points[0].bucket_ms + bucketMs,
              ),
            ]}
            tickFormatter={(value: number) =>
              formatBucketLabel(value, bucketMs, locale)
            }
            stroke="var(--muted-foreground)"
            fontSize={11}
            minTickGap={24}
          />
          <YAxis
            width={50}
            stroke="var(--muted-foreground)"
            fontSize={11}
            allowDecimals={latency}
          />
          <RechartsTooltip
            content={({ active, payload }) => {
              if (!active || !payload?.length) return null;
              const point = payload[0].payload as TimeseriesChartPoint;
              return (
                <div className="max-w-[min(22rem,85vw)] space-y-1 rounded-md border bg-popover p-3 text-xs text-popover-foreground shadow-md">
                  <div>
                    {formatFullTime(point.bucket_ms, locale)} –{" "}
                    {formatFullTime(point.bucket_ms + bucketMs - 1, locale)}
                  </div>
                  {point.partial && (
                    <div className="font-medium text-warning">
                      {t(WEBUI.queryRecorder.partialBucket)}
                    </div>
                  )}
                  <div>
                    {t(WEBUI.queryRecorder.totalQueriesSeries)}:{" "}
                    {formatNumber(point.total)}
                  </div>
                  <div>
                    {t(WEBUI.queryRecorder.executionErrorsMetric)}:{" "}
                    {formatNumber(point.error_count)}
                  </div>
                  <div>
                    {t(WEBUI.queryRecorder.noResponseMetric)}:{" "}
                    {formatNumber(point.no_response_count)}
                  </div>
                  <div>
                    {t(WEBUI.queryRecorder.servfailMetric)}:{" "}
                    {formatNumber(point.servfail_count)}
                  </div>
                  <div>
                    {t(WEBUI.queryRecorder.avgLatencySeries)}:{" "}
                    {point.avg_ms === null
                      ? "—"
                      : formatNumber(point.avg_ms, {
                          maximumFractionDigits: 2,
                        })}
                  </div>
                  <div>
                    P95 (ms):{" "}
                    {point.p95_ms === null ? "—" : formatNumber(point.p95_ms)}
                  </div>
                  <div className="flex items-center gap-1 border-t pt-1.5 font-medium text-primary">
                    <MousePointerClick className="size-3.5 shrink-0" />
                    {t(WEBUI.queryRecorder.viewBucketRecords)}
                  </div>
                </div>
              );
            }}
          />
          {!latency &&
            points
              .filter((point) => point.anomaly_count > 0)
              .map((point) => (
                <ReferenceDot
                  key={point.bucket_ms}
                  x={point.bucket_ms}
                  y={point.total}
                  r={4}
                  fill="var(--destructive)"
                  stroke="var(--background)"
                  strokeWidth={1.5}
                  onClick={() => setRecordBucket(point)}
                />
              ))}
          <Legend wrapperStyle={{ fontSize: 12 }} />
          <Line
            type="linear"
            dataKey={latency ? metric : "total"}
            name={
              latency ? latencyLabel : t(WEBUI.queryRecorder.totalQueriesSeries)
            }
            stroke="var(--chart-1)"
            strokeWidth={2}
            connectNulls={latency}
            dot={latency ? { r: 2 } : points.length === 1}
            isAnimationActive={false}
          />
          {!latency && (
            <Line
              type="linear"
              dataKey="anomaly_count"
              name={t(WEBUI.queryRecorder.anomalySeries)}
              stroke="var(--destructive)"
              strokeWidth={2}
              dot={latency ? { r: 2 } : points.length === 1}
              isAnimationActive={false}
            />
          )}
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
  return (
    <>
      <Card className="mt-3">
        <CardHeader className="grid gap-3 p-4 pb-2 sm:grid-cols-[1fr_auto] sm:items-start">
          <div className="min-w-0 space-y-2">
            <CardTitle className="flex items-center gap-2 text-sm">
              <TrendingUp className="h-4 w-4 text-primary" />
              {t(WEBUI.queryRecorder.trendTitle)}
            </CardTitle>
            <SampleSummary
              sample={data}
              count={data?.sample_size ?? 0}
              filters={filters}
            >
              {data && <Badge variant="outline">{bucketLabel}</Badge>}
              {data && (
                <Badge
                  variant="outline"
                  className={
                    anomalyTotal > 0
                      ? "border-destructive/30 text-destructive"
                      : undefined
                  }
                >
                  <MetricHelp
                    label={t(WEBUI.queryRecorder.anomalyTotal, {
                      count: formatNumber(anomalyTotal),
                    })}
                    help={t(WEBUI.queryRecorder.anomalyMarkersHelp)}
                  />
                </Badge>
              )}
            </SampleSummary>
          </div>
          <Button
            variant="outline"
            size="sm"
            disabled={loading}
            onClick={onRefresh}
          >
            <RefreshCw className="h-4 w-4" />
            {t(WEBUI.common.refresh)}
          </Button>
        </CardHeader>
        <CardContent className="space-y-4 p-4 pt-0">
          {error && (
            <div
              role="alert"
              className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive"
            >
              {error}
            </div>
          )}
          {points.length > 0 ? (
            <>
              <div>
                <h3 className="text-xs font-medium">
                  {t(WEBUI.queryRecorder.countsChartTitle)}
                </h3>
                {renderChart(false)}
              </div>
              <div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <h3 className="text-xs font-medium">
                    {t(WEBUI.queryRecorder.latencyChartTitle)}
                  </h3>
                  <div className="inline-flex gap-1">
                    <Button
                      variant={metric === "avg_ms" ? "secondary" : "ghost"}
                      size="sm"
                      aria-pressed={metric === "avg_ms"}
                      onClick={() => setMetric("avg_ms")}
                    >
                      {t(WEBUI.queryRecorder.avgMetricLabel)}
                    </Button>
                    <Button
                      variant={metric === "p95_ms" ? "secondary" : "ghost"}
                      size="sm"
                      aria-pressed={metric === "p95_ms"}
                      onClick={() => setMetric("p95_ms")}
                    >
                      P95
                    </Button>
                  </div>
                </div>
                {renderChart(true)}
              </div>
            </>
          ) : (
            <div className="rounded-md border bg-muted/20 px-3 py-8 text-center text-sm text-muted-foreground">
              {loading
                ? t(WEBUI.queryRecorder.loadingTrend)
                : t(WEBUI.queryRecorder.noData)}
            </div>
          )}
        </CardContent>
      </Card>
      <Dialog
        open={recordBucket !== null}
        onOpenChange={(open) => {
          if (!open) setRecordBucket(null);
        }}
      >
        <DialogContent className="flex max-h-[90dvh] flex-col gap-0 overflow-hidden p-0 sm:max-w-5xl">
          <DialogHeader className="shrink-0 border-b px-4 pt-5 pr-12 pb-3">
            <DialogTitle>
              {t(WEBUI.queryRecorder.bucketRecordsTitle)}
            </DialogTitle>
            <DialogDescription>
              {t(WEBUI.queryRecorder.bucketRecordsHint)}
            </DialogDescription>
            {recordFilters && (
              <p className="break-words text-xs text-muted-foreground">
                {t(WEBUI.queryRecorder.selectedRange, {
                  start: formatFullTime(recordFilters.sinceMs!, locale),
                  end: formatFullTime(recordFilters.untilMs!, locale),
                })}
              </p>
            )}
            {recordBucket?.partial && (
              <p className="text-xs text-warning">
                {t(WEBUI.queryRecorder.partialBucketRecordsHint)}
              </p>
            )}
          </DialogHeader>
          <div className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-4 pr-2">
            <div className="pr-2">
              {recordFilters && (
                <QueryRecordsPanelInner
                  key={`${recordFilters.sinceMs}-${recordFilters.untilMs}`}
                  tag={tag}
                  fixedFilters={recordFilters}
                />
              )}
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

function MetricChip({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border bg-muted/20 p-2">
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
      <div className="font-mono text-sm">{value}</div>
    </div>
  );
}

function TextFilterField({
  label,
  value,
  match,
  onValueChange,
  onMatchChange,
  placeholder,
}: {
  label: string;
  value: string;
  match: TextMatchMode;
  onValueChange: (value: string) => void;
  onMatchChange: (match: TextMatchMode) => void;
  placeholder: string;
}) {
  const { t } = useI18n();
  return (
    <FilterField label={label}>
      <div className="flex min-w-0">
        <Select
          value={match}
          onValueChange={(next) => onMatchChange(next as TextMatchMode)}
        >
          <SelectTrigger
            className="relative h-8 w-auto shrink-0 rounded-r-none border-r-0 px-2 text-xs focus-visible:z-10"
            aria-label={t(WEBUI.queryRecorder.textMatchMode, { field: label })}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="contains">
              {t(WEBUI.queryRecorder.containsMatch)}
            </SelectItem>
            <SelectItem value="exact">
              {t(WEBUI.queryRecorder.exactMatch)}
            </SelectItem>
          </SelectContent>
        </Select>
        <Input
          aria-label={label}
          value={value}
          onChange={(event) => onValueChange(event.target.value)}
          placeholder={placeholder}
          className="relative h-8 min-w-0 flex-1 rounded-l-none font-mono focus-visible:z-10"
        />
      </div>
    </FilterField>
  );
}

function FilterField({
  label,
  children,
}: {
  label: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="grid gap-1 text-xs text-muted-foreground">
      <span>{label}</span>
      {children}
    </div>
  );
}

function RecordDetailDialog({
  record,
  onClose,
}: {
  record: QueryRecordDetail | null;
  onClose: () => void;
}) {
  const { locale, t } = useI18n();
  const dependencyGraph = useAppStore((state) => state.dependencyGraph);
  const plugins = useAppStore((state) => state.plugins);

  return (
    <DnsRecordDetailDialog
      open={Boolean(record)}
      onOpenChange={(open) => !open && onClose()}
      title={t(WEBUI.queryRecorder.detailTitle, { id: record?.id ?? "" })}
      subtitle={
        record ? formatFullTime(record.created_at_ms, locale) : undefined
      }
      status={record ? queryStatusBadge(record) : undefined}
      summaryItems={
        record
          ? [
              {
                label: t(WEBUI.queryRecorder.clientLabel),
                value: record.client_ip,
                mono: true,
              },
              {
                label: t(WEBUI.queryRecorder.requestIdLabel),
                value: String(record.request_id),
                mono: true,
              },
              {
                label: t(WEBUI.queryRecorder.elapsedLabel),
                value: `${record.elapsed_ms}ms`,
                mono: true,
              },
              { label: "RCODE", value: record.rcode ?? "-", mono: true },
              {
                label: (
                  <span className="inline-flex items-center gap-1">
                    {t(WEBUI.dnsRecord.responseRecordsLabel)}
                    <Popover>
                      <PopoverTrigger asChild>
                        <button
                          type="button"
                          className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none"
                          aria-label={t(WEBUI.dnsRecord.responseRecordsAria)}
                        >
                          <Info className="h-3.5 w-3.5" />
                        </button>
                      </PopoverTrigger>
                      <PopoverContent
                        side="top"
                        className="w-auto max-w-[16rem] p-2 text-xs"
                      >
                        {t(WEBUI.dnsRecord.responseRecordsTooltip)}
                      </PopoverContent>
                    </Popover>
                  </span>
                ),
                value: `${record.answer_count} / ${record.authority_count} / ${record.additional_count}`,
                mono: true,
              },
              {
                label: t(WEBUI.queryRecorder.reqFlagsLabel),
                value: `RD=${flag(record.req_rd)} CD=${flag(record.req_cd)} AD=${flag(record.req_ad)}`,
                mono: true,
                wide: true,
              },
              {
                label: t(WEBUI.queryRecorder.respFlagsLabel),
                value: record.has_response
                  ? `AA=${flag(record.resp_aa)} TC=${flag(record.resp_tc)} RA=${flag(record.resp_ra)} AD=${flag(record.resp_ad)} CD=${flag(record.resp_cd)}`
                  : "-",
                mono: true,
                wide: true,
              },
            ]
          : []
      }
      questions={record?.questions_json}
      sections={
        record
          ? [
              {
                title: t(WEBUI.dnsRecord.answerSection),
                records: record.answers_json,
                emptyLabel: t(WEBUI.dnsRecord.emptyAnswer),
              },
              {
                title: t(WEBUI.dnsRecord.authoritySection),
                records: record.authorities_json,
                emptyLabel: t(WEBUI.dnsRecord.emptyAuthority),
              },
              {
                title: t(WEBUI.dnsRecord.additionalSection),
                records: record.additionals_json,
                emptyLabel: t(WEBUI.dnsRecord.emptyAdditional),
              },
              {
                title: t(WEBUI.dnsRecord.signatureSection),
                records: record.signature_json,
                emptyLabel: t(WEBUI.dnsRecord.emptySignature),
              },
            ]
          : []
      }
      error={record?.error ?? null}
      bottomBlocks={
        record
          ? [
              {
                title: t(WEBUI.queryRecorder.executionFlowTitle),
                children: (
                  <QueryRecordFlowCanvas
                    record={record}
                    dependencyGraph={dependencyGraph}
                    plugins={plugins}
                  />
                ),
              },
            ]
          : []
      }
      wide
    />
  );
}

function queryStatusBadge(record: QueryRecordRow | QueryRecordDetail) {
  if (record.error) {
    return <Badge variant="destructive">ERR</Badge>;
  }
  if (record.rcode?.toLowerCase() === "no error") {
    return <Badge variant="secondary">No Error</Badge>;
  }
  return <Badge variant="outline">{record.rcode ?? "-"}</Badge>;
}

function queryElapsedClassName(elapsedMs: number) {
  if (elapsedMs < 20) {
    return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  }
  if (elapsedMs < 100) {
    return "border-sky-500/30 bg-sky-500/10 text-sky-700 dark:text-sky-300";
  }
  if (elapsedMs < 300) {
    return "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300";
  }
  return "border-rose-500/30 bg-rose-500/10 text-rose-700 dark:text-rose-300";
}

function queryElapsedTitle(elapsedMs: number, t: TFn) {
  if (elapsedMs < 20) return t(WEBUI.queryRecorder.latencyFast);
  if (elapsedMs < 100) return t(WEBUI.queryRecorder.latencyNormal);
  if (elapsedMs < 300) return t(WEBUI.queryRecorder.latencySlow);
  return t(WEBUI.queryRecorder.latencyVerySlow);
}

function filtersFromForm(form: QueryRecordFilterForm): QueryRecordFilters {
  return {
    ...recordTextFilters(form),
    qtype: form.qtype === "all" ? undefined : form.qtype,
    rcode: form.rcode === "all" ? undefined : form.rcode,
    status: form.status === "all" ? undefined : form.status,
    sinceMs: parseLocalDateTime(form.sinceLocal),
    untilMs: parseLocalDateTime(form.untilLocal),
  };
}

function parseLocalDateTime(value: string) {
  if (!value) return undefined;
  const ms = new Date(value).getTime();
  return Number.isFinite(ms) ? ms : undefined;
}

type SseEvent = {
  event: string;
  data: string;
};

function parseSseEvent(chunk: string): SseEvent {
  const data: string[] = [];
  let event = "message";
  for (const rawLine of chunk.split("\n")) {
    const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
    if (!line || line.startsWith(":")) continue;

    const separator = line.indexOf(":");
    const field = separator === -1 ? line : line.slice(0, separator);
    let value = separator === -1 ? "" : line.slice(separator + 1);
    if (value.startsWith(" ")) value = value.slice(1);

    if (field === "event") {
      event = value || "message";
    } else if (field === "data") {
      data.push(value);
    }
  }
  return { event, data: data.join("\n") };
}

function parseSseErrorMessage(data: string, t: TFn) {
  try {
    const parsed = JSON.parse(data) as unknown;
    if (
      parsed &&
      typeof parsed === "object" &&
      "message" in parsed &&
      typeof parsed.message === "string"
    ) {
      return parsed.message;
    }
  } catch {
    // Fall through to the raw server payload preview.
  }
  return t(WEBUI.queryRecorder.streamErrorPrefix, { data: truncateText(data) });
}

function parseStreamRecord(data: string): QueryRecordDetail | null {
  try {
    const parsed = JSON.parse(data) as unknown;
    return isQueryRecordDetail(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function isQueryRecordDetail(value: unknown): value is QueryRecordDetail {
  if (!value || typeof value !== "object") return false;
  const record = value as Partial<QueryRecordDetail>;
  return (
    typeof record.id === "number" &&
    typeof record.created_at_ms === "number" &&
    typeof record.client_ip === "string" &&
    Array.isArray(record.questions_json) &&
    Array.isArray(record.steps)
  );
}

function recordMatchesFilters(
  record: QueryRecordRow | QueryRecordDetail,
  filters: QueryRecordFilters,
) {
  if (filters.sinceMs !== undefined && record.created_at_ms < filters.sinceMs) {
    return false;
  }
  if (filters.untilMs !== undefined && record.created_at_ms > filters.untilMs) {
    return false;
  }
  if (filters.qname) {
    const needle = filters.qname.toLowerCase();
    if (
      !record.questions_json.some((question) =>
        question.name.toLowerCase().includes(needle),
      )
    ) {
      return false;
    }
  }
  if (
    filters.qnameExact &&
    !record.questions_json.some(
      (question) =>
        question.name.toLowerCase() === filters.qnameExact!.toLowerCase(),
    )
  )
    return false;
  if (filters.clientIpExact && record.client_ip !== filters.clientIpExact)
    return false;
  if (filters.rcodeBucket) {
    const bucket =
      record.rcode ??
      (record.error != null
        ? "_ERROR"
        : !record.has_response
          ? "_NO_RESPONSE"
          : "_UNKNOWN");
    if (bucket !== filters.rcodeBucket) return false;
  }
  if (filters.qtype) {
    const qtype = filters.qtype.toLowerCase();
    if (
      !record.questions_json.some(
        (question) => question.qtype.toLowerCase() === qtype,
      )
    ) {
      return false;
    }
  }
  if (
    filters.clientIp &&
    !record.client_ip.toLowerCase().includes(filters.clientIp.toLowerCase())
  ) {
    return false;
  }
  if (
    filters.rcode &&
    (record.rcode ?? "").toLowerCase() !== filters.rcode.toLowerCase()
  ) {
    return false;
  }
  if (filters.status === "error" && !record.error) return false;
  if (filters.status === "has_response" && !record.has_response) return false;
  if (
    filters.status === "no_response" &&
    (record.error || record.has_response)
  ) {
    return false;
  }
  if (filters.matcherTag) {
    const steps = (record as Partial<QueryRecordDetail>).steps;
    if (
      !steps?.some(
        (step) =>
          step.kind === "matcher" &&
          (step.outcome === "matched" ||
            step.outcome === "always_true_matched" ||
            step.outcome === "always_false_matched") &&
          step.tag === filters.matcherTag,
      )
    ) {
      return false;
    }
  }
  return true;
}

function countActiveFilters(filters: QueryRecordFilters) {
  return [
    filters.qname,
    filters.qtype,
    filters.clientIp,
    filters.rcode,
    filters.status,
    filters.sinceMs,
    filters.untilMs,
    filters.matcherTag,
    filters.qnameExact,
    filters.clientIpExact,
    filters.rcodeBucket,
  ].filter((value) => value !== undefined && value !== "").length;
}

function safeRatio(numerator: number, denominator: number) {
  if (denominator <= 0) return 0;
  return numerator / denominator;
}

function formatPercent(value: number) {
  return `${(value * 100).toFixed(1)}%`;
}

function flag(value: unknown) {
  if (typeof value !== "boolean") return "-";
  return value ? "1" : "0";
}

function formatTime(ms: number, locale: ReturnType<typeof useI18n>["locale"]) {
  return new Date(ms).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatFullTime(
  ms: number,
  locale: ReturnType<typeof useI18n>["locale"],
) {
  return new Date(ms).toLocaleString(locale);
}

function formatBucketLabel(
  ms: number,
  bucketMs: number,
  locale: ReturnType<typeof useI18n>["locale"],
) {
  const date = new Date(ms);
  if (bucketMs >= 3_600_000) {
    return date.toLocaleString(locale, {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
  return date.toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
    ...(bucketMs < 60_000 ? { second: "2-digit" as const } : {}),
  });
}

function truncateMiddle(value: string, max: number) {
  if (value.length <= max) return value;
  const half = Math.max(2, Math.floor((max - 1) / 2));
  return `${value.slice(0, half)}…${value.slice(value.length - half)}`;
}

function truncateText(value: string, max = 160) {
  const singleLine = value.replace(/\s+/g, " ").trim();
  return singleLine.length > max
    ? `${singleLine.slice(0, Math.max(0, max - 1))}…`
    : singleLine;
}

function formatQuestion(record: QueryRecordRow) {
  const first = record.questions_json[0];
  if (!first) return "-";
  return `${first.name} ${first.qtype}`;
}

function formatQuestionName(record: QueryRecordRow) {
  const first = record.questions_json[0];
  return first?.name ?? "-";
}

function formatQuestionType(record: QueryRecordRow) {
  const first = record.questions_json[0];
  return first?.qtype ?? "-";
}

export const queryRecorderPlugin: PluginComponentDefinition = {
  Detail: QueryRecorderDetail,
};
