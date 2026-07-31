import { beforeEach, describe, expect, it, vi } from "vitest";

import { createDefaultStandardSettings } from "./standard-mode/defaults";
import type { StandardPlanResponse } from "./standard-mode/types";

const apiMocks = vi.hoisted(() => ({
  applyStandardMode: vi.fn(),
  fetchStandardTransactionStatus: vi.fn(),
  planStandardMode: vi.fn(),
}));

vi.mock("./oxidns-api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./oxidns-api")>();
  return { ...actual, ...apiMocks };
});

import { useAuthStore } from "./auth-store";
import { useAppStore } from "./store";

function plan(
  ownership: StandardPlanResponse["ownership"],
): StandardPlanResponse {
  const intent = createDefaultStandardSettings();
  return {
    ok: true,
    config_version: "config-v1",
    standard_version: "standard-v1",
    ownership,
    semantic_diff: {
      preserved_top_level: ["include", "api", "network"],
      generated_plugin_tags: ["standard_main"],
      replaced_plugin_tags: [],
      removed_plugin_tags: ownership === "managed" ? [] : ["expert_main"],
    },
    blockers:
      ownership === "managed"
        ? []
        : [
            {
              code: "takeover_confirmation_required",
              path: "takeover",
              message: "confirmation required",
            },
          ],
    can_apply: ownership === "managed",
    plan: {
      normalizedIntent: intent,
      diagnostics: [],
      generated: {
        yaml: "# oxidns-webui.mode: standard\nplugins: []\n",
        configVersion: "config-v2",
        pluginCount: 1,
        generatedTags: ["standard_main"],
        tagMap: {
          system: [],
          caches: {},
          upstreamGroups: {},
          paths: {},
          routingRules: {},
          exceptionRules: {},
        },
        summary: {
          upstreamGroupCount: 1,
          pathCount: 1,
          enabledUpstreamCount: 1,
          filteringEnabled: false,
          cacheEnabled: true,
          queryLogEnabled: true,
          routingRuleCount: 0,
          exceptionRuleCount: 0,
          deviceCount: 0,
          localPolicyCount: 0,
        },
      },
      canApply: true,
      details: {},
    },
  };
}

describe("Standard Mode transactional apply", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAuthStore.setState({
      isConnected: true,
      connectionEpoch: 100,
      serverConfig: {
        url: "/api",
        requiresAuth: false,
        username: "",
        password: "",
      },
    });
    useAppStore.setState({
      configVersion: "config-v1",
      webUiConfigVersion: "standard-v1",
      configPath: "/etc/oxidns/config.yaml",
      isOfflineMode: false,
      webUiMode: "expert",
      historyOpen: false,
      standardApplyConfirmation: null,
      loadConfig: vi.fn().mockImplementation(async () => {
        useAppStore.setState({ configVersion: "config-v2" });
      }),
    });
    apiMocks.applyStandardMode.mockResolvedValue({
      ok: true,
      transaction_id: "tx-1",
      status: "pending",
      target_config_version: "config-v2",
    });
    apiMocks.fetchStandardTransactionStatus.mockResolvedValue({
      ok: true,
      transaction: {
        schema: 1,
        transaction_id: "tx-1",
        status: "succeeded",
        completed_at_ms: 10,
        previous_config_version: "config-v1",
        candidate_config_version: "config-v2",
      },
    });
  });

  it("reviews an unmanaged takeover before sending the exact planned versions", async () => {
    const response = plan("unmanaged");
    apiMocks.planStandardMode.mockResolvedValue(response);

    const saving = useAppStore
      .getState()
      .saveStandardSettings(createDefaultStandardSettings(), { apply: true });
    await vi.waitFor(() => {
      expect(useAppStore.getState().standardApplyConfirmation).toBe(response);
    });
    expect(apiMocks.applyStandardMode).not.toHaveBeenCalled();

    useAppStore.getState().confirmStandardApply();
    await saving;

    expect(apiMocks.applyStandardMode).toHaveBeenCalledWith({
      intent: response.plan.normalizedIntent,
      baseConfigVersion: "config-v1",
      baseStandardVersion: "standard-v1",
      plannedConfigVersion: "config-v2",
      takeover: true,
    });
    expect(apiMocks.fetchStandardTransactionStatus).toHaveBeenCalled();
  });

  it("does not write when the review is cancelled", async () => {
    apiMocks.planStandardMode.mockResolvedValue(plan("managed"));
    const saving = useAppStore
      .getState()
      .saveStandardSettings(createDefaultStandardSettings(), { apply: true });
    await vi.waitFor(() => {
      expect(useAppStore.getState().standardApplyConfirmation).not.toBeNull();
    });

    useAppStore.getState().cancelStandardApply();
    await expect(saving).rejects.toThrow();
    expect(apiMocks.applyStandardMode).not.toHaveBeenCalled();
  });

  it("keeps Expert raw-YAML history inaccessible in Standard Mode", () => {
    useAppStore.setState({ isOfflineMode: true });
    useAppStore.getState().setHistoryOpen(true);
    expect(useAppStore.getState().historyOpen).toBe(true);

    useAppStore.getState().setWebUiMode("standard");
    expect(useAppStore.getState().historyOpen).toBe(false);
    useAppStore.getState().setHistoryOpen(true);
    expect(useAppStore.getState().historyOpen).toBe(false);
  });
});
