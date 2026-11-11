/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import type { BuildInfo, HealthResponse, SystemResponse } from "./oxidns-api";

const apiMocks = vi.hoisted(() => ({
  fetchBuildInfo: vi.fn(),
  fetchControl: vi.fn(),
  fetchHealth: vi.fn(),
  fetchReloadStatus: vi.fn(),
  fetchSystem: vi.fn(),
}));

vi.mock("./oxidns-api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./oxidns-api")>();
  return { ...actual, ...apiMocks };
});

import { useAppStore } from "./store";
import { useAuthStore } from "./auth-store";

const build: BuildInfo = {
  version: "0.9.0",
  bundle: "full",
  enabled_bundles: ["full"],
  enabled_features: ["plugin-upgrade"],
  supported_plugins: {
    servers: [],
    executors: [],
    matchers: [],
    providers: [],
  },
};

const health: HealthResponse = {
  status: "ok",
  version: build.version,
  uptime_ms: 10_000,
  checks: { api: "ok", plugin_init: "ok", server_startup: "ok" },
  plugins: { total: 4, servers: 1 },
};

const system: SystemResponse = {
  ok: true,
  version: build.version,
  build,
  os: "linux",
  arch: "x86_64",
  uptime_ms: 10_000,
  config_path: "/etc/oxidns/config.yaml",
  api_enabled: true,
  reload: {
    status: "success",
    pending: false,
    in_progress: false,
    running_version: "running-v2",
  },
};

describe("runtime state refresh", () => {
  let connectionEpoch = 0;

  beforeEach(() => {
    vi.clearAllMocks();
    connectionEpoch += 1;
    useAuthStore.setState({
      isConnected: true,
      connectionEpoch,
      serverConfig: {
        url: `/api-${connectionEpoch}`,
        requiresAuth: false,
        username: "",
        password: "",
      },
    });
    useAppStore.setState({
      health: null,
      system: null,
      buildInfo: null,
      reloadStatus: null,
      runningVersion: "running-v1",
      dependencyGraph: null,
      runningDependencyGraph: null,
    });
    apiMocks.fetchHealth.mockResolvedValue(health);
    apiMocks.fetchSystem.mockResolvedValue(system);
    apiMocks.fetchBuildInfo.mockResolvedValue({ ok: true, build });
  });

  it("refreshes health and system without redundant control/build/reload calls", async () => {
    await useAppStore.getState().refreshRuntimeState();

    expect(apiMocks.fetchHealth).toHaveBeenCalledTimes(1);
    expect(apiMocks.fetchSystem).toHaveBeenCalledTimes(1);
    expect(apiMocks.fetchControl).not.toHaveBeenCalled();
    expect(apiMocks.fetchReloadStatus).not.toHaveBeenCalled();
    expect(apiMocks.fetchBuildInfo).not.toHaveBeenCalled();
    expect(useAppStore.getState()).toMatchObject({
      health,
      system,
      buildInfo: build,
      reloadStatus: system.reload,
      runningVersion: "running-v2",
    });
  });

  it("fetches build metadata once when an older system response omits it", async () => {
    apiMocks.fetchSystem.mockResolvedValue({ ...system, build: undefined });

    await useAppStore.getState().refreshRuntimeState();

    expect(apiMocks.fetchBuildInfo).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().buildInfo).toEqual(build);
  });

  it("does not reuse build metadata from a previous backend", async () => {
    const previousBuild = { ...build, version: "0.8.0", bundle: "minimal" };
    useAppStore.setState({ buildInfo: previousBuild });
    apiMocks.fetchSystem.mockResolvedValue({ ...system, build: undefined });

    await useAppStore.getState().refreshRuntimeState();

    expect(apiMocks.fetchBuildInfo).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().buildInfo).toEqual(build);
  });

  it("deduplicates overlapping system refreshes", async () => {
    let resolveSystem: ((value: SystemResponse) => void) | undefined;
    apiMocks.fetchSystem.mockReturnValue(
      new Promise<SystemResponse>((resolve) => {
        resolveSystem = resolve;
      }),
    );

    const first = useAppStore.getState().refreshSystemState();
    const second = useAppStore.getState().refreshSystemState();
    resolveSystem?.(system);
    await Promise.all([first, second]);

    expect(first).toBe(second);
    expect(apiMocks.fetchSystem).toHaveBeenCalledTimes(1);
  });

  it("discards an in-flight response after switching backends", async () => {
    let resolvePrevious: ((value: SystemResponse) => void) | undefined;
    const previousResponse = { ...system, version: "old-backend" };
    const currentResponse = { ...system, version: "new-backend" };
    apiMocks.fetchSystem
      .mockReturnValueOnce(
        new Promise<SystemResponse>((resolve) => {
          resolvePrevious = resolve;
        }),
      )
      .mockResolvedValueOnce(currentResponse);

    const previousRefresh = useAppStore.getState().refreshSystemState();
    connectionEpoch += 1;
    useAuthStore.setState({
      connectionEpoch,
      serverConfig: {
        url: "/api-new",
        requiresAuth: false,
        username: "",
        password: "",
      },
    });
    const currentRefresh = useAppStore.getState().refreshSystemState();
    await currentRefresh;
    resolvePrevious?.(previousResponse);
    await previousRefresh;

    expect(apiMocks.fetchSystem).toHaveBeenCalledTimes(2);
    expect(useAppStore.getState().system?.version).toBe("new-backend");
  });
});
