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
    steps,
  };
}

function metadata(): StandardGeneratedMetadata {
  return {
    configVersion: "config",
    settingsRevision: "settings",
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
});
