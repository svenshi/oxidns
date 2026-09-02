"use client";

import { useMemo, useState } from "react";
import type React from "react";
import { SheetTitle } from "@/components/ui/sheet";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { AlertTriangle, Pencil, Pin, PinOff, Rocket, Save } from "lucide-react";
import { shouldPersistPluginMutation, useAppStore } from "@/lib/store";
import { extractOutboundProfileNames } from "@/lib/oxidns-config-schema";
import { isPluginKindSupported } from "@/lib/build-capabilities";
import { cn } from "@/lib/utils";
import { Spinner } from "@/components/ui/spinner";
import { usePluginAppliedStatus } from "@/hooks/use-plugin-applied";
import { WEBUI } from "@/lib/i18n";
import { pluginTypeLabel } from "@/lib/i18n/plugin-defined";
import { useI18n } from "@/lib/i18n/provider";
import {
  isReservedPluginTag,
  pluginTagValidationMessageKey,
  validatePluginTag,
} from "@/lib/plugin-tags";
import type { PluginDetailTemplateProps, PluginSummaryItem } from "./types";
import { pluginTypeColors, pluginTypeIcons } from "./display";
import { getPluginCatalogItem, renderPluginKindIcon } from "./catalog";
import { PluginConfigModeEditor } from "./plugin-config-mode-editor";
import { PluginMetricsPanel } from "./plugin-metrics-panel";
import { PluginDeleteButton } from "./plugin-delete-button";
import type { PluginReferenceImpact } from "@/lib/plugin-reference-operations";
import { MatcherRuntimeControl } from "./matcher-runtime-control";
import { ProviderRuntimeControl } from "./provider-runtime-control";

export function PluginDetailTemplate({
  plugin,
  onClose,
  icon,
  summaryItems,
  configContent,
  metricsContent,
  extraTabs,
}: PluginDetailTemplateProps) {
  const { locale, t } = useI18n();
  const {
    togglePluginPin,
    updatePluginConfig,
    renamePlugin,
    saveConfig,
    applyConfig,
    isConfigSaving,
    isApplying,
    isRestarting,
    configError,
    plugins,
    configModel,
    dependencyGraph,
    buildInfo,
  } = useAppStore();
  const outboundProfileNames = useMemo(
    () => extractOutboundProfileNames(configModel),
    [configModel],
  );
  const appliedStatus = usePluginAppliedStatus(plugin.name);
  const hasMetricSeries = useAppStore(
    (s) => (s.pluginMetrics[plugin.name]?.length ?? 0) > 0,
  );
  const definition = getPluginCatalogItem(plugin.pluginKind, locale);
  const supported = isPluginKindSupported(
    buildInfo,
    plugin.type,
    plugin.pluginKind,
  );
  const resolvedIcon =
    icon ??
    (definition
      ? renderPluginKindIcon(definition.icon, { className: "h-5 w-5" })
      : pluginTypeIcons[plugin.type]);
  const [configJson, setConfigJson] = useState(() =>
    JSON.stringify(plugin.config, null, 2),
  );
  const [configValues, setConfigValues] = useState<Record<string, unknown>>(
    () => (definition ? plugin.config : {}),
  );
  const [editingName, setEditingName] = useState(false);
  const [editingConfig, setEditingConfig] = useState(false);
  const [configValid, setConfigValid] = useState(true);
  const [newName, setNewName] = useState(plugin.name);
  const [nameError, setNameError] = useState<string | null>(null);
  const [pendingRename, setPendingRename] = useState<{
    name: string;
    references: PluginReferenceImpact[];
  } | null>(null);

  const configBusy = isConfigSaving || isApplying || isRestarting;
  const normalizedNewName = newName.trim();
  const tagValidationError = normalizedNewName
    ? validatePluginTag(normalizedNewName)
    : null;
  const nameValidationError = tagValidationError
    ? t(pluginTagValidationMessageKey(tagValidationError))
    : normalizedNewName && isReservedPluginTag(normalizedNewName)
      ? t(WEBUI.storeErrors.pluginNameReserved)
      : null;
  const displayedNameError = nameError ?? nameValidationError;
  const nameSaveDisabled =
    configBusy ||
    Boolean(configError) ||
    !normalizedNewName ||
    Boolean(nameValidationError);

  const handleSaveConfig = async () => {
    if (!configValid) return;
    if (definition) {
      try {
        const resolution = await updatePluginConfig(plugin.id, configValues);
        if (shouldPersistPluginMutation(resolution)) await saveConfig();
        if (resolution !== "cancelled") setEditingConfig(false);
      } catch {
        // Store-level error badge is shown in the full config editor.
      }
      return;
    }

    try {
      const resolution = await updatePluginConfig(
        plugin.id,
        JSON.parse(configJson),
      );
      if (shouldPersistPluginMutation(resolution)) await saveConfig();
      if (resolution !== "cancelled") setEditingConfig(false);
    } catch {
      // Invalid JSON. Validation UI can be added once backend config errors are wired in.
    }
  };

  const handleCancelConfigEdit = () => {
    setConfigJson(JSON.stringify(plugin.config, null, 2));
    setConfigValues(definition ? plugin.config : {});
    setConfigValid(true);
    setEditingConfig(false);
  };

  const handleSaveName = async () => {
    setNameError(null);
    if (!normalizedNewName) {
      setNameError(t(WEBUI.storeErrors.pluginNameRequired));
      return;
    }
    if (nameValidationError) {
      setNameError(nameValidationError);
      return;
    }
    try {
      const result = await renamePlugin(plugin.id, normalizedNewName);
      if (result.status === "invalid") {
        setNameError(result.message);
        return;
      }
      if (result.status === "needs-confirmation") {
        setPendingRename({
          name: normalizedNewName,
          references: result.references,
        });
        return;
      }
      if (result.status !== "cancelled") setEditingName(false);
    } catch (error) {
      setNameError(
        error instanceof Error ? error.message : t(WEBUI.plugins.renameFailed),
      );
    }
  };

  const handleConfirmRename = async () => {
    if (!pendingRename) return;
    setNameError(null);
    try {
      const result = await renamePlugin(plugin.id, pendingRename.name, {
        confirmed: true,
      });
      if (result.status === "invalid") {
        setNameError(result.message);
        return;
      }
      if (result.status !== "cancelled") {
        setPendingRename(null);
        setEditingName(false);
      }
    } catch (error) {
      setNameError(
        error instanceof Error ? error.message : t(WEBUI.plugins.renameFailed),
      );
    }
  };

  const resolvedSummaryItems = summaryItems ?? [];
  const hasStatsContent =
    metricsContent !== undefined &&
    metricsContent !== null &&
    metricsContent !== false;
  const resolvedExtraTabs = extraTabs ?? [];
  const visibleTabCount =
    1 +
    (hasStatsContent ? 1 : 0) +
    resolvedExtraTabs.length +
    (hasMetricSeries ? 1 : 0);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <header className="shrink-0 border-b bg-sidebar/70">
        <div className="mx-auto w-full max-w-6xl px-5 py-5">
          <div className="flex min-w-0 items-start gap-4 pr-14">
            <div className="flex size-12 shrink-0 items-center justify-center rounded-xl border border-primary/20 bg-primary/12 text-primary [&_svg]:size-5">
              {resolvedIcon}
            </div>
            <div className="min-w-0 flex-1 pt-0.5">
              {editingName ? (
                <div className="space-y-1.5">
                  <div className="flex items-center gap-2">
                    <Input
                      value={newName}
                      onChange={(e) => {
                        setNewName(e.target.value);
                        setNameError(null);
                      }}
                      disabled={configBusy || Boolean(configError)}
                      className="h-9 max-w-md font-mono text-lg"
                      aria-invalid={Boolean(displayedNameError)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && !nameSaveDisabled) {
                          void handleSaveName();
                        }
                        if (e.key === "Escape") {
                          setNewName(plugin.name);
                          setNameError(null);
                          setEditingName(false);
                        }
                      }}
                    />
                    <Button
                      size="icon-sm"
                      disabled={nameSaveDisabled}
                      onClick={() => void handleSaveName()}
                    >
                      <Save className="h-4 w-4" />
                    </Button>
                  </div>
                  {displayedNameError && (
                    <p className="text-xs text-destructive">
                      {displayedNameError}
                    </p>
                  )}
                </div>
              ) : (
                <SheetTitle
                  className={cn(
                    "truncate font-mono text-xl font-semibold leading-none transition-colors",
                    configBusy || configError
                      ? "cursor-default"
                      : "cursor-pointer hover:text-primary",
                  )}
                  onClick={() => {
                    if (!configBusy && !configError) setEditingName(true);
                  }}
                >
                  {plugin.name}
                </SheetTitle>
              )}
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <Badge
                  variant="outline"
                  className={cn("gap-1", pluginTypeColors[plugin.type])}
                >
                  {pluginTypeLabel(plugin.type, locale)}
                </Badge>
                <Badge variant="outline" className="bg-background/70">
                  {definition?.name ?? plugin.pluginKind}
                </Badge>
                {!supported && (
                  <Badge variant="outline" className="bg-background/70">
                    {t(WEBUI.common.notCompiled)}
                  </Badge>
                )}
              </div>
              {definition?.description && (
                <p className="mt-2 max-w-2xl text-sm text-muted-foreground">
                  {definition.description}
                </p>
              )}
            </div>
          </div>

          {resolvedSummaryItems.length > 0 && (
            <div className="mt-5 grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-4">
              {resolvedSummaryItems.map((item) => (
                <SummaryItem key={item.label} item={item} />
              ))}
            </div>
          )}

          {plugin.type === "matcher" ? (
            <MatcherRuntimeControl plugin={plugin} mode="detail" />
          ) : null}
          {plugin.type === "provider" ? (
            <ProviderRuntimeControl plugin={plugin} mode="detail" />
          ) : null}

          <div className="mt-4 flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => togglePluginPin(plugin.id)}
            >
              {plugin.pinned ? (
                <>
                  <PinOff className="mr-1.5 h-4 w-4" />
                  {t(WEBUI.plugins.unpin)}
                </>
              ) : (
                <>
                  <Pin className="mr-1.5 h-4 w-4" />
                  {t(WEBUI.plugins.pin)}
                </>
              )}
            </Button>
            <PluginDeleteButton
              plugin={plugin}
              variant="outline"
              size="sm"
              className="gap-1.5 text-destructive hover:text-destructive"
              iconClassName="mr-0 h-4 w-4"
              label={t(WEBUI.common.delete)}
              stopPropagation={false}
              onDeleted={onClose}
            />
          </div>
        </div>
      </header>

      {appliedStatus === "not-applied" && (
        <PluginNotAppliedBanner
          applying={isApplying}
          saving={isConfigSaving}
          restarting={isRestarting}
          disabled={Boolean(configError)}
          onApply={() => {
            void applyConfig().catch(() => {
              // ConfigSyncControl in the header surfaces the failure state.
            });
          }}
        />
      )}

      <Tabs
        defaultValue="config"
        className="mx-auto min-h-0 w-full max-w-6xl flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]"
      >
        <TabsList
          className={cn(
            "grid w-full",
            visibleTabCount === 1 && "grid-cols-1",
            visibleTabCount === 2 && "grid-cols-2",
            visibleTabCount === 3 && "grid-cols-3",
            visibleTabCount === 4 && "grid-cols-4",
            visibleTabCount >= 5 && "grid-cols-5",
          )}
        >
          <TabsTrigger value="config">{t(WEBUI.plugins.configTab)}</TabsTrigger>
          {hasStatsContent && (
            <TabsTrigger value="stats">{t(WEBUI.plugins.statsTab)}</TabsTrigger>
          )}
          {resolvedExtraTabs.map((tab) => (
            <TabsTrigger key={tab.value} value={tab.value}>
              {tab.icon}
              {tab.label}
            </TabsTrigger>
          ))}
          {hasMetricSeries && (
            <TabsTrigger value="metrics">
              {t(WEBUI.plugins.metricsTab)}
            </TabsTrigger>
          )}
        </TabsList>

        <TabsContent value="config" className="mt-4 space-y-4">
          {configContent ?? (
            <Card>
              <CardHeader className="p-4 pb-2">
                <CardTitle className="text-sm">
                  {t(WEBUI.plugins.configTab)}
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4 p-4 pt-0">
                {definition ? (
                  <PluginConfigModeEditor
                    key={plugin.id}
                    fields={definition.configSchema}
                    plugins={plugins}
                    values={configValues}
                    onChange={setConfigValues}
                    onValidityChange={setConfigValid}
                    defaultArrayObjectCollapsed={!editingConfig}
                    readOnly={!editingConfig}
                    pluginKind={plugin.pluginKind}
                    currentPluginName={plugin.name}
                    outboundProfileNames={outboundProfileNames}
                  />
                ) : (
                  <Textarea
                    value={configJson}
                    onChange={(event) => setConfigJson(event.target.value)}
                    className="min-h-[220px] font-mono text-sm"
                    disabled={!editingConfig}
                  />
                )}
                <div className="flex justify-end gap-2">
                  {editingConfig ? (
                    <>
                      <Button
                        key="cancel-config-edit"
                        variant="outline"
                        onClick={handleCancelConfigEdit}
                      >
                        {t(WEBUI.common.cancel)}
                      </Button>
                      <Button
                        key="save-config-edit"
                        onClick={handleSaveConfig}
                        disabled={!configValid || isConfigSaving}
                      >
                        <Save className="mr-1.5 h-4 w-4" />
                        {isConfigSaving
                          ? t(WEBUI.common.saving)
                          : t(WEBUI.common.saveConfig)}
                      </Button>
                    </>
                  ) : (
                    <Button
                      key="start-config-edit"
                      onClick={() => setEditingConfig(true)}
                    >
                      <Pencil className="mr-1.5 h-4 w-4" />
                      {t(WEBUI.common.editConfig)}
                    </Button>
                  )}
                </div>
              </CardContent>
            </Card>
          )}
          <DependencySection
            tag={plugin.name}
            dependencyGraph={dependencyGraph}
          />
        </TabsContent>

        {hasStatsContent && (
          <TabsContent value="stats" className="mt-4 space-y-4">
            {metricsContent}
          </TabsContent>
        )}

        {resolvedExtraTabs.map((tab) => (
          <TabsContent
            key={tab.value}
            value={tab.value}
            className="mt-4 space-y-4"
          >
            {tab.content}
          </TabsContent>
        ))}

        {hasMetricSeries && (
          <TabsContent value="metrics" className="mt-4 space-y-4">
            <PluginMetricsPanel tag={plugin.name} />
          </TabsContent>
        )}
      </Tabs>

      <AlertDialog
        open={Boolean(pendingRename)}
        onOpenChange={(open) => {
          if (!open) setPendingRename(null);
        }}
      >
        <AlertDialogContent size="lg">
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t(WEBUI.plugins.updateReferencesTitle)}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t(WEBUI.plugins.updateReferencesDescription, {
                oldName: plugin.name,
                newName: pendingRename?.name ?? "",
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="max-h-56 space-y-2 overflow-auto rounded-md border bg-muted/20 p-2">
            {pendingRename?.references.map((reference) => (
              <div
                key={`${reference.source_tag}:${reference.field}`}
                className="rounded-md border bg-background px-3 py-2 text-xs"
              >
                <div className="font-mono font-medium">
                  {reference.source_tag}
                </div>
                <div className="mt-1 font-mono text-muted-foreground">
                  {reference.field}
                </div>
              </div>
            ))}
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={configBusy}>
              {t(WEBUI.common.cancel)}
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={configBusy}
              onClick={(event) => {
                event.preventDefault();
                void handleConfirmRename();
              }}
            >
              {t(WEBUI.plugins.updateReferencesAction)}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function DependencySection({
  tag,
  dependencyGraph,
}: {
  tag: string;
  dependencyGraph: ReturnType<typeof useAppStore.getState>["dependencyGraph"];
}) {
  const { t } = useI18n();
  if (!dependencyGraph) return null;
  const initIndex = dependencyGraph.init_order.indexOf(tag);
  const upstream = dependencyGraph.edges.filter(
    (edge) => edge.source_tag === tag,
  );
  const downstream = dependencyGraph.edges.filter(
    (edge) => edge.target_tag === tag,
  );

  return (
    <Card>
      <CardHeader className="p-4 pb-2">
        <CardTitle className="text-sm">
          {t(WEBUI.plugins.dependencyTitle)}
        </CardTitle>
      </CardHeader>
      <CardContent className="grid gap-3 p-4 pt-0 text-sm sm:grid-cols-3">
        <SummaryItem
          item={{
            label: t(WEBUI.plugins.initOrder),
            value: initIndex >= 0 ? String(initIndex + 1) : "-",
          }}
        />
        <SummaryItem
          item={{
            label: t(WEBUI.plugins.dependencies),
            value: String(upstream.length),
          }}
        />
        <SummaryItem
          item={{
            label: t(WEBUI.plugins.referencedBy),
            value: String(downstream.length),
          }}
        />
        <div className="space-y-1 sm:col-span-3">
          {[...upstream, ...downstream].length ? (
            [...upstream, ...downstream].map((edge) => (
              <div
                key={`${edge.source_tag}-${edge.target_tag}-${edge.field}`}
                className="truncate rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground"
              >
                {edge.source_tag}.{edge.field} -&gt; {edge.target_tag}
              </div>
            ))
          ) : (
            <div className="rounded-md border border-dashed px-3 py-2 text-muted-foreground">
              {t(WEBUI.plugins.noDependencyEdges)}
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function SummaryItem({ item }: { item: PluginSummaryItem }) {
  return (
    <div className="rounded-lg border border-border/70 bg-background/70 px-3 py-2">
      <div className="text-xs text-muted-foreground">{item.label}</div>
      <div className="mt-1 truncate font-mono text-sm font-semibold">
        {item.value}
      </div>
    </div>
  );
}

// Inline placeholder used inside per-plugin stats / insights tabs when the plugin
// hasn't been applied to the backend yet — short-circuits all /api/plugins/{tag}/*
// fetches so the user gets a clear hint instead of an HTTP 404 noise wall.
export function PluginNotAppliedPlaceholder({
  title,
  description,
}: {
  title?: string;
  description?: string;
} = {}) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col items-start gap-2 rounded-lg border border-dashed border-yellow-500/40 bg-yellow-500/5 px-4 py-6 text-sm text-yellow-700 dark:text-yellow-400">
      <div className="flex items-center gap-2 font-medium">
        <AlertTriangle className="h-4 w-4 shrink-0" />
        {title ?? t(WEBUI.plugins.notAppliedTitle)}
      </div>
      <div className="text-xs text-yellow-700/80 dark:text-yellow-400/80">
        {description ?? t(WEBUI.plugins.notAppliedDescription)}
      </div>
    </div>
  );
}

function PluginNotAppliedBanner({
  applying,
  saving,
  restarting,
  disabled,
  onApply,
}: {
  applying: boolean;
  saving: boolean;
  restarting: boolean;
  disabled: boolean;
  onApply: () => void;
}) {
  const { t } = useI18n();
  const busy = applying || saving || restarting;
  return (
    <div className="shrink-0 border-b bg-yellow-500/5">
      <div className="mx-auto flex w-full max-w-6xl items-start gap-3 px-5 py-3">
        <div className="flex items-start gap-2 text-sm text-yellow-700 dark:text-yellow-400">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <div className="font-medium">
              {t(WEBUI.plugins.notAppliedTitle)}
            </div>
            <div className="mt-0.5 text-xs text-yellow-700/80 dark:text-yellow-400/80">
              {t(WEBUI.plugins.notAppliedDescription)}
            </div>
          </div>
        </div>
        <Button
          size="sm"
          variant="outline"
          className="ml-auto h-7 gap-1.5 rounded-md border-yellow-500/40 bg-yellow-500/10 px-2.5 text-yellow-700 hover:bg-yellow-500/20 hover:text-yellow-700 dark:text-yellow-300 dark:hover:text-yellow-300"
          disabled={busy || disabled}
          onClick={onApply}
        >
          {busy ? (
            <Spinner className="h-3.5 w-3.5" />
          ) : (
            <Rocket className="h-3.5 w-3.5" />
          )}
          {applying
            ? t(WEBUI.plugins.applying)
            : restarting
              ? t(WEBUI.plugins.restarting)
              : t(WEBUI.plugins.applyNow)}
        </Button>
      </div>
    </div>
  );
}
