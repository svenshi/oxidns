"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import {
  Ban,
  CheckCircle2,
  Loader2,
  RefreshCw,
  Route,
  Search,
} from "lucide-react";
import { AppHeader } from "@/components/shell/app-header";
import { DnsRecordDetailDialog } from "@/components/plugins/dns-record-detail-dialog";
import { QueryRecordFlowCanvas } from "@/components/plugins/query-record-flow";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  fetchQueryRecordDetail,
  fetchQueryRecords,
  type QueryRecordDetail,
  type QueryRecordFilters,
  type QueryRecordRow,
  type QueryRecordStatusFilter,
} from "@/lib/oxidns-api";
import {
  adGuardDomainRule,
  explainStandardQueryRecord,
  queryRecordDomain,
  queryRecordQtype,
  type StandardQueryExplanation,
  type StandardQueryOutcome,
} from "@/lib/standard-mode/query-explainer";
import type {
  StandardModeSettings,
  StandardRoutingRule,
} from "@/lib/standard-mode/types";
import {
  normalizeStandardFilteringSettings,
  normalizeStandardRoutingSettings,
  validateStandardFilteringSettings,
  validateStandardRoutingSettings,
} from "@/lib/standard-mode/validation";
import { selectStandardCapabilityMap } from "@/lib/standard-mode/selectors";
import { useAppStore } from "@/lib/store";

type QueryFilterForm = {
  qname: string;
  qtype: string;
  clientIp: string;
  rcode: string;
  status: QueryRecordStatusFilter;
};

const EMPTY_FILTERS: QueryFilterForm = {
  qname: "",
  qtype: "all",
  clientIp: "",
  rcode: "",
  status: "all",
};

const QTYPE_OPTIONS = [
  "A",
  "AAAA",
  "HTTPS",
  "CNAME",
  "TXT",
  "MX",
  "NS",
  "SOA",
  "SRV",
  "PTR",
  "SVCB",
  "CAA",
];

function filtersFromForm(form: QueryFilterForm): QueryRecordFilters {
  return {
    qname: optionalTrimmed(form.qname),
    qtype: form.qtype === "all" ? undefined : form.qtype,
    clientIp: optionalTrimmed(form.clientIp),
    rcode: optionalTrimmed(form.rcode),
    status: form.status === "all" ? undefined : form.status,
  };
}

function optionalTrimmed(value: string) {
  const trimmed = value.trim();
  return trimmed || undefined;
}

function questionName(record: QueryRecordRow | QueryRecordDetail) {
  return queryRecordDomain(record) || "-";
}

function createRoutingRule(
  settings: StandardModeSettings,
  domain: string,
  pathId: string,
): StandardRoutingRule {
  const id = nextRuleId(settings, domain);
  const defaultPathId = settings.paths[0]?.id ?? "default";
  return {
    id,
    name: domain,
    enabled: true,
    condition: { type: "domain", values: [domain] },
    action:
      pathId === defaultPathId
        ? { type: "use_default_path" }
        : { type: "use_path", pathId },
    source: "manual",
  };
}

function nextRuleId(settings: StandardModeSettings, domain: string) {
  const base = `query_${domain
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "") || "rule"}`;
  const used = new Set(settings.routing.rules.map((rule) => rule.id));
  if (!used.has(base)) return base;
  let index = 2;
  let id = `${base}_${index}`;
  while (used.has(id)) {
    index += 1;
    id = `${base}_${index}`;
  }
  return id;
}

function uniqueAppend(values: string[], value: string) {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const item of [...values, value]) {
    const next = item.trim();
    if (!next || seen.has(next)) continue;
    seen.add(next);
    result.push(next);
  }
  return result;
}

export default function StandardQueriesPage() {
  const plugins = useAppStore((s) => s.plugins);
  const buildInfo = useAppStore((s) => s.buildInfo);
  const standardSettings = useAppStore((s) => s.standardSettings);
  const standardLastGenerated = useAppStore((s) => s.standardLastGenerated);
  const saveStandardSettings = useAppStore((s) => s.saveStandardSettings);
  const isConfigSaving = useAppStore((s) => s.isConfigSaving);
  const isApplying = useAppStore((s) => s.isApplying);
  const dependencyGraph = useAppStore((s) => s.dependencyGraph);
  const { t, formatDateTime, formatNumber } = useI18n();
  const capabilities = useMemo(
    () => selectStandardCapabilityMap(buildInfo),
    [buildInfo],
  );
  const recorder =
    plugins.find((plugin) => plugin.name === "standard_recorder") ??
    plugins.find((plugin) => plugin.pluginKind === "query_recorder");
  const recorderName = recorder?.name;
  const [records, setRecords] = useState<QueryRecordRow[]>([]);
  const [detailsById, setDetailsById] = useState<Record<number, QueryRecordDetail>>(
    {},
  );
  const [selected, setSelected] = useState<QueryRecordDetail | null>(null);
  const [filterForm, setFilterForm] = useState<QueryFilterForm>(EMPTY_FILTERS);
  const [appliedFilters, setAppliedFilters] = useState<QueryRecordFilters>({});
  const [routePathId, setRoutePathId] = useState(
    standardSettings.paths[0]?.id ?? "default",
  );
  const [loading, setLoading] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [savingAction, setSavingAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const requestIdRef = useRef(0);
  const isBusy = isConfigSaving || isApplying || savingAction !== null;

  const explanations = useMemo(() => {
    const map = new Map<number, StandardQueryExplanation>();
    for (const record of records) {
      map.set(
        record.id,
        explainStandardQueryRecord(
          detailsById[record.id] ?? record,
          standardSettings,
          standardLastGenerated,
        ),
      );
    }
    return map;
  }, [detailsById, records, standardLastGenerated, standardSettings]);

  const load = async (filters: QueryRecordFilters = appliedFilters) => {
    if (!recorderName) return;
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setLoading(true);
    setError(null);
    setActionMessage(null);
    try {
      const response = await fetchQueryRecords(recorderName, {
        limit: 80,
        ...filters,
      });
      if (requestIdRef.current !== requestId) return;
      setRecords(response.records);
      setDetailsById({});
      if (response.records.length > 0) {
        void hydrateDetails(response.records, requestId);
      } else {
        setDetailLoading(false);
      }
    } catch (err) {
      if (requestIdRef.current !== requestId) return;
      setDetailLoading(false);
      setError(
        err instanceof Error ? err.message : t(WEBUI.standardQueries.readFailed),
      );
    } finally {
      if (requestIdRef.current === requestId) setLoading(false);
    }
  };

  const hydrateDetails = async (rows: QueryRecordRow[], requestId: number) => {
    if (!recorderName || rows.length === 0) return;
    setDetailLoading(true);
    const settled = await Promise.allSettled(
      rows.map((record) => fetchQueryRecordDetail(recorderName, record.id)),
    );
    if (requestIdRef.current !== requestId) return;
    const next: Record<number, QueryRecordDetail> = {};
    for (const result of settled) {
      if (result.status === "fulfilled") {
        next[result.value.record.id] = result.value.record;
      }
    }
    setDetailsById(next);
    setDetailLoading(false);
  };

  useEffect(() => {
    if (!recorderName) return;
    const timer = window.setTimeout(() => void load({}), 0);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recorderName]);

  const applyFilters = () => {
    const nextFilters = filtersFromForm(filterForm);
    setAppliedFilters(nextFilters);
    void load(nextFilters);
  };

  const clearFilters = () => {
    setFilterForm(EMPTY_FILTERS);
    setAppliedFilters({});
    void load({});
  };

  const openDetail = async (record: QueryRecordRow) => {
    const cached = detailsById[record.id];
    if (cached) {
      setSelected(cached);
      setRoutePathId(standardSettings.paths[0]?.id ?? "default");
      return;
    }
    if (!recorderName) return;
    setError(null);
    try {
      const detail = await fetchQueryRecordDetail(recorderName, record.id);
      setDetailsById((current) => ({
        ...current,
        [detail.record.id]: detail.record,
      }));
      setSelected(detail.record);
      setRoutePathId(standardSettings.paths[0]?.id ?? "default");
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t(WEBUI.standardQueries.readDetailFailed),
      );
    }
  };

  const selectedDomain = selected ? queryRecordDomain(selected) : "";

  const saveBlockRule = async () => {
    if (!selectedDomain) return;
    const rule = adGuardDomainRule(selectedDomain);
    const nextSettings = normalizeStandardFilteringSettings({
      ...standardSettings,
      filtering: {
        ...standardSettings.filtering,
        enabled: true,
        blockRules: uniqueAppend(standardSettings.filtering.blockRules, rule),
      },
    });
    const issues = validateStandardFilteringSettings(nextSettings, buildInfo);
    if (issues.length > 0) {
      setError(t(WEBUI.standardQueries.actionValidationFailed));
      return;
    }
    await saveAction("block", nextSettings, WEBUI.standardQueries.blockSaved);
  };

  const saveAllowRule = async () => {
    if (!selectedDomain) return;
    const rule = adGuardDomainRule(selectedDomain);
    const nextSettings = normalizeStandardFilteringSettings({
      ...standardSettings,
      filtering: {
        ...standardSettings.filtering,
        allowRules: uniqueAppend(standardSettings.filtering.allowRules, rule),
      },
    });
    const issues = validateStandardFilteringSettings(nextSettings, buildInfo);
    if (issues.length > 0) {
      setError(t(WEBUI.standardQueries.actionValidationFailed));
      return;
    }
    await saveAction("allow", nextSettings, WEBUI.standardQueries.allowSaved);
  };

  const saveRoutingRule = async () => {
    if (!selectedDomain) return;
    const rule = createRoutingRule(standardSettings, selectedDomain, routePathId);
    const nextSettings = normalizeStandardRoutingSettings({
      ...standardSettings,
      routing: {
        ...standardSettings.routing,
        enabled: true,
        rules: [...standardSettings.routing.rules, rule],
      },
    });
    const issues = validateStandardRoutingSettings(nextSettings, buildInfo);
    if (issues.length > 0) {
      setError(t(WEBUI.standardQueries.actionValidationFailed));
      return;
    }
    await saveAction("route", nextSettings, WEBUI.standardQueries.routeSaved);
  };

  const saveAction = async (
    action: string,
    settings: StandardModeSettings,
    successKey: string,
  ) => {
    setSavingAction(action);
    setError(null);
    setActionMessage(null);
    try {
      await saveStandardSettings(settings, { apply: true });
      setActionMessage(t(successKey, { domain: selectedDomain }));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingAction(null);
    }
  };

  return (
    <>
      <AppHeader title={t(WEBUI.standardQueries.title)} />
      <main className="oxidns-dialog-scrollbar min-h-0 flex-1 overflow-auto p-6">
        <div className="mx-auto max-w-7xl space-y-6">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight">
                {t(WEBUI.standardQueries.title)}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {t(WEBUI.standardQueries.description)}
              </p>
            </div>
            <Button onClick={() => void load()} disabled={!recorderName || loading}>
              {loading ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <RefreshCw className="size-4" />
              )}
              {t(WEBUI.standardQueries.refresh)}
            </Button>
          </div>

          {!capabilities.queryRecorder || !recorderName ? (
            <Card>
              <CardContent className="p-6 text-sm text-muted-foreground">
                {t(WEBUI.standardQueries.recorderUnavailable)}
              </CardContent>
            </Card>
          ) : (
            <Card>
              <CardHeader className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-center">
                <div>
                  <CardTitle className="text-base">
                    {t(WEBUI.standardQueries.recordsTitle)}
                  </CardTitle>
                  <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
                    <Badge variant="secondary">{recorderName}</Badge>
                    <Badge variant="outline">
                      {t(WEBUI.standardQueries.loadedRecords, {
                        count: formatNumber(records.length),
                      })}
                    </Badge>
                    {detailLoading ? (
                      <Badge variant="outline">
                        {t(WEBUI.standardQueries.enrichingRecords)}
                      </Badge>
                    ) : null}
                  </div>
                </div>
              </CardHeader>
              <CardContent className="space-y-4">
                <form
                  className="grid gap-3 rounded-lg border bg-muted/20 p-3 md:grid-cols-2 xl:grid-cols-[minmax(0,1fr)_150px_190px_150px_150px_auto]"
                  onSubmit={(event) => {
                    event.preventDefault();
                    applyFilters();
                  }}
                >
                  <FilterField label={t(WEBUI.standardQueries.qnameFilter)}>
                    <div className="relative">
                      <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
                      <Input
                        className="h-9 pl-8 font-mono"
                        value={filterForm.qname}
                        onChange={(event) =>
                          setFilterForm((current) => ({
                            ...current,
                            qname: event.target.value,
                          }))
                        }
                        placeholder="example.com"
                      />
                    </div>
                  </FilterField>
                  <FilterField label="QTYPE">
                    <Select
                      value={filterForm.qtype}
                      onValueChange={(qtype) =>
                        setFilterForm((current) => ({ ...current, qtype }))
                      }
                    >
                      <SelectTrigger className="h-9 font-mono">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="all">{t(WEBUI.common.all)}</SelectItem>
                        {QTYPE_OPTIONS.map((qtype) => (
                          <SelectItem key={qtype} value={qtype}>
                            {qtype}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </FilterField>
                  <FilterField label={t(WEBUI.standardQueries.clientFilter)}>
                    <Input
                      className="h-9 font-mono"
                      value={filterForm.clientIp}
                      onChange={(event) =>
                        setFilterForm((current) => ({
                          ...current,
                          clientIp: event.target.value,
                        }))
                      }
                      placeholder="192.168 / ::1"
                    />
                  </FilterField>
                  <FilterField label="RCODE">
                    <Input
                      className="h-9 font-mono"
                      value={filterForm.rcode}
                      onChange={(event) =>
                        setFilterForm((current) => ({
                          ...current,
                          rcode: event.target.value,
                        }))
                      }
                      placeholder="No Error"
                    />
                  </FilterField>
                  <FilterField label={t(WEBUI.standardQueries.statusFilter)}>
                    <Select
                      value={filterForm.status}
                      onValueChange={(status) =>
                        setFilterForm((current) => ({
                          ...current,
                          status: status as QueryRecordStatusFilter,
                        }))
                      }
                    >
                      <SelectTrigger className="h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="all">{t(WEBUI.common.all)}</SelectItem>
                        <SelectItem value="error">
                          {t(WEBUI.standardQueries.statusError)}
                        </SelectItem>
                        <SelectItem value="has_response">
                          {t(WEBUI.standardQueries.statusHasResponse)}
                        </SelectItem>
                        <SelectItem value="no_response">
                          {t(WEBUI.standardQueries.statusNoResponse)}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </FilterField>
                  <div className="flex items-end gap-2">
                    <Button type="submit" size="sm" className="h-9">
                      {t(WEBUI.common.apply)}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-9"
                      onClick={clearFilters}
                    >
                      {t(WEBUI.common.clear)}
                    </Button>
                  </div>
                </form>

                {error ? (
                  <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
                    {error}
                  </div>
                ) : null}
                {actionMessage ? (
                  <div className="rounded-md border border-primary/30 bg-primary/10 p-3 text-sm text-primary">
                    {actionMessage}
                  </div>
                ) : null}

                <div className="overflow-hidden rounded-md border">
                  <Table className="min-w-[1120px]">
                    <TableHeader>
                      <TableRow className="bg-muted/30 hover:bg-muted/30">
                        <TableHead>{t(WEBUI.standardQueries.timeColumn)}</TableHead>
                        <TableHead>{t(WEBUI.standardQueries.clientColumn)}</TableHead>
                        <TableHead>{t(WEBUI.standardQueries.domainColumn)}</TableHead>
                        <TableHead>QTYPE</TableHead>
                        <TableHead>RCODE</TableHead>
                        <TableHead>{t(WEBUI.standardQueries.pathColumn)}</TableHead>
                        <TableHead>{t(WEBUI.standardQueries.upstreamColumn)}</TableHead>
                        <TableHead>{t(WEBUI.standardQueries.ruleColumn)}</TableHead>
                        <TableHead>{t(WEBUI.standardQueries.outcomeColumn)}</TableHead>
                        <TableHead className="text-right">
                          {t(WEBUI.standardQueries.elapsedColumn)}
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {records.map((record) => {
                        const detail = detailsById[record.id] ?? record;
                        const explanation = explanations.get(record.id);
                        return (
                          <TableRow
                            key={record.id}
                            className="cursor-pointer"
                            onClick={() => void openDetail(record)}
                          >
                            <TableCell className="whitespace-nowrap font-mono text-xs text-muted-foreground">
                              {formatDateTime(record.created_at_ms)}
                            </TableCell>
                            <TableCell className="font-mono text-xs">
                              {record.client_ip}
                            </TableCell>
                            <TableCell className="max-w-[240px] truncate font-mono">
                              {questionName(record)}
                            </TableCell>
                            <TableCell className="font-mono">
                              {queryRecordQtype(record)}
                            </TableCell>
                            <TableCell>
                              {record.rcode ?? (record.error ? "ERROR" : "-")}
                            </TableCell>
                            <TableCell>
                              {explanation?.path?.name ??
                                t(WEBUI.standardQueries.unknown)}
                            </TableCell>
                            <TableCell>
                              {explanation?.upstreamGroup?.name ??
                                t(WEBUI.standardQueries.unknown)}
                            </TableCell>
                            <TableCell>
                              {ruleLabel(explanation, t)}
                            </TableCell>
                            <TableCell>
                              <OutcomeBadge
                                outcome={
                                  explanation?.outcome ??
                                  explainStandardQueryRecord(
                                    detail,
                                    standardSettings,
                                    standardLastGenerated,
                                  ).outcome
                                }
                              />
                            </TableCell>
                            <TableCell className="text-right font-mono">
                              {record.elapsed_ms}ms
                            </TableCell>
                          </TableRow>
                        );
                      })}
                      {records.length === 0 ? (
                        <TableRow>
                          <TableCell
                            colSpan={10}
                            className="h-24 text-center text-muted-foreground"
                          >
                            {loading
                              ? t(WEBUI.standardQueries.loading)
                              : t(WEBUI.standardQueries.empty)}
                          </TableCell>
                        </TableRow>
                      ) : null}
                    </TableBody>
                  </Table>
                </div>
              </CardContent>
            </Card>
          )}
        </div>
      </main>

      <RecordDetailDialog
        record={selected}
        explanation={
          selected
            ? explainStandardQueryRecord(
                selected,
                standardSettings,
                standardLastGenerated,
              )
            : null
        }
        routePathId={routePathId}
        settings={standardSettings}
        plugins={plugins}
        dependencyGraph={dependencyGraph}
        isBusy={isBusy}
        savingAction={savingAction}
        onRoutePathChange={setRoutePathId}
        onClose={() => setSelected(null)}
        onBlock={() => void saveBlockRule()}
        onAllow={() => void saveAllowRule()}
        onRoute={() => void saveRoutingRule()}
      />
    </>
  );
}

function FilterField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <Label className="grid gap-1 text-xs font-normal text-muted-foreground">
      <span>{label}</span>
      {children}
    </Label>
  );
}

function RecordDetailDialog({
  record,
  explanation,
  routePathId,
  settings,
  plugins,
  dependencyGraph,
  isBusy,
  savingAction,
  onRoutePathChange,
  onClose,
  onBlock,
  onAllow,
  onRoute,
}: {
  record: QueryRecordDetail | null;
  explanation: StandardQueryExplanation | null;
  routePathId: string;
  settings: StandardModeSettings;
  plugins: ReturnType<typeof useAppStore.getState>["plugins"];
  dependencyGraph: ReturnType<typeof useAppStore.getState>["dependencyGraph"];
  isBusy: boolean;
  savingAction: string | null;
  onRoutePathChange: (pathId: string) => void;
  onClose: () => void;
  onBlock: () => void;
  onAllow: () => void;
  onRoute: () => void;
}) {
  const { t, formatDateTime } = useI18n();
  const domain = record ? queryRecordDomain(record) : "";
  const canAct = Boolean(domain) && !isBusy;

  return (
    <DnsRecordDetailDialog
      open={Boolean(record)}
      onOpenChange={(open) => !open && onClose()}
      title={t(WEBUI.standardQueries.detailTitle, { id: record?.id ?? "" })}
      subtitle={record ? formatDateTime(record.created_at_ms) : undefined}
      status={
        explanation ? <OutcomeBadge outcome={explanation.outcome} /> : undefined
      }
      summaryItems={
        record
          ? [
              {
                label: t(WEBUI.standardQueries.clientColumn),
                value: record.client_ip,
                mono: true,
              },
              {
                label: t(WEBUI.standardQueries.domainColumn),
                value: domain || "-",
                mono: true,
              },
              {
                label: "QTYPE",
                value: queryRecordQtype(record),
                mono: true,
              },
              {
                label: t(WEBUI.standardQueries.elapsedColumn),
                value: `${record.elapsed_ms}ms`,
                mono: true,
              },
              { label: "RCODE", value: record.rcode ?? "-", mono: true },
              {
                label: t(WEBUI.standardQueries.pathColumn),
                value: explanation?.path?.name ?? t(WEBUI.standardQueries.unknown),
              },
              {
                label: t(WEBUI.standardQueries.upstreamColumn),
                value:
                  explanation?.upstreamGroup?.name ??
                  t(WEBUI.standardQueries.unknown),
              },
              {
                label: t(WEBUI.standardQueries.ruleColumn),
                value: ruleLabel(explanation, t),
              },
            ]
          : []
      }
      questions={record?.questions_json}
      leadingBlocks={
        record && explanation
          ? [
              {
                title: t(WEBUI.standardQueries.explanationTitle),
                children: <ExplanationSummary explanation={explanation} />,
              },
              {
                title: t(WEBUI.standardQueries.quickActionsTitle),
                children: (
                  <QuickActions
                    domain={domain}
                    paths={settings.paths}
                    routePathId={routePathId}
                    canAct={canAct}
                    savingAction={savingAction}
                    onRoutePathChange={onRoutePathChange}
                    onBlock={onBlock}
                    onAllow={onAllow}
                    onRoute={onRoute}
                  />
                ),
              },
            ]
          : []
      }
      sections={
        record
          ? [
              {
                title: t(WEBUI.dnsRecord.answerSection),
                records: record.answers_json,
                emptyLabel: t(WEBUI.dnsRecord.emptyAnswer),
              },
              {
                title: t(WEBUI.dnsRecord.authoritySection),
                records: record.authorities_json,
                emptyLabel: t(WEBUI.dnsRecord.emptyAuthority),
              },
              {
                title: t(WEBUI.dnsRecord.additionalSection),
                records: record.additionals_json,
                emptyLabel: t(WEBUI.dnsRecord.emptyAdditional),
              },
              {
                title: t(WEBUI.dnsRecord.signatureSection),
                records: record.signature_json,
                emptyLabel: t(WEBUI.dnsRecord.emptySignature),
              },
            ]
          : []
      }
      steps={record?.steps}
      error={record?.error ?? null}
      bottomBlocks={
        record
          ? [
              {
                title: t(WEBUI.standardQueries.executionFlowTitle),
                children: (
                  <QueryRecordFlowCanvas
                    record={record}
                    dependencyGraph={dependencyGraph}
                    plugins={plugins}
                  />
                ),
              },
            ]
          : []
      }
      wide
    />
  );
}

function ExplanationSummary({
  explanation,
}: {
  explanation: StandardQueryExplanation;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-3">
      {!explanation.hasTagMap || !explanation.hasSteps ? (
        <div className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-300">
          {t(WEBUI.standardQueries.rawOnlyNotice)}
        </div>
      ) : null}
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
        <StatusTile
          label={t(WEBUI.standardQueries.filteringStatus)}
          value={t(filteringLabelKey(explanation.filtering))}
        />
        <StatusTile
          label={t(WEBUI.standardQueries.cacheStatus)}
          value={t(cacheLabelKey(explanation.cache))}
        />
        <StatusTile
          label={t(WEBUI.standardQueries.queryLogStatus)}
          value={t(queryLogLabelKey(explanation.queryLog))}
        />
        <StatusTile
          label={t(WEBUI.standardQueries.outcomeColumn)}
          value={t(outcomeLabelKey(explanation.outcome))}
        />
      </div>
      {explanation.rawEvents.length > 0 ? (
        <div className="max-h-40 overflow-auto rounded-md border bg-muted/20 p-2 font-mono text-xs">
          {explanation.rawEvents.map((event, index) => (
            <div key={`${event}-${index}`} className="truncate">
              {event}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function StatusTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border bg-muted/20 px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 text-sm font-medium">{value}</div>
    </div>
  );
}

function QuickActions({
  domain,
  paths,
  routePathId,
  canAct,
  savingAction,
  onRoutePathChange,
  onBlock,
  onAllow,
  onRoute,
}: {
  domain: string;
  paths: StandardModeSettings["paths"];
  routePathId: string;
  canAct: boolean;
  savingAction: string | null;
  onRoutePathChange: (pathId: string) => void;
  onBlock: () => void;
  onAllow: () => void;
  onRoute: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-3">
      <div className="text-sm text-muted-foreground">
        {domain
          ? t(WEBUI.standardQueries.quickActionsDescription, { domain })
          : t(WEBUI.standardQueries.quickActionsUnavailable)}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button type="button" size="sm" disabled={!canAct} onClick={onBlock}>
          {savingAction === "block" ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <Ban className="size-4" />
          )}
          {t(WEBUI.standardQueries.addBlockRule)}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={!canAct}
          onClick={onAllow}
        >
          {savingAction === "allow" ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <CheckCircle2 className="size-4" />
          )}
          {t(WEBUI.standardQueries.addAllowRule)}
        </Button>
      </div>
      <div className="grid gap-2 md:grid-cols-[minmax(0,260px)_auto]">
        <Select value={routePathId} onValueChange={onRoutePathChange}>
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {paths.map((path) => (
              <SelectItem key={path.id} value={path.id}>
                {path.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          type="button"
          size="sm"
          variant="secondary"
          disabled={!canAct}
          onClick={onRoute}
        >
          {savingAction === "route" ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <Route className="size-4" />
          )}
          {t(WEBUI.standardQueries.createRouteRule)}
        </Button>
      </div>
    </div>
  );
}

function OutcomeBadge({ outcome }: { outcome: StandardQueryOutcome }) {
  const { t } = useI18n();
  const variant =
    outcome === "error" || outcome === "blocked" ? "destructive" : "secondary";
  return <Badge variant={variant}>{t(outcomeLabelKey(outcome))}</Badge>;
}

function ruleLabel(
  explanation: StandardQueryExplanation | undefined | null,
  t: ReturnType<typeof useI18n>["t"],
) {
  if (explanation?.exceptionRule) {
    return t(WEBUI.standardQueries.exceptionRuleLabel, {
      name: explanation.exceptionRule.name,
    });
  }
  if (explanation?.routingRule) {
    return t(WEBUI.standardQueries.routingRuleLabel, {
      name: explanation.routingRule.name,
    });
  }
  return t(WEBUI.standardQueries.unknown);
}

function outcomeLabelKey(outcome: StandardQueryOutcome) {
  if (outcome === "error") return WEBUI.standardQueries.outcomeError;
  if (outcome === "blocked") return WEBUI.standardQueries.outcomeBlocked;
  if (outcome === "cache") return WEBUI.standardQueries.outcomeCache;
  if (outcome === "exception") return WEBUI.standardQueries.outcomeException;
  if (outcome === "routing") return WEBUI.standardQueries.outcomeRouting;
  if (outcome === "resolved") return WEBUI.standardQueries.outcomeResolved;
  if (outcome === "no_response") return WEBUI.standardQueries.outcomeNoResponse;
  return WEBUI.standardQueries.unknown;
}

function filteringLabelKey(value: StandardQueryExplanation["filtering"]) {
  if (value === "blocked") return WEBUI.standardQueries.filteringBlocked;
  if (value === "checked") return WEBUI.standardQueries.filteringChecked;
  if (value === "skipped") return WEBUI.standardQueries.filteringSkipped;
  return WEBUI.standardQueries.unknown;
}

function cacheLabelKey(value: StandardQueryExplanation["cache"]) {
  if (value === "checked") return WEBUI.standardQueries.cacheChecked;
  return WEBUI.standardQueries.unknown;
}

function queryLogLabelKey(value: StandardQueryExplanation["queryLog"]) {
  if (value === "recorded") return WEBUI.standardQueries.queryLogRecorded;
  return WEBUI.standardQueries.unknown;
}
