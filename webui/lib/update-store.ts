/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { create } from "zustand";
import {
  fetchBuildInfo,
  fetchHealth,
  fetchUpgradeCheck,
  fetchUpgradeStatus,
  triggerUpgradeApply,
} from "./oxidns-api";
import { WEBUI, tClient } from "./i18n";
import {
  createProcessInstanceBaseline,
  hasProcessIdentityBaseline,
  processInstanceChanged,
  type ProcessInstanceBaseline,
} from "./process-instance";
import { useAppStore } from "./store";
import { useAuthStore } from "./auth-store";
import {
  isAutomaticUpdateCheckDue,
  updateCheckOptionsFingerprint,
  updateCheckRequestKey,
} from "./update-check-policy";

const STORAGE_KEY = "oxidns:upgrade-config";
const UPDATE_CHECK_STORAGE_KEY = "oxidns:update-check";

export type UpgradeBundle = "auto" | "full" | "minimal" | "standard";

export interface UpgradeConfig {
  repository: string;
  bundle: UpgradeBundle;
  outbound: string;
  socks5: string;
  githubToken: string;
  persistGithubToken: boolean;
  allowPrerelease: boolean;
  force: boolean;
  cleanupAfterUpgrade: boolean;
  autoCheck: boolean;
}

export const DEFAULT_UPGRADE_CONFIG: UpgradeConfig = {
  repository: "svenshi/oxidns",
  bundle: "auto",
  outbound: "",
  socks5: "",
  githubToken: "",
  persistGithubToken: false,
  allowPrerelease: false,
  force: false,
  cleanupAfterUpgrade: true,
  autoCheck: true,
};

type PersistedUpgradeConfig = Omit<UpgradeConfig, "githubToken"> & {
  githubToken?: string;
};

export interface UpdateInfo {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  assetName: string;
  releaseUrl: string;
}

interface PersistedUpdateCheck {
  requestKey: string;
  checkedAt: number;
  succeeded: boolean;
  updateInfo: UpdateInfo | null;
}

export type UpgradeApplyPhase =
  | "requesting"
  | "applying"
  | "waiting_up"
  | "verifying"
  | "completed";

interface UpdateState {
  upgradeConfig: UpgradeConfig;
  updateInfo: UpdateInfo | null;
  isChecking: boolean;
  isApplying: boolean;
  applyPhase: UpgradeApplyPhase | null;
  lastCheckedAt: number | null;
  lastAppliedVersion: string | null;
  checkError: string | null;
  applyError: string | null;

  setUpgradeConfig: (config: Partial<UpgradeConfig>) => void;
  checkForUpdates: (currentVersion: string) => Promise<void>;
  checkForUpdatesIfDue: (currentVersion: string) => Promise<void>;
  triggerUpgrade: () => Promise<void>;
  resetApplyState: () => void;
}

function loadUpgradeConfig(): UpgradeConfig {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored) as Partial<UpgradeConfig>;
      const persistGithubToken = parsed.persistGithubToken === true;
      return {
        ...DEFAULT_UPGRADE_CONFIG,
        ...pickPersistedUpgradeConfig(parsed),
        persistGithubToken,
        githubToken:
          persistGithubToken && typeof parsed.githubToken === "string"
            ? parsed.githubToken
            : "",
      };
    }
  } catch {
    // ignore
  }
  return { ...DEFAULT_UPGRADE_CONFIG };
}

function saveUpgradeConfig(config: UpgradeConfig): void {
  try {
    // Persist the token only after explicit user opt-in.
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify(pickPersistedUpgradeConfig(config)),
    );
  } catch {
    // ignore
  }
}

function pickPersistedUpgradeConfig(
  config: Partial<UpgradeConfig>,
): Partial<PersistedUpgradeConfig> {
  return {
    ...(config.repository !== undefined
      ? { repository: config.repository }
      : {}),
    ...(config.bundle !== undefined ? { bundle: config.bundle } : {}),
    ...(config.outbound !== undefined ? { outbound: config.outbound } : {}),
    ...(config.socks5 !== undefined ? { socks5: config.socks5 } : {}),
    ...(config.persistGithubToken !== undefined
      ? { persistGithubToken: config.persistGithubToken }
      : {}),
    ...(config.persistGithubToken && config.githubToken !== undefined
      ? { githubToken: config.githubToken }
      : {}),
    ...(config.allowPrerelease !== undefined
      ? { allowPrerelease: config.allowPrerelease }
      : {}),
    ...(config.force !== undefined ? { force: config.force } : {}),
    ...(config.cleanupAfterUpgrade !== undefined
      ? { cleanupAfterUpgrade: config.cleanupAfterUpgrade }
      : {}),
    ...(config.autoCheck !== undefined ? { autoCheck: config.autoCheck } : {}),
  };
}

function loadPersistedUpdateCheck(): PersistedUpdateCheck | null {
  try {
    const stored = localStorage.getItem(UPDATE_CHECK_STORAGE_KEY);
    if (!stored) return null;
    const parsed = JSON.parse(stored) as Partial<PersistedUpdateCheck>;
    if (
      typeof parsed.requestKey !== "string" ||
      typeof parsed.checkedAt !== "number" ||
      !Number.isFinite(parsed.checkedAt) ||
      typeof parsed.succeeded !== "boolean"
    ) {
      return null;
    }
    return {
      requestKey: parsed.requestKey,
      checkedAt: parsed.checkedAt,
      succeeded: parsed.succeeded,
      updateInfo: isUpdateInfo(parsed.updateInfo) ? parsed.updateInfo : null,
    };
  } catch {
    return null;
  }
}

function savePersistedUpdateCheck(check: PersistedUpdateCheck): void {
  try {
    localStorage.setItem(UPDATE_CHECK_STORAGE_KEY, JSON.stringify(check));
  } catch {
    // ignore
  }
}

function isUpdateInfo(value: unknown): value is UpdateInfo {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<UpdateInfo>;
  return (
    typeof candidate.currentVersion === "string" &&
    typeof candidate.latestVersion === "string" &&
    typeof candidate.updateAvailable === "boolean" &&
    typeof candidate.assetName === "string" &&
    typeof candidate.releaseUrl === "string"
  );
}

function createUpdateCheckRequestKey(
  currentVersion: string,
  config: UpgradeConfig,
): string {
  const backend = useAuthStore
    .getState()
    .serverConfig.url.trim()
    .replace(/\/+$/, "");
  return updateCheckRequestKey({
    backend,
    currentVersion,
    repository: config.repository,
    bundle: config.bundle,
    allowPrerelease: config.allowPrerelease,
    requestOptionsFingerprint: updateCheckOptionsFingerprint([
      config.outbound,
      config.socks5,
      config.githubToken,
    ]),
  });
}

const initialUpdateCheck =
  typeof window !== "undefined" ? loadPersistedUpdateCheck() : null;

export const useUpdateStore = create<UpdateState>((set, get) => ({
  upgradeConfig:
    typeof window !== "undefined"
      ? loadUpgradeConfig()
      : { ...DEFAULT_UPGRADE_CONFIG },
  updateInfo: initialUpdateCheck?.updateInfo ?? null,
  isChecking: false,
  isApplying: false,
  applyPhase: null,
  lastCheckedAt: initialUpdateCheck?.checkedAt ?? null,
  lastAppliedVersion: null,
  checkError: null,
  applyError: null,

  setUpgradeConfig: (partial) => {
    const next = { ...get().upgradeConfig, ...partial };
    saveUpgradeConfig(next);
    set({ upgradeConfig: next });
  },

  checkForUpdates: async (currentVersion: string) => {
    if (get().isChecking) return;
    const { upgradeConfig } = get();
    const requestKey = createUpdateCheckRequestKey(
      currentVersion,
      upgradeConfig,
    );
    if (loadPersistedUpdateCheck()?.requestKey !== requestKey) {
      set({ updateInfo: null });
    }
    set({ isChecking: true, checkError: null });
    try {
      const result = await fetchUpgradeCheck({
        repository: upgradeConfig.repository,
        bundle: upgradeConfig.bundle,
        outbound: upgradeConfig.outbound || undefined,
        socks5: upgradeConfig.socks5 || undefined,
        githubToken: upgradeConfig.githubToken.trim() || undefined,
        allowPrerelease: upgradeConfig.allowPrerelease,
      });
      const updateInfo = {
        currentVersion,
        latestVersion: result.latest_version,
        updateAvailable: result.update_available,
        assetName: result.asset_name,
        releaseUrl: result.release_url,
      };
      const checkedAt = Date.now();
      set({
        updateInfo,
        lastCheckedAt: checkedAt,
        isChecking: false,
      });
      savePersistedUpdateCheck({
        requestKey,
        checkedAt,
        succeeded: true,
        updateInfo,
      });
    } catch (error) {
      const checkedAt = Date.now();
      set({
        checkError:
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.updateCheckFailed),
        isChecking: false,
        lastCheckedAt: checkedAt,
      });
      savePersistedUpdateCheck({
        requestKey,
        checkedAt,
        succeeded: false,
        updateInfo: get().updateInfo,
      });
    }
  },

  checkForUpdatesIfDue: async (currentVersion: string) => {
    const state = get();
    if (state.isChecking) return;
    const requestKey = createUpdateCheckRequestKey(
      currentVersion,
      state.upgradeConfig,
    );
    const previous = loadPersistedUpdateCheck();
    if (!isAutomaticUpdateCheckDue(previous, requestKey)) return;
    await get().checkForUpdates(currentVersion);
  },

  triggerUpgrade: async () => {
    const { upgradeConfig, updateInfo } = get();
    const targetVersion = updateInfo?.latestVersion ?? null;
    let baseline = createProcessInstanceBaseline();
    try {
      baseline = createProcessInstanceBaseline(await fetchHealth());
    } catch {
      // Upgrade completion can still be detected through a temporary outage or
      // a fresh uptime signature if the initial health probe is unavailable.
    }

    set({
      isApplying: true,
      applyPhase: "requesting",
      applyError: null,
      lastAppliedVersion: null,
    });
    try {
      await triggerUpgradeApply({
        repository: upgradeConfig.repository,
        bundle: upgradeConfig.bundle,
        outbound: upgradeConfig.outbound || undefined,
        socks5: upgradeConfig.socks5 || undefined,
        githubToken: upgradeConfig.githubToken.trim() || undefined,
        allowPrerelease: upgradeConfig.allowPrerelease,
        force: upgradeConfig.force,
        cleanup: upgradeConfig.cleanupAfterUpgrade,
      });
      const installedVersion = await pollUpgradeCompletion({
        baseline,
        targetVersion,
        onPhase: (phase) => set({ applyPhase: phase }),
      });
      await useAppStore.getState().refreshRuntimeState();
      set((state) => ({
        applyPhase: "completed",
        isApplying: false,
        lastAppliedVersion: installedVersion,
        updateInfo: state.updateInfo
          ? {
              ...state.updateInfo,
              currentVersion: installedVersion,
              latestVersion: installedVersion,
              updateAvailable: false,
            }
          : state.updateInfo,
      }));

      // The backend may have replaced the bundled WebUI assets too. Reloading
      // after a verified backend version keeps the console code in sync.
      await delay(1200);
      if (typeof window !== "undefined") window.location.reload();
    } catch (error) {
      set({
        applyError:
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.upgradeStartFailed),
        isApplying: false,
        applyPhase: null,
      });
    }
  },

  resetApplyState: () =>
    set({
      isApplying: false,
      applyPhase: null,
      applyError: null,
      lastAppliedVersion: null,
    }),
}));

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

class UpgradeApplyFailedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "UpgradeApplyFailedError";
  }
}

const UPGRADE_APPLY_TIMEOUT_MS = 10 * 60_000;
const UPGRADE_RECONNECT_TIMEOUT_MS = 2 * 60_000;

async function pollUpgradeCompletion({
  baseline,
  targetVersion,
  onPhase,
}: {
  baseline: ProcessInstanceBaseline;
  targetVersion: string | null;
  onPhase: (phase: UpgradeApplyPhase) => void;
}): Promise<string> {
  let sawDown = false;

  onPhase("applying");
  const applyDeadline = Date.now() + UPGRADE_APPLY_TIMEOUT_MS;
  while (Date.now() < applyDeadline) {
    await delay(1500);
    try {
      const status = await fetchUpgradeStatus();
      if (status.state === "failed") {
        throw new UpgradeApplyFailedError(
          status.error ?? tClient(WEBUI.storeErrors.upgradeFailed),
        );
      }
      if (status.state === "skipped" || status.state === "completed") {
        return status.installed_version ?? targetVersion ?? "";
      }
      if (status.state === "restarting") {
        break;
      }

      const health = await fetchHealth();
      if (
        targetVersion &&
        versionsEqual(health.version, targetVersion) &&
        processInstanceChanged(health, baseline)
      ) {
        return verifyUpgradeVersion(targetVersion, health.version, onPhase);
      }
    } catch (error) {
      if (error instanceof UpgradeApplyFailedError) {
        throw error;
      }
      sawDown = true;
      break;
    }
  }

  if (!sawDown && Date.now() >= applyDeadline) {
    throw new Error(tClient(WEBUI.storeErrors.upgradeRestartNotObserved));
  }

  onPhase("waiting_up");
  const reconnectDeadline = Date.now() + UPGRADE_RECONNECT_TIMEOUT_MS;
  while (Date.now() < reconnectDeadline) {
    await delay(1500);
    try {
      const health = await fetchHealth();
      const fresh =
        processInstanceChanged(health, baseline) ||
        (sawDown && !hasProcessIdentityBaseline(baseline));
      if (!fresh) continue;
      return verifyUpgradeVersion(targetVersion, health.version, onPhase);
    } catch {
      sawDown = true;
      // The service is still starting.
    }
  }

  throw new Error(tClient(WEBUI.storeErrors.upgradeRestartTimeout));
}

async function verifyUpgradeVersion(
  targetVersion: string | null,
  healthVersion: string,
  onPhase: (phase: UpgradeApplyPhase) => void,
): Promise<string> {
  onPhase("verifying");
  const verifyDeadline = Date.now() + 45_000;
  let lastVersion = healthVersion;

  while (Date.now() < verifyDeadline) {
    try {
      const [{ build }, health] = await Promise.all([
        fetchBuildInfo(),
        fetchHealth(),
      ]);
      lastVersion = build.version || health.version || lastVersion;
      if (!targetVersion || versionsEqual(lastVersion, targetVersion)) {
        return lastVersion;
      }
    } catch {
      // API routes may still be warming up immediately after process start.
    }
    await delay(1000);
  }

  throw new Error(
    tClient(WEBUI.storeErrors.upgradeVerifyTimeout, {
      version: targetVersion ?? lastVersion,
    }),
  );
}

function versionsEqual(left: string, right: string): boolean {
  return normalizeVersion(left) === normalizeVersion(right);
}

function normalizeVersion(version: string): string {
  return version.trim().replace(/^v/i, "");
}
