"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from "react";
import dynamic from "next/dynamic";
import {
  Activity,
  ArrowDownRight,
  Clock3,
  GitBranch,
  LoaderCircle,
  Pause,
  Play,
  Radio,
  Search,
  ShieldCheck,
  Wifi,
  Zap,
} from "lucide-react";
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  fetchDNSObservations,
  fetchQueryRecordDetail,
  type DNSObservation,
  type DNSObservationEvent,
  type DNSObservationsResponse,
  type QueryRecordDetail,
} from "@/lib/oxidns-api";
import { useAppStore } from "@/lib/store";
import { useAuthStore } from "@/lib/auth-store";
import { useVisiblePolling } from "@/hooks/use-visible-polling";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { selectedUpstream } from "@/lib/observability";

const ResolutionFlow = dynamic(() => import("./resolution-flow"), {
  ssr: false,
  loading: () => (
    <div className="flex h-[420px] items-center justify-center text-sm text-muted-foreground">
      <LoaderCircle className="mr-2 size-4 animate-spin" />
    </div>
  ),
});

type View = "overview" | "queries" | "upstreams" | "ips" | "topology";
const tabs: { key: View; label: string; icon: typeof Activity }[] = [
  { key: "overview", label: WEBUI.observability.overview, icon: Activity },
  { key: "queries", label: WEBUI.observability.queries, icon: Radio },
  { key: "upstreams", label: WEBUI.observability.upstreams, icon: Wifi },
  { key: "ips", label: WEBUI.observability.ipSelection, icon: Zap },
  { key: "topology", label: WEBUI.observability.topology, icon: GitBranch },
];
const viewChangeEvent = "oxidns-observability-view";

function subscribeToView(onChange: () => void) {
  window.addEventListener(viewChangeEvent, onChange);
  return () => window.removeEventListener(viewChangeEvent, onChange);
}

function currentView(): View {
  const requested = new URLSearchParams(window.location.search).get("view");
  return tabs.some((tab) => tab.key === requested)
    ? (requested as View)
    : "overview";
}
const statusKeys: Record<string, string> = {
  running: "running",
  not_recorded: "notRecorded",
  success: "success",
  no_response: "noResponse",
  error: "error",
  cache_hit: "cacheHit",
  cache_stale: "cacheStale",
  cache_miss: "cacheMiss",
  not_started: "notStarted",
  cancelled: "cancelled",
  timeout: "timeout",
  failed: "failed",
  cached_score: "cachedScore",
  cached_failure: "cachedFailure",
  shared_success: "sharedSuccess",
  shared_failure: "sharedFailure",
  skipped_limit: "skippedLimit",
  retained: "retained",
  excluded: "excluded",
  candidate: "candidate",
  skipped_dnssec: "skippedDnssec",
};

export function DnsObservability() {
  const { t, formatDateTime, formatNumber } = useI18n();
  const plugins = useAppStore((state) => state.plugins);
  const connected = useAuthStore((state) => state.isConnected);
  const connectionEpoch = useAuthStore((state) => state.connectionEpoch);
  const recorder = plugins.find(
    (plugin) => plugin.pluginKind === "query_recorder",
  );
  const tag = recorder?.name ?? "";
  const [data, setData] = useState<DNSObservationsResponse | null>(null);
  const [error, setError] = useState("");
  const [updatedAt, setUpdatedAt] = useState(0);
  const [paused, setPaused] = useState(false);
  const configuredUpstreams = useMemo(
    () => [
      ...new Set(
        plugins
          .filter((plugin) => plugin.pluginKind === "forward")
          .flatMap((plugin) => {
            const upstreams = plugin.config.upstreams;
            if (!Array.isArray(upstreams)) return [];
            return upstreams.flatMap((item: unknown) =>
              typeof item === "object" &&
              item !== null &&
              "addr" in item &&
              typeof item.addr === "string"
                ? [item.addr]
                : [],
            );
          }),
      ),
    ],
    [plugins],
  );
  const view = useSyncExternalStore(
    subscribeToView,
    currentView,
    () => "overview",
  );
  const [selectedId, setSelectedId] = useState("");
  const [search, setSearch] = useState("");
  const [recordDetail, setRecordDetail] = useState<{
    id: string;
    record: QueryRecordDetail;
  } | null>(null);

  const refresh = useCallback(
    async (signal: AbortSignal) => {
      if (!tag) return;
      try {
        const response = await fetchDNSObservations(tag, signal);
        if (!signal.aborted) {
          setData(response);
          setError("");
          setUpdatedAt(Date.now());
        }
      } catch (cause) {
        if (!signal.aborted)
          setError(
            cause instanceof Error
              ? cause.message
              : t(WEBUI.observability.unavailable),
          );
      }
    },
    [tag, t],
  );
  useVisiblePolling(
    refresh,
    1500,
    connected && Boolean(tag) && !paused,
    `${connectionEpoch}:${tag}`,
  );

  const rows = useMemo(() => data?.queries ?? [], [data]);
  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return needle
      ? rows.filter((row) =>
          `${row.qname} ${row.qtype} ${row.client_ip} ${row.status}`
            .toLowerCase()
            .includes(needle),
        )
      : rows;
  }, [rows, search]);
  const selected = filtered.find((row) => row.id === selectedId) ?? rows[0];

  useEffect(() => {
    if (!tag || !selected?.record_id) return;
    const controller = new AbortController();
    void fetchQueryRecordDetail(tag, selected.record_id, controller.signal)
      .then((response) => {
        if (!controller.signal.aborted)
          setRecordDetail({ id: selected.id, record: response.record });
      })
      .catch(() => {});
    return () => controller.abort();
  }, [selected?.record_id, selected?.id, tag]);

  const completed = rows.filter((row) => row.status !== "running");
  const avg = completed.length
    ? completed.reduce((sum, row) => sum + row.elapsed_ms, 0) / completed.length
    : 0;
  const totalAttempts = rows
    .flatMap((row) => row.events)
    .filter((event) => event.kind === "upstream");
  const winnerCounts = new Map<string, number>();
  for (const event of totalAttempts)
    if (event.selected)
      winnerCounts.set(event.target, (winnerCounts.get(event.target) ?? 0) + 1);
  const winners = [...winnerCounts].sort((a, b) => b[1] - a[1]);
  const selectedRecord =
    recordDetail && selected && recordDetail.id === selected.id
      ? recordDetail.record
      : null;
  const timeline = completed
    .slice()
    .reverse()
    .slice(-36)
    .map((row) => ({
      time: formatDateTime(row.created_at_ms, {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      }),
      ms: Math.round(row.elapsed_ms * 100) / 100,
    }));

  const label = (key: string, params?: Record<string, string | number>) =>
    t(`webui.observability.${key}`, params);

  return (
    <main className="min-w-0 flex-1 space-y-4 p-4 md:p-6">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <p className="mb-1 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.16em] text-primary">
            <span
              className={cn(
                "size-1.5 rounded-full",
                connected && tag && !paused && !error
                  ? "animate-pulse bg-primary"
                  : "bg-muted-foreground",
              )}
            />
            OXIDNS / DNS
          </p>
          <h1 className="text-xl font-semibold tracking-tight">
            {t(WEBUI.observability.title)}
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {t(WEBUI.observability.description)}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <span className="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs text-muted-foreground">
            <Radio
              className={cn("size-3.5", connected && tag && "text-primary")}
            />
            {paused
              ? label("paused")
              : connected && tag && !error
                ? label("live")
                : label("waiting")}
          </span>
          {updatedAt > 0 && (
            <span className="text-xs text-muted-foreground">
              {label("refresh")}{" "}
              {formatDateTime(updatedAt, {
                hour: "2-digit",
                minute: "2-digit",
                second: "2-digit",
              })}
            </span>
          )}
          <Button
            size="sm"
            variant="outline"
            disabled={!tag}
            onClick={() => setPaused(!paused)}
          >
            {paused ? (
              <Play className="size-3.5" />
            ) : (
              <Pause className="size-3.5" />
            )}
            {paused ? label("resume") : label("pause")}
          </Button>
        </div>
      </header>

      {tag && (
        <nav
          aria-label={t(WEBUI.observability.title)}
          className="flex gap-1 overflow-x-auto border-b"
        >
          {tabs.map(({ key, icon: Icon }) => (
            <button
              key={key}
              type="button"
              onClick={() => {
                const url = new URL(window.location.href);
                if (key === "overview") url.searchParams.delete("view");
                else url.searchParams.set("view", key);
                window.history.replaceState(null, "", url);
                window.dispatchEvent(new Event(viewChangeEvent));
              }}
              className={cn(
                "flex shrink-0 items-center gap-2 border-b-2 px-3 py-2.5 text-sm transition-colors",
                view === key
                  ? "border-primary font-medium text-foreground"
                  : "border-transparent text-muted-foreground hover:text-foreground",
              )}
            >
              <Icon className="size-4" />
              {t(
                (WEBUI.observability as Record<string, string>)[
                  key === "ips" ? "ipSelection" : key
                ],
              )}
            </button>
          ))}
        </nav>
      )}

      {!connected ? (
        <Notice text={label("waiting")} />
      ) : !tag ? (
        <Notice text={label("noRecorder")} />
      ) : error && !data ? (
        <Notice text={error} />
      ) : (
        <>
          {error && (
            <p
              role="status"
              className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning-foreground"
            >
              {label("unavailable")}: {error}
            </p>
          )}
          {data?.enabled === false && (
            <p
              role="status"
              className="rounded-md border bg-muted/40 px-3 py-2 text-sm text-muted-foreground"
            >
              {label("detailsDisabled")}
            </p>
          )}
          {!data ? (
            <Notice text={label("waiting")} loading />
          ) : (
            <>
              <section className="grid grid-cols-2 gap-3 xl:grid-cols-4">
                <Metric
                  icon={Activity}
                  title={label("queryCount")}
                  value={formatNumber(rows.length)}
                  foot={label("sampleWindow", { count: data.capacity })}
                />
                <Metric
                  icon={Radio}
                  title={label("active")}
                  value={formatNumber(
                    rows.filter((row) => row.status === "running").length,
                  )}
                  foot={label("dropped", {
                    count: data.dropped + data.recorder_dropped,
                  })}
                />
                <Metric
                  icon={Clock3}
                  title={label("avgLatency")}
                  value={`${formatNumber(avg, { maximumFractionDigits: 2 })} ms`}
                  foot={label("completedCount", { count: completed.length })}
                />
                <Metric
                  icon={ShieldCheck}
                  title={label("upstreamWins")}
                  value={formatNumber(
                    totalAttempts.filter((event) => event.selected).length,
                  )}
                  foot={label("attemptCount", {
                    count: totalAttempts.filter(
                      (event) => event.status !== "not_started",
                    ).length,
                  })}
                />
              </section>

              {view === "overview" && (
                <Overview
                  rows={rows}
                  winners={winners}
                  timeline={timeline}
                  label={label}
                />
              )}
              {view === "upstreams" && (
                <Upstreams
                  rows={rows}
                  configured={configuredUpstreams}
                  label={label}
                  formatNumber={formatNumber}
                />
              )}
              {view === "ips" && (
                <Candidates
                  rows={rows}
                  label={label}
                  formatDateTime={formatDateTime}
                />
              )}
              {view === "topology" && <Topology row={selected} label={label} />}
              {(view === "overview" || view === "queries") && (
                <div className="grid min-w-0 gap-4 xl:grid-cols-[minmax(0,1.12fr)_minmax(360px,0.88fr)]">
                  <section className="min-w-0 overflow-hidden rounded-xl border bg-card">
                    <div className="flex flex-wrap items-center justify-between gap-3 border-b px-4 py-3">
                      <div>
                        <h2 className="text-sm font-semibold">
                          {label("records")}
                        </h2>
                        <p className="mt-0.5 text-xs text-muted-foreground">
                          {label("sampleWindow", {
                            count: data?.capacity ?? 100,
                          })}
                        </p>
                      </div>
                      <label className="flex h-8 w-full max-w-64 items-center gap-2 rounded-md border px-2.5 text-muted-foreground">
                        <Search className="size-3.5" />
                        <input
                          value={search}
                          onChange={(event) => setSearch(event.target.value)}
                          className="w-full bg-transparent text-sm text-foreground outline-none"
                          placeholder={label("search")}
                          aria-label={label("search")}
                        />
                      </label>
                    </div>
                    <div className="max-h-[570px] overflow-auto">
                      {filtered.length === 0 ? (
                        <Notice text={label("waiting")} />
                      ) : (
                        filtered.map((row) => (
                          <button
                            key={row.id}
                            type="button"
                            onClick={() => setSelectedId(row.id)}
                            className={cn(
                              "grid w-full grid-cols-[minmax(0,1fr)_auto] gap-x-3 border-b px-4 py-3 text-left transition-colors hover:bg-muted/50 sm:grid-cols-[minmax(0,1.2fr)_minmax(100px,.8fr)_70px_72px]",
                              row.id === selected?.id && "bg-primary/5",
                            )}
                          >
                            <span className="min-w-0">
                              <span className="block truncate text-sm font-medium">
                                {row.qname || "—"}
                              </span>
                              <span className="text-xs text-muted-foreground">
                                {row.qtype} · {row.client_ip} ·{" "}
                                {formatDateTime(row.created_at_ms, {
                                  hour: "2-digit",
                                  minute: "2-digit",
                                  second: "2-digit",
                                })}
                              </span>
                            </span>
                            <span className="hidden truncate self-center text-xs text-muted-foreground sm:block">
                              {selectedUpstream(row)?.target ??
                                (row.events.some(
                                  (event) =>
                                    event.kind === "cache" &&
                                    ["cache_hit", "cache_stale"].includes(
                                      event.status,
                                    ),
                                )
                                  ? label(
                                      row.events.some(
                                        (event) => event.status === "cache_hit",
                                      )
                                        ? "cacheHit"
                                        : "cacheStale",
                                    )
                                  : row.rcode) ??
                                "—"}
                            </span>
                            <span className="self-center text-right text-xs tabular-nums text-muted-foreground">
                              {row.elapsed_ms.toFixed(1)} ms
                            </span>
                            <Status value={row.status} label={label} />
                          </button>
                        ))
                      )}
                    </div>
                  </section>

                  <section className="min-w-0 overflow-hidden rounded-xl border bg-card">
                    <div className="flex items-start justify-between border-b px-4 py-3">
                      <div>
                        <h2 className="text-sm font-semibold">
                          {label("details")}
                        </h2>
                        <p className="mt-1 truncate text-xs text-muted-foreground">
                          {selected?.qname ?? label("selectQuery")}
                        </p>
                      </div>
                      {selected && (
                        <Status value={selected.status} label={label} />
                      )}
                    </div>
                    {selected ? (
                      <QueryDetail
                        row={selected}
                        record={selectedRecord}
                        label={label}
                        formatDateTime={formatDateTime}
                      />
                    ) : (
                      <Notice text={label("selectQuery")} />
                    )}
                  </section>
                </div>
              )}
            </>
          )}
        </>
      )}
    </main>
  );
}

function Metric({
  icon: Icon,
  title,
  value,
  foot,
}: {
  icon: typeof Activity;
  title: string;
  value: string;
  foot: string;
}) {
  return (
    <article className="rounded-xl border bg-card px-4 py-3.5">
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>{title}</span>
        <Icon className="size-4" />
      </div>
      <p className="mt-2 font-mono text-2xl font-semibold tabular-nums tracking-tight">
        {value}
      </p>
      <p className="mt-1 truncate text-xs text-muted-foreground">{foot}</p>
    </article>
  );
}

function Notice({
  text,
  loading = false,
}: {
  text: string;
  loading?: boolean;
}) {
  return (
    <div className="flex min-h-24 items-center justify-center gap-2 rounded-xl border border-dashed px-5 text-center text-sm text-muted-foreground">
      {loading && <LoaderCircle className="size-4 animate-spin" />}
      {text}
    </div>
  );
}

function Status({
  value,
  label,
}: {
  value: string;
  label: (key: string) => string;
}) {
  const good = [
    "success",
    "cache_hit",
    "retained",
    "cached_score",
    "shared_success",
  ].includes(value);
  const bad = [
    "error",
    "failed",
    "timeout",
    "excluded",
    "skipped_limit",
  ].includes(value);
  return (
    <span
      className={cn(
        "inline-flex max-w-full items-center rounded-full border px-2 py-0.5 text-[11px]",
        good && "border-primary/30 bg-primary/10 text-primary",
        bad && "border-destructive/30 bg-destructive/10 text-destructive",
        !good && !bad && "text-muted-foreground",
      )}
    >
      {label(statusKeys[value] ?? "unknownStatus")}
    </span>
  );
}

function Overview({
  rows,
  winners,
  timeline,
  label,
}: {
  rows: DNSObservation[];
  winners: [string, number][];
  timeline: { time: string; ms: number }[];
  label: (key: string, params?: Record<string, string | number>) => string;
}) {
  return (
    <section className="grid gap-4 xl:grid-cols-[minmax(0,1.5fr)_minmax(280px,.8fr)]">
      <div className="min-w-0 rounded-xl border bg-card p-4">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold">{label("latencyTrend")}</h2>
          <span className="text-xs text-muted-foreground">
            {label("windowSample")}
          </span>
        </div>
        <div className="h-52">
          {timeline.length ? (
            <ResponsiveContainer width="100%" height="100%">
              <LineChart
                data={timeline}
                margin={{ left: -16, right: 8, top: 10 }}
              >
                <CartesianGrid
                  strokeDasharray="3 4"
                  stroke="var(--border)"
                  vertical={false}
                />
                <XAxis
                  dataKey="time"
                  tick={{ fontSize: 10 }}
                  minTickGap={24}
                  axisLine={false}
                  tickLine={false}
                />
                <YAxis
                  tick={{ fontSize: 10 }}
                  axisLine={false}
                  tickLine={false}
                />
                <Tooltip
                  contentStyle={{
                    background: "var(--card)",
                    borderColor: "var(--border)",
                    borderRadius: 9,
                    fontSize: 12,
                  }}
                />
                <Line
                  type="monotone"
                  dataKey="ms"
                  name={label("latency")}
                  stroke="var(--primary)"
                  strokeWidth={2}
                  dot={false}
                  activeDot={{ r: 4 }}
                />
              </LineChart>
            </ResponsiveContainer>
          ) : (
            <Notice text={label("waiting")} />
          )}
        </div>
      </div>
      <div className="rounded-xl border bg-card">
        <div className="border-b px-4 py-3">
          <h2 className="text-sm font-semibold">{label("upstreamWins")}</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            {label("measuredEvents", { count: rows.length })}
          </p>
        </div>
        <div className="space-y-1 p-3">
          {winners.length ? (
            winners.slice(0, 6).map(([name, count], index) => {
              const share =
                count /
                Math.max(
                  1,
                  winners.reduce((sum, entry) => sum + entry[1], 0),
                );
              return (
                <div
                  key={name}
                  className="flex items-center gap-3 rounded-md px-2 py-2"
                >
                  <span className="font-mono text-xs text-muted-foreground">
                    0{index + 1}
                  </span>
                  <span className="min-w-0 flex-1 truncate font-mono text-xs">
                    {name}
                  </span>
                  <span className="text-xs tabular-nums">
                    {count} · {Math.round(share * 100)}%
                  </span>
                  <span className="h-1.5 w-16 overflow-hidden rounded-full bg-muted">
                    <span
                      className="block h-full rounded-full bg-primary"
                      style={{ width: `${share * 100}%` }}
                    />
                  </span>
                </div>
              );
            })
          ) : (
            <Notice text={label("noWinners")} />
          )}
        </div>
      </div>
    </section>
  );
}

function Upstreams({
  rows,
  configured,
  label,
  formatNumber,
}: {
  rows: DNSObservation[];
  configured: string[];
  label: (key: string, params?: Record<string, string | number>) => string;
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string;
}) {
  const events = rows
    .flatMap((row) => row.events)
    .filter((event) => event.kind === "upstream");
  const groups = new Map<string, DNSObservationEvent[]>(
    configured.map((name) => [name, []]),
  );
  for (const event of events)
    groups.set(event.target, [...(groups.get(event.target) ?? []), event]);
  return (
    <section className="overflow-hidden rounded-xl border bg-card">
      <div className="border-b px-4 py-3">
        <h2 className="text-sm font-semibold">{label("upstreams")}</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {label("measuredEvents", { count: rows.length })}
          {" · "}
          {label("upstreamSemantics")}
        </p>
      </div>
      {!groups.size ? (
        <Notice text={label("notRecorded")} />
      ) : (
        <div className="grid gap-px bg-border sm:grid-cols-2 xl:grid-cols-4">
          {[...groups].map(([name, attempts]) => {
            const completed = attempts.filter(
              (event) => event.status === "success",
            );
            const durations = completed.flatMap((event) =>
              event.elapsed_ms === null ? [] : [event.elapsed_ms],
            );
            const mean = durations.length
              ? durations.reduce((a, b) => a + b, 0) / durations.length
              : null;
            const wins = attempts.filter((event) => event.selected).length;
            return (
              <article key={name} className="bg-card p-4">
                <div className="flex items-center gap-2">
                  <Wifi className="size-4 text-primary" />
                  <span className="truncate font-mono text-xs">{name}</span>
                </div>
                <p className="mt-4 font-mono text-2xl tabular-nums">
                  {mean === null
                    ? "—"
                    : `${formatNumber(mean, { maximumFractionDigits: 2 })} ms`}
                </p>
                <p className="mt-1 text-xs text-muted-foreground">
                  {label("successfulLatency")} ·{" "}
                  {label("sampleSize", { count: durations.length })}
                </p>
                <div className="mt-4 grid grid-cols-3 gap-y-3 border-t pt-3 text-xs">
                  <span>
                    <b className="block font-mono text-sm">
                      {
                        attempts.filter(
                          (event) => event.status !== "not_started",
                        ).length
                      }
                    </b>
                    <span className="text-muted-foreground">
                      {label("attempts")}
                    </span>
                  </span>
                  <span>
                    <b className="block font-mono text-sm">{wins}</b>
                    <span className="text-muted-foreground">
                      {label("wins")}
                    </span>
                  </span>
                  <span>
                    <b className="block font-mono text-sm">
                      {
                        attempts.filter((event) =>
                          ["error", "failed"].includes(event.status),
                        ).length
                      }
                    </b>
                    <span className="text-muted-foreground">
                      {label("errors")}
                    </span>
                  </span>
                  {["timeout", "cancelled", "not_started"].map((status) => (
                    <span key={status}>
                      <b className="block font-mono text-sm">
                        {
                          attempts.filter((event) => event.status === status)
                            .length
                        }
                      </b>
                      <span className="text-muted-foreground">
                        {label(statusKeys[status])}
                      </span>
                    </span>
                  ))}
                </div>
                <div className="mt-4 space-y-1.5 border-t pt-3">
                  {attempts.slice(0, 6).map((event, index) => (
                    <div
                      key={`${event.id}-${index}`}
                      className="flex items-center justify-between gap-2 text-[11px]"
                    >
                      <span className="text-muted-foreground">
                        {event.method || "DNS"}
                      </span>
                      <Status value={event.status} label={label} />
                      <span className="font-mono tabular-nums">
                        {event.elapsed_ms === null
                          ? "—"
                          : `${event.elapsed_ms.toFixed(2)} ms`}
                      </span>
                    </div>
                  ))}
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}

function Candidates({
  rows,
  label,
  formatDateTime,
}: {
  rows: DNSObservation[];
  label: (key: string, params?: Record<string, string | number>) => string;
  formatDateTime: (
    value: Date | number | string,
    options?: Intl.DateTimeFormatOptions,
  ) => string;
}) {
  const events = rows.flatMap((row) =>
    row.events
      .filter(
        (event) => event.kind === "ip_probe" || event.kind === "ip_candidate",
      )
      .map((event) => ({
        ...event,
        qname: row.qname,
        client: row.client_ip,
        at: row.created_at_ms,
      })),
  );
  return (
    <section className="overflow-hidden rounded-xl border bg-card">
      <div className="border-b px-4 py-3">
        <h2 className="text-sm font-semibold">{label("ipSelection")}</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {label("measuredEvents", { count: rows.length })}
          {" · "}
          {label("probeScope")}
        </p>
      </div>
      {!events.length ? (
        <Notice text={label("noCandidates")} />
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[720px] text-left text-xs">
            <thead className="bg-muted/50 text-muted-foreground">
              <tr>
                {[
                  label("time"),
                  label("query"),
                  label("candidate"),
                  label("method"),
                  label("result"),
                  label("latency"),
                  label("sampledAt"),
                  label("reason"),
                ].map((title) => (
                  <th key={title} className="px-3 py-2.5 font-medium">
                    {title}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {events.slice(0, 250).map((event, index) => (
                <tr
                  key={`${event.at}-${event.id}-${index}`}
                  className="border-t"
                >
                  <td className="whitespace-nowrap px-3 py-2.5 tabular-nums text-muted-foreground">
                    {formatDateTime(event.at, {
                      hour: "2-digit",
                      minute: "2-digit",
                      second: "2-digit",
                    })}
                  </td>
                  <td
                    className="max-w-40 truncate px-3 py-2.5 font-mono"
                    title={event.qname}
                  >
                    {event.qname}
                  </td>
                  <td className="px-3 py-2.5 font-mono">
                    {event.target || "—"}
                  </td>
                  <td className="px-3 py-2.5">{event.method || "—"}</td>
                  <td className="px-3 py-2.5">
                    <Status value={event.status} label={label} />
                  </td>
                  <td className="whitespace-nowrap px-3 py-2.5 font-mono tabular-nums">
                    {event.latency_ms === null ? "—" : `${event.latency_ms} ms`}
                  </td>
                  <td className="whitespace-nowrap px-3 py-2.5 tabular-nums text-muted-foreground">
                    {event.sampled_at_ms === null
                      ? "—"
                      : formatDateTime(event.sampled_at_ms, {
                          hour: "2-digit",
                          minute: "2-digit",
                          second: "2-digit",
                        })}
                  </td>
                  <td
                    className="max-w-44 truncate px-3 py-2.5 text-muted-foreground"
                    title={event.reason ?? ""}
                  >
                    {event.reason ?? "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function Topology({
  row,
  label,
}: {
  row?: DNSObservation;
  label: (key: string) => string;
}) {
  return (
    <section className="overflow-hidden rounded-xl border bg-card">
      <div className="flex items-center gap-2 border-b px-4 py-3">
        <GitBranch className="size-4 text-primary" />
        <div>
          <h2 className="text-sm font-semibold">{label("topology")}</h2>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {row?.qname ?? label("selectQuery")}
          </p>
        </div>
      </div>
      {row ? (
        <ResolutionFlow row={row} />
      ) : (
        <Notice text={label("selectQuery")} />
      )}
      {row && (
        <div className="flex flex-wrap gap-2 border-t p-3 text-xs text-muted-foreground">
          <p className="w-full">{label("topologyProbes")}</p>
          {row.events.map((event) => (
            <span
              key={event.id}
              className="rounded-md border px-2 py-1 font-mono"
            >
              {event.kind}: {event.plugin || event.target || event.status}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}

function QueryDetail({
  row,
  record,
  label,
  formatDateTime,
}: {
  row: DNSObservation;
  record: QueryRecordDetail | null;
  label: (key: string, params?: Record<string, string | number>) => string;
  formatDateTime: (
    value: Date | number | string,
    options?: Intl.DateTimeFormatOptions,
  ) => string;
}) {
  return (
    <div className="max-h-[630px] space-y-4 overflow-auto p-4">
      <dl className="grid grid-cols-2 gap-x-4 gap-y-3 text-xs">
        <Fact title={label("query")} value={`${row.qname} ${row.qtype}`} />
        <Fact title={label("client")} value={row.client_ip} />
        <Fact
          title={label("time")}
          value={formatDateTime(row.created_at_ms, {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
          })}
        />
        <Fact
          title={label("latency")}
          value={`${row.elapsed_ms.toFixed(2)} ms`}
        />
        <Fact title="RCODE" value={row.rcode ?? "—"} />
        <Fact
          title={label("event")}
          value={record ? label("executionRecorded") : label("notRecorded")}
        />
      </dl>
      <div>
        <div className="mb-2 flex items-center justify-between gap-2">
          <h3 className="text-xs font-semibold">{label("executedPath")}</h3>
          {row.truncated && (
            <span className="text-[11px] text-warning-foreground">
              {label("truncatedObservation")}
            </span>
          )}
        </div>
        {row.events.length ? (
          <div className="space-y-1.5 border-l border-border pl-3">
            {row.events.map((event) => (
              <div
                key={event.id}
                className={cn(
                  "relative grid grid-cols-[minmax(0,1fr)_auto] gap-2 rounded-md border px-2.5 py-2 text-xs",
                  event.selected && "border-primary/40 bg-primary/5",
                  event.status === "not_started" && "opacity-60",
                )}
              >
                <span className="absolute -left-[17px] top-3 size-2 rounded-full border border-background bg-primary" />
                <span className="min-w-0">
                  <span className="block truncate font-medium">
                    {event.target || event.plugin || event.kind}
                  </span>
                  <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">
                    {event.kind} · {event.method || event.plugin}
                    {event.reason ? ` · ${event.reason}` : ""}
                  </span>
                </span>
                <span className="text-right">
                  <Status value={event.status} label={label} />
                  <span className="mt-1 block font-mono tabular-nums text-muted-foreground">
                    {event.elapsed_ms === null
                      ? "—"
                      : `${event.elapsed_ms.toFixed(2)} ms`}
                  </span>
                </span>
                {event.rcode && (
                  <span className="col-span-2 text-[11px] text-muted-foreground">
                    RCODE {event.rcode} ·{" "}
                    {label("answerCount", { count: event.answer_count ?? 0 })}
                  </span>
                )}
              </div>
            ))}
          </div>
        ) : (
          <p className="rounded-md border border-dashed p-3 text-xs text-muted-foreground">
            {label("notRecorded")}
          </p>
        )}
      </div>
      <div>
        <h3 className="mb-2 text-xs font-semibold">{label("answers")}</h3>
        {row.answers.length ? (
          <div className="flex flex-wrap gap-1.5">
            {row.answers.map((answer, index) => (
              <code
                key={`${answer}-${index}`}
                className="rounded bg-muted px-2 py-1 font-mono text-[11px]"
              >
                {answer}
              </code>
            ))}
          </div>
        ) : record?.answers_json?.length ? (
          <div className="space-y-1">
            {record.answers_json.map((answer, index) => (
              <div
                key={`${answer.name}-${index}`}
                className="grid grid-cols-[minmax(0,1fr)_auto] gap-2 rounded-md bg-muted/50 px-2.5 py-2 font-mono text-[11px]"
              >
                <span className="min-w-0 truncate">
                  {answer.name} → {answer.payload_text}
                </span>
                <span className="text-muted-foreground">
                  {answer.rr_type} · TTL {answer.ttl}
                </span>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-xs text-muted-foreground">—</p>
        )}
      </div>
      {record?.steps?.length ? (
        <div>
          <h3 className="mb-2 text-xs font-semibold">
            {label("executionSteps")}
          </h3>
          <div className="space-y-1">
            {record.steps.map((step) => (
              <div
                key={`${step.event_index}-${step.sequence_tag}`}
                className="flex flex-wrap items-center gap-2 text-xs"
              >
                <ArrowDownRight className="size-3 text-muted-foreground" />
                <span className="font-mono">
                  {step.tag ?? step.sequence_tag}
                </span>
                <span className="text-muted-foreground">
                  {step.kind} · {step.outcome}
                </span>
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function Fact({ title, value }: { title: string; value: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-muted-foreground">{title}</dt>
      <dd className="mt-1 truncate font-mono" title={value}>
        {value}
      </dd>
    </div>
  );
}
