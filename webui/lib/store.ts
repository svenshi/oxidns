"use client";

import { create } from "zustand";
import type { PluginInstance } from "./types";
import {
  configFromPlugins,
  createDefaultOxiDnsConfig,
  parseOxiDnsYaml,
  pluginsFromConfig,
  stringifyOxiDnsConfig,
  topLevelConfigChanged,
  type OxiDnsConfig,
} from "./oxidns-config";
import {
  patchPluginsYaml,
  type PluginYamlPatchResult,
} from "./oxidns-config-patch";
import {
  fetchBuildInfo,
  fetchConfigFile,
  fetchHealth,
  fetchMatcherStatus,
  fetchPrometheusMetrics,
  fetchReloadStatus,
  fetchSystem,
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
  type ReloadSnapshot,
  type SystemResponse,
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
} from "./config-history";
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

type StoreSet = (
  partial: Partial<AppState> | ((state: AppState) => Partial<AppState>),
) => void;

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
  | { status: PluginMutationResolution }
  | {
      status: "needs-confirmation";
      references: PluginReferenceImpact[];
    }
  | { status: "invalid"; message: string };

export type PluginMutationResolution =
  | "patched"
  | "forced"
  | "review"
  | "cancelled";

export type ConfigPatchConfirmationChoice = "cancel" | "review" | "force";

export interface ConfigPatchConfirmation {
  reason: string;
  affectedPath: string;
  canForce: boolean;
}

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
  selectedPlugin: PluginInstance | null;
  detailOpen: boolean;
  editorMode: boolean;
  historyOpen: boolean;
  isConfigLoading: boolean;
  isConfigSaving: boolean;
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
  /** Persisted/editor-reset baseline used by a staged local rebuild review. */
  configEditorBaseline: string | null;
  /** Editing a pasted/uploaded config with no backend connection. */
  isOfflineMode: boolean;
  /** Name of the uploaded file, used as the export download name. */
  offlineFileName: string | null;
  configPatchConfirmation: ConfigPatchConfirmation | null;

  setSelectedPlugin: (plugin: PluginInstance | null) => void;
  setDetailOpen: (open: boolean) => void;
  setEditorMode: (mode: boolean) => void;
  setHistoryOpen: (open: boolean) => void;
  setYamlConfig: (config: string) => void;
  resolveConfigPatchConfirmation: (
    choice: ConfigPatchConfirmationChoice,
  ) => void;
  enterOfflineConfig: (text: string, fileName?: string) => void;
  exitOfflineMode: () => void;
  loadConfig: () => Promise<void>;
  refreshHealthState: () => Promise<void>;
  refreshSystemState: () => Promise<void>;
  refreshRuntimeState: () => Promise<void>;
  /** Fetch matcher bypass state once at explicit config/list refresh boundaries. */
  refreshMatcherStates: () => Promise<void>;
  refreshMetrics: () => Promise<void>;
  validateCurrentConfig: () => Promise<void>;
  saveConfig: () => Promise<void>;
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
  reorderPlugins: (
    orderedVisibleIds: string[],
  ) => Promise<PluginMutationResolution>;
  updatePluginConfig: (
    id: string,
    config: Record<string, unknown>,
  ) => Promise<PluginMutationResolution>;
  previewPluginDelete: (id: string) => Promise<PluginDeletePreview>;
  confirmDeletePlugin: (id: string) => Promise<PluginMutationResolution>;
  replaceAndDeletePlugin: (
    id: string,
    replacementTag: string,
  ) => Promise<PluginMutationResolution>;
  removeReferencesAndDeletePlugin: (
    id: string,
  ) => Promise<PluginMutationResolution>;
  enterEditorForPluginReferences: () => void;
  addPlugin: (
    plugin: Omit<PluginInstance, "id" | "createdAt" | "updatedAt" | "metrics">,
  ) => Promise<PluginMutationResolution>;
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

interface PendingConfigPatch {
  result: Extract<PluginYamlPatchResult, { status: "needs_confirmation" }>;
  source: string;
  apply: (content: string) => void;
  resolve: (resolution: PluginMutationResolution) => void;
}

let pendingConfigPatch: PendingConfigPatch | null = null;

function currentBackendKey(): string {
  const { connectionEpoch, serverConfig } = useAuthStore.getState();
  return `${connectionEpoch}\0${serverConfig.url.trim()}`;
}

function isCurrentBackend(backendKey: string): boolean {
  const auth = useAuthStore.getState();
  return auth.isConnected && currentBackendKey() === backendKey;
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

const initialConfigModel = createDefaultOxiDnsConfig();
const initialConfigText = stringifyOxiDnsConfig(initialConfigModel);

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
  selectedPlugin: null,
  detailOpen: false,
  editorMode: false,
  historyOpen: false,
  isConfigLoading: false,
  isConfigSaving: false,
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
  configEditorBaseline: null,
  isOfflineMode: false,
  offlineFileName: null,
  configPatchConfirmation: null,

  setSelectedPlugin: (plugin) => set({ selectedPlugin: plugin }),
  setDetailOpen: (open) => set({ detailOpen: open }),
  setEditorMode: (mode) => set({ editorMode: mode }),
  setHistoryOpen: (open) => set({ historyOpen: open }),
  resolveConfigPatchConfirmation: (choice) => {
    const pending = pendingConfigPatch;
    if (!pending) return;
    pendingConfigPatch = null;
    set({ configPatchConfirmation: null });

    if (choice === "cancel") {
      pending.resolve("cancelled");
      return;
    }

    const candidate = pending.result.candidate?.content;
    if (!candidate) {
      if (choice === "review") set({ editorMode: true });
      pending.resolve(choice === "review" ? "review" : "cancelled");
      return;
    }

    pending.apply(candidate);
    if (choice === "review") {
      set({
        configEditorBaseline: pending.source,
        editorMode: true,
      });
    }
    pending.resolve(choice === "review" ? "review" : "forced");
  },
  setYamlConfig: (config) => {
    const parsed = parseOxiDnsYaml(config);
    if (!parsed.config) {
      set({
        configText: config,
        yamlConfig: config,
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
    cancelPendingConfigPatch(set);
    set({
      isOfflineMode: true,
      offlineFileName: fileName ?? null,
      configEditorBaseline: null,
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
    });
    get().setYamlConfig(text);
  },

  // Leave offline mode. When still disconnected this returns the user to the
  // import screen; on reconnect the layout's loadConfig() authoritatively
  // repopulates config state, so no manual backend restore is needed here.
  exitOfflineMode: () => set({ isOfflineMode: false, offlineFileName: null }),

  loadConfig: async () => {
    cancelPendingConfigPatch(set);
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
      configError: null,
      runningDependencyGraph: null,
    });
    try {
      const response = await fetchConfigFile();
      if (
        generation !== configLoadGeneration ||
        !isCurrentBackend(backendKey)
      ) {
        return;
      }
      applyConfigFileResponse(response, set, get());
      const scope = getScopeKey(response.path);
      recordSnapshot(scope, {
        content: response.content,
        version: response.version,
        source: "server",
        pluginCount: pluginCountOf(response.content),
        applyStatus: "applied",
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
        set({ isConfigLoading: false });
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
  saveConfig: () =>
    enqueueConfigSave(set, async () => {
      const state = get();
      if (state.configError) throw new Error(state.configError);

      set({ configError: null });
      try {
        const validation = await validateConfigText(state.configText);
        applyConfigValidationResponse(validation, set);
        const content = state.configText;
        const response = await saveConfigFile({
          content,
          baseVersion: state.configVersion,
          validate: true,
          reload: false,
        });
        const scope = getScopeKey(response.path);
        recordSnapshot(scope, {
          content,
          version: response.version,
          source: "save",
          pluginCount: pluginCountOf(content),
          applyStatus: "not-applied",
        });
        set({
          configVersion: response.version,
          configEditorBaseline: null,
          configPath: response.path,
          reloadStatus: response.reload ?? get().reloadStatus,
          configHistory: listSnapshots(scope),
        });
        await get().refreshRuntimeState();
      } catch (error) {
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
    await get().saveConfig();
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
    if (state.configError) return "cancelled";

    const visible = new Set(orderedVisibleIds);
    const byId = new Map(state.plugins.map((p) => [p.id, p] as const));
    const queue = orderedVisibleIds
      .map((id) => byId.get(id))
      .filter((p): p is PluginInstance => Boolean(p));
    if (queue.length === 0) return "cancelled";

    let next = 0;
    const reordered = state.plugins.map((p) =>
      visible.has(p.id) ? queue[next++] : p,
    );
    const unchanged = reordered.every((p, i) => p.id === state.plugins[i].id);
    if (unchanged) return "cancelled";

    // The CST patcher moves the original sequence-item tokens, so comments,
    // blank lines, scalar styles, and plugin-local formatting move with them.
    const resolution = await stagePluginListMutation(set, get, reordered);
    if (shouldPersistPluginMutation(resolution) && !get().isOfflineMode) {
      await get().saveConfig();
    }
    return resolution;
  },

  updatePluginConfig: async (id, config) => {
    const state = get();
    const plugins = state.plugins.map((plugin) =>
      plugin.id === id
        ? { ...plugin, config, updatedAt: new Date().toISOString() }
        : plugin,
    );
    return stagePluginListMutation(set, get, plugins);
  },

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
    const configModel = removePluginConfig(state.configModel, plugin.name);
    const resolution = await stageConfigModelMutation(set, get, configModel, {
      selectedTag: state.selectedPlugin?.id === id ? null : undefined,
      closeDetail: state.selectedPlugin?.id === id,
    });
    if (shouldPersistPluginMutation(resolution)) await get().saveConfig();
    return resolution;
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
    const configModel = removePluginConfig(replaced.config, plugin.name);
    const resolution = await stageConfigModelMutation(set, get, configModel, {
      selectedTag: state.selectedPlugin?.id === id ? null : undefined,
      closeDetail: state.selectedPlugin?.id === id,
    });
    if (shouldPersistPluginMutation(resolution)) await get().saveConfig();
    return resolution;
  },

  removeReferencesAndDeletePlugin: async (id) => {
    await get().validateCurrentConfig();
    const state = get();
    const plugin = state.plugins.find((p) => p.id === id);
    if (!plugin) throw new Error(tClient(WEBUI.storeErrors.pluginMissing));
    const references = incomingReferences(state, plugin.name);
    if (references.length === 0) {
      const configModel = removePluginConfig(state.configModel, plugin.name);
      const resolution = await stageConfigModelMutation(set, get, configModel, {
        selectedTag: state.selectedPlugin?.id === id ? null : undefined,
        closeDetail: state.selectedPlugin?.id === id,
      });
      if (shouldPersistPluginMutation(resolution)) await get().saveConfig();
      return resolution;
    }
    if (!references.every((edge) => edge.removable)) {
      throw new Error(tClient(WEBUI.storeErrors.unsafeReferences));
    }

    const removed = removeSafePluginReferences(state.configModel, references);
    const configModel = removePluginConfig(removed.config, plugin.name);
    const resolution = await stageConfigModelMutation(set, get, configModel, {
      selectedTag: state.selectedPlugin?.id === id ? null : undefined,
      closeDetail: state.selectedPlugin?.id === id,
    });
    if (shouldPersistPluginMutation(resolution)) await get().saveConfig();
    return resolution;
  },

  enterEditorForPluginReferences: () => set({ editorMode: true }),

  addPlugin: async (plugin) => {
    const state = get();
    const plugins = [
      ...state.plugins,
      {
        ...plugin,
        id: plugin.name,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        metrics: { calls: 0, avgLatency: 0, errorRate: 0, qps: 0 },
      },
    ];
    return stagePluginListMutation(set, get, plugins);
  },

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
    const resolution = await stageConfigModelMutation(
      set,
      get,
      renamed.config,
      {
        selectedTag: nextName,
        renamedTags: new Map([[plugin.name, nextName]]),
      },
    );
    if (shouldPersistPluginMutation(resolution)) await get().saveConfig();
    return { status: resolution };
  },
}));

function applyConfigFileResponse(
  response: ConfigFileResponse,
  set: StoreSet,
  state: AppState,
) {
  const parsed = parseOxiDnsYaml(response.content);
  if (!parsed.config) {
    set({
      configText: response.content,
      yamlConfig: response.content,
      configEditorBaseline: null,
      configVersion: response.version,
      configPath: response.path,
      configError:
        parsed.diagnostics[0] ?? tClient(WEBUI.storeErrors.configParseFailed),
      configDiagnostics: parsed.diagnostics,
    });
    return;
  }

  const plugins = restorePinnedState(pluginsFromConfig(parsed.config));
  set({
    configModel: parsed.config,
    configText: response.content,
    yamlConfig: response.content,
    configEditorBaseline: null,
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
    configError: parsed.diagnostics[0] ?? null,
    configDiagnostics: parsed.diagnostics,
  });
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

interface ConfigModelMutationOptions {
  plugins?: PluginInstance[];
  selectedTag?: string | null;
  renamedTags?: ReadonlyMap<string, string>;
  closeDetail?: boolean;
}

async function stagePluginListMutation(
  set: StoreSet,
  get: () => AppState,
  plugins: PluginInstance[],
) {
  const state = get();
  const configModel = configFromPlugins(state.configModel, plugins);
  return stageConfigModelMutation(set, get, configModel, { plugins });
}

async function stageConfigModelMutation(
  set: StoreSet,
  get: () => AppState,
  configModel: OxiDnsConfig,
  options: ConfigModelMutationOptions = {},
): Promise<PluginMutationResolution> {
  if (pendingConfigPatch) return "cancelled";
  const state = get();
  const result = patchPluginsYaml(
    state.configText,
    state.configModel,
    configModel,
    {
      renamedTags: options.renamedTags,
    },
  );
  const apply = (content: string) =>
    set((current) =>
      applyConfigModelToState(current, configModel, content, options),
    );

  if (result.status === "patched") {
    apply(result.content);
    return "patched";
  }

  return new Promise<PluginMutationResolution>((resolve) => {
    pendingConfigPatch = {
      result,
      source: state.configText,
      apply,
      resolve,
    };
    set({
      configPatchConfirmation: {
        reason: result.reason,
        affectedPath: result.affectedPath,
        canForce: Boolean(result.candidate),
      },
    });
  });
}

function applyConfigModelToState(
  state: AppState,
  configModel: OxiDnsConfig,
  configText: string,
  options: ConfigModelMutationOptions,
) {
  const plugins =
    options.plugins ?? restorePinnedState(pluginsFromConfig(configModel));
  return {
    plugins,
    matcherControls: reconcileMatcherControls(plugins, state.matcherControls),
    providerReloads: reconcileProviderReloads(plugins, state.providerReloads),
    configModel,
    configText,
    yamlConfig: configText,
    selectedPlugin:
      options.selectedTag === null
        ? null
        : options.selectedTag
          ? (plugins.find((plugin) => plugin.name === options.selectedTag) ??
            null)
          : syncSelectedPlugin(state.selectedPlugin, plugins),
    detailOpen: options.closeDetail ? false : state.detailOpen,
    configError: null,
    configDiagnostics: [],
  };
}

function removePluginConfig(config: OxiDnsConfig, tag: string): OxiDnsConfig {
  return {
    ...config,
    plugins: config.plugins.filter((plugin) => plugin.tag !== tag),
  };
}

export function shouldPersistPluginMutation(
  resolution: PluginMutationResolution,
) {
  return resolution === "patched" || resolution === "forced";
}

function cancelPendingConfigPatch(set: StoreSet) {
  const pending = pendingConfigPatch;
  if (!pending) return;
  pendingConfigPatch = null;
  set({ configPatchConfirmation: null });
  pending.resolve("cancelled");
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
