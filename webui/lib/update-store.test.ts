/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const apiMocks = vi.hoisted(() => ({
  fetchUpgradeCheck: vi.fn(),
}));

vi.mock("./oxidns-api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./oxidns-api")>();
  return { ...actual, fetchUpgradeCheck: apiMocks.fetchUpgradeCheck };
});

function createMemoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => values.delete(key),
    setItem: (key, value) => values.set(key, value),
  };
}

describe("update-check persistence", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: createMemoryStorage(),
    });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: globalThis,
    });
    apiMocks.fetchUpgradeCheck.mockResolvedValue({
      latest_version: "0.10.0",
      update_available: true,
      asset_name: "oxidns-full.tar.gz",
      release_url: "https://example.com/release",
    });
  });

  afterEach(() => {
    Reflect.deleteProperty(globalThis, "window");
    Reflect.deleteProperty(globalThis, "localStorage");
  });

  it("reuses a recent automatic check after the store is recreated", async () => {
    const firstModule = await import("./update-store");
    await firstModule.useUpdateStore.getState().checkForUpdatesIfDue("0.9.0");
    expect(apiMocks.fetchUpgradeCheck).toHaveBeenCalledTimes(1);

    vi.resetModules();
    const secondModule = await import("./update-store");
    await secondModule.useUpdateStore.getState().checkForUpdatesIfDue("0.9.0");

    expect(apiMocks.fetchUpgradeCheck).toHaveBeenCalledTimes(1);
    expect(secondModule.useUpdateStore.getState()).toMatchObject({
      lastCheckedAt: expect.any(Number),
      updateInfo: {
        currentVersion: "0.9.0",
        latestVersion: "0.10.0",
        updateAvailable: true,
      },
    });
  });

  it("keeps manual checks independent from the automatic interval", async () => {
    const { useUpdateStore } = await import("./update-store");
    await useUpdateStore.getState().checkForUpdatesIfDue("0.9.0");
    await useUpdateStore.getState().checkForUpdates("0.9.0");

    expect(apiMocks.fetchUpgradeCheck).toHaveBeenCalledTimes(2);
  });

  it("invalidates the automatic cache when request credentials change", async () => {
    const { useUpdateStore } = await import("./update-store");
    await useUpdateStore.getState().checkForUpdatesIfDue("0.9.0");
    useUpdateStore.getState().setUpgradeConfig({ githubToken: "new-token" });
    await useUpdateStore.getState().checkForUpdatesIfDue("0.9.0");

    expect(apiMocks.fetchUpgradeCheck).toHaveBeenCalledTimes(2);
  });

  it("keeps automatic caches separate for different backends", async () => {
    const [{ useUpdateStore }, { useAuthStore }] = await Promise.all([
      import("./update-store"),
      import("./auth-store"),
    ]);
    await useUpdateStore.getState().checkForUpdatesIfDue("0.9.0");
    useAuthStore.setState((state) => ({
      serverConfig: { ...state.serverConfig, url: "https://dns.example/api" },
    }));
    await useUpdateStore.getState().checkForUpdatesIfDue("0.9.0");

    expect(apiMocks.fetchUpgradeCheck).toHaveBeenCalledTimes(2);
  });

  it("persists the force-upgrade preference", async () => {
    const firstModule = await import("./update-store");
    firstModule.useUpdateStore.getState().setUpgradeConfig({ force: true });

    vi.resetModules();
    const secondModule = await import("./update-store");
    expect(secondModule.useUpdateStore.getState().upgradeConfig.force).toBe(
      true,
    );
  });

  it("persists the post-upgrade cleanup preference", async () => {
    const firstModule = await import("./update-store");
    firstModule.useUpdateStore
      .getState()
      .setUpgradeConfig({ cleanupAfterUpgrade: false });

    vi.resetModules();
    const secondModule = await import("./update-store");
    expect(
      secondModule.useUpdateStore.getState().upgradeConfig.cleanupAfterUpgrade,
    ).toBe(false);
  });
});
