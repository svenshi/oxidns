/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";
import type { QueryRecorderTimeseriesResponse } from "@/lib/oxidns-api";
import {
  bucketRecordFilters,
  aggregationRecordFilters,
  matcherRecordFilters,
  presetWindow,
  rangeToFilters,
  refreshWindow,
  timeseriesChartPoints,
  timeseriesCoverage,
  timeseriesDataIntervals,
  validateAggregationRange,
} from "./query-recorder-timeseries";

function response(timestamps: number[]): QueryRecorderTimeseriesResponse {
  return {
    ok: true,
    sample_size: timestamps.length,
    sample_limit: 10_000,
    sample_truncated: false,
    sample_since_ms: timestamps[0] ?? null,
    sample_until_ms: timestamps.at(-1) ?? null,
    bucket_ms: 60_000,
    points: timestamps.map((timestamp) => ({
      bucket_ms: timestamp,
      total: 1,
      error_count: 0,
      no_response_count: 0,
      servfail_count: 0,
      partial: false,
      avg_ms: 12,
      p95_ms: 12,
    })),
  };
}

describe("bounded aggregation time ranges", () => {
  it.each([
    ["10m", 600_000],
    ["1h", 3_600_000],
    ["24h", 86_400_000],
  ] as const)("requires both bounds for %s", (range, duration) => {
    const now = 100_000_000;
    expect(rangeToFilters(range, now)).toEqual({
      sinceMs: now - duration,
      untilMs: now,
    });
  });
  it("moves recent presets forward on refresh", () => {
    const initial = presetWindow("1h", 10_000_000);
    expect(refreshWindow(initial, 10_030_000)).toEqual({
      range: "1h",
      sinceMs: 6_430_000,
      untilMs: 10_030_000,
      nonce: 1,
    });
  });
  it("keeps a historical custom interval fixed on refresh", () => {
    const value = {
      range: "custom" as const,
      sinceMs: 100,
      untilMs: 200,
      nonce: 0,
    };
    expect(refreshWindow(value, 100_000_000)).toEqual({ ...value, nonce: 1 });
  });
  it.each([
    [NaN, 1, "missing"],
    [-1, 100, "missing"],
    [1, NaN, "missing"],
    [10, 10, "order"],
    [20, 10, "order"],
    [0, 86_400_001, "span"],
    [0, 86_400_000, null],
  ])("validates %s to %s", (since, until, error) => {
    expect(validateAggregationRange(since as number, until as number)).toBe(
      error,
    );
  });
});

describe("trend coverage and drilldown", () => {
  it("uses exact values for rankings and preserves special RCODE categories", () => {
    const window = presetWindow("1h", 10_000_123);
    const interval = { sinceMs: 6_400_123, untilMs: 10_000_123 };
    expect(aggregationRecordFilters("qname", "example.com.", window)).toEqual({
      ...interval,
      qnameExact: "example.com.",
    });
    expect(aggregationRecordFilters("client", "192.0.2.1", window)).toEqual({
      ...interval,
      clientIpExact: "192.0.2.1",
    });
    expect(aggregationRecordFilters("qtype", "AAAA", window)).toEqual({
      ...interval,
      qtype: "AAAA",
    });
    expect(aggregationRecordFilters("rcode", "Server Failure", window)).toEqual(
      { ...interval, rcode: "Server Failure" },
    );
    for (const category of ["_ERROR", "_NO_RESPONSE", "_UNKNOWN"] as const) {
      expect(aggregationRecordFilters("rcode", category, window)).toEqual({
        ...interval,
        rcodeBucket: category,
      });
    }
  });

  it("opens a matcher record view with only the exact applied matcher interval", () => {
    const window = {
      ...presetWindow("1h", 10_000_123),
      qname: "old.example",
      status: "error",
    };
    expect(matcherRecordFilters("ads", window)).toEqual({
      matcherTag: "ads",
      sinceMs: 6_400_123,
      untilMs: 10_000_123,
    });
  });
  it("fills known leading, interior and trailing gaps with zero counts and null latency", () => {
    const points = timeseriesChartPoints(response([60_000, 180_000]), {
      sinceMs: 123,
      untilMs: 240_123,
    });
    expect(points.map((point) => point.bucket_ms)).toEqual([
      0, 60_000, 120_000, 180_000, 240_000,
    ]);
    expect(points.map((point) => point.total)).toEqual([0, 1, 0, 1, 0]);
    expect(points.map((point) => point.avg_ms)).toEqual([
      null,
      12,
      null,
      12,
      null,
    ]);
    expect(points.map((point) => point.p95_ms)).toEqual([
      null,
      12,
      null,
      12,
      null,
    ]);
    expect(points[0].partial).toBe(true);
    expect(points.at(-1)?.partial).toBe(true);
  });
  it("never zero-fills time before a truncated sample", () => {
    const data = response([180_000, 240_000]);
    data.sample_truncated = true;
    data.sample_since_ms = 190_000;
    data.points[0].partial = true;
    const filters = { sinceMs: 0, untilMs: 300_000 };
    expect(timeseriesCoverage(data, filters)).toEqual({
      sinceMs: 190_000,
      untilMs: 300_000,
    });
    const points = timeseriesChartPoints(data, filters);
    expect(points.map((point) => point.bucket_ms)).toEqual([
      180_000, 240_000, 300_000,
    ]);
    expect(points[0].partial).toBe(true);
  });
  it("counts execution errors, missing responses and SERVFAIL as anomalies", () => {
    const data = response([0]);
    Object.assign(data.points[0], {
      total: 10,
      error_count: 1,
      no_response_count: 2,
      servfail_count: 3,
    });
    expect(
      timeseriesChartPoints(data, { sinceMs: 0, untilMs: 59_999 })[0]
        .anomaly_count,
    ).toBe(6);
  });
  it("intersects drilldown bounds and preserves non-time filters", () => {
    expect(
      bucketRecordFilters(60_000, 60_000, {
        sinceMs: 70_000,
        untilMs: 90_000,
        qname: "example",
        status: "error",
        matcherTag: "ads",
      }),
    ).toEqual({
      sinceMs: 70_000,
      untilMs: 90_000,
      qname: "example",
      status: "error",
      matcherTag: "ads",
    });
  });
  it("does not include the next bucket in drilldown", () => {
    expect(
      bucketRecordFilters(60_000, 60_000, { sinceMs: 0, untilMs: 240_000 }),
    ).toEqual({ sinceMs: 60_000, untilMs: 119_999 });
  });
  it("preserves a single point and empty response", () => {
    expect(
      timeseriesChartPoints(response([0]), { sinceMs: 0, untilMs: 59_999 }),
    ).toHaveLength(1);
    expect(
      timeseriesChartPoints(response([]), { sinceMs: 0, untilMs: 3_600_000 }),
    ).toEqual([]);
  });
});

describe("timeline sampled-record shading", () => {
  it("shades continuous sample coverage across empty buckets", () => {
    const data = response([0, 60_000, 180_000]);
    data.sample_until_ms = 200_000;
    expect(
      timeseriesDataIntervals(data, { sinceMs: 0, untilMs: 240_000 }),
    ).toEqual([{ sinceMs: 0, untilMs: 200_000 }]);
  });
  it("clips shading to sample and request bounds, including truncated partial buckets", () => {
    const data = response([120_000, 180_000]);
    data.sample_truncated = true;
    data.sample_since_ms = 150_123;
    data.sample_until_ms = 220_456;
    expect(
      timeseriesDataIntervals(data, { sinceMs: 0, untilMs: 210_000 }),
    ).toEqual([{ sinceMs: 150_123, untilMs: 210_000 }]);
  });
  it("preserves single-timestamp samples without widening the data interval", () => {
    expect(
      timeseriesDataIntervals(response([60_000]), {
        sinceMs: 0,
        untilMs: 120_000,
      }),
    ).toEqual([{ sinceMs: 60_000, untilMs: 60_000 }]);
  });
  it("does not shade empty responses, missing sample bounds, or unrelated intervals", () => {
    expect(
      timeseriesDataIntervals(response([]), { sinceMs: 0, untilMs: 60_000 }),
    ).toEqual([]);
    const data = response([0, 120_000]);
    expect(
      timeseriesDataIntervals(
        { ...data, sample_since_ms: null },
        { sinceMs: 0, untilMs: 60_000 },
      ),
    ).toEqual([]);
    expect(
      timeseriesDataIntervals(data, { sinceMs: 180_000, untilMs: 240_000 }),
    ).toEqual([]);
  });
});
