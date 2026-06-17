"use client";

import { create } from "zustand";
import type { PluginInstance } from "./types";
import {
  configFromPlugins,
  createDefaultOxiDnsConfig,
  parseOxiDnsYaml,
  pluginsFromConfig,
  serializePluginsPreserving,
  stringifyOxiDnsConfig,
  topLevelConfigChanged,
  type OxiDnsConfig,
} from "./oxidns-config";
import {
  fetchBuildInfo,
  fetchConfigFile,
  fetchHealth,
  fetchMatcherStatus,
  fetchPrometheusMetrics,
  fetchReloadStatus,
  fetchSystem,
  fetchWebUiConfig,
  patchWebUiConfig,
  requestReload,
  requestRestart,
  reloadProvider as requestProviderReload,
  saveConfigFile,
  setMatcherMode as requestMatcherMode,
  validateConfigText,
  type BuildInfo,
  type ConfigFileResponse,
  type ConfigValidateResponse,
  type DependencyGraphReport,
  type HealthResponse,
  ProviderReloadBusyError,
  type JsonObject,
  type JsonValue,
  type ReloadSnapshot,
  type SystemResponse,
  type WebUiConfigResponse,
} from "./oxidns-api";
import {
  parsePrometheusMetrics,
  type OutboundMetricsMap,
  type PluginMetricsMap,
} from "./metrics";
import {
  calculateDnsTrafficMetrics,
  sumServerRequestTotal,
  type DnsTrafficMetrics,
  type RequestCounterSample,
} from "./dashboard-traffic";
import {
  getIncomingPluginReferences,
  getReplacementCandidates,
  removeSafePluginReferences,
  renamePluginConfigTag,
  replacePluginReferences,
  type PluginReferenceImpact,
} from "./plugin-reference-operations";
import {
  annotateApply,
  clearSnapshots,
  deleteSnapshot,
  getScopeKey,
  listSnapshots,
  recordSnapshot,
  type ConfigSnapshot,
  type ConfigSnapshotSource,
} from "./config-history";
import { createSnapshotId } from "./config-identity";
import { WEBUI, tClient } from "./i18n";
import { useAuthStore } from "./auth-store";
import {
  isReservedPluginTag,
  pluginTagValidationMessageKey,
  validatePluginTag,
} from "./plugin-tags";
import {
  createProcessInstanceBaseline,
  hasProcessIdentityBaseline,
  processInstanceChanged,
  type ProcessInstanceBaseline,
} from "./process-instance";
import {
  reconcileMatcherControls,
  type MatcherControlState,
  type MatcherRuntimeMode,
} from "./matcher-control";
import {
  reconcileProviderReloads,
  type ProviderReloadState,
} from "./provider-reload";
import {
  parseWebUiConfigHeader,
  stripWebUiConfigHeader,
  writeWebUiConfigHeader,
  type WebUiMode,
} from "./webui-config-header";
import { createDefaultStandardSettings } from "./standard-mode/defaults";
import { generateStandardConfig } from "./standard-mode/generator";
import {
  buildStandardGeneratedMetadata,
  computeStandardSettingsRevision,
  normalizeStandardSettings,
  type StandardSettingsNotice,
} from "./standard-mode/schema";
import type {
  StandardGeneratedMetadata,
  StandardModeSettings,
} from "./standard-mode/types";

type StoreSet = (
  partial: Partial<AppState> | ((state: AppState) => Partial<AppState>),
) => void;

type StoreGet = () => AppState;

type WebUiConfigDocument = JsonObject & {
  schema: number;
  mode: WebUiMode;
  standard: JsonObject;
  ui: JsonObject & {
    modeSelectionDismissed: boolean;
  };
};

export type RestartPhase =
  | "saving"
  | "requesting"
  | "waiting_down"
  | "waiting_up"
  | "reloading";

export type PluginDeletePreview =
  | {
      status: "ready";
      plugin: PluginInstance;
      references: PluginReferenceImpact[];
      canRemoveReferences: boolean;
      replacementCandidates: PluginInstance[];
    }
  | { status: "blocked"; message: string };

export type PluginRenameResult =
  | { status: "renamed" }
  | {
      status: "needs-confirmation";
      references: PluginReferenceImpact[];
    }
  | { status: "invalid"; message: string };

interface AppState {
  plugins: PluginInstance[];
  health: HealthResponse | null;
  buildInfo: BuildInfo | null;
  system: SystemResponse | null;
  reloadStatus: ReloadSnapshot | null;
  pluginMetrics: PluginMetricsMap;
  outboundMetrics: OutboundMetricsMap;
  trafficMetrics: DnsTrafficMetrics;
  dependencyGraph: DependencyGraphReport | null;
  runningDependencyGraph: DependencyGraphReport | null;
  matcherControls: Record<string, MatcherControlState>;
  providerReloads: Record<string, ProviderReloadState>;
  configDiagnostics: string[];
  configHistory: ConfigSnapshot[];
  webUiMode: WebUiMode;
  webUiConfig: WebUiConfigDocument;
  webUiConfigVersion: string | null;
  webUiConfigPath: string | null;
  webUiConfigError: string | null;
  modeHeaderPresent: boolean;
  modeSelectionDismissed: boolean;
  standardSettings: StandardModeSettings;
  standardSettingsNotice: StandardSettingsNotice;
  standardLastGenerated: StandardGeneratedMetadata | null;
  standardConfigOutOfSync: boolean;
  selectedPlugin: PluginInstance | null;
  detailOpen: boolean;
  editorMode: boolean;
  historyOpen: boolean;
  isConfigLoading: boolean;
  isConfigSaving: boolean;
  isWebUiConfigLoading: boolean;
  isWebUiConfigSaving: boolean;
  isApplying: boolean;
  isRestarting: boolean;
  /**
   * Current phase of an in-flight restart, surfaced by the blocking overlay.
   * `null` when no restart is in progress.
   */
  restartPhase: RestartPhase | null;
  configModel: OxiDnsConfig;
  configText: string;
  configVersion: string | null;
  /** Version the backend is actually running now (proxy: last loaded/applied). */
  runningVersion: string | null;
  configPath: string;
  configError: string | null;
  yamlConfig: string;
  /** Editing a pasted/uploaded config with no backend connection. */
  isOfflineMode: boolean;
  /** Name of the uploaded file, used as the export download name. */
  offlineFileName: string | null;

  setSelectedPlugin: (plugin: PluginInstance | null) => void;
  setDetailOpen: (open: boolean) => void;
  setEditorMode: (mode: boolean) => void;
  setHistoryOpen: (open: boolean) => void;
  setYamlConfig: (config: string) => void;
  enterOfflineConfig: (text: string, fileName?: string) => void;
  exitOfflineMode: () => void;
  loadConfig: () => Promise<void>;
  refreshHealthState: () => Promise<void>;
  refreshSystemState: () => Promise<void>;
  loadWebUiConfig: () => Promise<void>;
  refreshRuntimeState: () => Promise<void>;
  /** Fetch matcher bypass state once at explicit config/list refresh boundaries. */
  refreshMatcherStates: () => Promise<void>;
  refreshMetrics: () => Promise<void>;
  validateCurrentConfig: () => Promise<void>;
  setWebUiMode: (
    mode: WebUiMode,
    options?: { dismissSelection?: boolean },
  ) => void;
  dismissModeSelection: () => void;
  updateStandardSettings: (settings: StandardModeSettings) => void;
  saveStandardSettings: (settings?: StandardModeSettings) => Promise<void>;
  saveConfig: (options?: SaveConfigOptions) => Promise<void>;
  applyConfig: () => Promise<void>;
  restartApp: () => Promise<void>;
  restoreSnapshot: (id: string) => void;
  rollbackToSnapshot: (id: string) => Promise<void>;
  deleteConfigSnapshot: (id: string) => void;
  clearConfigHistory: () => void;
  togglePluginPin: (id: string) => void;
  setMatcherMode: (id: string, mode: MatcherRuntimeMode) => Promise<void>;
  reloadProvider: (id: string) => Promise<void>;
  clearProviderReloadResult: (id: string) => void;
  reorderPlugins: (orderedVisibleIds: string[]) => Promise<void>;
  updatePluginConfig: (id: string, config: Record<string, unknown>) => void;
  previewPluginDelete: (id: string) => Promise<PluginDeletePreview>;
  confirmDeletePlugin: (id: string) => Promise<void>;
  replaceAndDeletePlugin: (id: string, replacementTag: string) => Promise<void>;
  removeReferencesAndDeletePlugin: (id: string) => Promise<void>;
  enterEditorForPluginReferences: () => void;
  addPlugin: (
    plugin: Omit<PluginInstance, "id" | "createdAt" | "updatedAt" | "metrics">,
  ) => void;
  renamePlugin: (
    id: string,
    name: string,
    options?: { confirmed?: boolean },
  ) => Promise<PluginRenameResult>;
}

let queuedConfigSave: Promise<void> = Promise.resolve();
let pendingConfigSaveCount = 0;
interface ScopedRefresh {
  backendKey: string;
  promise: Promise<void>;
}

let metricsRefreshInFlight: ScopedRefresh | null = null;
let healthRefreshInFlight: ScopedRefresh | null = null;
let systemRefreshInFlight: ScopedRefresh | null = null;
let runtimeRefreshInFlight: ScopedRefresh | null = null;
let buildInfoBackendKey: string | null = null;
let requestRateBaseline: RequestCounterSample | null = null;
let requestRateBaselineBackendKey: string | null = null;
let matcherRefreshGeneration = 0;
let configLoadGeneration = 0;
let configValidationGeneration = 0;
let activeBackendKey: string | null = null;

function currentBackendKey(): string {
  const { connectionEpoch, serverConfig } = useAuthStore.getState();
  return `${connectionEpoch}\0${serverConfig.url.trim()}`;
}

function isCurrentBackend(backendKey: string): boolean {
  const auth = useAuthStore.getState();
  return auth.isConnected && currentBackendKey() === backendKey;
}
let queuedWebUiConfigSave: Promise<void> = Promise.resolve();
let pendingWebUiConfigSaveCount = 0;

interface SaveConfigOptions {
  source?: ConfigSnapshotSource;
}

function enqueueConfigSave(
  set: StoreSet,
  task: () => Promise<void>,
): Promise<void> {
  pendingConfigSaveCount += 1;
  set({ isConfigSaving: true });

  const run = () => task();
  const current = queuedConfigSave.then(run, run);
  queuedConfigSave = current.catch(() => {});

  return current.finally(() => {
    pendingConfigSaveCount -= 1;
    if (pendingConfigSaveCount === 0) set({ isConfigSaving: false });
  });
}

function enqueueWebUiConfigSave(
  set: StoreSet,
  task: () => Promise<void>,
): Promise<void> {
  pendingWebUiConfigSaveCount += 1;
  set({ isWebUiConfigSaving: true });

  const run = () => task();
  const current = queuedWebUiConfigSave.then(run, run);
  queuedWebUiConfigSave = current.catch(() => {});

  return current.finally(() => {
    pendingWebUiConfigSaveCount -= 1;
    if (pendingWebUiConfigSaveCount === 0) {
      set({ isWebUiConfigSaving: false });
    }
  });
}

const initialConfigModel = createDefaultOxiDnsConfig();
const initialConfigText = stringifyOxiDnsConfig(initialConfigModel);
const initialWebUiConfig = createDefaultWebUiConfig();
export const useAppStore = create<AppState>((set, get) => ({
  plugins: [],
  health: null,
  buildInfo: null,
  system: null,
  reloadStatus: null,
  pluginMetrics: {},
  outboundMetrics: {},
  trafficMetrics: {
    status: "pending",
    qps: null,
    requestTotal: 0,
    sampleWindowSeconds: null,
  },
  dependencyGraph: null,
  runningDependencyGraph: null,
  matcherControls: {},
  providerReloads: {},
  configDiagnostics: [],
  configHistory: [],
  webUiMode: "expert",
  webUiConfig: initialWebUiConfig,
  webUiConfigVersion: null,
  webUiConfigPath: null,
  webUiConfigError: null,
  modeHeaderPresent: false,
  modeSelectionDismissed: false,
  standardSettings: createDefaultStandardSettings(),
  standardSettingsNotice: null,
  standardLastGenerated: null,
  standardConfigOutOfSync: false,
  selectedPlugin: null,
  detailOpen: false,
  editorMode: false,
  historyOpen: false,
  isConfigLoading: false,
  isConfigSaving: false,
  isWebUiConfigLoading: false,
  isWebUiConfigSaving: false,
  isApplying: false,
  isRestarting: false,
  restartPhase: null,
  configModel: initialConfigModel,
  configText: initialConfigText,
  configVersion: null,
  runningVersion: null,
  configPath: "/etc/oxidns/config.yaml",
  configError: null,
  yamlConfig: initialConfigText,
  isOfflineMode: false,
  offlineFileName: null,

  setSelectedPlugin: (plugin) => set({ selectedPlugin: plugin }),
  setDetailOpen: (open) => set({ detailOpen: open }),
  setEditorMode: (mode) =>
    set((state) => ({
      editorMode: mode && state.webUiMode === "standard" ? false : mode,
    })),
  setHistoryOpen: (open) => set({ historyOpen: open }),
  setWebUiMode: (mode, options) => {
    const modeSelectionDismissed = options?.dismissSelection ?? true;
    const patch: JsonObject = {
      mode,
      ui: { modeSelectionDismissed },
    };
    set((state) => ({
      webUiMode: mode,
      webUiConfig: applyWebUiConfigPatch(state.webUiConfig, patch),
      editorMode: mode === "standard" ? false : state.editorMode,
      modeSelectionDismissed,
      standardConfigOutOfSync:
        mode === "standard"
          ? isStandardConfigOutOfSync(state.standardLastGenerated, state.configVersion)
          : false,
    }));
    if (!get().isOfflineMode) {
      void persistWebUiConfigPatch(set, get, patch).catch(() => {});
    }
  },
  dismissModeSelection: () => {
    const patch: JsonObject = { ui: { modeSelectionDismissed: true } };
    set((state) => ({
      modeSelectionDismissed: true,
      webUiConfig: applyWebUiConfigPatch(state.webUiConfig, patch),
    }));
    if (!get().isOfflineMode) {
      void persistWebUiConfigPatch(set, get, patch).catch(() => {});
    }
  },
  updateStandardSettings: (settings) => set({ standardSettings: settings }),
  saveStandardSettings: async (settings) => {
    const state = get();
    const nextSettings = settings ?? state.standardSettings;
    const settingsRevision = computeStandardSettingsRevision(nextSettings);
    const settingsPatch: JsonObject = {
      mode: "standard",
      standard: {
        settings: nextSettings as unknown as JsonValue,
        meta: {
          settingsRevision,
        },
      },
      ui: { modeSelectionDismissed: true },
    };
    set((current) => ({
      webUiMode: "standard",
      webUiConfig: applyWebUiConfigPatch(current.webUiConfig, settingsPatch),
      modeSelectionDismissed: true,
      standardSettings: nextSettings,
      standardSettingsNotice: null,
      standardConfigOutOfSync: false,
      editorMode: false,
    }));
    if (!state.isOfflineMode) {
      await persistWebUiConfigPatch(set, get, settingsPatch);
    }

    const generated = generateStandardConfig(
      nextSettings,
      state.buildInfo,
      state.configModel,
    );
    const generatedText = stringifyOxiDnsConfig(generated.config);
    const content = writeWebUiConfigHeader(generatedText, {
      mode: "standard",
      modeHeaderPresent: true,
    });
    get().setYamlConfig(content);
    set({
      webUiMode: "standard",
      modeHeaderPresent: true,
      modeSelectionDismissed: true,
      standardSettings: nextSettings,
      standardSettingsNotice: null,
      standardConfigOutOfSync: false,
      editorMode: false,
    });
    await get().saveConfig({
      source: "standard-settings",
    });
    if (!get().isOfflineMode) {
      const metadata = buildStandardGeneratedMetadata(
        nextSettings,
        generated,
        get().configVersion,
      );
      const metadataPatch: JsonObject = {
        standard: {
          meta: {
            settingsRevision,
            lastGenerated: metadata as unknown as JsonValue,
          },
        },
      };
      set((current) => ({
        webUiConfig: applyWebUiConfigPatch(current.webUiConfig, metadataPatch),
        standardLastGenerated: metadata,
        standardConfigOutOfSync: false,
      }));
      await persistWebUiConfigPatch(set, get, metadataPatch);
    }
  },
  setYamlConfig: (config) => {
    const parsed = parseOxiDnsYaml(config);
    const headerState = headerStateFromText(config);
    if (!parsed.config) {
      set({
        configText: config,
        yamlConfig: config,
        ...headerState,
        configError:
          parsed.diagnostics[0] ?? tClient(WEBUI.storeErrors.configParseFailed),
        configDiagnostics: parsed.diagnostics,
      });
      return;
    }

    const plugins = restorePinnedState(pluginsFromConfig(parsed.config));
    set({
      configModel: parsed.config,
      configText: config,
      yamlConfig: config,
      ...headerState,
      plugins,
      matcherControls: reconcileMatcherControls(plugins, get().matcherControls),
      providerReloads: reconcileProviderReloads(plugins, get().providerReloads),
      selectedPlugin: syncSelectedPlugin(get().selectedPlugin, plugins),
      configError: parsed.diagnostics[0] ?? null,
      configDiagnostics: parsed.diagnostics,
    });
  },

  // Import a pasted/uploaded config for editing without a backend. Resets
  // every backend-derived field first so stale dependency graphs, history,
  // and (critically) configVersion can't leak in — a stale configVersion
  // would corrupt the editor's dirty/reset baseline. setYamlConfig runs the
  // existing client-side parse path; its set() payload omits the offline
  // keys so the flags below survive.
  enterOfflineConfig: (text, fileName) => {
    set({
      isOfflineMode: true,
      offlineFileName: fileName ?? null,
      configPath: fileName ?? tClient(WEBUI.storeErrors.unnamedOfflineConfig),
      configVersion: null,
      runningVersion: null,
      dependencyGraph: null,
      runningDependencyGraph: null,
      matcherControls: {},
      providerReloads: {},
      configHistory: [],
      reloadStatus: null,
      health: null,
      buildInfo: null,
      system: null,
      webUiMode: "expert",
      webUiConfig: createDefaultWebUiConfig(),
      webUiConfigVersion: null,
      webUiConfigPath: null,
      webUiConfigError: null,
      modeHeaderPresent: false,
      modeSelectionDismissed: false,
      standardSettings: createDefaultStandardSettings(),
      standardSettingsNotice: null,
      standardLastGenerated: null,
      standardConfigOutOfSync: false,
    });
    get().setYamlConfig(text);
  },

  // Leave offline mode. When still disconnected this returns the user to the
  // import screen; on reconnect the layout's loadConfig() authoritatively
  // repopulates config state, so no manual backend restore is needed here.
  exitOfflineMode: () => set({ isOfflineMode: false, offlineFileName: null }),

  loadConfig: async () => {
    const backendKey = currentBackendKey();
    const generation = ++configLoadGeneration;
    if (activeBackendKey !== backendKey) {
      activeBackendKey = backendKey;
      buildInfoBackendKey = null;
      requestRateBaseline = null;
      requestRateBaselineBackendKey = null;
      set({
        health: null,
        system: null,
        buildInfo: null,
        reloadStatus: null,
        pluginMetrics: {},
        outboundMetrics: {},
        trafficMetrics: {
          status: "pending",
          qps: null,
          requestTotal: 0,
          sampleWindowSeconds: null,
        },
        matcherControls: {},
        providerReloads: {},
      });
    }
    set({
      isConfigLoading: true,
      isWebUiConfigLoading: true,
      configError: null,
      runningDependencyGraph: null,
      webUiConfigError: null,
    });
    try {
      const [configResult, webUiConfigResult] = await Promise.allSettled([
        fetchConfigFile(),
        fetchWebUiConfig(),
      ]);
      if (
        generation !== configLoadGeneration ||
        !isCurrentBackend(backendKey)
      ) {
        return;
      }
      if (webUiConfigResult.status === "fulfilled") {
        applyWebUiConfigResponse(webUiConfigResult.value, set);
      } else {
        set({
          webUiConfigError:
            webUiConfigResult.reason instanceof Error
              ? webUiConfigResult.reason.message
              : tClient(WEBUI.storeErrors.readConfigFailed),
        });
      }
      if (configResult.status === "rejected") {
        throw configResult.reason;
      }
      const response = configResult.value;
      applyConfigFileResponse(response, set);
      const header = parseWebUiConfigHeader(response.content);
      const scope = getScopeKey(response.path);
      recordSnapshot(scope, {
        content: response.content,
        version: response.version,
        source: "server",
        pluginCount: pluginCountOf(response.content),
        applyStatus: "applied",
        mode: header.mode,
      });
      // The backend is running exactly what it just served us from disk.
      set({
        configHistory: listSnapshots(scope),
        runningVersion: response.version,
      });
      await get().validateCurrentConfig();
      if (
        generation !== configLoadGeneration ||
        !isCurrentBackend(backendKey)
      ) {
        return;
      }
      set({ runningDependencyGraph: get().dependencyGraph });
      await get().refreshRuntimeState();
      if (
        generation !== configLoadGeneration ||
        !isCurrentBackend(backendKey)
      ) {
        return;
      }
      // Matcher bypass state is intentionally sampled with the plugin list.
      // It is not polled; later manual apply/reload actions sample it again.
      await get().refreshMatcherStates();
    } catch (error) {
      if (
        generation !== configLoadGeneration ||
        !isCurrentBackend(backendKey)
      ) {
        return;
      }
      set({
        configError:
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.readConfigFailed),
      });
    } finally {
      if (generation === configLoadGeneration) {
        set({ isConfigLoading: false, isWebUiConfigLoading: false });
      }
    }
  },

  loadWebUiConfig: async () => {
    const backendKey = currentBackendKey();
    set({ isWebUiConfigLoading: true, webUiConfigError: null });
    try {
      const response = await fetchWebUiConfig();
      if (!isCurrentBackend(backendKey)) return;
      applyWebUiConfigResponse(response, set);
    } catch (error) {
      if (!isCurrentBackend(backendKey)) return;
      set({
        webUiConfigError:
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.readConfigFailed),
      });
    } finally {
      if (isCurrentBackend(backendKey)) {
        set({ isWebUiConfigLoading: false });
      }
    }
  },

  refreshHealthState: () => {
    const backendKey = currentBackendKey();
    if (healthRefreshInFlight?.backendKey === backendKey) {
      return healthRefreshInFlight.promise;
    }

    const refresh = fetchHealth().then((health) => {
      if (isCurrentBackend(backendKey)) set({ health });
    });
    const entry: ScopedRefresh = { backendKey, promise: refresh };
    entry.promise = refresh.finally(() => {
      if (healthRefreshInFlight === entry) healthRefreshInFlight = null;
    });
    healthRefreshInFlight = entry;
    return entry.promise;
  },

  refreshSystemState: () => {
    const backendKey = currentBackendKey();
    if (systemRefreshInFlight?.backendKey === backendKey) {
      return systemRefreshInFlight.promise;
    }

    const refresh = fetchSystem().then((system) => {
      if (!isCurrentBackend(backendKey)) return;
      const current = get();
      const nextReload = system.reload ?? current.reloadStatus;
      const nextRunningVersion =
        nextReload?.running_version ?? current.runningVersion;
      const runningDependencyGraph =
        nextRunningVersion === current.runningVersion
          ? current.runningDependencyGraph
          : nextRunningVersion === current.configVersion
            ? current.dependencyGraph
            : null;
      set({
        system,
        buildInfo:
          system.build ??
          (buildInfoBackendKey === backendKey ? current.buildInfo : null),
        reloadStatus: nextReload,
        // The backend authoritatively reports what config it is running; prefer
        // it over the load-time disk-version guess so the "not applied" state
        // survives page reloads. Falls back to the prior value for older
        // backends that don't report running_version.
        ...(nextReload?.running_version
          ? { runningVersion: nextReload.running_version }
          : {}),
        runningDependencyGraph,
      });
      if (system.build) buildInfoBackendKey = backendKey;
    });
    const entry: ScopedRefresh = { backendKey, promise: refresh };
    entry.promise = refresh.finally(() => {
      if (systemRefreshInFlight === entry) systemRefreshInFlight = null;
    });
    systemRefreshInFlight = entry;
    return entry.promise;
  },

  refreshRuntimeState: () => {
    const backendKey = currentBackendKey();
    if (runtimeRefreshInFlight?.backendKey === backendKey) {
      return runtimeRefreshInFlight.promise;
    }

    const refresh = (async () => {
      await Promise.allSettled([
        get().refreshHealthState(),
        get().refreshSystemState(),
      ]);

      if (!isCurrentBackend(backendKey)) return;

      // Current backends include build capabilities in /system. Fetch the
      // fallback once per backend connection when that field is absent.
      if (buildInfoBackendKey !== backendKey) {
        try {
          const response = await fetchBuildInfo();
          if (!isCurrentBackend(backendKey)) return;
          buildInfoBackendKey = backendKey;
          set({ buildInfo: response.build });
        } catch {
          // Runtime health/system data remains useful without build metadata.
        }
      }
    })();
    const entry: ScopedRefresh = { backendKey, promise: refresh };
    entry.promise = refresh.finally(() => {
      if (runtimeRefreshInFlight === entry) runtimeRefreshInFlight = null;
    });
    runtimeRefreshInFlight = entry;
    return entry.promise;
  },

  refreshMatcherStates: async () => {
    const generation = ++matcherRefreshGeneration;
    if (get().isOfflineMode) {
      set({ matcherControls: {} });
      return;
    }
    const matchers = get().plugins.filter(
      (plugin) => plugin.type === "matcher",
    );
    const matcherKey = matchers.map((plugin) => plugin.name).join("\0");
    if (matchers.length === 0) {
      set({ matcherControls: {} });
      return;
    }

    set((state) => ({
      matcherControls: Object.fromEntries(
        matchers.map((plugin) => [
          plugin.name,
          {
            availability: "loading" as const,
            pending: false,
            mode: state.matcherControls[plugin.name]?.mode ?? null,
            ...(state.matcherControls[plugin.name]?.error
              ? { error: state.matcherControls[plugin.name].error }
              : {}),
          },
        ]),
      ),
    }));

    const results = await Promise.allSettled(
      matchers.map(async (plugin) => ({
        tag: plugin.name,
        response: await fetchMatcherStatus(plugin.name),
      })),
    );
    if (generation !== matcherRefreshGeneration) return;

    set((state) => {
      if (state.isOfflineMode) return {};
      const currentMatcherKey = state.plugins
        .filter((plugin) => plugin.type === "matcher")
        .map((plugin) => plugin.name)
        .join("\0");
      if (currentMatcherKey !== matcherKey) return {};

      const matcherControls: Record<string, MatcherControlState> = {};
      results.forEach((result, index) => {
        const tag = matchers[index].name;
        if (result.status === "fulfilled") {
          matcherControls[tag] = {
            availability: "ready",
            pending: false,
            mode: result.value.response.mode,
          };
        } else {
          matcherControls[tag] = {
            availability: "unavailable",
            pending: false,
            mode: state.matcherControls[tag]?.mode ?? null,
            error:
              result.reason instanceof Error
                ? result.reason.message
                : tClient(WEBUI.plugins.matcherControlUnavailable),
          };
        }
      });
      return { matcherControls };
    });
  },

  refreshMetrics: () => {
    const backendKey = currentBackendKey();
    if (metricsRefreshInFlight?.backendKey === backendKey) {
      return metricsRefreshInFlight.promise;
    }

    const refresh = (async () => {
      try {
        const text = await fetchPrometheusMetrics();
        if (!isCurrentBackend(backendKey)) return;
        const metrics = parsePrometheusMetrics(text);
        const currentSample = {
          requestTotal: sumServerRequestTotal(metrics.byTag),
          sampledAtMs: Date.now(),
        };
        const trafficMetrics = calculateDnsTrafficMetrics(
          requestRateBaselineBackendKey === backendKey
            ? requestRateBaseline
            : null,
          currentSample,
        );
        requestRateBaseline = currentSample;
        requestRateBaselineBackendKey = backendKey;
        set({
          pluginMetrics: metrics.byTag,
          outboundMetrics: metrics.outbound,
          trafficMetrics,
        });
      } catch {
        if (!isCurrentBackend(backendKey)) return;
        // Do not keep showing a stale rate after a metrics fetch failure.
        // The next successful response establishes a fresh baseline.
        requestRateBaseline = null;
        requestRateBaselineBackendKey = backendKey;
        set((state) => ({
          trafficMetrics: {
            ...state.trafficMetrics,
            status: "unavailable",
            qps: null,
            sampleWindowSeconds: null,
          },
        }));
      }
    })();
    const entry: ScopedRefresh = { backendKey, promise: refresh };
    entry.promise = refresh.finally(() => {
      if (metricsRefreshInFlight === entry) metricsRefreshInFlight = null;
    });
    metricsRefreshInFlight = entry;
    return entry.promise;
  },

  validateCurrentConfig: async () => {
    const generation = ++configValidationGeneration;
    const state = get();
    if (state.configError) return;
    try {
      const response = await validateConfigText(state.configText);
      if (generation !== configValidationGeneration) return;
      applyConfigValidationResponse(response, set);
    } catch (error) {
      if (generation !== configValidationGeneration) return;
      const message =
        error instanceof Error
          ? error.message
          : tClient(WEBUI.configEditor.configValidationFailed);
      set({
        configError: message,
        configDiagnostics: [message],
        dependencyGraph: null,
      });
      throw error;
    }
  },

  // Save only. Hot-reload is a separate explicit step (applyConfig) so the
  // disk write and the running-config swap are never coupled.
  saveConfig: (options) =>
    enqueueConfigSave(set, async () => {
      const backendKey = currentBackendKey();
      const state = get();
      if (state.configError) throw new Error(state.configError);

      set({ configError: null });
      try {
        const snapshotId = createSnapshotId();
        const prepared = prepareConfigTextForSave(state);
        if (!isCurrentBackend(backendKey)) return;
        const validation = await validateConfigText(prepared.content);
        if (!isCurrentBackend(backendKey)) return;
        applyConfigValidationResponse(validation, set);
        const content = prepared.content;
        if (!isCurrentBackend(backendKey)) return;
        const response = await saveConfigFile({
          content,
          baseVersion: state.configVersion,
          validate: true,
          reload: false,
        });
        if (!isCurrentBackend(backendKey)) return;
        const scope = getScopeKey(response.path);
        recordSnapshot(scope, {
          id: snapshotId,
          content,
          version: response.version,
          source: options?.source ?? "save",
          pluginCount: pluginCountOf(content),
          applyStatus: "not-applied",
          mode: prepared.header.mode,
        });
        const nextHeaderState = headerStateFromText(content);
        set({
          configText: content,
          yamlConfig: content,
          configVersion: response.version,
          configPath: response.path,
          reloadStatus: response.reload ?? get().reloadStatus,
          configHistory: listSnapshots(scope),
          ...nextHeaderState,
          standardConfigOutOfSync: isStandardConfigOutOfSync(
            get().standardLastGenerated,
            response.version,
          ),
        });
        if (!isCurrentBackend(backendKey)) return;
        await get().refreshRuntimeState();
      } catch (error) {
        if (!isCurrentBackend(backendKey)) return;
        const message =
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.saveConfigFailed);
        set({ configError: message });
        throw error;
      }
    }),

  // Trigger a backend hot-reload of the on-disk config and wait for the
  // outcome. The backend already rolls the running pipeline back to the
  // previous in-memory config if assembly fails (src/app.rs handle_reload),
  // so a failed apply leaves the service running on the old config; we only
  // surface that state and annotate the snapshot.
  applyConfig: async () => {
    const before = get();
    const scope = getScopeKey(before.configPath);
    const version = before.configVersion;
    set({ isApplying: true });
    try {
      let baseline: number | undefined;
      try {
        baseline = (await fetchReloadStatus()).last_completed_ms;
      } catch {
        baseline = undefined;
      }

      let snapshot: ReloadSnapshot;
      try {
        await requestReload();
        snapshot = await pollReload(baseline);
      } catch (error) {
        // requestReload / polling threw (reload busy, network, API torn down
        // and never recovered) — surface it as a failed apply instead of a
        // silent no-op so the pill turns red rather than staying unchanged.
        const message =
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.hotReloadTriggerFailed);
        if (version) {
          annotateApply(scope, version, "apply-failed", message);
          set({ configHistory: listSnapshots(scope) });
        }
        throw new Error(message);
      }

      set({ reloadStatus: snapshot });
      const failed =
        snapshot.status === "failed" || Boolean(snapshot.last_error);
      if (version) {
        annotateApply(
          scope,
          version,
          failed ? "apply-failed" : "applied",
          snapshot.last_error,
        );
        set({
          configHistory: listSnapshots(scope),
          // On success the backend is now running this config. Prefer the
          // authoritative version it reports; fall back to the applied one.
          ...(failed
            ? {}
            : {
                runningVersion: snapshot.running_version ?? version,
                runningDependencyGraph: get().dependencyGraph,
              }),
        });
      }
      await get().refreshRuntimeState();
      // Applying the configuration is an explicit manual refresh boundary.
      await get().refreshMatcherStates();
      if (failed) {
        throw new Error(
          snapshot.last_error ||
            tClient(WEBUI.storeErrors.hotReloadNotSuccessful),
        );
      }
    } finally {
      set({ isApplying: false });
    }
  },

  // Save the current config to disk and restart the server process. After the
  // restart request is accepted the client polls the health endpoint until a
  // fresh backend instance is observed, then reloads the config from it.
  restartApp: async () => {
    set({ isRestarting: true, restartPhase: "saving" });
    let savedVersion: string | null = null;
    try {
      await get().saveConfig();
      savedVersion = get().configVersion;
      let baseline = createProcessInstanceBaseline();
      try {
        baseline = createProcessInstanceBaseline(await fetchHealth());
      } catch {
        // Health probe failures here are fine; pollReconnect can still use
        // an observed outage or fresh uptime signature as fallback evidence.
      }
      set({ restartPhase: "requesting" });
      await requestRestart();
      await pollReconnect(baseline, (phase) => set({ restartPhase: phase }));
      set({ restartPhase: "reloading" });
      await get().loadConfig();
    } catch (error) {
      if (savedVersion) {
        const scope = getScopeKey(get().configPath);
        annotateApply(
          scope,
          savedVersion,
          "apply-failed",
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.restartFailed),
        );
        set({ configHistory: listSnapshots(scope) });
      }
      throw error;
    } finally {
      set({ isRestarting: false, restartPhase: null });
    }
  },

  // Load a historical snapshot back into the editor only. It is NOT persisted
  // or applied; the operator still goes through Save -> Apply, so a rollback
  // also produces its own history entry.
  restoreSnapshot: (id) => {
    const entry = get().configHistory.find((s) => s.id === id);
    if (!entry) return;
    get().setYamlConfig(entry.content);
  },

  // One-click rollback usable in BOTH console and editor mode: load the
  // snapshot, persist it to disk, then choose hot-reload or full restart based
  // on whether the rollback touches restart-only top-level fields.
  rollbackToSnapshot: async (id) => {
    const entry = get().configHistory.find((s) => s.id === id);
    if (!entry) return;
    const running = get().configHistory.find(
      (s) => s.version === get().runningVersion,
    );
    const requiresRestart = Boolean(
      running && topLevelConfigChanged(entry.content, running.content),
    );
    get().setYamlConfig(entry.content);
    await get().saveConfig({ source: "rollback" });
    if (requiresRestart) {
      await get().restartApp();
    } else {
      await get().applyConfig();
    }
  },

  deleteConfigSnapshot: (id) => {
    const scope = getScopeKey(get().configPath);
    deleteSnapshot(scope, id);
    set({ configHistory: listSnapshots(scope) });
  },

  clearConfigHistory: () => {
    const scope = getScopeKey(get().configPath);
    clearSnapshots(scope);
    set({ configHistory: [] });
  },

  togglePluginPin: (id) =>
    set((state) => {
      const plugins = state.plugins.map((p) =>
        p.id === id ? { ...p, pinned: !p.pinned } : p,
      );
      savePinnedIds(new Set(plugins.filter((p) => p.pinned).map((p) => p.id)));
      return {
        plugins,
        selectedPlugin: syncSelectedPlugin(state.selectedPlugin, plugins),
      };
    }),

  setMatcherMode: async (id, mode) => {
    const plugin = get().plugins.find((candidate) => candidate.id === id);
    if (!plugin || plugin.type !== "matcher") return;
    const control = get().matcherControls[plugin.name];
    if (
      control?.availability !== "ready" ||
      control.mode === null ||
      control.pending
    )
      return;

    set((state) => ({
      matcherControls: {
        ...state.matcherControls,
        [plugin.name]: {
          availability: "ready",
          pending: true,
          mode: control.mode,
        },
      },
    }));

    try {
      const response = await requestMatcherMode(plugin.name, mode);
      set((state) => ({
        matcherControls: {
          ...state.matcherControls,
          [plugin.name]: {
            availability: "ready",
            pending: false,
            mode: response.mode,
          },
        },
      }));
    } catch (error) {
      const message =
        error instanceof Error
          ? error.message
          : tClient(WEBUI.plugins.matcherControlFailed);
      set((state) => ({
        matcherControls: {
          ...state.matcherControls,
          [plugin.name]: {
            availability: "ready",
            pending: false,
            mode: state.matcherControls[plugin.name]?.mode ?? null,
            error: message,
          },
        },
      }));
      throw error;
    }
  },

  reloadProvider: async (id) => {
    const plugin = get().plugins.find((candidate) => candidate.id === id);
    if (!plugin || plugin.type !== "provider") return;
    const current = get().providerReloads[plugin.name];
    if (current?.pending) return;

    set((state) => ({
      providerReloads: {
        ...state.providerReloads,
        [plugin.name]: {
          pending: true,
          outcome: "idle",
        },
      },
    }));

    try {
      await requestProviderReload(plugin.name);
      set((state) => {
        if (
          !state.plugins.some(
            (candidate) =>
              candidate.type === "provider" && candidate.name === plugin.name,
          )
        )
          return {};
        return {
          providerReloads: {
            ...state.providerReloads,
            [plugin.name]: {
              pending: false,
              outcome: "success",
            },
          },
        };
      });
    } catch (error) {
      const message =
        error instanceof ProviderReloadBusyError
          ? tClient(WEBUI.plugins.providerReloadBusy)
          : error instanceof Error
            ? error.message
            : tClient(WEBUI.plugins.providerReloadFailed);
      set((state) => {
        if (
          !state.plugins.some(
            (candidate) =>
              candidate.type === "provider" && candidate.name === plugin.name,
          )
        )
          return {};
        return {
          providerReloads: {
            ...state.providerReloads,
            [plugin.name]: {
              pending: false,
              outcome: "error",
              error: message,
            },
          },
        };
      });
      throw error;
    }
  },

  clearProviderReloadResult: (id) => {
    const plugin = get().plugins.find((candidate) => candidate.id === id);
    if (!plugin || plugin.type !== "provider") return;
    set((state) => {
      const current = state.providerReloads[plugin.name];
      if (!current || current.pending || current.outcome === "idle") return {};
      return {
        providerReloads: {
          ...state.providerReloads,
          [plugin.name]: {
            pending: false,
            outcome: "idle",
          },
        },
      };
    });
  },

  // Reorder plugins in the config file to match a drag-and-drop arrangement.
  // `orderedVisibleIds` is the new order of the *currently visible* cards
  // (a single type tab, or all of them). Plugins outside that visible subset
  // keep their absolute positions; only the slots the visible plugins occupy
  // are refilled in the new order, so reordering within one type tab never
  // disturbs the relative position of other types. The change is staged into
  // the editor buffer and persisted to disk (mirroring add/edit/delete), then
  // surfaced as an "apply changes" pill for the operator to hot-reload.
  reorderPlugins: async (orderedVisibleIds) => {
    const state = get();
    if (state.configError) return;

    const visible = new Set(orderedVisibleIds);
    const byId = new Map(state.plugins.map((p) => [p.id, p] as const));
    const queue = orderedVisibleIds
      .map((id) => byId.get(id))
      .filter((p): p is PluginInstance => Boolean(p));
    if (queue.length === 0) return;

    let next = 0;
    const reordered = state.plugins.map((p) =>
      visible.has(p.id) ? queue[next++] : p,
    );
    const unchanged = reordered.every((p, i) => p.id === state.plugins[i].id);
    if (unchanged) return;

    // No tags are passed as changed: every plugin reuses its original YAML
    // node verbatim (comments/blank lines preserved) — only the node order
    // changes.
    set(syncPluginsToConfig(state, () => reordered, []));
    if (!get().isOfflineMode) await get().saveConfig();
  },

  updatePluginConfig: (id, config) =>
    set((state) => {
      const tag = state.plugins.find((p) => p.id === id)?.name;
      return syncPluginsToConfig(
        state,
        (plugins) =>
          plugins.map((p) =>
            p.id === id
              ? { ...p, config, updatedAt: new Date().toISOString() }
              : p,
          ),
        tag ? [tag] : [],
      );
    }),

  previewPluginDelete: async (id) => {
    const state = get();
    if (state.configError) {
      return {
        status: "blocked",
        message: tClient(WEBUI.storeErrors.configHasErrorsBeforeDelete),
      };
    }
    const plugin = state.plugins.find((p) => p.id === id);
    if (!plugin) {
      return {
        status: "blocked",
        message: tClient(WEBUI.storeErrors.pluginMissing),
      };
    }

    await get().validateCurrentConfig();
    const latest = get();
    const references = incomingReferences(latest, plugin.name);
    return {
      status: "ready",
      plugin,
      references,
      canRemoveReferences:
        references.length > 0 && references.every((edge) => edge.removable),
      replacementCandidates: replacementCandidates(latest, plugin, references),
    };
  },

  confirmDeletePlugin: async (id) => {
    await get().validateCurrentConfig();
    const state = get();
    const plugin = state.plugins.find((p) => p.id === id);
    if (!plugin) throw new Error(tClient(WEBUI.storeErrors.pluginMissing));
    const references = incomingReferences(state, plugin.name);
    if (references.length > 0) {
      throw new Error(tClient(WEBUI.storeErrors.pluginStillReferenced));
    }
    set((current) => deletePluginFromState(current, id));
    await get().saveConfig();
  },

  replaceAndDeletePlugin: async (id, replacementTag) => {
    await get().validateCurrentConfig();
    const state = get();
    const plugin = state.plugins.find((p) => p.id === id);
    const replacement = state.plugins.find((p) => p.name === replacementTag);
    if (!plugin) throw new Error(tClient(WEBUI.storeErrors.pluginMissing));
    if (!replacement)
      throw new Error(tClient(WEBUI.storeErrors.replacementMissing));
    const references = incomingReferences(state, plugin.name);
    if (
      !replacementCandidates(state, plugin, references).some(
        (candidate) => candidate.name === replacementTag,
      )
    ) {
      throw new Error(tClient(WEBUI.storeErrors.replacementIncompatible));
    }

    const replaced = replacePluginReferences(
      state.configModel,
      references,
      plugin.name,
      replacementTag,
    );
    set((current) => {
      const applied = applyConfigModelToState(current, replaced.config, [
        ...replaced.changedTags,
        plugin.name,
      ]);
      return deletePluginFromState({ ...current, ...applied }, id);
    });
    await get().saveConfig();
  },

  removeReferencesAndDeletePlugin: async (id) => {
    await get().validateCurrentConfig();
    const state = get();
    const plugin = state.plugins.find((p) => p.id === id);
    if (!plugin) throw new Error(tClient(WEBUI.storeErrors.pluginMissing));
    const references = incomingReferences(state, plugin.name);
    if (references.length === 0) {
      set((current) => deletePluginFromState(current, id));
      await get().saveConfig();
      return;
    }
    if (!references.every((edge) => edge.removable)) {
      throw new Error(tClient(WEBUI.storeErrors.unsafeReferences));
    }

    const removed = removeSafePluginReferences(state.configModel, references);
    set((current) => {
      const applied = applyConfigModelToState(current, removed.config, [
        ...removed.changedTags,
        plugin.name,
      ]);
      return deletePluginFromState({ ...current, ...applied }, id);
    });
    await get().saveConfig();
  },

  enterEditorForPluginReferences: () =>
    set((state) => ({
      editorMode: state.webUiMode === "standard" ? false : true,
    })),

  addPlugin: (plugin) =>
    set((state) =>
      syncPluginsToConfig(state, (plugins) => [
        ...plugins,
        {
          ...plugin,
          id: plugin.name,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
          metrics: { calls: 0, avgLatency: 0, errorRate: 0, qps: 0 },
        },
      ]),
    ),

  renamePlugin: async (id, name, options) => {
    const nextName = name.trim();
    const state = get();
    const plugin = state.plugins.find((p) => p.id === id);
    if (!plugin) {
      return {
        status: "invalid",
        message: tClient(WEBUI.storeErrors.pluginMissing),
      };
    }
    if (!nextName) {
      return {
        status: "invalid",
        message: tClient(WEBUI.storeErrors.pluginNameRequired),
      };
    }
    const tagValidationError = validatePluginTag(nextName);
    if (tagValidationError) {
      return {
        status: "invalid",
        message: tClient(pluginTagValidationMessageKey(tagValidationError)),
      };
    }
    if (isReservedPluginTag(nextName)) {
      return {
        status: "invalid",
        message: tClient(WEBUI.storeErrors.pluginNameReserved),
      };
    }
    if (nextName === plugin.name) {
      return {
        status: "invalid",
        message: tClient(WEBUI.storeErrors.pluginNameUnchanged),
      };
    }
    if (state.plugins.some((p) => p.id !== id && p.name === nextName)) {
      return {
        status: "invalid",
        message: tClient(WEBUI.storeErrors.pluginNameExists),
      };
    }
    if (state.configError) {
      return {
        status: "invalid",
        message: tClient(WEBUI.storeErrors.configHasErrorsBeforeRename),
      };
    }

    await get().validateCurrentConfig();
    const latest = get();
    const references = incomingReferences(latest, plugin.name);
    if (references.length > 0 && !options?.confirmed) {
      return { status: "needs-confirmation", references };
    }

    const replaced = replacePluginReferences(
      latest.configModel,
      references,
      plugin.name,
      nextName,
    );
    const renamed = renamePluginConfigTag(
      replaced.config,
      plugin.name,
      nextName,
    );
    set((current) =>
      applyConfigModelToState(
        current,
        renamed.config,
        [...replaced.changedTags, ...renamed.changedTags],
        nextName,
      ),
    );
    await get().saveConfig();
    return { status: "renamed" };
  },
}));

function applyConfigFileResponse(
  response: ConfigFileResponse,
  set: StoreSet,
) {
  const parsed = parseOxiDnsYaml(response.content);
  const headerState = headerStateFromText(response.content);
  if (!parsed.config) {
    set({
      configText: response.content,
      yamlConfig: response.content,
      ...headerState,
      configVersion: response.version,
      configPath: response.path,
      configError:
        parsed.diagnostics[0] ?? tClient(WEBUI.storeErrors.configParseFailed),
      configDiagnostics: parsed.diagnostics,
    });
    return;
  }

  const config = parsed.config;
  const plugins = restorePinnedState(pluginsFromConfig(config));
  set((state) => ({
    configModel: config,
    configText: response.content,
    yamlConfig: response.content,
    ...headerState,
    configVersion: response.version,
    configPath: response.path,
    plugins,
    matcherControls: Object.fromEntries(
      plugins
        .filter((plugin) => plugin.type === "matcher")
        .map((plugin) => [
          plugin.name,
          {
            availability: "loading" as const,
            pending: false,
            mode: state.matcherControls[plugin.name]?.mode ?? null,
          },
        ]),
    ),
    providerReloads: Object.fromEntries(
      plugins
        .filter((plugin) => plugin.type === "provider")
        .map((plugin) => [
          plugin.name,
          {
            pending: false,
            outcome: "idle" as const,
          },
        ]),
    ),
    selectedPlugin: syncSelectedPlugin(state.selectedPlugin, plugins),
    standardConfigOutOfSync: isStandardConfigOutOfSync(
      state.standardLastGenerated,
      response.version,
    ),
    configError: parsed.diagnostics[0] ?? null,
    configDiagnostics: parsed.diagnostics,
  }));
}

function createDefaultWebUiConfig(): WebUiConfigDocument {
  return {
    schema: 1,
    mode: "expert",
    standard: {},
    ui: { modeSelectionDismissed: false },
  };
}

function normalizeWebUiConfig(value: unknown): WebUiConfigDocument {
  const source = isJsonObject(value) ? value : {};
  const ui = isJsonObject(source.ui) ? source.ui : {};
  const standard = isJsonObject(source.standard) ? source.standard : {};
  return {
    ...source,
    schema: typeof source.schema === "number" ? source.schema : 1,
    mode: source.mode === "standard" ? "standard" : "expert",
    standard,
    ui: {
      ...ui,
      modeSelectionDismissed: ui.modeSelectionDismissed === true,
    },
  };
}

function applyWebUiConfigResponse(
  response: WebUiConfigResponse,
  set: StoreSet,
) {
  const webUiConfig = normalizeWebUiConfig(response.config);
  const standardSettings = loadStandardSettingsFromWebUiConfig(webUiConfig);
  const standardLastGenerated = standardLastGeneratedFromWebUiConfig(webUiConfig);
  set((state) => ({
    webUiConfig,
    webUiConfigVersion: response.version,
    webUiConfigPath: response.path,
    webUiConfigError: null,
    webUiMode: webUiConfig.mode,
    modeSelectionDismissed: webUiConfig.ui.modeSelectionDismissed,
    editorMode: webUiConfig.mode === "standard" ? false : state.editorMode,
    standardSettings: standardSettings.settings,
    standardSettingsNotice: standardSettings.notice,
    standardLastGenerated,
    standardConfigOutOfSync:
      webUiConfig.mode === "standard"
        ? isStandardConfigOutOfSync(standardLastGenerated, state.configVersion)
        : false,
  }));
}

function applyWebUiConfigPatch(
  config: WebUiConfigDocument,
  patch: JsonObject,
): WebUiConfigDocument {
  return normalizeWebUiConfig(applyJsonMergePatch(config, patch));
}

function applyJsonMergePatch(
  target: JsonValue | undefined,
  patch: JsonValue,
): JsonValue {
  if (!isJsonObject(patch)) return patch;
  const merged: JsonObject = isJsonObject(target) ? { ...target } : {};
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) {
      delete merged[key];
    } else {
      merged[key] = applyJsonMergePatch(merged[key], value);
    }
  }
  return merged;
}

async function persistWebUiConfigPatch(
  set: StoreSet,
  get: StoreGet,
  patch: JsonObject,
): Promise<void> {
  await enqueueWebUiConfigSave(set, async () => {
    const backendKey = currentBackendKey();
    const state = get();
    if (state.isOfflineMode) return;
    set({ webUiConfigError: null });
    try {
      const response = await patchWebUiConfig({
        patch,
        baseVersion: state.webUiConfigVersion,
      });
      if (!isCurrentBackend(backendKey)) return;
      applyWebUiConfigResponse(response, set);
    } catch (error) {
      if (!isCurrentBackend(backendKey)) return;
      set({
        webUiConfigError:
          error instanceof Error
            ? error.message
            : tClient(WEBUI.storeErrors.saveConfigFailed),
      });
      throw error;
    }
  });
}

function loadStandardSettingsFromWebUiConfig(
  config: WebUiConfigDocument,
): { settings: StandardModeSettings; notice: StandardSettingsNotice } {
  const settings = config.standard.settings;
  if (settings === undefined) {
    return { settings: createDefaultStandardSettings(), notice: null };
  }
  return normalizeStandardSettings(settings);
}

function standardLastGeneratedFromWebUiConfig(
  config: WebUiConfigDocument,
): StandardGeneratedMetadata | null {
  const meta = isJsonObject(config.standard.meta) ? config.standard.meta : {};
  const value = meta.lastGenerated;
  if (!isJsonObject(value)) return null;
  if (
    typeof value.settingsRevision !== "string" ||
    !Array.isArray(value.generatedTags) ||
    !isJsonObject(value.tagMap) ||
    !isJsonObject(value.summary) ||
    typeof value.generatedAtMs !== "number"
  ) {
    return null;
  }
  return {
    configVersion:
      typeof value.configVersion === "string" ? value.configVersion : null,
    settingsRevision: value.settingsRevision,
    generatedTags: value.generatedTags.map(String),
    tagMap: value.tagMap as unknown as StandardGeneratedMetadata["tagMap"],
    summary: value.summary as unknown as StandardGeneratedMetadata["summary"],
    generatedAtMs: value.generatedAtMs,
  };
}

function isStandardConfigOutOfSync(
  lastGenerated: StandardGeneratedMetadata | null,
  configVersion: string | null,
): boolean {
  return Boolean(
    lastGenerated?.configVersion &&
      configVersion &&
      lastGenerated.configVersion !== configVersion,
  );
}

function isJsonObject(value: unknown): value is JsonObject {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function headerStateFromText(
  text: string,
): Pick<AppState, "modeHeaderPresent"> {
  const header = parseWebUiConfigHeader(text);
  return {
    modeHeaderPresent: header.modeHeaderPresent,
  };
}

function prepareConfigTextForSave(state: AppState): {
  content: string;
  header: { mode: WebUiMode };
} {
  const existingHeader = parseWebUiConfigHeader(state.configText);
  const mode = state.webUiMode ?? existingHeader.mode;
  const header = {
    mode,
  };
  const strippedConfigText = stripWebUiConfigHeader(state.configText);
  const hasAnyManagedHeader = strippedConfigText !== state.configText;
  const content =
    mode === "standard"
      ? writeWebUiConfigHeader(state.configText, {
          mode,
          modeHeaderPresent: true,
        })
      : state.modeHeaderPresent || hasAnyManagedHeader
        ? writeWebUiConfigHeader(state.configText, {
            mode,
            modeHeaderPresent: true,
          })
        : state.configText;

  return {
    content,
    header,
  };
}

function applyConfigValidationResponse(
  response: ConfigValidateResponse,
  set: StoreSet,
) {
  set({
    dependencyGraph: response.dependency_graph,
    configDiagnostics: [],
    configError: null,
  });
}

function syncPluginsToConfig(
  state: AppState,
  update: (plugins: PluginInstance[]) => PluginInstance[],
  changedTags: string[] = [],
) {
  const plugins = update(state.plugins);
  const configModel = configFromPlugins(state.configModel, plugins);
  // Preserve comments/blank lines: only the explicitly changed tags are
  // regenerated; every other plugin keeps its original YAML node verbatim.
  const configText = serializePluginsPreserving(
    state.configText,
    configModel,
    new Set(changedTags),
  );
  return {
    plugins,
    matcherControls: reconcileMatcherControls(plugins, state.matcherControls),
    providerReloads: reconcileProviderReloads(plugins, state.providerReloads),
    configModel,
    configText,
    yamlConfig: configText,
    standardConfigOutOfSync: isStandardConfigOutOfSync(
      state.standardLastGenerated,
      state.configVersion,
    ),
    selectedPlugin: syncSelectedPlugin(state.selectedPlugin, plugins),
    configError: null,
    configDiagnostics: [],
  };
}

function applyConfigModelToState(
  state: AppState,
  configModel: OxiDnsConfig,
  changedTags: string[],
  selectedTag?: string | null,
) {
  const plugins = restorePinnedState(pluginsFromConfig(configModel));
  const configText = serializePluginsPreserving(
    state.configText,
    configModel,
    new Set(changedTags),
  );
  return {
    plugins,
    matcherControls: reconcileMatcherControls(plugins, state.matcherControls),
    providerReloads: reconcileProviderReloads(plugins, state.providerReloads),
    configModel,
    configText,
    yamlConfig: configText,
    standardConfigOutOfSync: isStandardConfigOutOfSync(
      state.standardLastGenerated,
      state.configVersion,
    ),
    selectedPlugin:
      selectedTag === null
        ? null
        : selectedTag
          ? (plugins.find((plugin) => plugin.name === selectedTag) ?? null)
          : syncSelectedPlugin(state.selectedPlugin, plugins),
    configError: null,
    configDiagnostics: [],
  };
}

function deletePluginFromState(state: AppState, id: string) {
  const plugin = state.plugins.find((p) => p.id === id);
  if (!plugin) return {};
  const configModel: OxiDnsConfig = {
    ...state.configModel,
    plugins: state.configModel.plugins.filter((p) => p.tag !== plugin.name),
  };
  const selectedWasDeleted = state.selectedPlugin?.id === id;
  return {
    ...applyConfigModelToState(
      state,
      configModel,
      [plugin.name],
      selectedWasDeleted ? null : undefined,
    ),
    detailOpen: selectedWasDeleted ? false : state.detailOpen,
  };
}

function incomingReferences(state: AppState, tag: string) {
  return getIncomingPluginReferences(
    state.plugins,
    state.dependencyGraph?.edges,
    tag,
  );
}

function replacementCandidates(
  state: AppState,
  plugin: PluginInstance,
  references: PluginReferenceImpact[],
) {
  return getReplacementCandidates(state.plugins, plugin.id, references);
}

function syncSelectedPlugin(
  selectedPlugin: PluginInstance | null,
  plugins: PluginInstance[],
) {
  if (!selectedPlugin) return null;
  return plugins.find((plugin) => plugin.id === selectedPlugin.id) ?? null;
}

const PINNED_PLUGINS_KEY = "oxidns:pinned-plugins";

function loadPinnedIds(): Set<string> {
  try {
    const stored = localStorage.getItem(PINNED_PLUGINS_KEY);
    return stored ? new Set(JSON.parse(stored) as string[]) : new Set();
  } catch {
    return new Set();
  }
}

function savePinnedIds(ids: Set<string>): void {
  try {
    localStorage.setItem(PINNED_PLUGINS_KEY, JSON.stringify([...ids]));
  } catch {}
}

function restorePinnedState(plugins: PluginInstance[]): PluginInstance[] {
  const pinnedIds = loadPinnedIds();
  if (pinnedIds.size === 0) return plugins;
  return plugins.map((p) => ({ ...p, pinned: pinnedIds.has(p.id) }));
}

function pluginCountOf(text: string): number {
  return parseOxiDnsYaml(text).config?.plugins.length ?? 0;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// Wait for positive evidence that the backend instance changed after a restart
// request. Unix restarts use exec(), so the API may never be observably down.
async function pollReconnect(
  baseline: ProcessInstanceBaseline,
  onPhase?: (phase: "waiting_down" | "waiting_up") => void,
): Promise<void> {
  let sawDown = false;

  onPhase?.("waiting_down");
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    await delay(sawDown ? 1500 : 800);
    try {
      const health = await fetchHealth();
      const fresh =
        processInstanceChanged(health, baseline) ||
        (sawDown && !hasProcessIdentityBaseline(baseline));
      if (fresh) {
        return;
      }
    } catch {
      sawDown = true;
      onPhase?.("waiting_up");
    }
  }

  throw new Error(
    tClient(
      sawDown
        ? WEBUI.storeErrors.restartTimeout
        : WEBUI.storeErrors.restartNotObserved,
    ),
  );
}

// Poll the reload status until the backend settles on a new completion.
// During reassembly the API hub is briefly torn down, so transient fetch
// errors are expected and ignored. We treat the reload as done once it is
// no longer pending/in-progress AND a new completion timestamp appeared
// (distinct from the pre-reload baseline), or it explicitly failed.
async function pollReload(baselineCompleted?: number): Promise<ReloadSnapshot> {
  const maxAttempts = 40; // ~30s at 750ms intervals
  let last: ReloadSnapshot | null = null;
  for (let i = 0; i < maxAttempts; i += 1) {
    await delay(750);
    try {
      last = await fetchReloadStatus();
    } catch {
      continue;
    }
    const settled = !last.pending && !last.in_progress;
    const advanced =
      last.last_completed_ms !== undefined &&
      last.last_completed_ms !== baselineCompleted;
    if (settled && (advanced || last.status === "failed")) return last;
  }
  return last ?? { status: "unknown", pending: false, in_progress: false };
}
