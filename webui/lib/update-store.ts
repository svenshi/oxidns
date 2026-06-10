"use client";

import { create } from "zustand";
import { fetchUpgradeCheck, triggerUpgradeApply } from "./oxidns-api";

const STORAGE_KEY = "oxidns:upgrade-config";

export type UpgradeBundle = "auto" | "full" | "minimal" | "standard";

export interface UpgradeConfig {
  repository: string;
  bundle: UpgradeBundle;
  socks5: string;
  allowPrerelease: boolean;
  autoCheck: boolean;
}

export const DEFAULT_UPGRADE_CONFIG: UpgradeConfig = {
  repository: "svenshi/oxidns",
  bundle: "auto",
  socks5: "",
  allowPrerelease: false,
  autoCheck: true,
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
      return { ...DEFAULT_UPGRADE_CONFIG, ...(JSON.parse(stored) as Partial<UpgradeConfig>) };
    }
  } catch {
    // ignore
  }
  return { ...DEFAULT_UPGRADE_CONFIG };
}

function saveUpgradeConfig(config: UpgradeConfig): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
  } catch {
    // ignore
  }
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  upgradeConfig:
    typeof window !== "undefined" ? loadUpgradeConfig() : { ...DEFAULT_UPGRADE_CONFIG },
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
        socks5: upgradeConfig.socks5 || undefined,
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
        checkError: error instanceof Error ? error.message : "检查更新失败",
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
        socks5: upgradeConfig.socks5 || undefined,
        allowPrerelease: upgradeConfig.allowPrerelease,
      });
      // 服务端升级在后台运行，202 到达后服务即将重启。
      // 保持 isApplying=true 直到连接断开，由 resetApplyState 在断连时重置。
    } catch (error) {
      set({
        applyError: error instanceof Error ? error.message : "启动升级失败",
        isApplying: false,
      });
    }
  },

  resetApplyState: () => set({ isApplying: false, applyError: null }),
}));
