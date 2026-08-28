/*
 * SPDX-FileCopyrightText: 2025 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import { useState } from "react";
import {
  Clock,
  Loader2,
  Minus,
  Pencil,
  Play,
  Plus,
  Save,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { YamlEditor } from "@/components/config/yaml-editor";
import { useAppStore } from "@/lib/store";
import {
  parseArgsLevelPluginConfigYaml,
  stringifyArgsLevelPluginConfigYaml,
} from "@/lib/plugin-config-yaml";
import { CreatePluginDialog } from "@/components/plugins/create-plugin-dialog";
import { AdvancedSettingsSection } from "@/components/plugins/advanced-settings-section";
import { PluginReferencePicker } from "@/components/plugins/plugin-reference-picker";
import {
  InlineSelect,
  QuickSetupRow,
  createItemId,
  createStableItemId,
  firstQuickSetupKind,
  isQuickSetupValue,
  stripReferencePrefix,
} from "@/components/plugins/plugin-ref-editor";
import type {
  PluginComponentDefinition,
  PluginDetailComponentProps,
} from "@/components/plugins/types";
import { PluginDetailTemplate } from "@/components/plugins/plugin-detail-template";
import type { PluginInstance } from "@/lib/types";
import { cn } from "@/lib/utils";
import { WEBUI } from "@/lib/i18n";
import { useI18n } from "@/lib/i18n/provider";
import {
  CronJobAlreadyRunningError,
  CronJobUnavailableError,
  runCronJob,
} from "@/lib/oxidns-api";
import { usePluginAppliedStatus } from "@/hooks/use-plugin-applied";
import { cronManualRunRuntimeTag } from "@/lib/cron-manual-run";

// ─── Types ────────────────────────────────────────────────────────────────────

type ExecutorItemMode = "reference" | "quick_setup" | "text";

interface CronExecutorItem {
  id: string;
  mode: ExecutorItemMode;
  value: string;
}

interface CronJob {
  id: string;
  name: string;
  schedule: string;
  interval: string;
  executors: CronExecutorItem[];
}

// ─── Parse / serialize ────────────────────────────────────────────────────────

function parseExecutorItem(text: string, id: string): CronExecutorItem {
  const trimmed = text.trim();
  if (trimmed.startsWith("$")) {
    return { id, mode: "reference", value: trimmed.slice(1) };
  }
  if (isQuickSetupValue(trimmed, "executor")) {
    return { id, mode: "quick_setup", value: trimmed };
  }
  return { id, mode: "text", value: trimmed };
}

function serializeExecutorItem(item: CronExecutorItem): string {
  if (item.mode === "reference") return `$${item.value}`;
  return item.value;
}

function parseCronJobs(value: unknown): CronJob[] {
  if (!Array.isArray(value)) return [];
  return value.map((entry, jobIdx) => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
      return createEmptyCronJob();
    }
    const record = entry as Record<string, unknown>;
    const execRaw = Array.isArray(record.executors) ? record.executors : [];
    const executors = execRaw
      .filter((e): e is string => typeof e === "string" && e.trim().length > 0)
      .map((e, i) =>
        parseExecutorItem(e, createStableItemId(`job_${jobIdx}_exec`, i)),
      );
    return {
      id: createStableItemId("job", jobIdx),
      name: typeof record.name === "string" ? record.name : "",
      schedule: typeof record.schedule === "string" ? record.schedule : "",
      interval: typeof record.interval === "string" ? record.interval : "",
      executors,
    };
  });
}

function serializeCronJobs(jobs: CronJob[]): object[] {
  return jobs.map((job) => {
    // `name` is required by the backend (see cron docs: args.jobs[].name).
    // Always emit it — even when empty — so saving an unfilled job surfaces
    // the validation error instead of producing a silently-malformed config.
    const entry: Record<string, unknown> = { name: job.name.trim() };
    if (job.schedule.trim()) entry.schedule = job.schedule.trim();
    if (job.interval.trim()) entry.interval = job.interval.trim();
    const executors = job.executors
      .map(serializeExecutorItem)
      .filter((s) => s.trim().length > 0);
    if (executors.length > 0) entry.executors = executors;
    return entry;
  });
}

function createEmptyCronJob(existing: CronJob[] = []): CronJob {
  const used = new Set(existing.map((job) => job.name.trim()).filter(Boolean));
  let index = existing.length + 1;
  let candidate = `job_${index}`;
  while (used.has(candidate)) {
    index += 1;
    candidate = `job_${index}`;
  }
  return {
    id: createItemId(),
    name: candidate,
    schedule: "",
    interval: "",
    executors: [],
  };
}

function createEmptyExecutorItem(): CronExecutorItem {
  return {
    id: createItemId(),
    mode: "reference",
    value: "",
  };
}

// ─── CronComposer ─────────────────────────────────────────────────────────────

interface CronComposerProps {
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
  plugins: PluginInstance[];
  readOnly?: boolean;
  runtimeTag?: string;
}

export function CronComposer({
  value,
  onChange,
  plugins,
  readOnly = false,
  runtimeTag,
}: CronComposerProps) {
  const { t } = useI18n();
  const [view, setView] = useState<"visual" | "yaml">("visual");
  const [yamlText, setYamlText] = useState(() =>
    stringifyArgsLevelPluginConfigYaml(value),
  );
  const [yamlError, setYamlError] = useState<string | null>(null);

  const jobs = parseCronJobs(value.jobs);
  const timezone = typeof value.timezone === "string" ? value.timezone : "";

  const updateJobs = (nextJobs: CronJob[]) => {
    onChange({ ...value, jobs: serializeCronJobs(nextJobs) });
  };

  const addJob = () => updateJobs([...jobs, createEmptyCronJob(jobs)]);

  const updateJob = (jobId: string, patch: Partial<CronJob>) => {
    updateJobs(
      jobs.map((job) => (job.id === jobId ? { ...job, ...patch } : job)),
    );
  };

  const deleteJob = (jobId: string) => {
    updateJobs(jobs.filter((job) => job.id !== jobId));
  };

  const handleViewChange = (nextView: "visual" | "yaml") => {
    if (nextView === "yaml") {
      setYamlText(stringifyArgsLevelPluginConfigYaml(value));
      setYamlError(null);
    }
    setView(nextView);
  };

  const handleYamlChange = (nextYaml: string) => {
    setYamlText(nextYaml);
    if (readOnly) return;
    const parsed = parseArgsLevelPluginConfigYaml(nextYaml);
    if (parsed.error) {
      setYamlError(parsed.error);
      return;
    }
    if (
      parsed.value &&
      typeof parsed.value === "object" &&
      !Array.isArray(parsed.value)
    ) {
      setYamlError(null);
      onChange(parsed.value as Record<string, unknown>);
      return;
    }
    setYamlError(t(WEBUI.cron.yamlMustBeObject));
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Tabs
          value={view}
          onValueChange={(v) => handleViewChange(v as typeof view)}
        >
          <TabsList className="grid w-44 max-w-full grid-cols-2">
            <TabsTrigger value="visual">{t(WEBUI.cron.taskTab)}</TabsTrigger>
            <TabsTrigger value="yaml">YAML</TabsTrigger>
          </TabsList>
        </Tabs>
        {view === "yaml" && yamlError && (
          <Badge
            variant="destructive"
            className="h-auto gap-1 whitespace-normal py-1"
          >
            {yamlError}
          </Badge>
        )}
        {!readOnly && view === "visual" && (
          <div className="flex flex-wrap items-center gap-2">
            <CreateDependencyCronButton />
            <Button type="button" size="sm" onClick={addJob}>
              <Plus className="h-4 w-4" />
              {t(WEBUI.cron.addTask)}
            </Button>
          </div>
        )}
      </div>

      {view === "visual" && (
        <div className="space-y-3">
          <AdvancedSettingsSection
            defaultOpen={Object.prototype.hasOwnProperty.call(
              value,
              "timezone",
            )}
          >
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">
                {t(WEBUI.cron.timezone)}
              </span>
              <Input
                value={timezone}
                onChange={(e) =>
                  onChange({ ...value, timezone: e.target.value || undefined })
                }
                placeholder={t(WEBUI.cron.timezonePlaceholder)}
                className="h-8 max-w-xs font-mono text-xs"
                disabled={readOnly}
              />
            </div>
          </AdvancedSettingsSection>
          {jobs.length === 0 ? (
            <div className="rounded-lg border border-dashed p-8 text-center">
              <Clock className="mx-auto h-8 w-8 text-muted-foreground" />
              <div className="mt-3 text-sm font-medium">
                {t(WEBUI.cron.noTasks)}
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                {t(WEBUI.cron.noTasksDesc)}
              </p>
              {!readOnly && (
                <Button type="button" className="mt-4" onClick={addJob}>
                  <Plus className="h-4 w-4" />
                  {t(WEBUI.cron.addTask)}
                </Button>
              )}
            </div>
          ) : (
            jobs.map((job, index) => (
              <CronJobCard
                key={job.id}
                job={job}
                index={index}
                total={jobs.length}
                plugins={plugins}
                readOnly={readOnly}
                runtimeTag={runtimeTag}
                onChange={(patch) => updateJob(job.id, patch)}
                onDelete={() => deleteJob(job.id)}
              />
            ))
          )}
        </div>
      )}

      {view === "yaml" && (
        <YamlEditor
          value={yamlText}
          onChange={handleYamlChange}
          readOnly={readOnly}
          className="min-h-[260px]"
          variant="plugin-args"
          plugins={plugins}
          pluginKind="cron"
        />
      )}
    </div>
  );
}

// ─── CronJobCard ──────────────────────────────────────────────────────────────

function CronJobCard({
  job,
  index,
  total,
  plugins,
  readOnly,
  runtimeTag,
  onChange,
  onDelete,
}: {
  job: CronJob;
  index: number;
  total: number;
  plugins: PluginInstance[];
  readOnly: boolean;
  runtimeTag?: string;
  onChange: (patch: Partial<CronJob>) => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  const [manualRunPending, setManualRunPending] = useState(false);
  const [manualRunNotice, setManualRunNotice] = useState<{
    tone: "success" | "error";
    message: string;
  } | null>(null);
  const addExecutor = () => {
    onChange({ executors: [...job.executors, createEmptyExecutorItem()] });
  };

  const updateExecutor = (itemId: string, patch: Partial<CronExecutorItem>) => {
    onChange({
      executors: job.executors.map((item) =>
        item.id === itemId ? { ...item, ...patch } : item,
      ),
    });
  };

  const deleteExecutor = (itemId: string) => {
    onChange({ executors: job.executors.filter((item) => item.id !== itemId) });
  };

  const handleManualRun = async () => {
    if (!runtimeTag || !job.name.trim()) return;
    setManualRunPending(true);
    setManualRunNotice(null);
    try {
      await runCronJob(runtimeTag, job.name.trim());
      setManualRunNotice({
        tone: "success",
        message: t(WEBUI.cron.runStarted, { name: job.name.trim() }),
      });
    } catch (error) {
      setManualRunNotice({
        tone: "error",
        message:
          error instanceof CronJobAlreadyRunningError
            ? t(WEBUI.cron.runBusy, { name: job.name.trim() })
            : error instanceof CronJobUnavailableError
              ? t(WEBUI.cron.runUnavailable, { name: job.name.trim() })
            : error instanceof Error
              ? error.message
              : t(WEBUI.cron.runFailed, { name: job.name.trim() }),
      });
    } finally {
      setManualRunPending(false);
    }
  };

  return (
    <Card className="rounded-lg border bg-background shadow-sm">
      <CardHeader className="p-3 pb-2">
        <div className="flex min-w-0 items-center gap-2">
          <Clock className="h-4 w-4 shrink-0 text-muted-foreground" />
          {readOnly ? (
            <span className="min-w-0 flex-1 truncate font-mono text-sm font-medium">
              {job.name || (
                <span className="text-muted-foreground">
                  {t(WEBUI.cron.unnamedTask, { index: index + 1 })}
                </span>
              )}
            </span>
          ) : (
            <div className="flex min-w-0 flex-1 items-center gap-1.5">
              <Input
                value={job.name}
                onChange={(e) => onChange({ name: e.target.value })}
                placeholder={t(WEBUI.cron.taskNamePlaceholder)}
                aria-invalid={!job.name.trim()}
                className={cn(
                  "h-7 min-w-0 flex-1 font-mono text-xs",
                  !job.name.trim() &&
                    "border-destructive focus-visible:ring-destructive",
                )}
              />
              <span
                className="text-destructive"
                aria-hidden="true"
                title={t(WEBUI.cron.required)}
              >
                *
              </span>
            </div>
          )}
          <Badge variant="outline" className="shrink-0 font-mono text-[10px]">
            #{index + 1} / {total}
          </Badge>
          {readOnly && runtimeTag && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 shrink-0 px-2 text-xs"
              onClick={() => void handleManualRun()}
              disabled={manualRunPending || !job.name.trim()}
              title={t(WEBUI.cron.runNow)}
            >
              {manualRunPending ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Play className="h-3.5 w-3.5" />
              )}
              {manualRunPending
                ? t(WEBUI.cron.runStarting)
                : t(WEBUI.cron.runNow)}
            </Button>
          )}
          {!readOnly && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
              onClick={onDelete}
              aria-label={t(WEBUI.cron.deleteTask)}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-3 p-3 pt-0">
        {manualRunNotice && (
          <div
            role={manualRunNotice.tone === "error" ? "alert" : "status"}
            className={cn(
              "rounded-md border px-2.5 py-1.5 text-xs",
              manualRunNotice.tone === "error"
                ? "border-destructive/40 bg-destructive/5 text-destructive"
                : "border-primary/30 bg-primary/5 text-primary",
            )}
          >
            {manualRunNotice.message}
          </div>
        )}
        {/* Schedule / interval */}
        <div className="grid gap-2 sm:grid-cols-2">
          <div className="space-y-1">
            <div className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              {t(WEBUI.cron.cronExpr)}
            </div>
            <Input
              value={job.schedule}
              onChange={(e) => onChange({ schedule: e.target.value })}
              placeholder="0 */6 * * *"
              className="h-8 font-mono text-xs"
              disabled={readOnly}
            />
          </div>
          <div className="space-y-1">
            <div className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              {t(WEBUI.cron.fixedInterval)}
            </div>
            <Input
              value={job.interval}
              onChange={(e) => onChange({ interval: e.target.value })}
              placeholder="5m / 1h"
              className="h-8 font-mono text-xs"
              disabled={readOnly}
            />
          </div>
        </div>

        {/* Executors */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <div className="text-[10px] font-semibold uppercase tracking-wide text-sky-700 dark:text-sky-300">
              {t(WEBUI.cron.executorList)}
            </div>
            {!readOnly && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-6 px-2 text-xs"
                onClick={addExecutor}
              >
                <Plus className="h-3 w-3" />
                {t(WEBUI.cron.addExecutor)}
              </Button>
            )}
          </div>
          {job.executors.length === 0 ? (
            <div className="rounded-md border border-dashed border-sky-300/60 bg-sky-50/30 px-3 py-3 text-center text-xs italic text-muted-foreground dark:border-sky-800/40 dark:bg-sky-950/15">
              {t(WEBUI.cron.noExecutors)}
            </div>
          ) : (
            <div className="space-y-1.5">
              {job.executors.map((item) => (
                <CronExecutorEditor
                  key={item.id}
                  item={item}
                  plugins={plugins}
                  readOnly={readOnly}
                  onChange={(patch) => updateExecutor(item.id, patch)}
                  onDelete={() => deleteExecutor(item.id)}
                />
              ))}
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

// ─── CronExecutorEditor ───────────────────────────────────────────────────────

function CronExecutorEditor({
  item,
  plugins,
  readOnly,
  onChange,
  onDelete,
}: {
  item: CronExecutorItem;
  plugins: PluginInstance[];
  readOnly: boolean;
  onChange: (patch: Partial<CronExecutorItem>) => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  const [localMode, setLocalMode] = useState<ExecutorItemMode>(item.mode);

  const handleModeChange = (mode: ExecutorItemMode) => {
    if (mode === localMode) return;
    setLocalMode(mode);
    if (mode === "reference") {
      const tag = stripReferencePrefix(item.value);
      onChange({ mode, value: tag || "" });
    } else if (mode === "quick_setup") {
      onChange({ mode, value: firstQuickSetupKind("executor") || "drop_resp" });
    } else {
      onChange({ mode, value: "" });
    }
  };

  return (
    <div className="rounded-md border border-sky-200/80 bg-sky-50/40 px-2 py-1.5 dark:border-sky-800/40 dark:bg-sky-950/20">
      <div className="flex min-w-0 items-center gap-1.5">
        <InlineSelect
          value={localMode}
          onChange={(m) => handleModeChange(m as ExecutorItemMode)}
          disabled={readOnly}
          className={cn(
            "shrink-0",
            localMode === "quick_setup" ? "w-[4.5rem]" : "w-[4.5rem]",
          )}
          options={[
            { value: "reference", label: t(WEBUI.sequence.modeReference) },
            { value: "quick_setup", label: t(WEBUI.sequence.modeQuickSetup) },
            { value: "text", label: t(WEBUI.sequence.modeText) },
          ]}
        />
        <div className="min-w-0 flex-1">
          {localMode === "reference" ? (
            <PluginReferencePicker
              plugins={plugins}
              value={stripReferencePrefix(item.value)}
              referenceTypes={["executor"]}
              disabled={readOnly}
              placeholder={t(WEBUI.sequence.selectExecutor)}
              createDescription={t(WEBUI.cron.createRefDesc)}
              allowCreate
              onChange={(tag) => onChange({ value: tag })}
            />
          ) : localMode === "quick_setup" ? (
            <QuickSetupRow
              type="executor"
              value={item.value}
              plugins={plugins}
              readOnly={readOnly}
              onChange={(next) => onChange({ value: next })}
            />
          ) : (
            <Input
              value={item.value}
              onChange={(e) => onChange({ value: e.target.value })}
              placeholder="debug_print / reload / reload_provider $geosite_cn"
              className="h-8 w-full font-mono text-xs"
              disabled={readOnly}
            />
          )}
        </div>
        {!readOnly && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
            onClick={onDelete}
            aria-label={t(WEBUI.cron.deleteExecutor)}
          >
            <Minus className="h-3.5 w-3.5" />
          </Button>
        )}
      </div>
    </div>
  );
}

// ─── Standalone create dependency button ─────────────────────────────────────

function CreateDependencyCronButton() {
  const { t } = useI18n();
  return (
    <CreatePluginDialog
      defaultType="executor"
      supportedTypes={["executor"]}
      title={t(WEBUI.cron.createDepsTitle)}
      description={t(WEBUI.cron.createDepsDesc)}
      trigger={
        <Button type="button" variant="outline" size="sm">
          <Plus className="h-4 w-4" />
          {t(WEBUI.cron.createDepsBtn)}
        </Button>
      }
    />
  );
}

// ─── CronDetail (kind component entry point) ─────────────────────────────────

function CronDetail({
  plugin,
  chartData,
  onClose,
}: PluginDetailComponentProps) {
  const { t } = useI18n();
  const updatePluginConfig = useAppStore((state) => state.updatePluginConfig);
  const saveConfig = useAppStore((state) => state.saveConfig);
  const isConfigSaving = useAppStore((state) => state.isConfigSaving);
  const plugins = useAppStore((state) => state.plugins);
  const configVersion = useAppStore((state) => state.configVersion);
  const runningVersion = useAppStore((state) => state.runningVersion);
  const appliedStatus = usePluginAppliedStatus(plugin.name);
  const [editing, setEditing] = useState(false);
  const [configValues, setConfigValues] = useState<Record<string, unknown>>(
    () => plugin.config,
  );

  const jobCount = parseCronJobs(configValues.jobs).length;

  const handleCancel = () => {
    setConfigValues(plugin.config);
    setEditing(false);
  };

  const handleSave = async () => {
    updatePluginConfig(plugin.id, configValues);
    try {
      await saveConfig();
      setEditing(false);
    } catch {
      // Store-level config errors are surfaced in the full config editor.
    }
  };

  return (
    <PluginDetailTemplate
      plugin={plugin}
      chartData={chartData}
      onClose={onClose}
      summaryItems={[
        { label: t(WEBUI.cron.taskCount), value: String(jobCount) },
      ]}
      configContent={
        <Card>
          <CardHeader className="grid grid-cols-[1fr_auto] items-center p-4 pb-2">
            <CardTitle className="text-sm">
              {t(WEBUI.cron.arrangement)}
            </CardTitle>
            <div className="flex gap-2">
              {editing ? (
                <>
                  <Button variant="outline" size="sm" onClick={handleCancel}>
                    {t(WEBUI.common.cancel)}
                  </Button>
                  <Button
                    size="sm"
                    onClick={handleSave}
                    disabled={isConfigSaving}
                  >
                    <Save className="h-4 w-4" />
                    {isConfigSaving
                      ? t(WEBUI.sequence.saving)
                      : t(WEBUI.common.saveConfig)}
                  </Button>
                </>
              ) : (
                <Button size="sm" onClick={() => setEditing(true)}>
                  <Pencil className="h-4 w-4" />
                  {t(WEBUI.common.editConfig)}
                </Button>
              )}
            </div>
          </CardHeader>
          <CardContent className="p-4 pt-0">
            <CronComposer
              value={configValues}
              onChange={setConfigValues}
              plugins={plugins}
              readOnly={!editing}
              runtimeTag={cronManualRunRuntimeTag(
                editing,
                appliedStatus,
                plugin.name,
                configVersion,
                runningVersion,
              )}
            />
          </CardContent>
        </Card>
      }
    />
  );
}

export const cronPlugin: PluginComponentDefinition = {
  Detail: CronDetail,
};
