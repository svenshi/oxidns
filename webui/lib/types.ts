/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

export type PluginType = "server" | "executor" | "matcher" | "provider";

export type PluginStatus = "running" | "stopped" | "error";

export interface PluginInstance {
  id: string;
  name: string;
  type: PluginType;
  pluginKind: string;
  status: PluginStatus;
  enabled: boolean;
  pinned: boolean;
  config: Record<string, unknown>;
  metrics: {
    calls: number;
    hitRate?: number;
    avgLatency: number;
    errorRate: number;
    qps: number;
  };
  createdAt: string;
  updatedAt: string;
}

export interface SystemMetrics {
  totalPlugins: number;
  runningPlugins: number;
  cpuUsage: number;
  memoryUsage: number;
  memoryTotal: number;
  currentQps: number;
  uptime: number;
}

export interface SystemInfo {
  version: string;
  latestVersion: string;
  os: string;
  arch: string;
  threads: number;
  maxConcurrency: number;
  logLevel: string;
  logRolling: string;
}
