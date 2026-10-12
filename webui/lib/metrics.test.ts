/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import {
  MAX_DNS_TRAFFIC_SAMPLE_WINDOW_MS,
  calculateDnsTrafficMetrics,
  sumServerRequestTotal,
} from "./dashboard-traffic";
import {
  formatMetricValue,
  selectCardMetrics,
  type PluginMetricsMap,
} from "./metrics";
import { pluginKindDefinitions } from "./plugin-definitions";

describe("plugin metric formatting", () => {
  it("keeps every curated metric card within the six-item surface", () => {
    const oversized = pluginKindDefinitions
      .filter((definition) => definition.metrics)
      .filter(
        (definition) =>
          (definition.metrics?.derivedCard?.length ?? 0) +
            (definition.metrics?.cardPriority?.length ?? 0) >
          6,
      )
      .map((definition) => definition.kind);

    expect(oversized).toEqual([]);
  });

  it("provides a display label for every card-priority metric", () => {
    const unlabeled = pluginKindDefinitions.flatMap((definition) =>
      (definition.metrics?.cardPriority ?? [])
        .filter((name) => !definition.metrics?.metricLabels?.[name])
        .map((name) => `${definition.kind}:${name}`),
    );

    expect(unlabeled).toEqual([]);
  });

  it("formats timestamp gauges as local date-times", () => {
    const timestamp = 1_784_701_820;

    expect(
      formatMetricValue(timestamp, "zh-CN", {
        metricName: "ros_route_last_write_success_timestamp_seconds",
      }),
    ).not.toBe("1,784,701,820");
    expect(
      formatMetricValue(timestamp, "zh-CN", {
        metricName: "ros_route_last_write_success_timestamp_seconds",
      }),
    ).toContain("2026");
    expect(
      formatMetricValue(timestamp, "zh-CN", {
        metricName: "ros_route_last_write_success_timestamp_seconds",
        compact: true,
      }),
    ).not.toContain("2026");
  });

  it("shows an unset timestamp gauge as unavailable", () => {
    expect(
      formatMetricValue(0, "en-US", {
        metricName: "ros_address_list_last_reconcile_success_timestamp_seconds",
      }),
    ).toBe("—");
  });

  it.each([
    [
      "ros_route",
      "ros_route_last_write_success_timestamp_seconds",
      "ros_route_write_success_total",
    ],
    [
      "ros_address_list",
      "ros_address_list_last_write_success_timestamp_seconds",
      "ros_address_list_write_success_total",
    ],
  ])(
    "prioritizes %s write counts and formats its card timestamp",
    (kind, timestampName, successName) => {
      const metrics = selectCardMetrics(
        [
          { name: timestampName, labels: {}, value: 1_784_701_820 },
          { name: successName, labels: {}, value: 42 },
          { name: `${kind}_write_error_total`, labels: {}, value: 3 },
          { name: `${kind}_dropped_total`, labels: {}, value: 2 },
          { name: `${kind}_degraded`, labels: {}, value: 1 },
          { name: `${kind}_managed_entries`, labels: {}, value: 24 },
          { name: `${kind}_pending_observations`, labels: {}, value: 5 },
        ],
        kind,
        6,
        "zh-CN",
      );

      expect(metrics.map((metric) => metric.label)).toEqual([
        "写入成功",
        "写入失败",
        "异步丢弃",
        "最近写入成功",
        kind === "ros_route" ? "受管路由" : "受管条目",
        "待处理观测",
      ]);
      expect(metrics[0]?.value).toBe("42");
      expect(metrics[3]?.value).not.toBe("1,784,701,820");
      expect(metrics[3]?.value).not.toContain("2026");
      expect(metrics).toHaveLength(6);
    },
  );

  it("keeps counter formatting unchanged", () => {
    expect(
      formatMetricValue(1_784_701_820, "en-US", {
        metricName: "ros_route_write_success_total",
      }),
    ).toBe("1,784,701,820");
  });
});

describe("dashboard DNS traffic metrics", () => {
  it("sums inbound requests across server plugins only", () => {
    const metrics: PluginMetricsMap = {
      udp: [
        { name: "server_request_total", labels: {}, value: 120 },
        { name: "server_inflight", labels: {}, value: 2 },
      ],
      tcp: [{ name: "server_request_total", labels: {}, value: 80 }],
      cache: [{ name: "cache_hit_total", labels: {}, value: 999 }],
    };

    expect(sumServerRequestTotal(metrics)).toBe(200);
  });

  it("calculates QPS from the actual sampling window", () => {
    expect(
      calculateDnsTrafficMetrics(
        { requestTotal: 100, sampledAtMs: 1_000 },
        { requestTotal: 145, sampledAtMs: 4_000 },
      ),
    ).toEqual({
      status: "available",
      qps: 15,
      requestTotal: 145,
      sampleWindowSeconds: 3,
    });
  });

  it("reports zero QPS when the request counter did not change", () => {
    expect(
      calculateDnsTrafficMetrics(
        { requestTotal: 145, sampledAtMs: 1_000 },
        { requestTotal: 145, sampledAtMs: 4_000 },
      ),
    ).toEqual({
      status: "available",
      qps: 0,
      requestTotal: 145,
      sampleWindowSeconds: 3,
    });
  });

  it("does not invent a QPS value without a valid monotonic baseline", () => {
    expect(
      calculateDnsTrafficMetrics(null, {
        requestTotal: 145,
        sampledAtMs: 4_000,
      }),
    ).toEqual({
      status: "available",
      qps: null,
      requestTotal: 145,
      sampleWindowSeconds: null,
    });
    expect(
      calculateDnsTrafficMetrics(
        { requestTotal: 145, sampledAtMs: 1_000 },
        { requestTotal: 4, sampledAtMs: 4_000 },
      ),
    ).toEqual({
      status: "available",
      qps: null,
      requestTotal: 4,
      sampleWindowSeconds: null,
    });
  });

  it("re-establishes the QPS baseline after a long polling gap", () => {
    expect(
      calculateDnsTrafficMetrics(
        { requestTotal: 100, sampledAtMs: 1_000 },
        {
          requestTotal: 1_000,
          sampledAtMs: 1_000 + MAX_DNS_TRAFFIC_SAMPLE_WINDOW_MS + 1,
        },
      ),
    ).toEqual({
      status: "available",
      qps: null,
      requestTotal: 1_000,
      sampleWindowSeconds: null,
    });
  });
});
