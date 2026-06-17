"use client";

import { useMemo, useState } from "react";
import {
  GitBranch,
  Loader2,
  Plus,
  Route,
  Save,
  Sparkles,
  Trash2,
} from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import type {
  StandardModeSettings,
  StandardResolutionPath,
  StandardRoutingRule,
  StandardScenario,
} from "@/lib/standard-mode/types";
import {
  isPathReferencedByRouting,
  normalizeStandardRoutingSettings,
  standardRoutingCapabilityMap,
  validateStandardRoutingSettings,
  type StandardRoutingValidationIssue,
} from "@/lib/standard-mode/validation";
import { useAppStore } from "@/lib/store";

type RoutingConditionType = Extract<
  StandardRoutingRule["condition"]["type"],
  "domain" | "suffix" | "keyword" | "client_cidr" | "qtype"
>;

type PathPolicy = "inherit" | "enabled" | "disabled";

const conditionLabelKeys: Record<RoutingConditionType, string> = {
  domain: WEBUI.standardRouting.conditionDomain,
  suffix: WEBUI.standardRouting.conditionSuffix,
  keyword: WEBUI.standardRouting.conditionKeyword,
  client_cidr: WEBUI.standardRouting.conditionClientCidr,
  qtype: WEBUI.standardRouting.conditionQtype,
};

const policyLabelKeys: Record<PathPolicy, string> = {
  inherit: WEBUI.standardRouting.policyInherit,
  enabled: WEBUI.standardRouting.policyEnabled,
  disabled: WEBUI.standardRouting.policyDisabled,
};

const scenarioLabelKeys: Record<StandardScenario["kind"], string> = {
  privacy: WEBUI.standardRouting.scenarioPrivacy,
  gaming: WEBUI.standardRouting.scenarioGaming,
  child_protection: WEBUI.standardRouting.scenarioChildProtection,
  domestic_optimization: WEBUI.standardRouting.scenarioDomesticOptimization,
};

const conditionPlaceholders: Record<RoutingConditionType, string> = {
  domain: "example.com",
  suffix: "example.com",
  keyword: "game",
  client_cidr: "192.168.1.0/24",
  qtype: "A\nAAAA",
};

const scenarioKinds: StandardScenario["kind"][] = [
  "privacy",
  "gaming",
  "child_protection",
  "domestic_optimization",
];

function lines(value: string) {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const line of value.split("\n")) {
    const next = line.trim();
    if (!next || seen.has(next)) continue;
    seen.add(next);
    result.push(next);
  }
  return result;
}

function nextId(prefix: string, existing: string[]) {
  const used = new Set(existing);
  let index = existing.length + 1;
  let id = `${prefix}_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `${prefix}_${index}`;
  }
  return id;
}

function createPath(paths: StandardResolutionPath[]): StandardResolutionPath {
  const id = nextId("path", paths.map((path) => path.id));
  return {
    id,
    name: id,
    upstreamGroupId: paths[0]?.upstreamGroupId ?? "default",
    filtering: "inherit",
    cache: "inherit",
    queryLog: "inherit",
    dualStack: "inherit",
    ipSelection: "inherit",
    ecs: "inherit",
  };
}

function createRule(settings: StandardModeSettings): StandardRoutingRule {
  const id = nextId("rule", settings.routing.rules.map((rule) => rule.id));
  return {
    id,
    name: id,
    enabled: true,
    condition: { type: "suffix", values: [] },
    action: {
      type: "use_path",
      pathId: settings.paths[1]?.id ?? settings.paths[0]?.id ?? "default",
    },
    source: "manual",
  };
}

function scenarioPath(
  kind: StandardScenario["kind"],
  settings: StandardModeSettings,
  name: string,
) {
  const idBase = kind.replace(/_/g, "-");
  const id = nextId(idBase, settings.paths.map((path) => path.id));
  const path: StandardResolutionPath = {
    ...createPath(settings.paths),
    id,
    name,
    description: name,
    upstreamGroupId: settings.upstreamGroups[0]?.id ?? "default",
    filtering: kind === "child_protection" ? "enabled" : "inherit",
    cache: "inherit",
    queryLog: kind === "privacy" ? "disabled" : "inherit",
    dualStack: "inherit",
    ipSelection: kind === "gaming" ? "enabled" : "inherit",
    ecs: kind === "privacy" ? "disabled" : "inherit",
  };
  const scenario: StandardScenario = {
    id,
    name,
    enabled: true,
    kind,
  };
  return { path, scenario };
}

export default function StandardRoutingPage() {
  const storeSettings = useAppStore((s) => s.standardSettings);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const saveStandardSettings = useAppStore((s) => s.saveStandardSettings);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isApplying = useAppStore((s) => s.isApplying);
  const { t, formatNumber } = useI18n();
  const [draftSettings, setDraftSettings] =
    useState<StandardModeSettings | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const settings = draftSettings ?? storeSettings;
  const capabilities = useMemo(
    () => standardRoutingCapabilityMap(buildInfo),
    [buildInfo],
  );
  const validationIssues = useMemo(
    () => validateStandardRoutingSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const isBusy = isConfigSaving || isApplying;
  const canSave = validationIssues.length === 0 && !isBusy;
  const enabledRules = settings.routing.rules.filter((rule) => rule.enabled);

  const setSettings = (nextSettings: StandardModeSettings) => {
    setSaveError(null);
    setDraftSettings(nextSettings);
  };

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setSettings({ ...settings, ...patch });
  };

  const setRouting = (patch: Partial<StandardModeSettings["routing"]>) => {
    setPartial({ routing: { ...settings.routing, ...patch } });
  };

  const updatePath = (pathId: string, patch: Partial<StandardResolutionPath>) => {
    setPartial({
      paths: settings.paths.map((path) =>
        path.id === pathId ? { ...path, ...patch } : path,
      ),
    });
  };

  const removePath = (pathId: string) => {
    if (
      pathId === settings.paths[0]?.id ||
      isPathReferencedByRouting(pathId, settings.routing)
    ) {
      return;
    }
    setPartial({
      paths: settings.paths.filter((path) => path.id !== pathId),
    });
  };

  const updateRule = (
    ruleId: string,
    patch: Partial<StandardRoutingRule>,
  ) => {
    setRouting({
      rules: settings.routing.rules.map((rule) =>
        rule.id === ruleId ? { ...rule, ...patch } : rule,
      ),
    });
  };

  const removeRule = (ruleId: string) => {
    setRouting({
      rules: settings.routing.rules.filter((rule) => rule.id !== ruleId),
    });
  };

  const addScenario = (kind: StandardScenario["kind"]) => {
    const next = scenarioPath(kind, settings, t(scenarioLabelKeys[kind]));
    setPartial({
      paths: [...settings.paths, next.path],
      routing: {
        ...settings.routing,
        scenarios: [...settings.routing.scenarios, next.scenario],
      },
    });
  };

  const handleSave = async () => {
    const nextSettings = normalizeStandardRoutingSettings(settings);
    const issues = validateStandardRoutingSettings(nextSettings, buildInfo);
    if (issues.length > 0) return;
    setSaveError(null);
    try {
      await saveStandardSettings(nextSettings, { apply: true });
      setDraftSettings(nextSettings);
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <>
      <AppHeader title={t(WEBUI.standardRouting.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-6xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardRouting.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardRouting.description)}
              </p>
            </div>
            <Button onClick={handleSave} disabled={!canSave}>
              {isBusy ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Save className="size-4" />
              )}
              {isBusy
                ? t(WEBUI.standardRouting.savingApplying)
                : t(WEBUI.standardRouting.saveApply)}
            </Button>
          </div>

          {validationIssues.length > 0 || saveError ? (
            <ValidationPanel issues={validationIssues} saveError={saveError} />
          ) : null}

          <div className="grid gap-3 sm:grid-cols-3">
            <MetricCard
              label={t(WEBUI.standardRouting.pathCount)}
              value={formatNumber(settings.paths.length)}
            />
            <MetricCard
              label={t(WEBUI.standardRouting.ruleCount)}
              value={formatNumber(settings.routing.rules.length)}
            />
            <MetricCard
              label={t(WEBUI.standardRouting.enabledRuleCount)}
              value={formatNumber(enabledRules.length)}
            />
          </div>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0">
              <CardTitle className="flex items-center gap-2 text-base">
                <GitBranch className="size-4" />
                {t(WEBUI.standardRouting.overviewTitle)}
              </CardTitle>
              {!capabilities.sequence ? (
                <Badge variant="destructive">
                  {t(WEBUI.standardRouting.unsupportedRouting)}
                </Badge>
              ) : null}
            </CardHeader>
            <CardContent>
              <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
                {t(WEBUI.standardRouting.enabled)}
                <Switch
                  checked={settings.routing.enabled}
                  onCheckedChange={(checked) => setRouting({ enabled: checked })}
                />
              </Label>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="flex items-center gap-2 text-base">
                  <Sparkles className="size-4" />
                  {t(WEBUI.standardRouting.scenariosTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardRouting.scenariosDescription)}
                </p>
              </div>
            </CardHeader>
            <CardContent className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              {scenarioKinds.map((kind) => (
                <Button
                  key={kind}
                  type="button"
                  variant="outline"
                  className="justify-start"
                  onClick={() => addScenario(kind)}
                >
                  <Plus className="size-4" />
                  {t(scenarioLabelKeys[kind])}
                </Button>
              ))}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="flex items-center gap-2 text-base">
                  <Route className="size-4" />
                  {t(WEBUI.standardRouting.pathsTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardRouting.pathsDescription)}
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  setPartial({ paths: [...settings.paths, createPath(settings.paths)] })
                }
              >
                <Plus className="size-4" />
                {t(WEBUI.standardRouting.addPath)}
              </Button>
            </CardHeader>
            <CardContent className="space-y-3">
              {settings.paths.map((path, index) => (
                <PathEditor
                  key={path.id}
                  path={path}
                  isDefault={index === 0}
                  canRemove={
                    index > 0 && !isPathReferencedByRouting(path.id, settings.routing)
                  }
                  settings={settings}
                  onChange={(patch) => updatePath(path.id, patch)}
                  onRemove={() => removePath(path.id)}
                />
              ))}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
              <div>
                <CardTitle className="text-base">
                  {t(WEBUI.standardRouting.rulesTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardRouting.rulesDescription)}
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  setRouting({
                    rules: [...settings.routing.rules, createRule(settings)],
                  })
                }
              >
                <Plus className="size-4" />
                {t(WEBUI.standardRouting.addRule)}
              </Button>
            </CardHeader>
            <CardContent className="space-y-3">
              {settings.routing.rules.length === 0 ? (
                <div className="rounded-lg border border-dashed p-6 text-sm text-muted-foreground">
                  {t(WEBUI.standardRouting.rulesEmpty)}
                </div>
              ) : (
                settings.routing.rules.map((rule) => (
                  <RuleEditor
                    key={rule.id}
                    rule={rule}
                    settings={settings}
                    capabilities={capabilities}
                    onChange={(patch) => updateRule(rule.id, patch)}
                    onRemove={() => removeRule(rule.id)}
                  />
                ))
              )}
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground">
          {label}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-semibold">{value}</div>
      </CardContent>
    </Card>
  );
}

function PathEditor({
  path,
  isDefault,
  canRemove,
  settings,
  onChange,
  onRemove,
}: {
  path: StandardResolutionPath;
  isDefault: boolean;
  canRemove: boolean;
  settings: StandardModeSettings;
  onChange: (patch: Partial<StandardResolutionPath>) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border bg-card/40 p-4">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <span className="font-medium">{path.name || path.id}</span>
          {isDefault ? (
            <Badge variant="secondary">{t(WEBUI.common.defaultValue)}</Badge>
          ) : null}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={!canRemove}
          onClick={onRemove}
        >
          <Trash2 className="size-4" />
          {t(WEBUI.standardRouting.removePath)}
        </Button>
      </div>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <div className="space-y-2">
          <Label htmlFor={`${path.id}-name`}>
            {t(WEBUI.standardRouting.pathName)}
          </Label>
          <Input
            id={`${path.id}-name`}
            value={path.name}
            onChange={(event) => onChange({ name: event.target.value })}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${path.id}-group`}>
            {t(WEBUI.standardRouting.pathUpstreamGroup)}
          </Label>
          <Select
            value={path.upstreamGroupId}
            onValueChange={(value) => onChange({ upstreamGroupId: value })}
          >
            <SelectTrigger id={`${path.id}-group`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {settings.upstreamGroups.map((group) => (
                <SelectItem key={group.id} value={group.id}>
                  {group.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <PolicySelect
          id={`${path.id}-filtering`}
          label={t(WEBUI.standardRouting.pathFiltering)}
          value={path.filtering}
          onChange={(value) => onChange({ filtering: value })}
        />
        <PolicySelect
          id={`${path.id}-cache`}
          label={t(WEBUI.standardRouting.pathCache)}
          value={path.cache}
          onChange={(value) => onChange({ cache: value })}
        />
        <PolicySelect
          id={`${path.id}-query-log`}
          label={t(WEBUI.standardRouting.pathQueryLog)}
          value={path.queryLog}
          onChange={(value) => onChange({ queryLog: value })}
        />
        <div className="space-y-2 md:col-span-2 xl:col-span-3">
          <Label htmlFor={`${path.id}-description`}>
            {t(WEBUI.standardRouting.pathDescription)}
          </Label>
          <Input
            id={`${path.id}-description`}
            value={path.description ?? ""}
            onChange={(event) => onChange({ description: event.target.value })}
          />
        </div>
      </div>
    </div>
  );
}

function PolicySelect({
  id,
  label,
  value,
  onChange,
}: {
  id: string;
  label: string;
  value: PathPolicy;
  onChange: (value: PathPolicy) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-2">
      <Label htmlFor={id}>{label}</Label>
      <Select value={value} onValueChange={(next) => onChange(next as PathPolicy)}>
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {(Object.keys(policyLabelKeys) as PathPolicy[]).map((policy) => (
            <SelectItem key={policy} value={policy}>
              {t(policyLabelKeys[policy])}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

function RuleEditor({
  rule,
  settings,
  capabilities,
  onChange,
  onRemove,
}: {
  rule: StandardRoutingRule;
  settings: StandardModeSettings;
  capabilities: ReturnType<typeof standardRoutingCapabilityMap>;
  onChange: (patch: Partial<StandardRoutingRule>) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  const conditionType = isSupportedCondition(rule.condition.type)
    ? rule.condition.type
    : "suffix";
  const values = "values" in rule.condition ? rule.condition.values : [];
  const matcherSupported =
    conditionType === "client_cidr"
      ? capabilities.clientIp
      : conditionType === "qtype"
        ? capabilities.qtype
        : capabilities.qname;
  const targetPathId =
    rule.action.type === "use_path"
      ? rule.action.pathId
      : settings.paths[0]?.id ?? "default";

  return (
    <div className="rounded-lg border bg-card/40 p-4">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <Label className="text-sm font-normal">
          <Switch
            checked={rule.enabled}
            onCheckedChange={(checked) => onChange({ enabled: checked })}
          />
          {t(WEBUI.standardRouting.ruleEnabled)}
        </Label>
        <div className="flex items-center gap-2">
          {!matcherSupported ? (
            <Badge variant="destructive">
              {t(WEBUI.standardRouting.unsupportedMatcher)}
            </Badge>
          ) : null}
          <Button type="button" variant="ghost" size="sm" onClick={onRemove}>
            <Trash2 className="size-4" />
            {t(WEBUI.standardRouting.removeRule)}
          </Button>
        </div>
      </div>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <div className="space-y-2">
          <Label htmlFor={`${rule.id}-name`}>
            {t(WEBUI.standardRouting.ruleName)}
          </Label>
          <Input
            id={`${rule.id}-name`}
            value={rule.name}
            onChange={(event) => onChange({ name: event.target.value })}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${rule.id}-condition`}>
            {t(WEBUI.standardRouting.ruleCondition)}
          </Label>
          <Select
            value={conditionType}
            onValueChange={(value) =>
              onChange({
                condition: {
                  type: value as RoutingConditionType,
                  values: [],
                },
              })
            }
          >
            <SelectTrigger id={`${rule.id}-condition`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(Object.keys(conditionLabelKeys) as RoutingConditionType[]).map(
                (type) => (
                  <SelectItem key={type} value={type}>
                    {t(conditionLabelKeys[type])}
                  </SelectItem>
                ),
              )}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${rule.id}-path`}>
            {t(WEBUI.standardRouting.ruleTargetPath)}
          </Label>
          <Select
            value={targetPathId}
            onValueChange={(value) =>
              onChange({ action: { type: "use_path", pathId: value } })
            }
          >
            <SelectTrigger id={`${rule.id}-path`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {settings.paths.map((path) => (
                <SelectItem key={path.id} value={path.id}>
                  {path.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${rule.id}-note`}>
            {t(WEBUI.standardRouting.ruleNote)}
          </Label>
          <Input
            id={`${rule.id}-note`}
            value={rule.note ?? ""}
            onChange={(event) => onChange({ note: event.target.value })}
          />
        </div>
        <div className="space-y-2 md:col-span-2 xl:col-span-4">
          <Label htmlFor={`${rule.id}-values`}>
            {t(WEBUI.standardRouting.ruleValues)}
          </Label>
          <Textarea
            id={`${rule.id}-values`}
            className="min-h-24 font-mono text-sm"
            value={values.join("\n")}
            placeholder={conditionPlaceholders[conditionType]}
            onChange={(event) =>
              onChange({
                condition: {
                  type: conditionType,
                  values: lines(event.target.value),
                },
              })
            }
          />
        </div>
      </div>
    </div>
  );
}

function ValidationPanel({
  issues,
  saveError,
}: {
  issues: StandardRoutingValidationIssue[];
  saveError: string | null;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
      <div className="font-medium">{t(WEBUI.standardRouting.validationTitle)}</div>
      <ul className="mt-2 list-disc space-y-1 pl-5 text-muted-foreground">
        {issues.map((issue, index) => (
          <li key={`${issue.field}-${issue.code}-${index}`}>
            {validationMessage(issue, t)}
          </li>
        ))}
        {saveError ? <li>{saveError}</li> : null}
      </ul>
    </div>
  );
}

function validationMessage(
  issue: StandardRoutingValidationIssue,
  t: ReturnType<typeof useI18n>["t"],
) {
  if (issue.code === "capability_required") {
    return t(WEBUI.standardRouting.validationCapabilityRequired);
  }
  if (issue.code === "path_required") {
    return t(WEBUI.standardRouting.validationPathRequired);
  }
  if (issue.code === "path_name_required") {
    return t(WEBUI.standardRouting.validationPathNameRequired);
  }
  if (issue.code === "path_upstream_group_required") {
    return t(WEBUI.standardRouting.validationPathUpstreamRequired);
  }
  if (issue.code === "path_delete_blocked") {
    return t(WEBUI.standardRouting.validationPathDeleteBlocked);
  }
  if (issue.code === "rule_name_required") {
    return t(WEBUI.standardRouting.validationRuleNameRequired);
  }
  if (issue.code === "rule_condition_required") {
    return t(WEBUI.standardRouting.validationRuleConditionRequired);
  }
  if (issue.code === "rule_action_required") {
    return t(WEBUI.standardRouting.validationRuleActionRequired);
  }
  if (issue.code === "rule_action_unsupported") {
    return t(WEBUI.standardRouting.validationRuleActionUnsupported);
  }
  if (issue.code === "rule_condition_unsupported") {
    return t(WEBUI.standardRouting.validationRuleConditionUnsupported);
  }
  return t(WEBUI.standardRouting.validationRuleMatcherUnsupported);
}

function isSupportedCondition(
  value: StandardRoutingRule["condition"]["type"],
): value is RoutingConditionType {
  return (
    value === "domain" ||
    value === "suffix" ||
    value === "keyword" ||
    value === "client_cidr" ||
    value === "qtype"
  );
}
