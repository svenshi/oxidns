"use client";

import type { HealthResponse } from "./oxidns-api";

const FRESH_PROCESS_BUFFER_MS = 2_000;

export interface ProcessInstanceBaseline {
  instanceId?: string;
  startedAtMs?: number;
  uptimeMs?: number;
  observedAtMs: number;
}

export function createProcessInstanceBaseline(
  health?: HealthResponse,
): ProcessInstanceBaseline {
  return {
    instanceId: health?.instance_id,
    startedAtMs: health?.started_at_ms,
    uptimeMs: health?.uptime_ms,
    observedAtMs: Date.now(),
  };
}

export function processInstanceChanged(
  health: HealthResponse,
  baseline: ProcessInstanceBaseline,
): boolean {
  if (
    baseline.instanceId &&
    health.instance_id &&
    health.instance_id !== baseline.instanceId
  ) {
    return true;
  }

  if (
    baseline.startedAtMs !== undefined &&
    health.started_at_ms !== undefined &&
    health.started_at_ms !== baseline.startedAtMs
  ) {
    return true;
  }

  if (baseline.uptimeMs !== undefined && health.uptime_ms < baseline.uptimeMs) {
    return true;
  }

  return (
    health.uptime_ms <
    Date.now() - baseline.observedAtMs + FRESH_PROCESS_BUFFER_MS
  );
}

export function hasProcessIdentityBaseline(
  baseline: ProcessInstanceBaseline,
): boolean {
  return Boolean(baseline.instanceId || baseline.startedAtMs !== undefined);
}
