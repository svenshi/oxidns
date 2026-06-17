"use client";

import { useMemo, useState } from "react";
import { Loader2, Plus, Save, ShieldAlert, Trash2 } from "lucide-react";
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
  StandardExceptionRule,
  StandardModeSettings,
  StandardRuleAction,
  StandardRuleCondition,
} from "@/lib/standard-mode/types";
import {
  normalizeStandardExceptionSettings,
  standardExceptionCapabilityMap,
  validateStandardExceptionSettings,
  type StandardExceptionCapabilityMap,
  type StandardExceptionValidationIssue,
} from "@/lib/standard-mode/validation";
import { useAppStore } from "@/lib/store";

type ExceptionConditionType = Extract<
  StandardRuleCondition["type"],
  "domain" | "suffix" | "keyword" | "client_cidr" | "qtype"
>;

type ExceptionActionType = StandardRuleAction["type"];

const conditionLabelKeys: Record<ExceptionConditionType, string> = {
  domain: WEBUI.standardExceptions.conditionDomain,
  suffix: WEBUI.standardExceptions.conditionSuffix,
  keyword: WEBUI.standardExceptions.conditionKeyword,
  client_cidr: WEBUI.standardExceptions.conditionClientCidr,
  qtype: WEBUI.standardExceptions.conditionQtype,
};

const actionLabelKeys: Record<ExceptionActionType, string> = {
  allow: WEBUI.standardExceptions.actionAllow,
  block: WEBUI.standardExceptions.actionBlock,
  skip_filtering: WEBUI.standardExceptions.actionSkipFiltering,
  use_path: WEBUI.standardExceptions.actionUsePath,
  use_default_path: WEBUI.standardExceptions.actionUseDefaultPath,
  prefer_ipv4: WEBUI.standardExceptions.actionPreferIpv4,
  prefer_ipv6: WEBUI.standardExceptions.actionPreferIpv6,
  disable_logging: WEBUI.standardExceptions.actionDisableLogging,
};

const conditionPlaceholders: Record<ExceptionConditionType, string> = {
  domain: "example.com",
  suffix: "example.com",
  keyword: "ads",
  client_cidr: "192.168.1.10\n192.168.1.0/24",
  qtype: "A\nAAAA",
};

const actionTypes: ExceptionActionType[] = [
  "allow",
  "block",
  "skip_filtering",
  "use_path",
  "use_default_path",
  "prefer_ipv4",
  "prefer_ipv6",
  "disable_logging",
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

function nextId(existing: string[]) {
  const used = new Set(existing);
  let index = existing.length + 1;
  let id = `exception_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `exception_${index}`;
  }
  return id;
}

function createException(settings: StandardModeSettings): StandardExceptionRule {
  const id = nextId(settings.exceptions.map((exception) => exception.id));
  return {
    id,
    name: id,
    enabled: true,
    condition: { type: "suffix", values: [] },
    action: { type: "allow" },
  };
}

export default function StandardExceptionsPage() {
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
    () => standardExceptionCapabilityMap(buildInfo),
    [buildInfo],
  );
  const validationIssues = useMemo(
    () => validateStandardExceptionSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const isBusy = isConfigSaving || isApplying;
  const canSave = validationIssues.length === 0 && !isBusy;
  const enabledExceptions = settings.exceptions.filter((rule) => rule.enabled);
  const blockExceptions = enabledExceptions.filter(
    (rule) => rule.action.type === "block",
  );

  const setSettings = (nextSettings: StandardModeSettings) => {
    setSaveError(null);
    setDraftSettings(nextSettings);
  };

  const setPartial = (patch: Partial<StandardModeSettings>) => {
    setSettings({ ...settings, ...patch });
  };

  const updateException = (
    exceptionId: string,
    patch: Partial<StandardExceptionRule>,
  ) => {
    setPartial({
      exceptions: settings.exceptions.map((exception) =>
        exception.id === exceptionId ? { ...exception, ...patch } : exception,
      ),
    });
  };

  const removeException = (exceptionId: string) => {
    setPartial({
      exceptions: settings.exceptions.filter(
        (exception) => exception.id !== exceptionId,
      ),
    });
  };

  const handleSave = async () => {
    const nextSettings = normalizeStandardExceptionSettings(settings);
    const issues = validateStandardExceptionSettings(nextSettings, buildInfo);
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
      <AppHeader title={t(WEBUI.standardExceptions.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-6xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardExceptions.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardExceptions.description)}
              </p>
            </div>
            <Button onClick={handleSave} disabled={!canSave}>
              {isBusy ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Save className="size-4" />
              )}
              {isBusy
                ? t(WEBUI.standardExceptions.savingApplying)
                : t(WEBUI.standardExceptions.saveApply)}
            </Button>
          </div>

          {validationIssues.length > 0 || saveError ? (
            <ValidationPanel issues={validationIssues} saveError={saveError} />
          ) : null}

          <div className="grid gap-3 sm:grid-cols-3">
            <MetricCard
              label={t(WEBUI.standardExceptions.totalCount)}
              value={formatNumber(settings.exceptions.length)}
            />
            <MetricCard
              label={t(WEBUI.standardExceptions.enabledCount)}
              value={formatNumber(enabledExceptions.length)}
            />
            <MetricCard
              label={t(WEBUI.standardExceptions.blockCount)}
              value={formatNumber(blockExceptions.length)}
            />
          </div>

          <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_320px]">
            <Card>
              <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
                <div>
                  <CardTitle className="flex items-center gap-2 text-base">
                    <ShieldAlert className="size-4" />
                    {t(WEBUI.standardExceptions.rulesTitle)}
                  </CardTitle>
                  <p className="mt-1 text-sm text-muted-foreground">
                    {t(WEBUI.standardExceptions.rulesDescription)}
                  </p>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    setPartial({
                      exceptions: [
                        ...settings.exceptions,
                        createException(settings),
                      ],
                    })
                  }
                >
                  <Plus className="size-4" />
                  {t(WEBUI.standardExceptions.addRule)}
                </Button>
              </CardHeader>
              <CardContent className="space-y-3">
                {settings.exceptions.length === 0 ? (
                  <div className="rounded-lg border border-dashed p-6 text-sm text-muted-foreground">
                    {t(WEBUI.standardExceptions.rulesEmpty)}
                  </div>
                ) : (
                  settings.exceptions.map((exception) => (
                    <ExceptionEditor
                      key={exception.id}
                      exception={exception}
                      settings={settings}
                      capabilities={capabilities}
                      onChange={(patch) => updateException(exception.id, patch)}
                      onRemove={() => removeException(exception.id)}
                    />
                  ))
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-base">
                  {t(WEBUI.standardExceptions.priorityTitle)}
                </CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(WEBUI.standardExceptions.priorityDescription)}
                </p>
              </CardHeader>
              <CardContent>
                <ol className="list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
                  <li>{t(WEBUI.standardExceptions.priorityBlock)}</li>
                  <li>{t(WEBUI.standardExceptions.priorityAllow)}</li>
                  <li>{t(WEBUI.standardExceptions.prioritySkipFiltering)}</li>
                  <li>{t(WEBUI.standardExceptions.priorityPath)}</li>
                  <li>{t(WEBUI.standardExceptions.priorityPrefer)}</li>
                  <li>{t(WEBUI.standardExceptions.priorityRouting)}</li>
                  <li>{t(WEBUI.standardExceptions.priorityDefault)}</li>
                </ol>
              </CardContent>
            </Card>
          </div>
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

function ExceptionEditor({
  exception,
  settings,
  capabilities,
  onChange,
  onRemove,
}: {
  exception: StandardExceptionRule;
  settings: StandardModeSettings;
  capabilities: StandardExceptionCapabilityMap;
  onChange: (patch: Partial<StandardExceptionRule>) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  const conditionType = isSupportedCondition(exception.condition.type)
    ? exception.condition.type
    : "suffix";
  const values =
    "values" in exception.condition ? exception.condition.values : [];
  const actionType = exception.action.type;
  const targetPathId =
    exception.action.type === "use_path"
      ? exception.action.pathId
      : settings.paths[0]?.id ?? "default";
  const supported =
    conditionSupported(conditionType, capabilities) &&
    actionSupported(actionType, capabilities);

  return (
    <div className="rounded-lg border bg-card/40 p-4">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <Label className="text-sm font-normal">
          <Switch
            checked={exception.enabled}
            onCheckedChange={(checked) => onChange({ enabled: checked })}
          />
          {t(WEBUI.standardExceptions.ruleEnabled)}
        </Label>
        <div className="flex items-center gap-2">
          {!supported ? (
            <Badge variant="destructive">
              {t(WEBUI.standardExceptions.unsupportedCapability)}
            </Badge>
          ) : null}
          <Button type="button" variant="ghost" size="sm" onClick={onRemove}>
            <Trash2 className="size-4" />
            {t(WEBUI.standardExceptions.removeRule)}
          </Button>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <div className="space-y-2">
          <Label htmlFor={`${exception.id}-name`}>
            {t(WEBUI.standardExceptions.ruleName)}
          </Label>
          <Input
            id={`${exception.id}-name`}
            value={exception.name}
            onChange={(event) => onChange({ name: event.target.value })}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor={`${exception.id}-condition`}>
            {t(WEBUI.standardExceptions.ruleCondition)}
          </Label>
          <Select
            value={conditionType}
            onValueChange={(value) =>
              onChange({
                condition: {
                  type: value as ExceptionConditionType,
                  values: [],
                },
              })
            }
          >
            <SelectTrigger id={`${exception.id}-condition`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(Object.keys(conditionLabelKeys) as ExceptionConditionType[]).map(
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
          <Label htmlFor={`${exception.id}-action`}>
            {t(WEBUI.standardExceptions.ruleAction)}
          </Label>
          <Select
            value={actionType}
            onValueChange={(value) =>
              onChange({
                action: createAction(value as ExceptionActionType, targetPathId),
              })
            }
          >
            <SelectTrigger id={`${exception.id}-action`} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {actionTypes.map((type) => (
                <SelectItem key={type} value={type}>
                  {t(actionLabelKeys[type])}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {exception.action.type === "use_path" ? (
          <div className="space-y-2">
            <Label htmlFor={`${exception.id}-path`}>
              {t(WEBUI.standardExceptions.ruleTargetPath)}
            </Label>
            <Select
              value={targetPathId}
              onValueChange={(value) =>
                onChange({ action: { type: "use_path", pathId: value } })
              }
            >
              <SelectTrigger id={`${exception.id}-path`} className="w-full">
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
        ) : (
          <div className="space-y-2">
            <Label htmlFor={`${exception.id}-note`}>
              {t(WEBUI.standardExceptions.ruleNote)}
            </Label>
            <Input
              id={`${exception.id}-note`}
              value={exception.note ?? ""}
              onChange={(event) => onChange({ note: event.target.value })}
            />
          </div>
        )}
        {exception.action.type === "use_path" ? (
          <div className="space-y-2 md:col-span-2 xl:col-span-4">
            <Label htmlFor={`${exception.id}-note-wide`}>
              {t(WEBUI.standardExceptions.ruleNote)}
            </Label>
            <Input
              id={`${exception.id}-note-wide`}
              value={exception.note ?? ""}
              onChange={(event) => onChange({ note: event.target.value })}
            />
          </div>
        ) : null}
        <div className="space-y-2 md:col-span-2 xl:col-span-4">
          <Label htmlFor={`${exception.id}-values`}>
            {t(WEBUI.standardExceptions.ruleValues)}
          </Label>
          <Textarea
            id={`${exception.id}-values`}
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
  issues: StandardExceptionValidationIssue[];
  saveError: string | null;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
      <div className="font-medium">
        {t(WEBUI.standardExceptions.validationTitle)}
      </div>
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

function createAction(
  type: ExceptionActionType,
  currentPathId: string,
): StandardRuleAction {
  if (type === "use_path") return { type, pathId: currentPathId };
  return { type };
}

function conditionSupported(
  type: ExceptionConditionType,
  capabilities: StandardExceptionCapabilityMap,
) {
  if (type === "client_cidr") return capabilities.clientIp;
  if (type === "qtype") return capabilities.qtype;
  return capabilities.qname;
}

function actionSupported(
  type: ExceptionActionType,
  capabilities: StandardExceptionCapabilityMap,
) {
  if (type === "block") return capabilities.blackHole;
  if (type === "prefer_ipv4") return capabilities.preferIpv4;
  if (type === "prefer_ipv6") return capabilities.preferIpv6;
  return true;
}

function validationMessage(
  issue: StandardExceptionValidationIssue,
  t: ReturnType<typeof useI18n>["t"],
) {
  if (issue.code === "capability_required") {
    return t(WEBUI.standardExceptions.validationCapabilityRequired);
  }
  if (issue.code === "exception_name_required") {
    return t(WEBUI.standardExceptions.validationNameRequired);
  }
  if (issue.code === "exception_condition_required") {
    return t(WEBUI.standardExceptions.validationConditionRequired);
  }
  if (issue.code === "exception_action_required") {
    return t(WEBUI.standardExceptions.validationActionRequired);
  }
  if (issue.code === "exception_action_unsupported") {
    return t(WEBUI.standardExceptions.validationActionUnsupported);
  }
  if (issue.code === "exception_condition_unsupported") {
    return t(WEBUI.standardExceptions.validationConditionUnsupported);
  }
  return t(WEBUI.standardExceptions.validationMatcherUnsupported);
}

function isSupportedCondition(
  value: StandardRuleCondition["type"],
): value is ExceptionConditionType {
  return (
    value === "domain" ||
    value === "suffix" ||
    value === "keyword" ||
    value === "client_cidr" ||
    value === "qtype"
  );
}
