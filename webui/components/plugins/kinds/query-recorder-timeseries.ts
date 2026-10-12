import type {
  QueryRecordFilters,
  QueryRecorderSampleMetadata,
  QueryRecorderTimeseriesPoint,
  QueryRecorderTimeseriesResponse,
} from "@/lib/oxidns-api";

export type InsightsRangeKey = "10m" | "1h" | "24h" | "custom";
export type PresetRangeKey = Exclude<InsightsRangeKey, "custom">;
export const MAX_AGGREGATION_SPAN_MS = 86_400_000;
export const TIMESERIES_OPTIONS = { bucket: "auto", buckets: 120 } as const;

export interface AggregationWindow {
  range: InsightsRangeKey;
  sinceMs: number;
  untilMs: number;
  nonce: number;
}

export function rangeToFilters(range: PresetRangeKey, now = Date.now()) {
  const minutes = range === "10m" ? 10 : range === "1h" ? 60 : 24 * 60;
  return { sinceMs: Math.max(0, now - minutes * 60_000), untilMs: now };
}

export function presetWindow(
  range: PresetRangeKey = "1h",
  now = Date.now(),
  nonce = 0,
): AggregationWindow {
  return { range, ...rangeToFilters(range, now), nonce };
}

export function refreshWindow(
  value: AggregationWindow,
  now = Date.now(),
): AggregationWindow {
  return value.range === "custom"
    ? { ...value, nonce: value.nonce + 1 }
    : presetWindow(value.range, now, value.nonce + 1);
}

export function validateAggregationRange(
  sinceMs: number,
  untilMs: number,
): "missing" | "order" | "span" | null {
  if (!Number.isFinite(sinceMs) || !Number.isFinite(untilMs) || sinceMs < 0)
    return "missing";
  if (sinceMs >= untilMs) return "order";
  if (untilMs - sinceMs > MAX_AGGREGATION_SPAN_MS) return "span";
  return null;
}

export type TimeseriesChartPoint = Omit<
  QueryRecorderTimeseriesPoint,
  "avg_ms" | "p95_ms"
> & {
  avg_ms: number | null;
  p95_ms: number | null;
  anomaly_count: number;
};

export function timeseriesCoverage(
  data: QueryRecorderSampleMetadata,
  filters: QueryRecordFilters,
) {
  const sinceMs = data.sample_truncated
    ? data.sample_since_ms
    : (filters.sinceMs ?? data.sample_since_ms);
  const untilMs = filters.untilMs ?? data.sample_until_ms;
  return sinceMs == null || untilMs == null ? null : { sinceMs, untilMs };
}

export function bucketRecordFilters(
  timestamp: number,
  bucketMs: number,
  filters: QueryRecordFilters,
): QueryRecordFilters {
  return {
    ...filters,
    sinceMs: Math.max(timestamp, filters.sinceMs ?? timestamp),
    untilMs: Math.min(
      timestamp + bucketMs - 1,
      filters.untilMs ?? timestamp + bucketMs - 1,
    ),
  };
}

export type AggregationDrilldownKind =
  | "matcher"
  | "client"
  | "qname"
  | "qtype"
  | "rcode";

export function aggregationRecordFilters(
  kind: AggregationDrilldownKind,
  key: string,
  window: AggregationWindow,
): QueryRecordFilters {
  const interval = { sinceMs: window.sinceMs, untilMs: window.untilMs };
  switch (kind) {
    case "matcher":
      return { ...interval, matcherTag: key };
    case "client":
      return { ...interval, clientIpExact: key };
    case "qname":
      return { ...interval, qnameExact: key };
    case "qtype":
      return { ...interval, qtype: key };
    case "rcode":
      return key === "_ERROR" || key === "_NO_RESPONSE" || key === "_UNKNOWN"
        ? { ...interval, rcodeBucket: key }
        : { ...interval, rcode: key };
  }
}

export function matcherRecordFilters(
  matcherTag: string,
  window: AggregationWindow,
): QueryRecordFilters {
  return aggregationRecordFilters("matcher", matcherTag, window);
}

export function timeseriesChartPoints(
  data: QueryRecorderTimeseriesResponse,
  filters: QueryRecordFilters,
): TimeseriesChartPoint[] {
  if (!data.points.length || data.bucket_ms <= 0) return [];
  const coverage = timeseriesCoverage(data, filters);
  if (!coverage) return [];
  const bucketMs = data.bucket_ms;
  const first = Math.floor(coverage.sinceMs / bucketMs);
  const last = Math.floor(coverage.untilMs / bucketMs);
  const decorate = (
    point: QueryRecorderTimeseriesPoint,
  ): TimeseriesChartPoint => ({
    ...point,
    anomaly_count:
      point.error_count + point.no_response_count + point.servfail_count,
  });
  // Keep allocations bounded even when talking to a server with a different bucket budget.
  if (last - first + 1 > 720) return data.points.map(decorate);
  const byTimestamp = new Map(
    data.points.map((point) => [point.bucket_ms, point]),
  );
  const points: TimeseriesChartPoint[] = [];
  for (let index = first; index <= last; index++) {
    const timestamp = index * bucketMs;
    const point = byTimestamp.get(timestamp);
    points.push(
      point
        ? decorate(point)
        : {
            bucket_ms: timestamp,
            total: 0,
            error_count: 0,
            no_response_count: 0,
            servfail_count: 0,
            partial:
              timestamp < coverage.sinceMs ||
              timestamp + bucketMs - 1 > coverage.untilMs,
            avg_ms: null,
            p95_ms: null,
            anomaly_count: 0,
          },
    );
  }
  return points;
}

// Show continuous sample coverage while preserving its exact observed edges.
export function timeseriesDataIntervals(
  data: QueryRecorderSampleMetadata,
  filters: QueryRecordFilters,
) {
  if (data.sample_since_ms == null || data.sample_until_ms == null) return [];
  const sinceMs = Math.max(
    data.sample_since_ms,
    filters.sinceMs ?? data.sample_since_ms,
  );
  const untilMs = Math.min(
    data.sample_until_ms,
    filters.untilMs ?? data.sample_until_ms,
  );
  return untilMs < sinceMs ? [] : [{ sinceMs, untilMs }];
}
