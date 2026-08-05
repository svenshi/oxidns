import { parse, stringify } from "yaml";

import type { BuildInfo } from "../oxidns-api";
import { normalizeStandardSettings } from "./schema";
import type {
  StandardCompilationExplanation,
  StandardDiagnostic,
  StandardGeneratedPlan,
  StandardGenerationSummary,
  StandardModeSettings,
  StandardPolicyPlan,
  StandardResolutionPath,
  StandardRuleCondition,
  StandardTagMap,
  StandardUpstream,
  StandardUpstreamGroup,
} from "./types";
import {
  validateStandardDeviceSettings,
  validateStandardDnsSettings,
  validateStandardExceptionSettings,
  validateStandardFilteringSettings,
  validateStandardLocalSettings,
  validateStandardRoutingSettings,
} from "./validation";

const QUERY_RECORD_MARK = 0xffff_fffe;
const QUERY_SKIP_MARK = 0xffff_ffff;
const FILTER_DIR = "./data/standard-filter-subscriptions";
const RULE_DATA_DIR = "./data/standard-rule-data";
const LEARNING_DIR = "./data/standard-dynamic-learning";

type Json = null | boolean | number | string | Json[] | { [key: string]: Json | undefined };
type Plugin = { tag: string; type: string; args: Json };
type MutableTagMap = StandardTagMap & {
  caches: Record<string, string>;
  filtering: string[];
  filterSubscriptions: NonNullable<StandardTagMap["filterSubscriptions"]>;
  local: Record<string, string>;
  upstreamMembers: NonNullable<StandardTagMap["upstreamMembers"]>;
  devices: Record<string, string>;
  ruleData: Record<string, string>;
  ruleDataSources: NonNullable<StandardTagMap["ruleDataSources"]>;
  smartRouting: Record<string, string>;
  dedicatedGroups: NonNullable<StandardTagMap["dedicatedGroups"]>;
  dynamicLearning: NonNullable<StandardTagMap["dynamicLearning"]>;
  advancedRules: Record<string, string>;
};

export interface StandardCompilerInput {
  intent: unknown;
  baseYaml?: string | null;
  build: BuildInfo;
}

export async function standardIntentRevision(intent: unknown): Promise<string> {
  const normalized = normalizeStandardSettings(intent).settings;
  return `sha256:${await sha256(JSON.stringify(normalized))}`;
}

export async function compileStandardIntent({
  intent,
  baseYaml,
  build,
}: StandardCompilerInput): Promise<StandardPolicyPlan> {
  const loaded = normalizeStandardSettings(intent);
  const normalizedIntent = loaded.settings;
  const diagnostics: StandardDiagnostic[] = [];
  if (loaded.notice === "invalid_fallback") {
    diagnostics.push(error("intent_invalid", "intent", "Standard intent is invalid"));
  } else if (loaded.notice === "legacy_migrated") {
    diagnostics.push({
      severity: "warning",
      code: "schema_migrated",
      path: "schema",
      message: "The intent was migrated to schema 6",
    });
  }

  let base: Record<string, unknown> = {};
  if (baseYaml?.trim()) {
    try {
      const parsed = parse(baseYaml);
      if (!isRecord(parsed)) throw new Error("root must be a mapping");
      base = parsed;
    } catch (cause) {
      diagnostics.push(
        error(
          "base_config_invalid",
          "baseConfig",
          cause instanceof Error ? cause.message : String(cause),
        ),
      );
    }
  }
  diagnostics.push(...validateIntent(normalizedIntent, build));
  if (diagnostics.some((item) => item.severity === "error")) {
    return {
      normalizedIntent,
      diagnostics,
      canApply: false,
      details: { intentRevision: await standardIntentRevision(normalizedIntent) },
    };
  }

  const generated = await generate(normalizedIntent, base, build);
  diagnostics.push(...capabilityDiagnostics(generated, build));
  return {
    normalizedIntent,
    diagnostics,
    generated:
      diagnostics.some((item) => item.severity === "error") ? undefined : generated,
    canApply: !diagnostics.some((item) => item.severity === "error"),
    details: {
      intentRevision: generated.explanation?.intentRevision,
      managedTopLevel: ["runtime.worker_threads", "log.level", "plugins"],
      preservedTopLevel: ["include", "api", "network", "log.* except level"],
    },
  };
}

async function generate(
  intent: StandardModeSettings,
  base: Record<string, unknown>,
  build: BuildInfo,
): Promise<StandardGeneratedPlan> {
  const plugins: Plugin[] = [];
  const tagMap = emptyTagMap();
  const intentRevision = await standardIntentRevision(intent);

  if (build.supported_plugins.executors.includes("metrics_collector")) {
    add(plugins, "standard_metrics", "metrics_collector", {});
    tagMap.system.push("standard_metrics");
  }
  if (effectiveQueryLogUsed(intent)) {
    add(plugins, "standard_recorder", "query_recorder", {
      path: "./data/standard-query-recorder.sqlite",
      queue_size: 8192,
      batch_size: 256,
      flush_interval_ms: 200,
      memory_tail: 1024,
      retention_days: Math.max(1, intent.queryLog.retentionDays),
      cleanup_interval_hours: 1,
      reader_concurrency: 2,
      max_steps: 512,
      context: {
        schema: "standard-query-diagnostic:1",
        intentRevision,
        role: "standard",
      },
      include_marks: intent.queryLog.enabled ? [] : [QUERY_RECORD_MARK],
      exclude_marks: intent.queryLog.enabled ? [QUERY_SKIP_MARK] : [],
    });
    tagMap.queryLog = "standard_recorder";
  }

  compileFiltering(intent, plugins, tagMap);
  compileLocal(intent, plugins, tagMap);
  compileRuleData(intent, plugins, tagMap);
  const learning = compileLearningPrimitives(intent, plugins, tagMap);

  if (intent.exceptions.some((rule) => rule.enabled && rule.action.type === "prefer_ipv4")) {
    add(plugins, "standard_prefer_ipv4", "prefer_ipv4", { cache: true, cache_ttl: 3600 });
  }
  if (intent.exceptions.some((rule) => rule.enabled && rule.action.type === "prefer_ipv6")) {
    add(plugins, "standard_prefer_ipv6", "prefer_ipv6", { cache: true, cache_ttl: 3600 });
  }

  for (const group of intent.upstreamGroups) {
    const tag = standardTag("forward", group.id);
    compileForward(tag, group.strategy, group.upstreams, plugins);
    tagMap.upstreamGroups[group.id] = tag;
    tagMap.upstreamMembers[group.id] = Object.fromEntries(
      group.upstreams.filter((item) => item.enabled).map((item) => [item.id, item.id]),
    );
  }

  const advanced = compileAdvanced(intent, learning.tail, plugins, tagMap);
  for (const path of intent.paths) {
    const forward = tagMap.upstreamGroups[path.upstreamGroupId];
    const tag = compilePath(path, intent, forward, path.id, plugins, tagMap, {
      tail: [...learning.tail, ...(advanced.responseTails[path.id] ?? [])],
    });
    tagMap.paths[path.id] = tag;
    if (pathCacheEnabled(path, intent)) tagMap.caches[path.id] = standardTag("cache", path.id);
  }

  const dedicatedRoutes = compileDedicated(intent, learning.tail, plugins, tagMap);
  const defaultPath = intent.paths[0];
  const defaultPathTag = tagMap.paths[defaultPath.id];

  const exceptionActions: Record<string, string> = {};
  for (const rule of intent.exceptions.filter((item) => item.enabled)) {
    const matcher = compileMatcher(rule.condition);
    const matcherTag = standardTag("exception_match", rule.id);
    add(plugins, matcherTag, matcher.type, matcher.args);
    tagMap.exceptionRules[rule.id] = matcherTag;
    if (rule.action.type !== "use_path" && rule.action.type !== "use_default_path") {
      const actionTag = standardTag("exception_action", rule.id);
      add(
        plugins,
        actionTag,
        "sequence",
        exceptionSequence(rule.action.type, defaultPath, intent, tagMap),
      );
      exceptionActions[rule.id] = actionTag;
    }
  }

  const deviceActions: Record<string, string> = {};
  for (const device of intent.devices.filter(deviceHasPolicy)) {
    const matcherTag = standardTag("device_match", device.id);
    add(plugins, matcherTag, "client_ip", device.addresses);
    tagMap.devices[device.id] = matcherTag;
    const path = intent.paths.find((item) => item.id === device.assignedPathId) ?? defaultPath;
    const actionTag = standardTag("device_action", device.id);
    add(
      plugins,
      actionTag,
      "sequence",
      buildPathSequence(path, intent, tagMap.upstreamGroups[path.upstreamGroupId], tagMap, {
        disableFiltering: device.filtering === "disabled",
        forceFiltering: device.filtering === "enabled",
        disableQueryLog: device.queryLog === "disabled",
        forceQueryLog: device.queryLog === "enabled",
      }),
    );
    deviceActions[device.id] = actionTag;
  }

  if (intent.routing.enabled) {
    for (const rule of intent.routing.rules.filter((item) => item.enabled)) {
      const matcher = compileMatcher(rule.condition);
      const tag = standardTag("route_match", rule.id);
      add(plugins, tag, matcher.type, matcher.args);
      tagMap.routingRules[rule.id] = tag;
    }
  }
  const learnedRoutes = compileLearningRoutes(intent, plugins, tagMap);
  const smart = compileSmart(intent, plugins, tagMap);

  const main: Json[] = [];
  if (tagMap.system.includes("standard_metrics")) main.push({ exec: "$standard_metrics" });
  if (tagMap.queryLog) main.push({ exec: "$standard_recorder" });
  for (const key of ["hosts", "records", "redirect"]) {
    const tag = tagMap.local[key];
    if (tag) main.push({ exec: `$${tag}` });
  }
  if (tagMap.local.qtypeMatcher && tagMap.local.qtypeAction) {
    main.push({ matches: `$${tagMap.local.qtypeMatcher}`, exec: `$${tagMap.local.qtypeAction}` });
  }
  appendExceptionRoutes(main, orderedExceptions(intent).filter((rule) =>
    ["block", "allow", "skip_filtering"].includes(rule.action.type),
  ), tagMap, exceptionActions, defaultPathTag);
  if (tagMap.local.ddnsMatcher) {
    const path = intent.paths.find((item) => item.id === intent.local.ddns.pathId) ?? defaultPath;
    const tag = "standard_local_ddns_action";
    add(plugins, tag, "sequence", buildPathSequence(
      path, intent, tagMap.upstreamGroups[path.upstreamGroupId], tagMap,
      { disableCache: true, responseTtlTag: "standard_local_ddns_ttl" },
    ));
    tagMap.local.ddnsAction = tag;
    main.push({ matches: `$${tagMap.local.ddnsMatcher}`, exec: `$${tag}` });
  }
  for (const device of intent.devices.filter(deviceHasPolicy)) {
    main.push({ matches: `$${tagMap.devices[device.id]}`, exec: `$${deviceActions[device.id]}` });
  }
  for (const [matcher, action] of dedicatedRoutes) main.push({ matches: `$${matcher}`, exec: `$${action}` });
  appendExceptionRoutes(main, orderedExceptions(intent).filter((rule) =>
    !["block", "allow", "skip_filtering"].includes(rule.action.type),
  ), tagMap, exceptionActions, defaultPathTag);
  if (intent.routing.enabled) {
    for (const rule of intent.routing.rules.filter((item) => item.enabled)) {
      const target = rule.action.type === "use_path"
        ? tagMap.paths[rule.action.pathId]
        : rule.action.type === "use_default_path" ? defaultPathTag : undefined;
      if (target) main.push({ matches: `$${tagMap.routingRules[rule.id]}`, exec: `$${target}` });
    }
  }
  main.push(...advanced.requestRoutes.map(([matches, action]) => ({ matches, exec: `$${action}` })));
  for (const [matcher, action] of learnedRoutes) main.push({ matches: `$${matcher}`, exec: `$${action}` });
  if (smart) {
    for (const [matcher, action] of smart.routes) main.push({ matches: `$${matcher}`, exec: `$${action}` });
    main.push({ exec: `$${smart.unknown}` });
  } else {
    main.push({ exec: `$${defaultPathTag}` });
  }
  main.push({ exec: "accept" });
  add(plugins, "standard_main_sequence", "sequence", main);
  if (intent.listen.udp) add(plugins, "standard_udp", "udp_server", { listen: intent.listen.address, entry: "standard_main_sequence" });
  if (intent.listen.tcp) add(plugins, "standard_tcp", "tcp_server", { listen: intent.listen.address, entry: "standard_main_sequence" });

  const root: Record<string, unknown> = {};
  for (const key of ["include", "api", "network"] as const) if (base[key] !== undefined) root[key] = base[key];
  const runtime = isRecord(base.runtime) ? { ...base.runtime } : {};
  delete runtime.threads;
  if (intent.system.threads) runtime.worker_threads = intent.system.threads;
  else delete runtime.worker_threads;
  if (Object.keys(runtime).length) root.runtime = runtime;
  const log = isRecord(base.log) ? { ...base.log } : {};
  log.level = intent.system.logLevel;
  root.log = log;
  root.plugins = plugins;
  const yaml = `# oxidns-webui.mode: standard\n${stringify(root, { lineWidth: 0 })}`;
  const generatedTags = plugins.map((plugin) => plugin.tag);
  const explanation = buildExplanation(intent, tagMap, generatedTags, build, intentRevision);
  return {
    yaml,
    configVersion: await sha256(yaml),
    pluginCount: plugins.length,
    generatedTags,
    tagMap,
    summary: summarize(intent),
    managedFiles: learning.managedFiles,
    explanation,
  };
}

function compileFiltering(intent: StandardModeSettings, plugins: Plugin[], tags: MutableTagMap) {
  const used = effectiveFilteringUsed(intent);
  const subscriptions = intent.filtering.subscriptions.filter((item) => item.enabled);
  const files = intent.filtering.localFiles.filter((item) => item.enabled);
  for (const subscription of used ? subscriptions : []) {
    const component = safe(subscription.id);
    const download = `standard_filter_download_${component}`;
    const filename = `${component}.txt`;
    add(plugins, download, "download", { startup_if_missing: true, fail_on_error: true, downloads: [{ url: subscription.url, dir: FILTER_DIR, filename }] });
    tags.filtering.push(download);
  }
  const hasRules = used && Boolean(intent.filtering.blockRules.length || intent.filtering.allowRules.length || subscriptions.length || files.length);
  if (hasRules) {
    add(plugins, "standard_ad_rules", "adguard_rule", {
      files: [...subscriptions.map((item) => `${FILTER_DIR}/${safe(item.id)}.txt`), ...files.map((item) => item.path)],
      rules: [...intent.filtering.blockRules, ...intent.filtering.allowRules],
    });
    tags.filtering.push("standard_ad_rules");
  }
  if (hasRules || intent.exceptions.some((rule) => rule.enabled && rule.action.type === "block")) {
    add(plugins, "standard_blocked", "black_hole", { mode: blockMode(intent.filtering.blockResponse), short_circuit: true });
    tags.filtering.push("standard_blocked");
  }
  if (used && subscriptions.length) {
    add(plugins, "standard_filter_reload", "reload_provider", ["$standard_ad_rules"]);
    tags.filtering.push("standard_filter_reload");
    for (const subscription of subscriptions) {
      const component = safe(subscription.id);
      const download = `standard_filter_download_${component}`;
      const cron = `standard_filter_cron_${component}`;
      const job = `refresh_filter_${component}`;
      add(plugins, cron, "cron", { jobs: [{ name: job, interval: `${Math.max(1, subscription.updateIntervalHours)}h`, executors: [`$${download}`, "$standard_filter_reload"], stop_on_error: true }] });
      tags.filtering.push(cron);
      tags.filterSubscriptions[subscription.id] = { download, cron, job };
    }
  }
}

function compileLocal(intent: StandardModeSettings, plugins: Plugin[], tags: MutableTagMap) {
  const local = intent.local;
  if (local.hosts.entries.length || local.hosts.files.length) {
    add(plugins, "standard_local_hosts", "hosts", { entries: local.hosts.entries, files: local.hosts.files, short_circuit: true });
    tags.local.hosts = "standard_local_hosts";
  }
  if (local.records.rules.length || local.records.files.length) {
    add(plugins, "standard_local_records", "arbitrary", { rules: local.records.rules, files: local.records.files, short_circuit: true });
    tags.local.records = "standard_local_records";
  }
  if (local.redirects.rules.length || local.redirects.files.length) {
    add(plugins, "standard_local_redirect", "redirect", { rules: local.redirects.rules, files: local.redirects.files });
    tags.local.redirect = "standard_local_redirect";
  }
  if (local.responseTtl.enabled) {
    add(plugins, "standard_local_response_ttl", "ttl", { min: local.responseTtl.min, max: local.responseTtl.max });
    tags.local.responseTtl = "standard_local_response_ttl";
  }
  if (local.qtypePolicy.enabled) {
    add(plugins, "standard_local_qtype_match", "qtype", local.qtypePolicy.qtypes);
    add(plugins, "standard_local_qtype_action", "black_hole", { mode: blockMode(local.qtypePolicy.response), short_circuit: true });
    tags.local.qtypeMatcher = "standard_local_qtype_match";
    tags.local.qtypeAction = "standard_local_qtype_action";
  }
  if (local.ddns.enabled) {
    add(plugins, "standard_local_ddns_match", "qname", local.ddns.domains.map((domain) => `full:${domain}`));
    add(plugins, "standard_local_ddns_ttl", "ttl", { fix: local.ddns.ttl });
    tags.local.ddnsMatcher = "standard_local_ddns_match";
    tags.local.ddnsTtl = "standard_local_ddns_ttl";
  }
}

function compileRuleData(intent: StandardModeSettings, plugins: Plugin[], tags: MutableTagMap) {
  for (const [roleName, role] of Object.entries(intent.ruleData) as Array<
    [string, StandardModeSettings["ruleData"][keyof StandardModeSettings["ruleData"]]]
  >) {
    const sources = role.sources.filter((source) => source.enabled);
    if (!sources.length) continue;
    const isIp = roleName === "domesticIps";
    const roleKey = camelToSnake(roleName);
    const roleComponent = safe(roleKey);
    const roleTag = `standard_rule_data_${roleComponent}`;
    const rules: string[] = [];
    const files: string[] = [];
    const sets: string[] = [];
    const subscriptions: Array<[string, string, string, string, number]> = [];
    for (const source of sources) {
      const component = safe(source.id);
      if (source.type === "manual") rules.push(...source.rules);
      if (source.type === "local_file") files.push(source.path);
      if (source.type === "native_dat") {
        const tag = `standard_rule_data_native_${roleComponent}_${component}`;
        add(plugins, tag, isIp ? "geoip" : "geosite", { file: source.path, selectors: source.selectors });
        sets.push(tag);
      }
      if (source.type === "subscription") {
        const filename = `${roleComponent}_${component}.txt`;
        const download = `standard_rule_data_download_${roleComponent}_${component}`;
        const cron = `standard_rule_data_cron_${roleComponent}_${component}`;
        const job = `refresh_rule_data_${roleComponent}_${component}`;
        add(plugins, download, "download", { startup_if_missing: true, fail_on_error: true, downloads: [{ url: source.url, dir: RULE_DATA_DIR, filename }] });
        files.push(`${RULE_DATA_DIR}/${filename}`);
        subscriptions.push([`${roleKey}:${source.id}`, download, cron, job, source.updateIntervalHours]);
      }
    }
    add(plugins, roleTag, isIp ? "ip_set" : "domain_set", isIp
      ? { ips: rules, files, sets }
      : { exps: rules, files, sets });
    tags.ruleData[roleKey] = roleTag;
    if (subscriptions.length) {
      const reload = `standard_rule_data_reload_${roleComponent}`;
      add(plugins, reload, "reload_provider", [`$${roleTag}`]);
      for (const [key, download, cron, job, interval] of subscriptions) {
        add(plugins, cron, "cron", { jobs: [{ name: job, interval: `${Math.max(1, interval)}h`, executors: [`$${download}`, `$${reload}`], stop_on_error: true }] });
        tags.ruleDataSources[key] = { download, cron, job };
      }
    }
  }
}

function compileLearningPrimitives(intent: StandardModeSettings, plugins: Plugin[], tags: MutableTagMap) {
  const tail: Json[] = [];
  const managedFiles: string[] = [];
  for (const profile of [...intent.dynamicLearning.profiles].filter((item) => item.enabled).sort(byPriority)) {
    const component = safe(profile.id);
    const provider = standardTag("learn_provider", profile.id);
    const learner = standardTag("learn_exec", profile.id);
    const matcher = standardTag("learn_match", profile.id);
    const qtype = standardTag("learn_qtype", profile.id);
    const rcode = standardTag("learn_rcode", profile.id);
    const answer = standardTag("learn_answer", profile.id);
    const responseIp = profile.responseIpRole ? standardTag("learn_resp_ip", profile.id) : undefined;
    const rulesPath = `${LEARNING_DIR}/${component}.txt`;
    const metadataPath = `${LEARNING_DIR}/${component}.meta.json`;
    add(plugins, provider, "dynamic_domain_set", {
      path: rulesPath, metadata_path: metadataPath, max_entries: profile.maxEntries,
      entry_ttl_seconds: profile.entryTtlSeconds, cleanup_interval_seconds: profile.cleanupIntervalSeconds,
      queue_size: profile.queueSize, batch_size: profile.batchSize, flush_interval_ms: profile.flushIntervalMs,
    });
    add(plugins, matcher, "qname", [`$${provider}`]);
    add(plugins, qtype, "qtype", profile.qtypes);
    add(plugins, rcode, "rcode", profile.rcodes);
    if (profile.answerRequired) add(plugins, answer, "has_wanted_ans", {});
    if (responseIp) add(plugins, responseIp, "resp_ip", [`$${tags.ruleData[camelToSnake(profile.responseIpRole!)] ?? tags.ruleData[profile.responseIpRole!]}`]);
    add(plugins, learner, "learn_domain", {
      provider, phase: "before", questions: "first", qtypes: profile.qtypes,
      success_only: false, answer_required: false, rule_kind: profile.ruleKind,
      async: profile.failurePolicy === "continue", error_mode: profile.failurePolicy === "continue" ? "continue" : "fail",
      timeout: "1s", paused: profile.paused,
    });
    const matches = [`$${qtype}`, `$${rcode}`];
    if (profile.answerRequired) matches.push(`$${answer}`);
    if (responseIp) matches.push(`$${responseIp}`);
    tail.push({ matches, exec: `$${learner}` });
    managedFiles.push(rulesPath, metadataPath);
    tags.dynamicLearning[profile.id] = { provider, learner, matcher, action: "", rulesPath, metadataPath };
  }
  managedFiles.sort();
  return { tail, managedFiles };
}

function compileLearningRoutes(intent: StandardModeSettings, plugins: Plugin[], tags: MutableTagMap): Array<[string, string]> {
  return [...intent.dynamicLearning.profiles].filter((item) => item.enabled).sort(byPriority).map((profile) => {
    const action = standardTag("learn_action", profile.id);
    add(plugins, action, "sequence", [{ exec: `$${tags.paths[profile.targetPathId]}` }, { exec: "accept" }]);
    tags.dynamicLearning[profile.id].action = action;
    return [tags.dynamicLearning[profile.id].matcher, action];
  });
}

function compileDedicated(intent: StandardModeSettings, tail: Json[], plugins: Plugin[], tags: MutableTagMap): Array<[string, string]> {
  const routes: Array<[number, number, string, string]> = [];
  intent.dedicatedGroups.forEach((group, index) => {
    if (!group.enabled) return;
    const provider = standardTag("dedicated_provider", group.id);
    const matcher = standardTag("dedicated_match", group.id);
    const forward = standardTag("dedicated_forward", group.id);
    add(plugins, provider, "domain_set", { exps: group.rules, files: [], sets: [] });
    add(plugins, matcher, "qname", [`$${provider}`]);
    compileForward(forward, group.strategy, group.upstreams, plugins);
    tags.upstreamMembers[`dedicated:${group.id}`] = Object.fromEntries(group.upstreams.filter((item) => item.enabled).map((item) => [item.id, item.id]));
    const path: StandardResolutionPath = { id: `dedicated_${group.id}`, name: group.name, description: group.description, upstreamGroupId: group.id, ...group.path };
    const pathTag = compilePath(path, intent, forward, `dedicated:${group.id}`, plugins, tags, { tail });
    const cache = pathCacheEnabled(path, intent) ? pathBundleTag("cache", `dedicated:${group.id}`) : undefined;
    if (cache) tags.caches[`dedicated:${group.id}`] = cache;
    const entry = standardTag("dedicated_entry", group.id);
    add(plugins, entry, "sequence", [...(tags.queryLog ? [{ exec: "$standard_recorder" }] : []), { exec: `$${pathTag}` }, { exec: "accept" }]);
    let udpListener: string | undefined;
    let tcpListener: string | undefined;
    if (group.listener.enabled && group.listener.udp) {
      udpListener = standardTag("dedicated_udp", group.id);
      add(plugins, udpListener, "udp_server", { listen: group.listener.address, entry });
    }
    if (group.listener.enabled && group.listener.tcp) {
      tcpListener = standardTag("dedicated_tcp", group.id);
      add(plugins, tcpListener, "tcp_server", { listen: group.listener.address, entry });
    }
    tags.dedicatedGroups[group.id] = { provider, matcher, upstreamGroup: forward, path: pathTag, entry, cache, udpListener, tcpListener };
    routes.push([group.priority, index, matcher, pathTag]);
  });
  return routes.sort((a, b) => a[0] - b[0] || a[1] - b[1]).map(([, , matcher, action]) => [matcher, action]);
}

function compileAdvanced(intent: StandardModeSettings, learningTail: Json[], plugins: Plugin[], tags: MutableTagMap) {
  const requestRoutes: Array<[string[], string]> = [];
  const responseTails: Record<string, Json[]> = {};
  for (const rule of [...intent.advancedRules].filter((item) => item.enabled).sort(byPriority)) {
    const matches: string[] = [];
    let sourcePath: string | undefined;
    rule.conditions.forEach((condition, index) => {
      if (condition.type === "source_path") { sourcePath = condition.pathId; return; }
      const tag = standardTag("advanced_match", `${rule.id}_${index}`);
      const compiled = compileAdvancedMatcher(condition, tags);
      add(plugins, tag, compiled.type, compiled.args);
      matches.push(`${compiled.invert ? "!" : ""}$${tag}`);
    });
    const action = standardTag("advanced_action", rule.id);
    if (rule.phase === "request") {
      if (rule.action.type === "use_path") add(plugins, action, "sequence", [{ exec: `$${standardTag("path", rule.action.pathId)}` }, { exec: "accept" }]);
      else add(plugins, action, "black_hole", { mode: blockMode(rule.action.response), short_circuit: true });
      requestRoutes.push([matches, action]);
    } else if (rule.action.type === "use_path" && sourcePath) {
      const targetPathId = rule.action.pathId;
      const target = intent.paths.find((path) => path.id === targetPathId)!;
      const drop = standardTag("advanced_drop", rule.id);
      add(plugins, drop, "drop_resp", { reason: `advanced_rule_${rule.id}` });
      const targetTag = compilePath(target, intent, tags.upstreamGroups[target.upstreamGroupId], `advanced_target_${rule.id}`, plugins, tags, { prelude: [{ exec: `$${drop}` }], tail: learningTail });
      const secondary = standardTag("advanced_secondary", rule.id);
      if (rule.failurePolicy === "fail_open") add(plugins, secondary, "sequence", [{ exec: "accept" }]);
      else add(plugins, secondary, "black_hole", { mode: rule.failureResponse, short_circuit: true });
      add(plugins, action, "fallback", { primary: targetTag, secondary, threshold: 60000, short_circuit: true, fallback_on_timeout: false, fallback_on_error: true, fallback_on_no_response: true });
      (responseTails[sourcePath] ??= []).push({ matches, exec: `$${action}` });
    }
    tags.advancedRules[rule.id] = action;
  }
  return { requestRoutes, responseTails };
}

function compileAdvancedMatcher(condition: StandardModeSettings["advancedRules"][number]["conditions"][number], tags: MutableTagMap): { type: string; args: Json; invert?: boolean } {
  switch (condition.type) {
    case "domain": return { type: "qname", args: condition.values.map((value) => `full:${value}`) };
    case "suffix": return { type: "qname", args: condition.values.map((value) => `domain:${value}`) };
    case "keyword": return { type: "qname", args: condition.values.map((value) => `keyword:${value}`) };
    case "client_cidr": return { type: "client_ip", args: condition.values };
    case "qtype": return { type: "qtype", args: condition.values };
    case "time": return { type: "time", args: { timezone: condition.timezone, periods: condition.periods as unknown as Json } };
    case "rate_limit_exceeded": return { type: "rate_limiter", args: { qps: condition.qps, burst: condition.burst, mask4: condition.mask4, mask6: condition.mask6 }, invert: true };
    case "cname": return { type: "cname", args: condition.values };
    case "rcode": return { type: "rcode", args: condition.values };
    case "has_wanted_answer": return { type: "has_wanted_ans", args: {} };
    case "response_ip_role": return { type: "resp_ip", args: [`$${tags.ruleData[camelToSnake(condition.role)] ?? tags.ruleData[condition.role]}`], invert: condition.invert };
    case "source_path": throw new Error("source_path is not a matcher");
  }
}

function compileSmart(intent: StandardModeSettings, plugins: Plugin[], tags: MutableTagMap): { routes: Array<[string, string]>; unknown: string } | null {
  const smart = intent.smartRouting;
  if (!smart.enabled) return null;
  const domestic = intent.paths.find((path) => path.id === smart.domesticPathId)!;
  const remote = intent.paths.find((path) => path.id === smart.remotePathId)!;
  const domesticForward = tags.upstreamGroups[domestic.upstreamGroupId];
  const remoteForward = tags.upstreamGroups[remote.upstreamGroupId];
  const fixed: Array<[string, string, Json]> = [
    ["standard_smart_address_qtype", "qtype", ["A", "AAAA"]],
    ["standard_smart_rcode_noerror", "rcode", ["NOERROR"]],
    ["standard_smart_rcode_nxdomain", "rcode", ["NXDOMAIN"]],
    ["standard_smart_rcode_servfail", "rcode", ["SERVFAIL"]],
    ["standard_smart_has_wanted_answer", "has_wanted_ans", {}],
    ["standard_smart_has_cname", "cname", ["regexp:.*"]],
    ["standard_smart_domestic_response_ip", "resp_ip", [`$${tags.ruleData.domestic_ips}`]],
  ];
  for (const [tag, type, args] of fixed) add(plugins, tag, type, args);
  const drops: Record<string, string> = {};
  for (const reason of ["domestic_ip_mismatch", "cname_only", "nodata", "nxdomain", "servfail"]) {
    drops[reason] = `standard_smart_drop_${reason}`;
    add(plugins, drops[reason], "drop_resp", { reason });
  }
  const tail: Json[] = [];
  const drop = (enabled: boolean, matches: string[], reason: string) => { if (enabled) tail.push({ matches, exec: `$${drops[reason]}` }); };
  drop(smart.responsePolicy.servfail, ["$standard_smart_address_qtype", "$standard_smart_rcode_servfail"], "servfail");
  drop(smart.responsePolicy.nxdomain, ["$standard_smart_address_qtype", "$standard_smart_rcode_nxdomain"], "nxdomain");
  drop(smart.responsePolicy.cnameOnly, ["$standard_smart_address_qtype", "$standard_smart_rcode_noerror", "!$standard_smart_has_wanted_answer", "$standard_smart_has_cname"], "cname_only");
  drop(smart.responsePolicy.nodata, ["$standard_smart_address_qtype", "$standard_smart_rcode_noerror", "!$standard_smart_has_wanted_answer", "!$standard_smart_has_cname"], "nodata");
  drop(smart.responsePolicy.domesticIpMismatch, ["$standard_smart_address_qtype", "$standard_smart_rcode_noerror", "$standard_smart_has_wanted_answer", "!$standard_smart_domestic_response_ip"], "domestic_ip_mismatch");
  const variant = (path: StandardResolutionPath, forward: string, namespace: string, responseTail: Json[] = []) => {
    const tag = compilePath(path, intent, forward, namespace, plugins, tags, { tail: responseTail });
    if (pathCacheEnabled(path, intent)) tags.caches[`smart:${namespace}`] = standardTag("cache", namespace);
    tags.smartRouting[namespace] = tag;
    return tag;
  };
  const domesticPrimary = variant(domestic, domesticForward, "smart_domestic_primary", tail);
  const domesticRemote = variant(remote, remoteForward, "smart_domestic_remote_fallback");
  const domesticAction = "standard_smart_domestic_fallback";
  add(plugins, domesticAction, "fallback", fallbackArgs(domesticPrimary, domesticRemote, smart.fallbackThresholdMs, smart));
  tags.smartRouting.domesticAction = domesticAction;
  const remoteAction = variant(remote, remoteForward, "smart_remote");
  let unknown: string;
  if (smart.unknownMode === "compatibility_first") {
    const primary = variant(domestic, domesticForward, "unknown_compatibility_domestic", tail);
    const secondary = variant(remote, remoteForward, "unknown_compatibility_remote");
    unknown = "standard_smart_unknown_compatibility";
    add(plugins, unknown, "fallback", fallbackArgs(primary, secondary, smart.fallbackThresholdMs, smart));
  } else if (smart.unknownMode === "privacy_first" && smart.privacyFallbackToDomestic) {
    const primary = variant(remote, remoteForward, "unknown_privacy_remote");
    const secondary = variant(domestic, domesticForward, "unknown_privacy_domestic", tail);
    unknown = "standard_smart_unknown_privacy";
    add(plugins, unknown, "fallback", fallbackArgs(primary, secondary, smart.fallbackThresholdMs, smart));
  } else unknown = variant(remote, remoteForward, smart.unknownMode === "strict_remote" ? "unknown_strict_remote" : "unknown_privacy_remote");
  tags.smartRouting.unknownAction = unknown;
  const routes: Array<[string, string]> = [];
  for (const [role, action] of [["domestic_domains", domesticAction], ["foreign_domains", remoteAction], ["direct_domains", domesticAction], ["remote_domains", remoteAction]] as const) {
    if (!tags.ruleData[role]) continue;
    const matcher = `standard_smart_match_${safe(role)}`;
    add(plugins, matcher, "qname", [`$${tags.ruleData[role]}`]);
    tags.smartRouting[`matcher:${role}`] = matcher;
    routes.push([matcher, action]);
  }
  return { routes, unknown };
}

function compilePath(path: StandardResolutionPath, intent: StandardModeSettings, forward: string, namespace: string, plugins: Plugin[], tags: MutableTagMap, overrides: PathOverrides = {}): string {
  const prelude: Json[] = [];
  if (path.dualStack !== "inherit" && path.dualStack !== "disabled") {
    const tag = pathBundleTag("dual", namespace);
    const [type, args] = path.dualStack === "prefer_ipv4" ? ["prefer_ipv4", { cache: true, cache_ttl: 3600 }]
      : path.dualStack === "prefer_ipv6" ? ["prefer_ipv6", { cache: true, cache_ttl: 3600 }]
      : path.dualStack === "ipv4_only" ? ["arbitrary", { rules: ["AAAA 0 NOERROR"], short_circuit: true }]
      : ["arbitrary", { rules: ["A 0 NOERROR"], short_circuit: true }];
    add(plugins, tag, type, args as Json);
    prelude.push({ exec: `$${tag}` });
  }
  if (path.ecs.mode !== "inherit") {
    const tag = pathBundleTag("ecs", namespace);
    const args = path.ecs.mode === "remove" ? { forward: false, send: false }
      : path.ecs.mode === "preserve_client" ? { forward: true, send: false }
      : path.ecs.mode === "client_subnet" ? { forward: false, send: true, mask4: path.ecs.mask4, mask6: path.ecs.mask6 }
      : { forward: false, send: false, preset: path.ecs.address, mask4: path.ecs.mask4, mask6: path.ecs.mask6 };
    add(plugins, tag, "ecs_handler", args);
    prelude.push({ exec: `$${tag}` });
  }
  if (path.ipSelection.enabled) {
    const selection = path.ipSelection;
    const tag = pathBundleTag("ip_selector", namespace);
    add(plugins, tag, "ip_selector", {
      selection_mode: selection.selectionMode, outbound: selection.outbound, socks5: selection.socks5,
      probe_methods: selection.probeMethods, probe_stagger: selection.probeStaggerMs,
      probe_timeout: selection.probeTimeoutMs, max_wait: selection.maxWaitMs, top_n: selection.topN,
      dnssec_policy: selection.dnssecPolicy, max_parallel_probes: selection.maxParallelProbes,
      cache: { enabled: selection.cacheEnabled, size: selection.cacheSize, ttl: selection.cacheTtlSeconds, failure_ttl: selection.failureTtlSeconds },
    });
    prelude.push({ exec: `$${tag}` });
  }
  let cacheTag: string | undefined;
  if (!overrides.disableCache && pathCacheEnabled(path, intent)) {
    cacheTag = pathBundleTag("cache", namespace);
    add(plugins, cacheTag, "cache", {
      size: intent.cache.size, min_positive_ttl: intent.cache.minPositiveTtl,
      max_positive_ttl: intent.cache.maxPositiveTtl, max_negative_ttl: intent.cache.maxNegativeTtl,
      negative_ttl_without_soa: intent.cache.negativeTtlWithoutSoa,
      ecs_in_key: path.ecs.mode === "client_subnet" || path.ecs.mode === "preset", short_circuit: true,
    });
  }
  const tag = pathBundleTag("path", namespace);
  add(plugins, tag, "sequence", buildPathSequence(path, intent, forward, tags, { ...overrides, cacheTag, prelude: [...prelude, ...(overrides.prelude ?? [])] }));
  return tag;
}

interface PathOverrides {
  disableFiltering?: boolean;
  forceFiltering?: boolean;
  disableQueryLog?: boolean;
  forceQueryLog?: boolean;
  prependExec?: string;
  disableCache?: boolean;
  responseTtlTag?: string;
  cacheTag?: string;
  prelude?: Json[];
  tail?: Json[];
}

function buildPathSequence(path: StandardResolutionPath, intent: StandardModeSettings, forward: string, tags: MutableTagMap, overrides: PathOverrides = {}): Json[] {
  const filtering = !overrides.disableFiltering && (overrides.forceFiltering || path.filtering === "enabled" || (path.filtering === "inherit" && intent.filtering.enabled));
  const queryLog = !overrides.disableQueryLog && (overrides.forceQueryLog || path.queryLog === "enabled" || (path.queryLog === "inherit" && intent.queryLog.enabled));
  const sequence: Json[] = [];
  if (tags.queryLog) {
    if (intent.queryLog.enabled && !queryLog) sequence.push({ exec: `mark ${QUERY_SKIP_MARK}` });
    else if (!intent.queryLog.enabled && queryLog) sequence.push({ exec: `mark ${QUERY_RECORD_MARK}` });
  }
  sequence.push(...(overrides.prelude ?? []));
  if (overrides.prependExec) sequence.push({ exec: overrides.prependExec });
  if (filtering && tags.filtering.includes("standard_ad_rules") && tags.filtering.includes("standard_blocked")) sequence.push({ matches: "qname $standard_ad_rules", exec: "$standard_blocked" });
  const cache = !overrides.disableCache ? (overrides.cacheTag ?? tags.caches[path.id]) : undefined;
  if (cache) sequence.push({ exec: `$${cache}` });
  sequence.push({ matches: "!has_resp", exec: `$${forward}` });
  const ttl = overrides.responseTtlTag ?? tags.local.responseTtl;
  if (ttl) sequence.push({ exec: `$${ttl}` });
  sequence.push(...(overrides.tail ?? []), { exec: "accept" });
  return sequence;
}

function compileForward(tag: string, strategy: StandardUpstreamGroup["strategy"], upstreams: StandardUpstream[], plugins: Plugin[]) {
  const enabled = upstreams.filter((item) => item.enabled);
  if (strategy === "ordered_fallback" && enabled.length > 1) {
    const members = enabled.map((upstream, index) => {
      const member = `${tag}_member_${index}`;
      add(plugins, member, "forward", { upstreams: [compileUpstream(upstream)], concurrent: 1, response_selection: "balanced" });
      return member;
    });
    let secondary = members.at(-1)!;
    for (let index = members.length - 2; index >= 0; index--) {
      const fallback = index === 0 ? tag : `${tag}_fallback_${index}`;
      add(plugins, fallback, "fallback", { primary: members[index], secondary, threshold: 500 });
      secondary = fallback;
    }
  } else {
    add(plugins, tag, "forward", {
      upstreams: enabled.map(compileUpstream),
      concurrent: Math.max(1, Math.min(3, enabled.length)),
      response_selection: strategy === "ordered_fallback" ? "balanced" : strategy,
    });
  }
}

function compileUpstream(upstream: StandardUpstream): Json {
  return compact({
    tag: upstream.id, addr: upstreamAddress(upstream), bootstrap: upstream.bootstrap,
    bootstrap_version: upstream.bootstrapVersion, dial_addr: upstream.dialAddress,
    outbound: upstream.outbound, socks5: upstream.socks5, timeout: upstream.timeoutSeconds,
    idle_timeout: upstream.idleTimeoutSeconds, max_conns: upstream.maxConns, min_conns: upstream.minConns,
    enable_pipeline: upstream.enablePipeline || undefined,
    insecure_skip_verify: upstream.tlsVerify === false || undefined,
    enable_http3: upstream.protocol === "doh3" || upstream.enableHttp3 || undefined,
  });
}

function compileMatcher(condition: StandardRuleCondition): { type: string; args: Json } {
  switch (condition.type) {
    case "domain": return { type: "qname", args: condition.values.map((value) => `full:${value}`) };
    case "suffix": return { type: "qname", args: condition.values.map((value) => `domain:${value}`) };
    case "keyword": return { type: "qname", args: condition.values.map((value) => `keyword:${value}`) };
    case "client_cidr": return { type: "client_ip", args: condition.values };
    case "qtype": return { type: "qtype", args: condition.values };
    default: throw new Error(`unsupported Standard condition: ${condition.type}`);
  }
}

function exceptionSequence(action: StandardModeSettings["exceptions"][number]["action"]["type"], path: StandardResolutionPath, intent: StandardModeSettings, tags: MutableTagMap): Json[] {
  if (action === "block") return [{ exec: "$standard_blocked" }, { exec: "accept" }];
  return buildPathSequence(path, intent, tags.upstreamGroups[path.upstreamGroupId], tags, {
    disableFiltering: action === "allow" || action === "skip_filtering",
    disableQueryLog: action === "disable_logging",
    prependExec: action === "prefer_ipv4" ? "$standard_prefer_ipv4" : action === "prefer_ipv6" ? "$standard_prefer_ipv6" : undefined,
  });
}

function appendExceptionRoutes(main: Json[], rules: StandardModeSettings["exceptions"], tags: MutableTagMap, actions: Record<string, string>, defaultPath: string) {
  for (const rule of rules) {
    const action = rule.action.type === "use_path" ? tags.paths[rule.action.pathId]
      : rule.action.type === "use_default_path" ? defaultPath : actions[rule.id];
    if (action) main.push({ matches: `$${tags.exceptionRules[rule.id]}`, exec: `$${action}` });
  }
}

function buildExplanation(intent: StandardModeSettings, tags: MutableTagMap, generatedTags: string[], build: BuildInfo, revision: string): StandardCompilationExplanation {
  const mappings: StandardCompilationExplanation["mappings"] = [];
  const pushMap = (intentPath: string, category: string, stableId: string, values: Array<string | undefined>) => mappings.push({ intentPath, category, stableId, generatedTags: values.filter((value): value is string => Boolean(value)) });
  for (const [id, tag] of Object.entries(tags.upstreamGroups)) pushMap(`upstreamGroups.${id}`, "upstream_group", id, [tag, ...Object.values(tags.upstreamMembers[id] ?? {})]);
  for (const [id, tag] of Object.entries(tags.paths)) pushMap(`paths.${id}`, "path", id, [tag, tags.caches[id]]);
  for (const [id, tag] of Object.entries(tags.routingRules)) pushMap(`routing.rules.${id}`, "routing_rule", id, [tag]);
  for (const [id, tag] of Object.entries(tags.exceptionRules)) pushMap(`exceptions.${id}`, "exception", id, [tag]);
  for (const [id, tag] of Object.entries(tags.devices)) pushMap(`devices.${id}`, "device", id, [tag, standardTag("device_action", id)]);
  for (const [id, tag] of Object.entries(tags.ruleData)) pushMap(`ruleData.${id}`, "rule_data", id, [tag]);
  for (const [id, value] of Object.entries(tags.dedicatedGroups)) pushMap(`dedicatedGroups.${id}`, "dedicated_group", id, [value.provider, value.matcher, value.upstreamGroup, value.path, value.entry, value.cache, value.udpListener, value.tcpListener]);
  for (const [id, value] of Object.entries(tags.dynamicLearning)) pushMap(`dynamicLearning.profiles.${id}`, "dynamic_learning", id, [value.provider, value.learner, value.matcher, value.action]);
  for (const [id, tag] of Object.entries(tags.advancedRules)) pushMap(`advancedRules.${id}`, "advanced_rule", id, [tag]);
  mappings.sort((a, b) => a.intentPath.localeCompare(b.intentPath));
  const pathBoundaries = intent.paths.map((path) => {
    const group = intent.upstreamGroups.find((item) => item.id === path.upstreamGroupId)!;
    const cacheEnabled = pathCacheEnabled(path, intent);
    return {
      pathId: path.id, pathTag: tags.paths[path.id], upstreamGroupId: group.id,
      upstreamGroupTag: tags.upstreamGroups[group.id], upstreamMemberIds: group.upstreams.filter((item) => item.enabled).map((item) => item.id),
      cacheTag: tags.caches[path.id], cacheNamespace: cacheEnabled ? `path:${path.id}` : "none", cacheEnabled,
      ecsMode: path.ecs.mode, ecsInKey: path.ecs.mode === "client_subnet" || path.ecs.mode === "preset",
      filteringEnabled: pathFilteringEnabled(path, intent), queryLogEnabled: pathQueryLogEnabled(path, intent),
      dualStack: path.dualStack, ipSelectionEnabled: path.ipSelection.enabled,
    };
  });
  return {
    schema: 1, intentRevision: revision, mappings, finalPriority: priorityRows(intent, tags), pathBoundaries,
    generatedTags,
    capabilities: {
      features: [...build.enabled_features].sort(), servers: [...build.supported_plugins.servers].sort(),
      executors: [...build.supported_plugins.executors].sort(), matchers: [...build.supported_plugins.matchers].sort(),
      providers: [...build.supported_plugins.providers].sort(),
      missingOptional: build.supported_plugins.executors.includes("metrics_collector") ? [] : ["executor:metrics_collector"],
    },
  };
}

function priorityRows(intent: StandardModeSettings, tags: MutableTagMap): StandardCompilationExplanation["finalPriority"] {
  const rows: Array<StandardCompilationExplanation["finalPriority"][number] & { explicit: number; index: number }> = [];
  intent.exceptions.filter((item) => item.enabled).forEach((rule, index) => rows.push({ ordinal: 0, slot: ["block"].includes(rule.action.type) ? 2 : ["allow", "skip_filtering"].includes(rule.action.type) ? 3 : 7, category: "exception", stableId: rule.id, phase: "request", matcherTags: [tags.exceptionRules[rule.id]], actionTag: rule.action.type === "use_path" ? tags.paths[rule.action.pathId] : standardTag("exception_action", rule.id), selectedPathId: rule.action.type === "use_path" ? rule.action.pathId : undefined, explicit: 0, index }));
  intent.dedicatedGroups.filter((item) => item.enabled).forEach((group, index) => rows.push({ ordinal: 0, slot: 5, category: "dedicated_group", stableId: group.id, phase: "request", matcherTags: [tags.dedicatedGroups[group.id].matcher], actionTag: tags.dedicatedGroups[group.id].path, explicit: group.priority, index }));
  intent.dynamicLearning.profiles.filter((item) => item.enabled).forEach((profile, index) => rows.push({ ordinal: 0, slot: 8, category: "dynamic_learning", stableId: profile.id, phase: "request", matcherTags: [tags.dynamicLearning[profile.id].matcher], actionTag: tags.dynamicLearning[profile.id].action, selectedPathId: profile.targetPathId, explicit: profile.priority, index }));
  if (!intent.smartRouting.enabled) rows.push({ ordinal: 0, slot: 10, category: "default_path", stableId: intent.paths[0].id, phase: "request", matcherTags: [], actionTag: tags.paths[intent.paths[0].id], selectedPathId: intent.paths[0].id, explicit: 0, index: 0 });
  return rows
    .sort((a, b) => a.slot - b.slot || a.explicit - b.explicit || a.index - b.index)
    .map((row, ordinal) => {
      const result: Partial<(typeof rows)[number]> = { ...row, ordinal: ordinal + 1 };
      delete result.explicit;
      delete result.index;
      return result as StandardCompilationExplanation["finalPriority"][number];
    });
}

function capabilityDiagnostics(generated: StandardGeneratedPlan, build: BuildInfo): StandardDiagnostic[] {
  const supported = new Set([...build.supported_plugins.servers, ...build.supported_plugins.executors, ...build.supported_plugins.matchers, ...build.supported_plugins.providers]);
  const root = parse(generated.yaml) as { plugins?: Array<{ tag?: string; type?: string }> };
  return (root.plugins ?? []).filter((plugin) => plugin.type && !supported.has(plugin.type)).map((plugin) => error("required_capability_missing", `plugins.${plugin.tag ?? "unknown"}`, `This build does not provide plugin '${plugin.type}'`));
}

function validateIntent(
  intent: StandardModeSettings,
  build: BuildInfo,
): StandardDiagnostic[] {
  const issues = [
    ...validateStandardDnsSettings(intent, build),
    ...validateStandardFilteringSettings(intent, build),
    ...validateStandardLocalSettings(intent, build),
    ...validateStandardDeviceSettings(intent, build),
    ...validateStandardRoutingSettings(intent, build),
    ...validateStandardExceptionSettings(intent, build),
  ];
  const seen = new Set<string>();
  return issues.flatMap((issue) => {
    const code =
      issue.code === "capability_required" ||
      issue.code.endsWith("_capability_required")
        ? "required_capability_missing"
        : issue.code === "protocol_unsupported"
          ? "upstream_protocol_unavailable"
          : issue.code;
    const key = `${code}\u0000${issue.field}`;
    if (seen.has(key)) return [];
    seen.add(key);
    return [
      error(
        code,
        issue.field,
        `Standard intent validation failed: ${issue.code}`,
      ),
    ];
  });
}

function summarize(intent: StandardModeSettings): StandardGenerationSummary {
  return {
    upstreamGroupCount: intent.upstreamGroups.length, pathCount: intent.paths.length,
    enabledUpstreamCount: intent.upstreamGroups.reduce((sum, group) => sum + group.upstreams.filter((item) => item.enabled).length, 0),
    filteringEnabled: intent.filtering.enabled, cacheEnabled: intent.cache.enabled, queryLogEnabled: intent.queryLog.enabled,
    routingRuleCount: intent.routing.rules.filter((item) => item.enabled).length,
    exceptionRuleCount: intent.exceptions.filter((item) => item.enabled).length, deviceCount: intent.devices.length,
    localPolicyCount: [intent.local.hosts.entries.length || intent.local.hosts.files.length, intent.local.redirects.rules.length || intent.local.redirects.files.length, intent.local.records.rules.length || intent.local.records.files.length, intent.local.responseTtl.enabled, intent.local.qtypePolicy.enabled, intent.local.ddns.enabled].filter(Boolean).length,
    ruleDataSourceCount: (Object.values(intent.ruleData) as Array<StandardModeSettings["ruleData"][keyof StandardModeSettings["ruleData"]]>).reduce((sum, role) => sum + role.sources.filter((item) => item.enabled).length, 0),
    smartRoutingEnabled: intent.smartRouting.enabled, dedicatedGroupCount: intent.dedicatedGroups.filter((item) => item.enabled).length,
    dynamicLearningProfileCount: intent.dynamicLearning.profiles.filter((item) => item.enabled).length,
    advancedRuleCount: intent.advancedRules.filter((item) => item.enabled).length,
  };
}

function emptyTagMap(): MutableTagMap {
  return { system: [], caches: {}, filtering: [], filterSubscriptions: {}, local: {}, upstreamGroups: {}, upstreamMembers: {}, paths: {}, routingRules: {}, exceptionRules: {}, devices: {}, ruleData: {}, ruleDataSources: {}, smartRouting: {}, dedicatedGroups: {}, dynamicLearning: {}, advancedRules: {} };
}

function add(plugins: Plugin[], tag: string, type: string, args: Json) { plugins.push({ tag, type, args: compact(args) }); }
function compact<T extends Json>(value: T): T { return JSON.parse(JSON.stringify(value)) as T; }
function error(code: string, path: string, message: string): StandardDiagnostic { return { severity: "error", code, path, message }; }
function isRecord(value: unknown): value is Record<string, unknown> { return Boolean(value) && typeof value === "object" && !Array.isArray(value); }
function safe(value: string): string { const result = value.toLowerCase().replace(/[^a-z0-9_-]+/g, "_").replace(/^_+|_+$/g, ""); return result || "item"; }
function standardTag(kind: string, id: string): string { return `standard_${safe(kind)}_${safe(id)}`; }
function pathBundleTag(kind: string, namespace: string): string { if (namespace.startsWith("dedicated:")) return standardTag(`dedicated_${kind}`, namespace.slice(10)); return kind === "cache" || kind === "path" ? standardTag(kind, namespace) : `standard_path_${safe(kind)}_${safe(namespace)}`; }
function blockMode(value: string): string { return value === "null_ip" ? "null" : value; }
function pathCacheEnabled(path: StandardResolutionPath, intent: StandardModeSettings) { return path.cache === "enabled" || (path.cache === "inherit" && intent.cache.enabled); }
function pathFilteringEnabled(path: StandardResolutionPath, intent: StandardModeSettings) { return path.filtering === "enabled" || (path.filtering === "inherit" && intent.filtering.enabled); }
function pathQueryLogEnabled(path: StandardResolutionPath, intent: StandardModeSettings) { return path.queryLog === "enabled" || (path.queryLog === "inherit" && intent.queryLog.enabled); }
function effectiveFilteringUsed(intent: StandardModeSettings) { return intent.filtering.enabled || intent.paths.some((path) => path.filtering === "enabled") || intent.devices.some((device) => device.filtering === "enabled"); }
function effectiveQueryLogUsed(intent: StandardModeSettings) { return intent.queryLog.enabled || intent.paths.some((path) => path.queryLog === "enabled") || intent.devices.some((device) => device.queryLog === "enabled") || intent.dedicatedGroups.some((group) => group.enabled && group.path.queryLog === "enabled"); }
function deviceHasPolicy(device: StandardModeSettings["devices"][number]) { return Boolean(device.assignedPathId || (device.filtering && device.filtering !== "inherit") || (device.queryLog && device.queryLog !== "inherit")); }
function byPriority<T extends { priority: number }>(a: T, b: T) { return a.priority - b.priority; }
function orderedExceptions(intent: StandardModeSettings) { const order: Record<string, number> = { block: 0, allow: 1, skip_filtering: 2, use_path: 3, use_default_path: 3, prefer_ipv4: 4, prefer_ipv6: 4, disable_logging: 5 }; return intent.exceptions.filter((item) => item.enabled).map((rule, index) => ({ rule, index })).sort((a, b) => order[a.rule.action.type] - order[b.rule.action.type] || a.index - b.index).map(({ rule }) => rule); }
function camelToSnake(value: string) { return value.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`); }
function upstreamAddress(upstream: StandardUpstream): string { const address = upstream.address.trim(); if (upstream.protocol === "auto") return address; const scheme = upstream.protocol === "udp" ? "udp://" : upstream.protocol === "tcp" ? "tcp://" : upstream.protocol === "dot" ? "tls://" : upstream.protocol === "doq" ? "quic://" : "https://"; const base = address.includes("://") ? address : `${scheme}${address}`; if ((upstream.protocol === "doh" || upstream.protocol === "doh3") && !base.slice("https://".length).includes("/")) return `${base}${upstream.dohPath || "/dns-query"}`; return base; }
function fallbackArgs(primary: string, secondary: string, threshold: number, smart: StandardModeSettings["smartRouting"]): Json { return { primary, secondary, threshold, short_circuit: true, fallback_on_timeout: smart.responsePolicy.timeout, fallback_on_error: smart.responsePolicy.transportFailure, fallback_on_no_response: true }; }

async function sha256(value: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}
