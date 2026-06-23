"use client";

import { create } from "zustand";
import { fetchUpgradeCheck, triggerUpgradeApply } from "./oxidns-api";
import { WEBUI, tClient } from "./i18n";

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

interface UpdateState {
  upgradeConfig: UpgradeConfig;
  updateInfo: UpdateInfo | null;
  isChecking: boolean;
  isApplying: boolean;
  lastCheckedAt: number | null;
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
  lastCheckedAt: null,
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
    const { upgradeConfig } = get();
    set({ isApplying: true, applyError: null });
    try {
      await triggerUpgradeApply({
        repository: upgradeConfig.repository,
        bundle: upgradeConfig.bundle,
        outbound: upgradeConfig.outbound || undefined,
        socks5: upgradeConfig.socks5 || undefined,
        githubToken: upgradeConfig.githubToken.trim() || undefined,
        allowPrerelease: upgradeConfig.allowPrerelease,
      });
      // The server-side upgrade runs in the background; after the 202 response,
      // the service is about to restart. Keep isApplying=true until the
      // connection drops and resetApplyState clears it.
    } catch (error) {
      set({
        applyError:
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.upgradeStartFailed),
        isApplying: false,
      });
    }
  },

  resetApplyState: () => set({ isApplying: false, applyError: null }),
}));
