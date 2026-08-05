import { beforeEach, describe, expect, it, vi } from "vitest";

import { createDefaultStandardSettings } from "./standard-mode/defaults";
import type { StandardPolicyPlan } from "./standard-mode/types";

const apiMocks = vi.hoisted(() => ({
  applyConfigFile: vi.fn(),
  fetchConfigApplyStatus: vi.fn(),
  validateConfigText: vi.fn(),
  patchWebUiConfig: vi.fn(),
  fetchWebUiConfig: vi.fn(),
}));
const compilerMocks = vi.hoisted(() => ({ compileStandardIntent: vi.fn() }));

vi.mock("./oxidns-api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./oxidns-api")>()),
  ...apiMocks,
}));
vi.mock("./standard-mode/compiler", () => compilerMocks);

import { useAuthStore } from "./auth-store";
import { useAppStore } from "./store";

function policy(): StandardPolicyPlan {
  const intent = createDefaultStandardSettings();
  return {
    normalizedIntent: intent,
    diagnostics: [],
    generated: {
      yaml: "# oxidns-webui.mode: standard\nplugins: []\n",
      configVersion: "config-v2",
      pluginCount: 1,
      generatedTags: ["standard_main"],
      tagMap: {
        system: [], caches: {}, upstreamGroups: {}, paths: {},
        routingRules: {}, exceptionRules: {},
      },
      summary: {
        upstreamGroupCount: 1, pathCount: 1, enabledUpstreamCount: 1,
        filteringEnabled: false, cacheEnabled: true, queryLogEnabled: true,
        routingRuleCount: 0, exceptionRuleCount: 0, deviceCount: 0,
        localPolicyCount: 0,
      },
      explanation: {
        schema: 1, intentRevision: "sha256:intent", mappings: [],
        finalPriority: [], pathBoundaries: [], generatedTags: ["standard_main"],
        capabilities: { features: [], servers: [], executors: [], matchers: [], providers: [], missingOptional: [] },
      },
    },
    canApply: true,
    details: {},
  };
}

describe("Standard Mode generic transactional apply", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAuthStore.setState({
      isConnected: true,
      connectionEpoch: 100,
      serverConfig: { url: "/api", requiresAuth: false, username: "", password: "" },
    });
    useAppStore.setState({
      configVersion: "config-v1",
      configText: "plugins:\n  - tag: expert_main\n    type: sequence\n    args: []\n",
      webUiConfigVersion: "standard-v1",
      configPath: "/etc/oxidns/config.yaml",
      isOfflineMode: false,
      webUiMode: "expert",
      modeHeaderPresent: false,
      historyOpen: false,
      standardApplyConfirmation: null,
      buildInfo: {
        version: "test", bundle: "standard", enabled_bundles: ["standard"], enabled_features: [],
        supported_plugins: { servers: [], executors: [], matchers: [], providers: [] },
      },
      loadConfig: vi.fn().mockImplementation(async () => {
        useAppStore.setState({ configVersion: "config-v2" });
      }),
    });
    compilerMocks.compileStandardIntent.mockResolvedValue(policy());
    apiMocks.validateConfigText.mockResolvedValue({
      ok: true, source: "body", path: "/etc/oxidns/config.yaml", plugin_count: 1,
      dependency_graph: { nodes: [], edges: [], init_order: [] }, version: "config-v2", message: "valid",
    });
    apiMocks.applyConfigFile.mockResolvedValue({
      ok: true, transaction_id: "config-1-2-abcdef", status: "pending",
      previous_config_version: "config-v1", candidate_config_version: "config-v2",
    });
    apiMocks.fetchConfigApplyStatus.mockResolvedValue({
      ok: true,
      transaction: {
        schema: 1, transaction_id: "config-1-2-abcdef", status: "succeeded",
        created_at_ms: 1, completed_at_ms: 10,
        previous_config_version: "config-v1", candidate_config_version: "config-v2",
      },
    });
    apiMocks.patchWebUiConfig.mockResolvedValue({
      ok: true, path: "/etc/oxidns/config.yaml.webui.json", version: "standard-v2",
      updated_at_ms: 10, defaulted: false, recovered: false, backup_path: null,
      config: { schema: 1, mode: "standard", ui: { modeSelectionDismissed: true }, standard: {} },
    });
  });

  it("reviews an unmanaged takeover then submits only native YAML and versions", async () => {
    const saving = useAppStore.getState().saveStandardSettings(createDefaultStandardSettings(), { apply: true });
    await vi.waitFor(() => expect(useAppStore.getState().standardApplyConfirmation?.ownership).toBe("unmanaged"));
    expect(apiMocks.applyConfigFile).not.toHaveBeenCalled();
    useAppStore.getState().confirmStandardApply();
    await saving;
    expect(apiMocks.applyConfigFile).toHaveBeenCalledWith(
      "# oxidns-webui.mode: standard\nplugins: []\n",
      "config-v1",
      "config-v2",
    );
    expect(apiMocks.fetchConfigApplyStatus).toHaveBeenCalled();
    expect(apiMocks.patchWebUiConfig).toHaveBeenCalledTimes(1);
  });

  it("does not write when review is cancelled", async () => {
    const saving = useAppStore.getState().saveStandardSettings(createDefaultStandardSettings(), { apply: true });
    await vi.waitFor(() => expect(useAppStore.getState().standardApplyConfirmation).not.toBeNull());
    useAppStore.getState().cancelStandardApply();
    await expect(saving).rejects.toThrow();
    expect(apiMocks.applyConfigFile).not.toHaveBeenCalled();
  });

  it("keeps DNS applied when opaque workspace CAS fails three times", async () => {
    apiMocks.patchWebUiConfig.mockRejectedValue(new Error("CAS conflict"));
    apiMocks.fetchWebUiConfig.mockResolvedValue({
      ok: true, path: "/etc/oxidns/config.yaml.webui.json", version: "changed",
      updated_at_ms: 10, defaulted: false, recovered: false, backup_path: null,
      config: { schema: 1 },
    });
    const saving = useAppStore.getState().saveStandardSettings(createDefaultStandardSettings(), { apply: true });
    await vi.waitFor(() => expect(useAppStore.getState().standardApplyConfirmation).not.toBeNull());
    useAppStore.getState().confirmStandardApply();
    await saving;
    expect(apiMocks.applyConfigFile).toHaveBeenCalledTimes(1);
    expect(apiMocks.patchWebUiConfig).toHaveBeenCalledTimes(3);
    expect(useAppStore.getState().webUiConfigError).toContain("CAS conflict");
  });
});
