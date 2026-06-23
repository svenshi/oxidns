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
import { useAppStore } from "./store";

const STORAGE_KEY = "oxidns:upgrade-config";

export type UpgradeBundle = "auto" | "full" | "minimal" | "standard";

export interface UpgradeConfig {
  repository: string;
  bundle: UpgradeBundle;
  outbound: string;
  socks5: string;
  githubToken: string;
  persistGithubToken: boolean;
  allowPrerelease: boolean;
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
    ...(config.autoCheck !== undefined ? { autoCheck: config.autoCheck } : {}),
  };
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  upgradeConfig:
    typeof window !== "undefined"
      ? loadUpgradeConfig()
      : { ...DEFAULT_UPGRADE_CONFIG },
  updateInfo: null,
  isChecking: false,
  isApplying: false,
  applyPhase: null,
  lastCheckedAt: null,
  lastAppliedVersion: null,
  checkError: null,
  applyError: null,

  setUpgradeConfig: (partial) => {
    const next = { ...get().upgradeConfig, ...partial };
    saveUpgradeConfig(next);
    set({ upgradeConfig: next });
  },

  checkForUpdates: async (currentVersion: string) => {
    const { upgradeConfig } = get();
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
      set({
        updateInfo: {
          currentVersion,
          latestVersion: result.latest_version,
          updateAvailable: result.update_available,
          assetName: result.asset_name,
          releaseUrl: result.release_url,
        },
        lastCheckedAt: Date.now(),
        isChecking: false,
      });
    } catch (error) {
      set({
        checkError:
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.updateCheckFailed),
        isChecking: false,
        lastCheckedAt: Date.now(),
      });
    }
  },

  triggerUpgrade: async () => {
    const { upgradeConfig, updateInfo } = get();
    const targetVersion = updateInfo?.latestVersion ?? null;
    let baselineUptimeMs: number | undefined;
    try {
      baselineUptimeMs = (await fetchHealth()).uptime_ms;
    } catch {
      baselineUptimeMs = undefined;
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
      });
      const installedVersion = await pollUpgradeCompletion({
        baselineUptimeMs,
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
const FRESH_PROCESS_BUFFER_MS = 2_000;

async function pollUpgradeCompletion({
  baselineUptimeMs,
  targetVersion,
  onPhase,
}: {
  baselineUptimeMs?: number;
  targetVersion: string | null;
  onPhase: (phase: UpgradeApplyPhase) => void;
}): Promise<string> {
  const startTime = Date.now();
  let sawDown = false;

  onPhase("applying");
  const applyDeadline = startTime + UPGRADE_APPLY_TIMEOUT_MS;
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
        processLooksFresh(health.uptime_ms, baselineUptimeMs, startTime)
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
        sawDown ||
        processLooksFresh(health.uptime_ms, baselineUptimeMs, startTime);
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

function processLooksFresh(
  uptimeMs: number,
  baselineUptimeMs: number | undefined,
  startTime: number,
): boolean {
  return (
    (baselineUptimeMs !== undefined && uptimeMs < baselineUptimeMs) ||
    uptimeMs < Date.now() - startTime + FRESH_PROCESS_BUFFER_MS
  );
}

function versionsEqual(left: string, right: string): boolean {
  return normalizeVersion(left) === normalizeVersion(right);
}

function normalizeVersion(version: string): string {
  return version.trim().replace(/^v/i, "");
}
