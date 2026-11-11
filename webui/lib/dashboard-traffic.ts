/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import type { PluginMetricsMap } from "./metrics";

export interface RequestCounterSample {
  requestTotal: number;
  sampledAtMs: number;
}

export interface DnsTrafficMetrics {
  status: "pending" | "available" | "unavailable";
  qps: number | null;
  requestTotal: number;
  sampleWindowSeconds: number | null;
}

export const MAX_DNS_TRAFFIC_SAMPLE_WINDOW_MS = 30_000;

/** Sum inbound requests across every configured DNS server plugin. */
export function sumServerRequestTotal(metrics: PluginMetricsMap): number {
  let total = 0;
  for (const series of Object.values(metrics)) {
    for (const sample of series) {
      if (
        sample.name === "server_request_total" &&
        Number.isFinite(sample.value)
      ) {
        total += sample.value;
      }
    }
  }
  return total;
}

/**
 * Derive the current DNS request rate from two monotonic counter snapshots.
 * A missing or reset baseline intentionally produces no rate instead of a
 * misleading negative or stale value.
 */
export function calculateDnsTrafficMetrics(
  previous: RequestCounterSample | null,
  current: RequestCounterSample,
): DnsTrafficMetrics {
  const sampleWindowMs = previous
    ? current.sampledAtMs - previous.sampledAtMs
    : 0;
  if (
    !previous ||
    sampleWindowMs <= 0 ||
    sampleWindowMs > MAX_DNS_TRAFFIC_SAMPLE_WINDOW_MS ||
    current.requestTotal < previous.requestTotal
  ) {
    return {
      status: "available",
      qps: null,
      requestTotal: current.requestTotal,
      sampleWindowSeconds: null,
    };
  }

  const sampleWindowSeconds = sampleWindowMs / 1_000;
  return {
    status: "available",
    qps: (current.requestTotal - previous.requestTotal) / sampleWindowSeconds,
    requestTotal: current.requestTotal,
    sampleWindowSeconds,
  };
}
