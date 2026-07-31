import type {
  QueryRecordDetail,
  QueryRecordRow,
  QueryRecorderStep,
} from "../oxidns-api";
import type {
  StandardExceptionRule,
  StandardGeneratedMetadata,
  StandardModeSettings,
  StandardResolutionPath,
  StandardRoutingRule,
  StandardUpstreamGroup,
} from "./types";

export type StandardQueryOutcome =
  | "error"
  | "blocked"
  | "cache"
  | "exception"
  | "routing"
  | "resolved"
  | "no_response"
  | "unknown";

export interface StandardQueryObjectRef {
  id: string;
  name: string;
  tag?: string;
}

export interface StandardQueryExplanation {
  initialPath?: StandardQueryObjectRef;
  finalPath?: StandardQueryObjectRef;
  path?: StandardQueryObjectRef;
  upstreamGroup?: StandardQueryObjectRef;
  routingRule?: StandardQueryObjectRef;
  exceptionRule?: StandardQueryObjectRef;
  semanticRole?: StandardQueryObjectRef;
  dedicatedGroup?: StandardQueryObjectRef;
  learningProfile?: StandardQueryObjectRef;
  advancedRule?: StandardQueryObjectRef & { phase: "request" | "response"; templateOrigin?: string };
  validationResult?: string;
  fallbackReason?: string;
  fallbackBranch?: "primary" | "secondary";
  outcome: StandardQueryOutcome;
  filtering: "blocked" | "checked" | "skipped" | "unknown";
  cache: "checked" | "unknown";
  queryLog: "recorded" | "unknown";
  rawEvents: string[];
  hasSteps: boolean;
  hasTagMap: boolean;
}

interface StandardQueryIndexes {
  pathsByTag: Map<string, StandardResolutionPath>;
  upstreamGroupsByTag: Map<string, StandardUpstreamGroup>;
  routingRulesByMatcherTag: Map<string, StandardRoutingRule>;
  exceptionRulesByMatcherTag: Map<string, StandardExceptionRule>;
  semanticRolesByMatcherTag: Map<string, string>;
  dedicatedGroupsByTag: Map<string, StandardQueryObjectRef>;
  learningProfilesByTag: Map<string, StandardQueryObjectRef>;
  advancedRulesByTag: Map<string, StandardQueryObjectRef & { phase: "request" | "response"; templateOrigin?: string }>;
  filteringTags: Set<string>;
  cacheTags: Set<string>;
  queryLogTag?: string;
}

export function explainStandardQueryRecord(
  record: QueryRecordRow | QueryRecordDetail,
  settings: StandardModeSettings,
  metadata: StandardGeneratedMetadata | null,
): StandardQueryExplanation {
  const steps = queryRecordSteps(record);
  const indexes = buildStandardQueryIndexes(settings, metadata);
  const matchedRouting = lastMatchedRule(
    steps,
    indexes.routingRulesByMatcherTag,
  );
  const matchedException = lastMatchedRule(
    steps,
    indexes.exceptionRulesByMatcherTag,
  );
  const paths = executedObjects(steps, indexes.pathsByTag);
  const upstreamGroups = executedObjects(steps, indexes.upstreamGroupsByTag);
  const initialPath = paths[0];
  const path = paths.at(-1);
  const upstreamGroup = upstreamGroups.at(-1);
  const semanticRole = lastMatchedRule(steps, indexes.semanticRolesByMatcherTag);
  const dedicatedGroup = lastObservedObject(steps, indexes.dedicatedGroupsByTag);
  const learningProfile = lastObservedObject(steps, indexes.learningProfilesByTag);
  const advancedRule = lastObservedObject(steps, indexes.advancedRulesByTag);
  const fallback = [...steps]
    .reverse()
    .find((step) => step.kind === "fallback" && step.outcome);
  const decision = [...steps]
    .reverse()
    .find((step) => step.kind === "decision" && step.outcome);
  const fallbackMatch = fallback?.outcome.match(/^(primary|secondary)_(.+)$/);
  const validationResult = decision?.outcome ??
    (steps.some(
      (step) =>
        step.kind === "matcher" &&
        step.tag === "standard_smart_domestic_response_ip" &&
        step.outcome === "matched",
    )
      ? "domestic_ip_valid"
      : undefined);
  const blocked = hasExecutedTag(steps, "standard_blocked");
  const filteringChecked =
    hasTag(steps, "standard_ad_rules") ||
    steps.some((step) => step.tag && indexes.filteringTags.has(step.tag));
  const cacheChecked =
    indexes.cacheTags.size > 0
      ? steps.some((step) => step.tag && indexes.cacheTags.has(step.tag))
      : hasTag(steps, "standard_cache");
  const queryLogRecorded = indexes.queryLogTag
    ? hasTag(steps, indexes.queryLogTag)
    : hasTag(steps, "standard_recorder");

  return {
    initialPath: initialPath ? pathRef(initialPath, metadata) : undefined,
    finalPath: path ? pathRef(path, metadata) : undefined,
    path: path ? pathRef(path, metadata) : undefined,
    upstreamGroup: upstreamGroup
      ? upstreamGroupRef(upstreamGroup, metadata)
      : undefined,
    routingRule: matchedRouting
      ? ruleRef(matchedRouting.rule, matchedRouting.tag)
      : undefined,
    exceptionRule: matchedException
      ? ruleRef(matchedException.rule, matchedException.tag)
      : undefined,
    semanticRole: semanticRole
      ? { id: semanticRole.rule, name: semanticRole.rule, tag: semanticRole.tag }
      : undefined,
    dedicatedGroup,
    learningProfile,
    advancedRule,
    validationResult,
    fallbackBranch: fallbackMatch?.[1] as "primary" | "secondary" | undefined,
    fallbackReason:
      fallbackMatch?.[1] === "secondary" ? fallbackMatch[2] : undefined,
    outcome: deriveOutcome(record, {
      blocked,
      cacheChecked,
      matchedException: Boolean(matchedException),
      matchedRouting: Boolean(matchedRouting),
      upstreamGroup: Boolean(upstreamGroup),
    }),
    filtering: blocked ? "blocked" : filteringChecked ? "checked" : "unknown",
    cache: cacheChecked ? "checked" : "unknown",
    queryLog: queryLogRecorded ? "recorded" : "unknown",
    rawEvents: steps.map(formatRawEvent),
    hasSteps: steps.length > 0,
    hasTagMap: Boolean(metadata?.tagMap),
  };
}

function queryRecordSteps(
  record: QueryRecordRow | QueryRecordDetail,
): QueryRecorderStep[] {
  return Array.isArray((record as Partial<QueryRecordDetail>).steps)
    ? (record as QueryRecordDetail).steps
    : [];
}

export function queryRecordDomain(
  record: QueryRecordRow | QueryRecordDetail,
): string {
  return cleanDomain(record.questions_json[0]?.name ?? "");
}

export function queryRecordQtype(
  record: QueryRecordRow | QueryRecordDetail,
): string {
  return record.questions_json[0]?.qtype ?? "-";
}

export function adGuardDomainRule(domain: string): string {
  const clean = cleanDomain(domain);
  return clean ? `||${clean}^` : "";
}

function buildStandardQueryIndexes(
  settings: StandardModeSettings,
  metadata: StandardGeneratedMetadata | null,
): StandardQueryIndexes {
  const pathsById = new Map(settings.paths.map((path) => [path.id, path]));
  const upstreamGroupsById = new Map(
    settings.upstreamGroups.map((group) => [group.id, group]),
  );
  const routingRulesById = new Map(
    settings.routing.rules.map((rule) => [rule.id, rule]),
  );
  const exceptionRulesById = new Map(
    settings.exceptions.map((rule) => [rule.id, rule]),
  );
  const pathsByTag = new Map<string, StandardResolutionPath>();
  const upstreamGroupsByTag = new Map<string, StandardUpstreamGroup>();
  const routingRulesByMatcherTag = new Map<string, StandardRoutingRule>();
  const exceptionRulesByMatcherTag = new Map<string, StandardExceptionRule>();
  const semanticRolesByMatcherTag = new Map<string, string>();
  const dedicatedGroupsByTag = new Map<string, StandardQueryObjectRef>();
  const learningProfilesByTag = new Map<string, StandardQueryObjectRef>();
  const advancedRulesByTag = new Map<string, StandardQueryObjectRef & { phase: "request" | "response"; templateOrigin?: string }>();
  const tagMap = metadata?.tagMap;

  for (const [id, tag] of Object.entries(tagMap?.paths ?? {})) {
    const path = pathsById.get(id);
    if (path) pathsByTag.set(tag, path);
  }
  for (const [id, tag] of Object.entries(tagMap?.upstreamGroups ?? {})) {
    const group = upstreamGroupsById.get(id);
    if (group) upstreamGroupsByTag.set(tag, group);
  }
  for (const [id, tag] of Object.entries(tagMap?.routingRules ?? {})) {
    const rule = routingRulesById.get(id);
    if (rule) routingRulesByMatcherTag.set(tag, rule);
  }
  for (const [id, tag] of Object.entries(tagMap?.exceptionRules ?? {})) {
    const rule = exceptionRulesById.get(id);
    if (rule) exceptionRulesByMatcherTag.set(tag, rule);
  }
  for (const [key, tag] of Object.entries(tagMap?.smartRouting ?? {})) {
    if (key.startsWith("matcher:")) {
      semanticRolesByMatcherTag.set(tag, key.slice("matcher:".length));
      continue;
    }
    const pathId = key.includes("remote")
      ? settings.smartRouting.remotePathId
      : key.includes("domestic") || key === "smart_ddns"
        ? settings.smartRouting.domesticPathId
        : undefined;
    const path = pathId ? pathsById.get(pathId) : undefined;
    if (path) pathsByTag.set(tag, path);
  }
  for (const group of settings.dedicatedGroups) {
    const tags = tagMap?.dedicatedGroups?.[group.id];
    if (!tags) continue;
    const value = { id: group.id, name: group.name, tag: tags.path };
    for (const tag of [tags.matcher, tags.path, tags.entry]) dedicatedGroupsByTag.set(tag, value);
  }
  for (const profile of settings.dynamicLearning.profiles) {
    const tags = tagMap?.dynamicLearning?.[profile.id];
    if (!tags) continue;
    const value = { id: profile.id, name: profile.name, tag: tags.action };
    for (const tag of [tags.matcher, tags.learner, tags.action]) learningProfilesByTag.set(tag, value);
  }
  for (const rule of settings.advancedRules) {
    const tag = tagMap?.advancedRules?.[rule.id];
    if (!tag) continue;
    advancedRulesByTag.set(tag, {
      id: rule.id,
      name: rule.name,
      tag,
      phase: rule.phase,
      ...(rule.templateOrigin ? { templateOrigin: rule.templateOrigin } : {}),
    });
  }

  return {
    pathsByTag,
    upstreamGroupsByTag,
    routingRulesByMatcherTag,
    exceptionRulesByMatcherTag,
    semanticRolesByMatcherTag,
    dedicatedGroupsByTag,
    learningProfilesByTag,
    advancedRulesByTag,
    filteringTags: new Set(tagMap?.filtering ?? []),
    cacheTags: new Set([
      ...Object.values(tagMap?.caches ?? {}),
      ...(tagMap?.cache ? [tagMap.cache] : []),
    ]),
    queryLogTag: tagMap?.queryLog,
  };
}

function lastObservedObject<T>(steps: QueryRecorderStep[], objects: Map<string, T>): T | undefined {
  for (const step of [...steps].reverse()) {
    if (!step.tag) continue;
    const value = objects.get(step.tag);
    if (value && (step.outcome === "matched" || step.outcome === "entered")) return value;
  }
  return undefined;
}

function lastMatchedRule<T>(
  steps: QueryRecorderStep[],
  rulesByTag: Map<string, T>,
): { rule: T; tag: string } | undefined {
  for (const step of [...steps].reverse()) {
    if (step.kind !== "matcher" || step.outcome !== "matched" || !step.tag) {
      continue;
    }
    const rule = rulesByTag.get(step.tag);
    if (rule) return { rule, tag: step.tag };
  }
  return undefined;
}

function executedObjects<T>(
  steps: QueryRecorderStep[],
  objectsByTag: Map<string, T>,
): T[] {
  const values: T[] = [];
  for (const step of steps) {
    if (step.kind !== "executor" || step.outcome !== "entered" || !step.tag) {
      continue;
    }
    const value = objectsByTag.get(step.tag);
    if (value) values.push(value);
  }
  return values;
}

function hasTag(steps: QueryRecorderStep[], tag: string): boolean {
  return steps.some((step) => step.tag === tag);
}

function hasExecutedTag(steps: QueryRecorderStep[], tag: string): boolean {
  return steps.some(
    (step) =>
      step.kind === "executor" &&
      step.tag === tag &&
      step.outcome === "entered",
  );
}

function deriveOutcome(
  record: QueryRecordRow | QueryRecordDetail,
  facts: {
    blocked: boolean;
    cacheChecked: boolean;
    matchedException: boolean;
    matchedRouting: boolean;
    upstreamGroup: boolean;
  },
): StandardQueryOutcome {
  if (record.error) return "error";
  if (facts.blocked) return "blocked";
  if (facts.cacheChecked && record.has_response && !facts.upstreamGroup)
    return "cache";
  if (facts.matchedException) return "exception";
  if (facts.matchedRouting) return "routing";
  if (record.has_response) return "resolved";
  if (!record.has_response) return "no_response";
  return "unknown";
}

function pathRef(
  path: StandardResolutionPath,
  metadata: StandardGeneratedMetadata | null,
): StandardQueryObjectRef {
  return { id: path.id, name: path.name, tag: metadata?.tagMap.paths[path.id] };
}

function upstreamGroupRef(
  group: StandardUpstreamGroup,
  metadata: StandardGeneratedMetadata | null,
): StandardQueryObjectRef {
  return {
    id: group.id,
    name: group.name,
    tag: metadata?.tagMap.upstreamGroups[group.id],
  };
}

function ruleRef(
  rule: StandardRoutingRule | StandardExceptionRule,
  tag: string,
): StandardQueryObjectRef {
  return { id: rule.id, name: rule.name, tag };
}

function formatRawEvent(step: QueryRecorderStep): string {
  const target = step.tag ? `${step.kind}:${step.tag}` : step.kind;
  const node =
    typeof step.node_index === "number"
      ? `#${step.node_index}`
      : `#${step.event_index}`;
  return `${node} ${step.sequence_tag} ${target} ${step.outcome}`;
}

function cleanDomain(value: string): string {
  return value.trim().replace(/\.$/, "").toLowerCase();
}
