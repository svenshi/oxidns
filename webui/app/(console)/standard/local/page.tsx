"use client";

import { useMemo, useState } from "react";
import { FileCode2, Loader2, Save, TimerReset } from "lucide-react";
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
  StandardBlockResponse,
  StandardLocalSettings,
  StandardModeSettings,
} from "@/lib/standard-mode/types";
import {
  normalizeStandardLocalSettings,
  validateStandardLocalSettings,
  type StandardLocalValidationIssue,
} from "@/lib/standard-mode/validation";
import { useAppStore } from "@/lib/store";

const DEFAULT_DDNS_PATH = "__default__";

const responseLabelKeys: Record<StandardBlockResponse, string> = {
  null_ip: WEBUI.standardLocal.responseNullIp,
  nxdomain: WEBUI.standardLocal.responseNxdomain,
  nodata: WEBUI.standardLocal.responseNodata,
  refused: WEBUI.standardLocal.responseRefused,
};

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

function optionalNumber(value: string) {
  if (!value.trim()) return undefined;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(0, Math.trunc(parsed)) : undefined;
}

export default function StandardLocalPage() {
  const storeSettings = useAppStore((state) => state.standardSettings);
  const buildInfo = useAppStore((state) => state.buildInfo);
  const standardLastGenerated = useAppStore(
    (state) => state.standardLastGenerated,
  );
  const saveStandardSettings = useAppStore(
    (state) => state.saveStandardSettings,
  );
  const isConfigSaving = useAppStore((state) => state.isConfigSaving);
  const isApplying = useAppStore((state) => state.isApplying);
  const { t } = useI18n();
  const [draftSettings, setDraftSettings] =
    useState<StandardModeSettings | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const settings = draftSettings ?? storeSettings;
  const validationIssues = useMemo(
    () => validateStandardLocalSettings(settings, buildInfo),
    [settings, buildInfo],
  );
  const activeTags = Object.keys(standardLastGenerated?.tagMap.local ?? {});
  const isBusy = isConfigSaving || isApplying;
  const canSave = validationIssues.length === 0 && !isBusy;

  const setLocal = (patch: Partial<StandardLocalSettings>) => {
    setSaveError(null);
    setDraftSettings((current) => ({
      ...(current ?? settings),
      local: { ...(current ?? settings).local, ...patch },
    }));
  };

  const handleSave = async () => {
    const nextSettings = normalizeStandardLocalSettings(settings);
    const issues = validateStandardLocalSettings(nextSettings, buildInfo);
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
      <AppHeader title={t(WEBUI.standardLocal.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-6xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardLocal.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardLocal.description)}
              </p>
            </div>
            <Button onClick={handleSave} disabled={!canSave}>
              {isBusy ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Save className="size-4" />
              )}
              {isBusy
                ? t(WEBUI.standardLocal.savingApplying)
                : t(WEBUI.standardLocal.saveApply)}
            </Button>
          </div>

          <div className="rounded-lg border bg-muted/20 p-4 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium">
                {t(WEBUI.standardLocal.runtimeTitle)}
              </span>
              {activeTags.length > 0 ? (
                activeTags.map((tag) => (
                  <Badge key={tag} variant="secondary">
                    {t(localTagLabel(tag))}
                  </Badge>
                ))
              ) : (
                <span className="text-muted-foreground">
                  {t(WEBUI.standardLocal.runtimeEmpty)}
                </span>
              )}
            </div>
            <p className="mt-2 text-xs text-muted-foreground">
              {t(WEBUI.standardLocal.boundaryNotice)}
            </p>
          </div>

          {validationIssues.length > 0 || saveError ? (
            <ValidationPanel issues={validationIssues} saveError={saveError} />
          ) : null}

          <div className="grid gap-6 lg:grid-cols-2">
            <RuleSourceCard
              title={t(WEBUI.standardLocal.hostsTitle)}
              description={t(WEBUI.standardLocal.hostsDescription)}
              rulesLabel={t(WEBUI.standardLocal.hostsEntries)}
              filesLabel={t(WEBUI.standardLocal.files)}
              rules={settings.local.hosts.entries}
              files={settings.local.hosts.files}
              rulesPlaceholder={
                "router.local 192.168.1.1\nfull:gateway.local 192.168.1.2"
              }
              onRulesChange={(entries) =>
                setLocal({ hosts: { ...settings.local.hosts, entries } })
              }
              onFilesChange={(files) =>
                setLocal({ hosts: { ...settings.local.hosts, files } })
              }
            />
            <RuleSourceCard
              title={t(WEBUI.standardLocal.redirectsTitle)}
              description={t(WEBUI.standardLocal.redirectsDescription)}
              rulesLabel={t(WEBUI.standardLocal.redirectRules)}
              filesLabel={t(WEBUI.standardLocal.files)}
              rules={settings.local.redirects.rules}
              files={settings.local.redirects.files}
              rulesPlaceholder="full:old.example.com new.example.net"
              onRulesChange={(rules) =>
                setLocal({
                  redirects: { ...settings.local.redirects, rules },
                })
              }
              onFilesChange={(files) =>
                setLocal({
                  redirects: { ...settings.local.redirects, files },
                })
              }
            />
            <RuleSourceCard
              title={t(WEBUI.standardLocal.recordsTitle)}
              description={t(WEBUI.standardLocal.recordsDescription)}
              rulesLabel={t(WEBUI.standardLocal.recordRules)}
              filesLabel={t(WEBUI.standardLocal.files)}
              rules={settings.local.records.rules}
              files={settings.local.records.files}
              rulesPlaceholder={
                'example.com. 60 IN TXT "hello world"\nwww.example.com. 120 IN A 192.0.2.10'
              }
              onRulesChange={(rules) =>
                setLocal({ records: { ...settings.local.records, rules } })
              }
              onFilesChange={(files) =>
                setLocal({ records: { ...settings.local.records, files } })
              }
            />

            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2 text-base">
                  <TimerReset className="size-4" />
                  {t(WEBUI.standardLocal.ttlTitle)}
                </CardTitle>
                <p className="text-sm text-muted-foreground">
                  {t(WEBUI.standardLocal.ttlDescription)}
                </p>
              </CardHeader>
              <CardContent className="space-y-4">
                <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
                  {t(WEBUI.standardLocal.ttlEnabled)}
                  <Switch
                    checked={settings.local.responseTtl.enabled}
                    onCheckedChange={(enabled) =>
                      setLocal({
                        responseTtl: {
                          ...settings.local.responseTtl,
                          enabled,
                        },
                      })
                    }
                  />
                </Label>
                <div className="grid gap-4 sm:grid-cols-2">
                  <NumberField
                    id="standard-local-ttl-min"
                    label={t(WEBUI.standardLocal.ttlMin)}
                    value={settings.local.responseTtl.min}
                    disabled={!settings.local.responseTtl.enabled}
                    onChange={(min) =>
                      setLocal({
                        responseTtl: {
                          ...settings.local.responseTtl,
                          min,
                        },
                      })
                    }
                  />
                  <NumberField
                    id="standard-local-ttl-max"
                    label={t(WEBUI.standardLocal.ttlMax)}
                    value={settings.local.responseTtl.max}
                    disabled={!settings.local.responseTtl.enabled}
                    onChange={(max) =>
                      setLocal({
                        responseTtl: {
                          ...settings.local.responseTtl,
                          max,
                        },
                      })
                    }
                  />
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-base">
                  {t(WEBUI.standardLocal.qtypeTitle)}
                </CardTitle>
                <p className="text-sm text-muted-foreground">
                  {t(WEBUI.standardLocal.qtypeDescription)}
                </p>
              </CardHeader>
              <CardContent className="space-y-4">
                <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
                  {t(WEBUI.standardLocal.qtypeEnabled)}
                  <Switch
                    checked={settings.local.qtypePolicy.enabled}
                    onCheckedChange={(enabled) =>
                      setLocal({
                        qtypePolicy: {
                          ...settings.local.qtypePolicy,
                          enabled,
                        },
                      })
                    }
                  />
                </Label>
                <div className="grid gap-4 sm:grid-cols-2">
                  <div className="space-y-2">
                    <Label htmlFor="standard-local-qtypes">
                      {t(WEBUI.standardLocal.qtypes)}
                    </Label>
                    <Textarea
                      id="standard-local-qtypes"
                      className="min-h-24 font-mono"
                      disabled={!settings.local.qtypePolicy.enabled}
                      value={settings.local.qtypePolicy.qtypes.join("\n")}
                      placeholder={"HTTPS\nSVCB\nAAAA"}
                      onChange={(event) =>
                        setLocal({
                          qtypePolicy: {
                            ...settings.local.qtypePolicy,
                            qtypes: lines(event.target.value),
                          },
                        })
                      }
                    />
                  </div>
                  <div className="space-y-2">
                    <Label>{t(WEBUI.standardLocal.qtypeResponse)}</Label>
                    <Select
                      disabled={!settings.local.qtypePolicy.enabled}
                      value={settings.local.qtypePolicy.response}
                      onValueChange={(response) =>
                        setLocal({
                          qtypePolicy: {
                            ...settings.local.qtypePolicy,
                            response: response as StandardBlockResponse,
                          },
                        })
                      }
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {(
                          ["nodata", "nxdomain", "null_ip", "refused"] as const
                        ).map((response) => (
                          <SelectItem key={response} value={response}>
                            {t(responseLabelKeys[response])}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              </CardContent>
            </Card>

            <Card id="ddns" className="scroll-mt-6">
              <CardHeader>
                <CardTitle className="text-base">
                  {t(WEBUI.standardLocal.ddnsTitle)}
                </CardTitle>
                <p className="text-sm text-muted-foreground">
                  {t(WEBUI.standardLocal.ddnsDescription)}
                </p>
              </CardHeader>
              <CardContent className="space-y-4">
                <Label className="flex min-h-10 items-center justify-between rounded-lg border px-3 text-sm font-normal">
                  {t(WEBUI.standardLocal.ddnsEnabled)}
                  <Switch
                    checked={settings.local.ddns.enabled}
                    onCheckedChange={(enabled) =>
                      setLocal({ ddns: { ...settings.local.ddns, enabled } })
                    }
                  />
                </Label>
                <div className="grid gap-4 sm:grid-cols-2">
                  <div className="space-y-2 sm:row-span-2">
                    <Label htmlFor="standard-local-ddns-domains">
                      {t(WEBUI.standardLocal.ddnsDomains)}
                    </Label>
                    <Textarea
                      id="standard-local-ddns-domains"
                      className="min-h-32 font-mono"
                      disabled={!settings.local.ddns.enabled}
                      value={settings.local.ddns.domains.join("\n")}
                      placeholder="home.example.com"
                      onChange={(event) =>
                        setLocal({
                          ddns: {
                            ...settings.local.ddns,
                            domains: lines(event.target.value),
                          },
                        })
                      }
                    />
                  </div>
                  <div className="space-y-2">
                    <Label>{t(WEBUI.standardLocal.ddnsPath)}</Label>
                    <Select
                      disabled={!settings.local.ddns.enabled}
                      value={settings.local.ddns.pathId ?? DEFAULT_DDNS_PATH}
                      onValueChange={(pathId) =>
                        setLocal({
                          ddns: {
                            ...settings.local.ddns,
                            pathId:
                              pathId === DEFAULT_DDNS_PATH ? undefined : pathId,
                          },
                        })
                      }
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={DEFAULT_DDNS_PATH}>
                          {t(WEBUI.standardLocal.ddnsDefaultPath)}
                        </SelectItem>
                        {settings.paths.map((path) => (
                          <SelectItem key={path.id} value={path.id}>
                            {path.name || path.id}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <NumberField
                    id="standard-local-ddns-ttl"
                    label={t(WEBUI.standardLocal.ddnsTtl)}
                    value={settings.local.ddns.ttl}
                    disabled={!settings.local.ddns.enabled}
                    onChange={(ttl) =>
                      setLocal({
                        ddns: { ...settings.local.ddns, ttl: ttl ?? 30 },
                      })
                    }
                  />
                </div>
              </CardContent>
            </Card>
          </div>
        </div>
      </main>
    </>
  );
}

function RuleSourceCard({
  title,
  description,
  rulesLabel,
  filesLabel,
  rules,
  files,
  rulesPlaceholder,
  onRulesChange,
  onFilesChange,
}: {
  title: string;
  description: string;
  rulesLabel: string;
  filesLabel: string;
  rules: string[];
  files: string[];
  rulesPlaceholder: string;
  onRulesChange: (rules: string[]) => void;
  onFilesChange: (files: string[]) => void;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <FileCode2 className="size-4" />
          {title}
        </CardTitle>
        <p className="text-sm text-muted-foreground">{description}</p>
      </CardHeader>
      <CardContent className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-2">
          <Label>{rulesLabel}</Label>
          <Textarea
            className="min-h-36 font-mono text-sm"
            value={rules.join("\n")}
            placeholder={rulesPlaceholder}
            onChange={(event) => onRulesChange(lines(event.target.value))}
          />
        </div>
        <div className="space-y-2">
          <Label>{filesLabel}</Label>
          <Textarea
            className="min-h-36 font-mono text-sm"
            value={files.join("\n")}
            placeholder="./rules/local.txt"
            onChange={(event) => onFilesChange(lines(event.target.value))}
          />
        </div>
      </CardContent>
    </Card>
  );
}

function NumberField({
  id,
  label,
  value,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  value: number | undefined;
  disabled: boolean;
  onChange: (value: number | undefined) => void;
}) {
  return (
    <div className="space-y-2">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="number"
        min={0}
        disabled={disabled}
        value={value ?? ""}
        onChange={(event) => onChange(optionalNumber(event.target.value))}
      />
    </div>
  );
}

function ValidationPanel({
  issues,
  saveError,
}: {
  issues: StandardLocalValidationIssue[];
  saveError: string | null;
}) {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
      <div className="font-medium">
        {t(WEBUI.standardLocal.validationTitle)}
      </div>
      <ul className="mt-2 list-disc space-y-1 pl-5">
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
  issue: StandardLocalValidationIssue,
  t: (key: string) => string,
) {
  const keys: Record<StandardLocalValidationIssue["code"], string> = {
    capability_required: WEBUI.standardLocal.validationCapabilityRequired,
    ttl_required: WEBUI.standardLocal.validationTtlRequired,
    ttl_range_invalid: WEBUI.standardLocal.validationTtlRangeInvalid,
    qtype_required: WEBUI.standardLocal.validationQtypeRequired,
    ddns_domain_required: WEBUI.standardLocal.validationDdnsDomainRequired,
    ddns_path_required: WEBUI.standardLocal.validationDdnsPathRequired,
    ddns_ttl_invalid: WEBUI.standardLocal.validationDdnsTtlInvalid,
  };
  return t(keys[issue.code]);
}

function localTagLabel(tag: string) {
  const keys: Record<string, string> = {
    hosts: WEBUI.standardLocal.runtimeHosts,
    records: WEBUI.standardLocal.runtimeRecords,
    redirect: WEBUI.standardLocal.runtimeRedirects,
    responseTtl: WEBUI.standardLocal.runtimeTtl,
    qtypeMatcher: WEBUI.standardLocal.runtimeQtype,
    qtypeAction: WEBUI.standardLocal.runtimeQtype,
    ddnsMatcher: WEBUI.standardLocal.runtimeDdns,
    ddnsTtl: WEBUI.standardLocal.runtimeDdns,
  };
  return keys[tag] ?? WEBUI.standardLocal.runtimePolicy;
}
