import { describe, expect, it } from "vitest";

import type { QueryRecordDetail } from "../oxidns-api";
import { createDefaultStandardSettings } from "./defaults";
import { explainStandardQueryRecord } from "./query-explainer";
import type { StandardGeneratedMetadata } from "./types";

function record(steps: QueryRecordDetail["steps"]): QueryRecordDetail {
  return {
    id: 1,
    created_at_ms: 1,
    elapsed_ms: 4,
    request_id: 10,
    client_ip: "192.0.2.10",
    questions_json: [{ name: "route.example.", qtype: "A", qclass: "IN" }],
    has_response: true,
    rcode: "NoError",
    answer_count: 1,
    authority_count: 0,
    additional_count: 0,
    answers_json: [],
    authorities_json: [],
    additionals_json: [],
    signature_json: [],
    context: { intentRevision: "sha256:current" },
    steps_truncated: false,
    dropped_step_count: 0,
    steps,
  };
}

function metadata(): StandardGeneratedMetadata {
  return {
    configVersion: "config",
    settingsRevision: "settings",
    intentRevision: "sha256:current",
    generatedTags: [],
    tagMap: {
      system: [],
      filtering: ["standard_ad_rules", "standard_blocked"],
      caches: { private: "standard_cache_private" },
      queryLog: "standard_recorder",
      upstreamGroups: { private: "standard_forward_private" },
      paths: { private: "standard_path_private" },
      routingRules: { private_rule: "standard_route_match_private_rule" },
      exceptionRules: {},
    },
    summary: {
      upstreamGroupCount: 1,
      pathCount: 1,
      enabledUpstreamCount: 1,
      filteringEnabled: true,
      cacheEnabled: true,
      queryLogEnabled: true,
      routingRuleCount: 1,
      exceptionRuleCount: 0,
      deviceCount: 0,
      localPolicyCount: 0,
    },
    generatedAtMs: 1,
  };
}

describe("Standard query explanation", () => {
  it("maps runtime tags back to the selected rule, path, and upstream group", () => {
    const settings = createDefaultStandardSettings();
    settings.upstreamGroups[0].id = "private";
    settings.paths[0] = {
      ...settings.paths[0],
      id: "private",
      name: "Private path",
      upstreamGroupId: "private",
    };
    settings.routing.rules = [
      {
        id: "private_rule",
        name: "Private domains",
        enabled: true,
        condition: { type: "suffix", values: ["example"] },
        action: { type: "use_path", pathId: "private" },
        source: "manual",
      },
    ];

    const explanation = explainStandardQueryRecord(
      record([
        {
          event_index: 1,
          sequence_tag: "standard_main_sequence",
          kind: "matcher",
          tag: "standard_route_match_private_rule",
          outcome: "matched",
        },
        {
          event_index: 2,
          sequence_tag: "standard_main_sequence",
          kind: "executor",
          tag: "standard_path_private",
          outcome: "entered",
        },
        {
          event_index: 3,
          sequence_tag: "standard_path_private",
          kind: "executor",
          tag: "standard_cache_private",
          outcome: "entered",
        },
        {
          event_index: 4,
          sequence_tag: "standard_path_private",
          kind: "executor",
          tag: "standard_forward_private",
          outcome: "entered",
        },
      ]),
      settings,
      metadata(),
    );

    expect(explanation).toMatchObject({
      outcome: "routing",
      path: { id: "private", name: "Private path" },
      upstreamGroup: { id: "private" },
      routingRule: { id: "private_rule", name: "Private domains" },
      cache: "checked",
    });
  });

  it("classifies the native filter executor as a blocked outcome", () => {
    const settings = createDefaultStandardSettings();
    const explanation = explainStandardQueryRecord(
      record([
        {
          event_index: 1,
          sequence_tag: "standard_path_default",
          kind: "matcher",
          tag: "standard_ad_rules",
          outcome: "matched",
        },
        {
          event_index: 2,
          sequence_tag: "standard_path_default",
          kind: "executor",
          tag: "standard_blocked",
          outcome: "entered",
        },
      ]),
      settings,
      metadata(),
    );

    expect(explanation.outcome).toBe("blocked");
    expect(explanation.filtering).toBe("blocked");
  });

  it("explains initial validation, fallback reason, semantic role, and final path", () => {
    const settings = createDefaultStandardSettings();
    settings.paths[0].name = "Domestic";
    settings.paths.push({
      ...settings.paths[0],
      id: "remote",
      name: "Remote",
      upstreamGroupId: "private",
    });
    settings.smartRouting = {
      ...settings.smartRouting,
      enabled: true,
      domesticPathId: "default",
      remotePathId: "remote",
    };
    const generated = metadata();
    generated.tagMap.paths.default = "standard_path_default";
    generated.tagMap.paths.remote = "standard_path_remote";
    generated.tagMap.smartRouting = {
      "matcher:domestic_domains": "standard_smart_match_domestic_domains",
      smart_domestic_primary: "standard_path_smart_domestic_primary",
      smart_domestic_remote_fallback:
        "standard_path_smart_domestic_remote_fallback",
    };

    const explanation = explainStandardQueryRecord(
      record([
        {
          event_index: 1,
          sequence_tag: "standard_main_sequence",
          kind: "matcher",
          tag: "standard_smart_match_domestic_domains",
          outcome: "matched",
        },
        {
          event_index: 2,
          sequence_tag: "standard_main_sequence",
          kind: "executor",
          tag: "standard_path_smart_domestic_primary",
          outcome: "entered",
        },
        {
          event_index: 3,
          sequence_tag: "standard_smart_drop_domestic_ip_mismatch",
          kind: "decision",
          tag: "standard_smart_drop_domestic_ip_mismatch",
          outcome: "domestic_ip_mismatch",
        },
        {
          event_index: 4,
          sequence_tag: "standard_main_sequence",
          kind: "executor",
          tag: "standard_path_smart_domestic_remote_fallback",
          outcome: "entered",
        },
        {
          event_index: 5,
          sequence_tag: "standard_smart_domestic_fallback",
          kind: "fallback",
          tag: "standard_smart_domestic_fallback",
          outcome: "secondary_domestic_ip_mismatch",
        },
      ]),
      settings,
      generated,
    );

    expect(explanation).toMatchObject({
      initialPath: { id: "default", name: "Domestic" },
      finalPath: { id: "remote", name: "Remote" },
      semanticRole: { id: "domestic_domains" },
      validationResult: "domestic_ip_mismatch",
      fallbackBranch: "secondary",
      fallbackReason: "domestic_ip_mismatch",
    });
  });

  it("does not apply current object mappings to a record from another intent revision", () => {
    const settings = createDefaultStandardSettings();
    settings.upstreamGroups[0].id = "private";
    settings.paths[0] = {
      ...settings.paths[0],
      id: "private",
      name: "Current private path",
      upstreamGroupId: "private",
    };
    const historical = record([
      {
        event_index: 1,
        sequence_tag: "standard_main_sequence",
        kind: "executor",
        tag: "standard_path_private",
        outcome: "entered",
      },
    ]);
    historical.context = { intentRevision: "sha256:historical" };

    const explanation = explainStandardQueryRecord(
      historical,
      settings,
      metadata(),
    );

    expect(explanation.hasTagMap).toBe(false);
    expect(explanation.path).toBeUndefined();
    expect(explanation.upstreamGroup).toBeUndefined();
    expect(explanation.rawEvents).toHaveLength(1);
  });
});
